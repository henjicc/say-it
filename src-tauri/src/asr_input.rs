//! 识别输入与取消分离。Finish 保持音频顺序；Stop 不等待积压的音频被识别。
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};

pub(crate) enum AsrStreamInput {
    RawF32(Vec<f32>),
    Finish,
    Stop,
}

#[derive(Clone)]
pub(crate) struct AsrStreamHandle {
    pub(crate) tx: AsrInputSender,
    cancellation: Arc<AsrCancellation>,
}

// 可暂停的文件输入最多提前排队约一秒 16 kHz 单声道 f32；设备回调不能在此等待。
const PACED_QUEUE_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct QueueBudget {
    bytes: AtomicUsize,
    space: Notify,
}

struct QueuedInput {
    input: Option<AsrStreamInput>,
    budget: Arc<QueueBudget>,
    bytes: usize,
}
impl QueuedInput {
    fn cost(input: &AsrStreamInput) -> usize {
        std::mem::size_of::<Self>()
            + match input {
                AsrStreamInput::RawF32(samples) => samples.capacity() * std::mem::size_of::<f32>(),
                _ => 0,
            }
    }
    fn into_input(mut self) -> AsrStreamInput {
        self.input.take().unwrap()
    }
}
impl Drop for QueuedInput {
    fn drop(&mut self) {
        self.budget.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
        self.budget.space.notify_waiters();
    }
}

#[derive(Clone)]
pub(crate) struct AsrInputSender {
    inner: mpsc::UnboundedSender<QueuedInput>,
    budget: Arc<QueueBudget>,
    cancellation: Arc<AsrCancellation>,
}
impl AsrInputSender {
    // 设备回调沿用不等待的发送；同样计费，让可等待输入看见完整队列占用。
    pub(crate) fn send(
        &self,
        input: AsrStreamInput,
    ) -> Result<(), mpsc::error::SendError<AsrStreamInput>> {
        if self.cancellation.is_cancelled() && !matches!(input, AsrStreamInput::Stop) {
            return Err(mpsc::error::SendError(input));
        }
        let bytes = QueuedInput::cost(&input);
        self.budget.bytes.fetch_add(bytes, Ordering::AcqRel);
        self.send_reserved(input, bytes)
    }

    fn send_reserved(
        &self,
        input: AsrStreamInput,
        bytes: usize,
    ) -> Result<(), mpsc::error::SendError<AsrStreamInput>> {
        self.inner
            .send(QueuedInput {
                input: Some(input),
                budget: self.budget.clone(),
                bytes,
            })
            .map_err(|error| mpsc::error::SendError(error.0.into_input()))
    }

    pub(crate) async fn send_paced(
        &self,
        input: AsrStreamInput,
    ) -> Result<(), mpsc::error::SendError<AsrStreamInput>> {
        let bytes = QueuedInput::cost(&input);
        if bytes > PACED_QUEUE_BYTES {
            return Err(mpsc::error::SendError(input));
        }
        loop {
            // notify_waiters 不保存 permit：必须先登记，再检查预算/取消，避免丢失释放通知。
            let notified = self.budget.space.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.cancellation.is_cancelled() || self.inner.is_closed() {
                return Err(mpsc::error::SendError(input));
            }
            if self
                .budget
                .bytes
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                    used.checked_add(bytes)
                        .filter(|total| *total <= PACED_QUEUE_BYTES)
                })
                .is_ok()
            {
                return self.send_reserved(input, bytes);
            }
            tokio::select! {
                _ = notified => {},
                _ = self.inner.closed() => return Err(mpsc::error::SendError(input)),
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct AsrCancellation {
    flag: Arc<AtomicBool>,
    wake: Notify,
}

impl AsrCancellation {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(crate) async fn cancelled(&self) {
        // 单个会话消费者等待取消；notify_one 保留 permit，防止检查与 await 间漏唤醒。
        let notified = self.wake.notified();
        if !self.is_cancelled() {
            notified.await;
        }
    }
}

impl AsrStreamHandle {
    pub(crate) fn channel() -> (Self, AsrStreamReceiver) {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancellation = Arc::new(AsrCancellation::default());
        let budget = Arc::new(QueueBudget::default());
        (
            Self {
                tx: AsrInputSender {
                    inner: tx,
                    budget,
                    cancellation: cancellation.clone(),
                },
                cancellation: cancellation.clone(),
            },
            AsrStreamReceiver {
                inner: Some(rx),
                cancellation,
            },
        )
    }

    pub(crate) fn stop(&self) {
        self.cancellation.flag.store(true, Ordering::Release);
        self.cancellation.wake.notify_one();
        self.tx.budget.space.notify_waiters();
        // 唤醒空队列上的 blocking_recv/recv；接收端读取前后都优先检查独立取消位。
        let _ = self.tx.send(AsrStreamInput::Stop);
    }
}

pub(crate) struct AsrStreamReceiver {
    inner: Option<mpsc::UnboundedReceiver<QueuedInput>>,
    cancellation: Arc<AsrCancellation>,
}

impl AsrStreamReceiver {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub(crate) fn cancellation_flag(&self) -> Arc<AtomicBool> {
        self.cancellation.flag.clone()
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn cancellation(&self) -> Arc<AsrCancellation> {
        self.cancellation.clone()
    }

    fn take_stop(&mut self) -> bool {
        // 取消后直接释放接收器和已排队的 PCM；持有旧 sender 的采集端会收到关闭错误。
        self.is_cancelled() && self.inner.take().is_some()
    }

    pub(crate) fn blocking_recv(&mut self) -> Option<AsrStreamInput> {
        if self.take_stop() {
            return Some(AsrStreamInput::Stop);
        }
        let input = self
            .inner
            .as_mut()?
            .blocking_recv()
            .map(QueuedInput::into_input);
        if self.take_stop() {
            Some(AsrStreamInput::Stop)
        } else {
            input
        }
    }

    pub(crate) fn try_recv(&mut self) -> Result<AsrStreamInput, mpsc::error::TryRecvError> {
        if self.take_stop() {
            return Ok(AsrStreamInput::Stop);
        }
        let input = self
            .inner
            .as_mut()
            .ok_or(mpsc::error::TryRecvError::Disconnected)?
            .try_recv()
            .map(QueuedInput::into_input);
        if self.take_stop() {
            Ok(AsrStreamInput::Stop)
        } else {
            input
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(crate) async fn recv(&mut self) -> Option<AsrStreamInput> {
        if self.take_stop() {
            return Some(AsrStreamInput::Stop);
        }
        let input = self
            .inner
            .as_mut()?
            .recv()
            .await
            .map(QueuedInput::into_input);
        if self.take_stop() {
            Some(AsrStreamInput::Stop)
        } else {
            input
        }
    }
}

#[cfg(all(test, windows))]
mod performance_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod paced_tests;

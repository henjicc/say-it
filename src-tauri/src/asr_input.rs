//! 识别输入与取消分离。Finish 保持音频顺序；Stop 不等待积压的音频被识别。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};

pub(crate) enum AsrStreamInput {
    RawF32(Vec<f32>),
    Finish,
    Stop,
}

#[derive(Clone)]
pub(crate) struct AsrStreamHandle {
    pub(crate) tx: mpsc::UnboundedSender<AsrStreamInput>,
    cancellation: Arc<AsrCancellation>,
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
        (
            Self {
                tx,
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
        // 唤醒空队列上的 blocking_recv/recv；接收端读取前后都优先检查独立取消位。
        let _ = self.tx.send(AsrStreamInput::Stop);
    }
}

pub(crate) struct AsrStreamReceiver {
    inner: Option<mpsc::UnboundedReceiver<AsrStreamInput>>,
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
        let input = self.inner.as_mut()?.blocking_recv();
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
            .try_recv();
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
        let input = self.inner.as_mut()?.recv().await;
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

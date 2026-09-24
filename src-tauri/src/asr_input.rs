//! 有序音频输入、按字节等待的文件输入，以及慢实时消费者的无损暂存。
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::cancellation::CancellationFlag;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{mpsc, Notify};
mod spool;

pub(crate) enum AsrStreamInput {
    RawF32(Vec<f32>),
    Finish,
    Stop,
    Failed(String),
}
#[derive(Clone)]
pub(crate) struct AsrStreamHandle {
    pub(crate) tx: AsrInputSender,
    cancellation: Arc<AsrCancellation>,
}
// 文件可暂停投喂；设备输入不能等待消费者，超出短队列后交给独立写盘线程。
const PACED_QUEUE_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy)]
struct Limits {
    memory: usize,
    resident: usize,
    queued: usize,
    packets: usize,
    packet: usize,
}
impl Default for Limits {
    fn default() -> Self {
        // 分别限制短队列、待写浮点缓冲、逻辑积压、票据数量和单块分配。
        // 按会话计费；耗尽时报告失败，绝不丢音频后继续报告成功。
        Self {
            memory: 2 * 1024 * 1024,
            resident: 64 * 1024 * 1024,
            queued: 512 * 1024 * 1024,
            packets: 32768,
            packet: 64 * 1024,
        }
    }
}
#[derive(Default)]
struct QueueBudget {
    bytes: AtomicUsize,
    resident: AtomicUsize,
    packets: AtomicUsize,
    space: Notify,
    limits: Limits,
}
struct QueueCharge {
    budget: Arc<QueueBudget>,
    bytes: usize,
}
impl Drop for QueueCharge {
    fn drop(&mut self) {
        self.budget.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
        self.budget.packets.fetch_sub(1, Ordering::AcqRel);
        self.budget.space.notify_waiters();
    }
}
struct ResidentCharge {
    budget: Arc<QueueBudget>,
    bytes: usize,
}
impl ResidentCharge {
    fn reserve(budget: &Arc<QueueBudget>, bytes: usize) -> Option<Self> {
        budget
            .resident
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= budget.limits.resident)
            })
            .ok()?;
        Some(Self {
            budget: budget.clone(),
            bytes,
        })
    }
}
impl Drop for ResidentCharge {
    fn drop(&mut self) {
        self.budget.resident.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
enum Payload {
    Memory(AsrStreamInput),
    Disk(spool::Ticket),
    Reading(tokio::sync::oneshot::Receiver<(spool::Reader, Result<Vec<f32>, String>)>),
}
struct QueuedInput {
    payload: Option<Payload>,
    _charge: QueueCharge,
    _resident: Option<ResidentCharge>,
}
impl QueuedInput {
    fn cost(input: &AsrStreamInput) -> usize {
        std::mem::size_of::<Self>() + Self::audio_bytes(input)
    }
    fn audio_bytes(input: &AsrStreamInput) -> usize {
        match input {
            AsrStreamInput::RawF32(samples) => samples.capacity() * 4,
            _ => 0,
        }
    }
    fn into_input(mut self) -> AsrStreamInput {
        match self.payload.take().unwrap() {
            Payload::Memory(input) => input,
            Payload::Disk(_) => unreachable!("磁盘票据不能同步还原"),
            Payload::Reading(_) => unreachable!("读盘票据不能同步还原"),
        }
    }
}
#[derive(Clone)]
pub(crate) struct AsrInputSender {
    inner: mpsc::UnboundedSender<QueuedInput>,
    budget: Arc<QueueBudget>,
    cancellation: Arc<AsrCancellation>,
    spool: Arc<spool::Spool>,
}
fn signal_failure(
    cancellation: &AsrCancellation,
    budget: &Arc<QueueBudget>,
    output: &mpsc::WeakUnboundedSender<QueuedInput>,
    error: String,
) {
    let mut failure = cancellation
        .failure
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if cancellation.flag.swap(true, Ordering::AcqRel) {
        return;
    }
    *failure = Some(error);
    drop(failure);
    budget.space.notify_waiters();
    if let Some(output) = output.upgrade() {
        budget.packets.fetch_add(1, Ordering::AcqRel);
        let _ = output.send(QueuedInput {
            payload: Some(Payload::Memory(AsrStreamInput::Stop)),
            _charge: QueueCharge {
                budget: budget.clone(),
                bytes: 0,
            },
            _resident: None,
        });
    }
}
impl AsrInputSender {
    pub(crate) fn is_closed(&self) -> bool {
        self.inner.is_closed() || self.cancellation.is_cancelled()
    }
    pub(crate) fn fail(&self, error: String) {
        signal_failure(
            &self.cancellation,
            &self.budget,
            &self.inner.downgrade(),
            error,
        );
    }
    pub(crate) fn send(
        &self,
        input: AsrStreamInput,
    ) -> Result<(), mpsc::error::SendError<AsrStreamInput>> {
        if self.inner.is_closed()
            || (self.cancellation.is_cancelled() && !matches!(input, AsrStreamInput::Stop))
        {
            return Err(mpsc::error::SendError(input));
        }
        let bytes = QueuedInput::cost(&input);
        let audio_bytes = QueuedInput::audio_bytes(&input);
        if audio_bytes > self.budget.limits.packet {
            self.fail("音频输入块超过暂存容量，请重新开始".into());
            return Err(mpsc::error::SendError(input));
        }
        if self
            .budget
            .packets
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < self.budget.limits.packets).then_some(count + 1)
            })
            .is_err()
        {
            self.fail("音频处理长期落后，音频排队数量已达上限，本次任务已停止".into());
            return Err(mpsc::error::SendError(input));
        }
        if self
            .budget
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= self.budget.limits.queued)
            })
            .is_err()
        {
            self.budget.packets.fetch_sub(1, Ordering::AcqRel);
            self.fail("音频处理长期落后，音频暂存已达上限，本次任务已停止".into());
            return Err(mpsc::error::SendError(input));
        }
        let charge = QueueCharge {
            budget: self.budget.clone(),
            bytes,
        };
        let Some(resident) = ResidentCharge::reserve(&self.budget, audio_bytes) else {
            self.fail("音频暂存来不及写入，内存缓冲已达上限，本次任务已停止".into());
            return Err(mpsc::error::SendError(input));
        };
        if self.budget.resident.load(Ordering::Acquire) > self.budget.limits.memory {
            if let AsrStreamInput::RawF32(samples) = &input {
                let budget = self.budget.clone();
                let cancellation = self.cancellation.clone();
                let output = self.inner.downgrade();
                let failure =
                    Arc::new(move |error| signal_failure(&cancellation, &budget, &output, error));
                // 原输入仅保留到非阻塞入队完成，以便接收端关闭竞态时完整还给采集方。
                let ticket = match self.spool.submit(samples.clone(), resident, failure) {
                    Ok(ticket) => ticket,
                    Err(error) => {
                        self.fail(error);
                        return Err(mpsc::error::SendError(input));
                    }
                };
                let queued = QueuedInput {
                    payload: Some(Payload::Disk(ticket)),
                    _charge: charge,
                    _resident: None,
                };
                if self.inner.send(queued).is_err() {
                    self.spool.shutdown();
                    return Err(mpsc::error::SendError(input));
                }
                return Ok(());
            }
        }
        self.inner
            .send(QueuedInput {
                payload: Some(Payload::Memory(input)),
                _charge: charge,
                _resident: Some(resident),
            })
            .map_err(|error| mpsc::error::SendError(error.0.into_input()))
    }
    fn send_reserved(
        &self,
        input: AsrStreamInput,
        bytes: usize,
    ) -> Result<(), mpsc::error::SendError<AsrStreamInput>> {
        self.budget.packets.fetch_add(1, Ordering::AcqRel);
        let charge = QueueCharge {
            budget: self.budget.clone(),
            bytes,
        };
        let Some(resident) =
            ResidentCharge::reserve(&self.budget, QueuedInput::audio_bytes(&input))
        else {
            self.fail("音频输入缓冲已达上限".into());
            return Err(mpsc::error::SendError(input));
        };
        self.inner
            .send(QueuedInput {
                payload: Some(Payload::Memory(input)),
                _charge: charge,
                _resident: Some(resident),
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
            tokio::select! { _=notified=>{}, _=self.inner.closed()=>return Err(mpsc::error::SendError(input)) }
        }
    }
}
#[derive(Default)]
pub(crate) struct AsrCancellation {
    flag: Arc<CancellationFlag>,
    failure: Mutex<Option<String>>,
}
impl AsrCancellation {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
    pub(crate) async fn cancelled(&self) {
        self.flag.cancelled().await;
    }
    fn terminal(&self) -> AsrStreamInput {
        match self
            .failure
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
        {
            Some(error) => AsrStreamInput::Failed(error),
            None => AsrStreamInput::Stop,
        }
    }
}
impl AsrStreamHandle {
    pub(crate) fn channel() -> (Self, AsrStreamReceiver) {
        Self::with_limits(Limits::default())
    }
    fn with_limits(limits: Limits) -> (Self, AsrStreamReceiver) {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancellation = Arc::new(AsrCancellation::default());
        let budget = Arc::new(QueueBudget {
            limits,
            ..Default::default()
        });
        let spool = Arc::new(spool::Spool::default());
        (
            Self {
                tx: AsrInputSender {
                    inner: tx,
                    budget,
                    cancellation: cancellation.clone(),
                    spool: spool.clone(),
                },
                cancellation: cancellation.clone(),
            },
            AsrStreamReceiver {
                inner: Some(rx),
                cancellation,
                pending: None,
                reader: spool::Reader::default(),
                spool: Arc::downgrade(&spool),
            },
        )
    }
    pub(crate) fn stop(&self) {
        self.cancellation.flag.store(true, Ordering::Release);
        self.tx.budget.space.notify_waiters();
        let _ = self.tx.send(AsrStreamInput::Stop);
    }
}
pub(crate) struct AsrStreamReceiver {
    inner: Option<mpsc::UnboundedReceiver<QueuedInput>>,
    cancellation: Arc<AsrCancellation>,
    pending: Option<QueuedInput>,
    reader: spool::Reader,
    spool: Weak<spool::Spool>,
}
impl Drop for AsrStreamReceiver {
    fn drop(&mut self) {
        self.cancellation.flag.store(true, Ordering::Release);
        if let Some(spool) = self.spool.upgrade() {
            spool.shutdown();
        }
    }
}
impl AsrStreamReceiver {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
    pub(crate) fn take_failure(&self) -> Option<String> {
        self.cancellation
            .failure
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
    }
    pub(crate) fn cancellation_flag(&self) -> Arc<CancellationFlag> {
        self.cancellation.flag.clone()
    }
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn cancellation(&self) -> Arc<AsrCancellation> {
        self.cancellation.clone()
    }
    fn take_terminal(&mut self) -> Option<AsrStreamInput> {
        if self.is_cancelled() && self.inner.take().is_some() {
            self.pending = None;
            self.reader = spool::Reader::default();
            if let Some(spool) = self.spool.upgrade() {
                spool.shutdown();
            }
            Some(self.cancellation.terminal())
        } else {
            None
        }
    }
    fn read_result(&mut self, result: Result<spool::DiskPacket, String>) -> AsrStreamInput {
        if let Some(terminal) = self.take_terminal() {
            return terminal;
        }
        match result.and_then(|packet| self.reader.read(packet)) {
            Ok(samples) => AsrStreamInput::RawF32(samples),
            Err(error) => {
                self.inner = None;
                self.pending = None;
                self.reader = spool::Reader::default();
                *self
                    .cancellation
                    .failure
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) = Some(error.clone());
                self.cancellation.flag.store(true, Ordering::Release);
                if let Some(spool) = self.spool.upgrade() {
                    spool.shutdown();
                }
                AsrStreamInput::Failed(error)
            }
        }
    }
    pub(crate) fn blocking_recv(&mut self) -> Option<AsrStreamInput> {
        if let Some(terminal) = self.take_terminal() {
            return Some(terminal);
        }
        let mut queued = match self.pending.take() {
            Some(queued) => queued,
            None => self.inner.as_mut()?.blocking_recv()?,
        };
        if let Some(terminal) = self.take_terminal() {
            return Some(terminal);
        }
        let input = match queued.payload.take().unwrap() {
            Payload::Memory(input) => input,
            Payload::Disk(ticket) => {
                let cancellation = self.cancellation.clone();
                let result = tauri::async_runtime::block_on(async {
                    tokio::select! {
                        biased;
                        _=cancellation.cancelled()=>None,
                        result=ticket=>Some(result.unwrap_or_else(|_|Err("音频暂存任务提前结束".into()))),
                    }
                });
                match result {
                    Some(result) => self.read_result(result),
                    None => return self.take_terminal(),
                }
            }
            Payload::Reading(ticket) => {
                let cancellation = self.cancellation.clone();
                let result = tauri::async_runtime::block_on(async {
                    tokio::select! {
                        biased;_=cancellation.cancelled()=>None,result=ticket=>Some(result),
                    }
                });
                match result {
                    None => return self.take_terminal(),
                    Some(Ok((reader, result))) => {
                        self.reader = reader;
                        match result {
                            Ok(samples) => AsrStreamInput::RawF32(samples),
                            Err(error) => self.read_result(Err(error)),
                        }
                    }
                    Some(Err(_)) => self.read_result(Err("读取音频暂存任务提前结束".into())),
                }
            }
        };
        self.take_terminal().or(Some(input))
    }
    #[cfg(test)]
    pub(crate) fn try_recv(&mut self) -> Result<AsrStreamInput, mpsc::error::TryRecvError> {
        if let Some(terminal) = self.take_terminal() {
            return Ok(terminal);
        }
        let mut queued = match self.pending.take() {
            Some(queued) => queued,
            None => self
                .inner
                .as_mut()
                .ok_or(mpsc::error::TryRecvError::Disconnected)?
                .try_recv()?,
        };
        let input = match queued.payload.take().unwrap() {
            Payload::Memory(input) => input,
            Payload::Disk(mut ticket) => match ticket.try_recv() {
                Ok(result) => self.read_result(result),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    queued.payload = Some(Payload::Disk(ticket));
                    self.pending = Some(queued);
                    return Err(mpsc::error::TryRecvError::Empty);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.read_result(Err("音频暂存任务提前结束".into()))
                }
            },
            Payload::Reading(mut ticket) => match ticket.try_recv() {
                Ok((reader, result)) => {
                    self.reader = reader;
                    match result {
                        Ok(samples) => AsrStreamInput::RawF32(samples),
                        Err(error) => self.read_result(Err(error)),
                    }
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    queued.payload = Some(Payload::Reading(ticket));
                    self.pending = Some(queued);
                    return Err(mpsc::error::TryRecvError::Empty);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.read_result(Err("读取音频暂存任务提前结束".into()))
                }
            },
        };
        Ok(self.take_terminal().unwrap_or(input))
    }
    pub(crate) async fn recv(&mut self) -> Option<AsrStreamInput> {
        loop {
            if let Some(terminal) = self.take_terminal() {
                return Some(terminal);
            }
            if self.pending.is_none() {
                self.pending = Some(self.inner.as_mut()?.recv().await?);
            }
            if let Some(terminal) = self.take_terminal() {
                return Some(terminal);
            }
            let cancellation = self.cancellation.clone();
            // pending 归接收器所有：Apple 的 select 分支放弃 recv 时不能丢掉已出队音频。
            match self.pending.as_mut().unwrap().payload.as_mut().unwrap() {
                Payload::Memory(_) => {
                    let input = self.pending.take().unwrap().into_input();
                    return self.take_terminal().or(Some(input));
                }
                Payload::Disk(ticket) => {
                    let result = tokio::select! {biased;_=cancellation.cancelled()=>return self.take_terminal(),result=ticket=>result.unwrap_or_else(|_|Err("音频暂存任务提前结束".into()))};
                    match result {
                        Ok(packet) => {
                            let mut reader = std::mem::take(&mut self.reader);
                            let (reply, ticket) = tokio::sync::oneshot::channel();
                            self.pending.as_mut().unwrap().payload = Some(Payload::Reading(ticket));
                            tauri::async_runtime::spawn_blocking(move || {
                                let result = reader.read(packet);
                                let _ = reply.send((reader, result));
                            });
                        }
                        Err(error) => {
                            self.pending.take();
                            return Some(self.read_result(Err(error)));
                        }
                    }
                }
                Payload::Reading(ticket) => {
                    let result = tokio::select! {biased;_=cancellation.cancelled()=>return self.take_terminal(),result=ticket=>result};
                    let input = match result {
                        Ok((reader, result)) => {
                            self.reader = reader;
                            match result {
                                Ok(samples) => AsrStreamInput::RawF32(samples),
                                Err(error) => self.read_result(Err(error)),
                            }
                        }
                        Err(_) => self.read_result(Err("读取音频暂存任务提前结束".into())),
                    };
                    self.pending.take();
                    return self.take_terminal().or(Some(input));
                }
            }
        }
    }
}
// 原始采集消费者也复用同一份容量/暂存实现；接口将失败与正常结束分开，避免尾部损坏被当作成功。
pub(crate) struct RawAudioReceiver(AsrStreamReceiver);
impl RawAudioReceiver {
    pub(crate) fn channel() -> (AsrInputSender, Self) {
        let (handle, receiver) = AsrStreamHandle::channel();
        (handle.tx, Self(receiver))
    }
    fn samples(input: Option<AsrStreamInput>) -> Result<Option<Vec<f32>>, String> {
        match input {
            Some(AsrStreamInput::RawF32(samples)) => Ok(Some(samples)),
            Some(AsrStreamInput::Failed(error)) => Err(error),
            Some(AsrStreamInput::Finish | AsrStreamInput::Stop) | None => Ok(None),
        }
    }
    pub(crate) fn blocking_recv(&mut self) -> Result<Option<Vec<f32>>, String> {
        Self::samples(self.0.blocking_recv())
    }
    pub(crate) async fn recv(&mut self) -> Result<Option<Vec<f32>>, String> {
        Self::samples(self.0.recv().await)
    }
    #[cfg(test)]
    pub(crate) fn try_recv(&mut self) -> Result<AsrStreamInput, mpsc::error::TryRecvError> {
        self.0.try_recv()
    }
}
#[cfg(test)]
mod paced_tests;
#[cfg(all(test, windows))]
mod performance_tests;
#[cfg(test)]
mod raw_tests;
#[cfg(all(test, windows))]
mod spool_performance_tests;
#[cfg(test)]
mod spool_tests;
#[cfg(test)]
mod tests;

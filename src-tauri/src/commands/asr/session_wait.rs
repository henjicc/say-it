//! 专属 JS 线程只等通道/宿主通知；不在解释器调用栈中驱动网络 Future。
use crate::state::{AsrStreamInput, AsrStreamReceiver};
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;
use tokio::sync::Notify;

struct WakeThread(std::thread::Thread);
impl Wake for WakeThread {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

pub(super) enum SessionWake {
    Input(Option<AsrStreamInput>),
    HostEvents,
    Deadline,
}

pub(super) struct SessionWait {
    waker: Waker,
    #[cfg(test)]
    polls: std::cell::Cell<usize>,
}
impl SessionWait {
    // 必须在实际消费线程上创建；重复等待复用同一个 waker。
    pub(super) fn new() -> Self {
        Self {
            waker: Waker::from(Arc::new(WakeThread(std::thread::current()))),
            #[cfg(test)]
            polls: std::cell::Cell::new(0),
        }
    }

    pub(super) fn wait(
        &self,
        receiver: &mut AsrStreamReceiver,
        host: &Notify,
        deadline: Option<Instant>,
    ) -> SessionWake {
        let mut input = std::pin::pin!(receiver.recv());
        let mut host_event = std::pin::pin!(host.notified());
        let mut context = Context::from_waker(&self.waker);
        loop {
            #[cfg(test)]
            self.polls.set(self.polls.get() + 1);
            if let Poll::Ready(input) = input.as_mut().poll(&mut context) {
                return SessionWake::Input(input);
            }
            if deadline.is_some_and(|end| Instant::now() >= end) {
                return SessionWake::Deadline;
            }
            if host_event.as_mut().poll(&mut context).is_ready() {
                // recv 的磁盘票据由 receiver 自己保留，切换分支不能丢音频。
                return SessionWake::HostEvents;
            }
            // unpark 的许可覆盖 poll 与 park 之间的通知；虚假唤醒只会重新检查。
            if let Some(end) = deadline {
                std::thread::park_timeout(end.saturating_duration_since(Instant::now()));
            } else {
                std::thread::park();
            }
        }
    }
}

#[cfg(test)]
mod tests;

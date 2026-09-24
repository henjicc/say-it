//! 保留同步原子检查，同时让异步等待方收到取消通知。
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
pub(crate) struct CancellationFlag {
    value: AtomicBool,
    changed: Notify,
}
impl CancellationFlag {
    pub(crate) fn new(value: bool) -> Self {
        Self {
            value: AtomicBool::new(value),
            changed: Notify::new(),
        }
    }
    pub(crate) fn load(&self, ordering: Ordering) -> bool {
        self.value.load(ordering)
    }
    pub(crate) fn store(&self, value: bool, ordering: Ordering) {
        self.value.store(value, ordering);
        self.changed.notify_waiters();
    }
    pub(crate) fn swap(&self, value: bool, ordering: Ordering) -> bool {
        let previous = self.value.swap(value, ordering);
        self.changed.notify_waiters();
        previous
    }
    pub(crate) async fn cancelled(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            // 注册先于状态检查；多个网络等待方都必须被唤醒，且不能丢失抢先到达的取消。
            changed.as_mut().enable();
            if self.value.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }
}

#[cfg(test)]
mod tests;

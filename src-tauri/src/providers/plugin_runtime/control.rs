use std::sync::Mutex;
use std::time::Instant;
use tokio::sync::Notify;

pub(super) struct Deadline {
    value: Mutex<Instant>,
    earlier: Notify,
}
impl Deadline {
    pub(super) fn new(value: Instant) -> Self {
        Self {
            value: Mutex::new(value),
            earlier: Notify::new(),
        }
    }
    pub(super) fn get(&self) -> Result<Instant, String> {
        self.value
            .lock()
            .map(|value| *value)
            .map_err(|_| "插件截止时间锁定失败".into())
    }
    pub(super) fn set(&self, next: Instant) -> Result<(), String> {
        let mut value = self.value.lock().map_err(|_| "插件截止时间锁定失败")?;
        let earlier = next < *value;
        *value = next;
        drop(value);
        // 音频包会频繁续期；延长无需立即唤醒，原定时器到点后会读取最新时间。
        if earlier {
            self.earlier.notify_waiters();
        }
        Ok(())
    }
    pub(super) fn expired(&self) -> bool {
        self.get().map(|end| Instant::now() >= end).unwrap_or(true)
    }
    pub(super) async fn elapsed(&self) {
        loop {
            let changed = self.earlier.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let Ok(end) = self.get() else {
                return;
            };
            if Instant::now() >= end {
                return;
            }
            tokio::select! {
                _ = changed => {},
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(end)) => {},
            }
            // 缩短和续期都重新检查，旧定时器到点不能误报已延长的截止时间。
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(all(test, windows))]
mod performance_tests;

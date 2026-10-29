use std::time::{Duration, Instant};

const STABLE_CONNECTION: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct Budget {
    attempts: u32,
    started_at: Option<Instant>,
}

impl Budget {
    pub(super) fn started(&mut self, now: Instant) {
        // 准备会话不代表服务可用，不能在此重置连续失败计数。
        self.started_at = Some(now);
    }

    pub(super) fn received_text(&mut self, text: &str) {
        if !text.trim().is_empty() {
            self.attempts = 0;
        }
    }

    pub(super) fn disconnected(&mut self, now: Instant) -> u32 {
        // 长时间安静但连接正常的会话也视为恢复，避免累计不相关的断线。
        if self
            .started_at
            .take()
            .is_some_and(|started| now.saturating_duration_since(started) >= STABLE_CONNECTION)
        {
            self.attempts = 0;
        }
        self.attempts = self.attempts.saturating_add(1);
        self.attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparing_repeatedly_failing_connections_does_not_reset_budget() {
        let start = Instant::now();
        let mut budget = Budget::default();
        for attempt in 1..=super::super::MAX_RECONNECT_ATTEMPTS + 1 {
            let now = start + Duration::from_secs(attempt as u64);
            budget.started(now);
            budget.received_text(" \n");
            assert_eq!(
                budget.disconnected(now + Duration::from_millis(50)),
                attempt
            );
        }
    }

    #[test]
    fn recognized_text_restores_budget() {
        let now = Instant::now();
        let mut budget = Budget::default();
        budget.started(now);
        assert_eq!(budget.disconnected(now), 1);
        budget.started(now);
        budget.received_text("恢复后的字幕");
        assert_eq!(budget.disconnected(now), 1);
    }

    #[test]
    fn stable_silent_connection_restores_budget_at_boundary() {
        let now = Instant::now();
        let mut budget = Budget::default();
        budget.started(now);
        assert_eq!(budget.disconnected(now), 1);
        budget.started(now);
        assert_eq!(
            budget.disconnected(now + STABLE_CONNECTION - Duration::from_millis(1)),
            2
        );
        budget.started(now);
        assert_eq!(budget.disconnected(now + STABLE_CONNECTION), 1);
    }
}

use std::time::Instant;

/// 单位区间的三次 ease-out，沿用原生窗口的浮点运算顺序。
pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// 出场/退场过渡的时长。入场与 indicator.css 的 wrapIn（0.24s ease-out）一致。
pub const TRANSITION_ENTER_MS: u64 = 240;
pub const TRANSITION_EXIT_MS: u64 = 140;

/// 窗口出现/消失的过渡动画驱动器：只输出 0..1 的视觉进度，
/// 透明度与位移怎么用它由各自窗口决定。只认 Show/Hide 语义——
/// 状态内容更新（如录音中改文案）不重播动画。
pub struct Transition {
    /// 未缓动的线性进度：0 完全隐藏，1 完全显示。
    progress: f32,
    /// +1 入场中，-1 退场中，0 静止。
    direction: i8,
    last_tick: Option<Instant>,
}

impl Transition {
    pub fn new(shown: bool) -> Self {
        Self {
            progress: if shown { 1.0 } else { 0.0 },
            direction: 0,
            last_tick: None,
        }
    }

    /// 进入或回到显示态。已完全显示时是空操作；退场中途调用则从当前位置反向回播。
    pub fn show(&mut self) {
        if self.progress >= 1.0 {
            // Hide 后首帧尚未推进就收到 Show：必须撤销退场方向，
            // 否则随后定时器仍会把刚重新打开的窗口隐藏。
            self.direction = 0;
            self.last_tick = None;
        } else if self.direction != 1 {
            self.direction = 1;
            self.last_tick = None;
        }
    }

    /// 开始退场；返回当前是否有可见内容可退（没有则调用方应立即隐藏，不必等动画）。
    pub fn hide(&mut self) -> bool {
        if self.progress > 0.0 {
            self.direction = -1;
            self.last_tick = None;
            true
        } else {
            false
        }
    }

    pub fn is_animating(&self) -> bool {
        self.direction != 0
    }

    pub fn is_fully_hidden(&self) -> bool {
        self.progress <= 0.0 && self.direction <= 0
    }

    /// 推进一帧，返回（缓动后的视觉进度，本次是否刚好到达终点）。
    pub fn tick(&mut self, now: Instant) -> (f32, bool) {
        let dt = self
            .last_tick
            .replace(now)
            .map(|last| now.saturating_duration_since(last).as_secs_f32())
            .unwrap_or(0.0);
        self.advance(dt)
    }

    /// 与 tick 分离的纯推进逻辑，便于单测注入固定帧间隔。
    pub fn advance(&mut self, dt_seconds: f32) -> (f32, bool) {
        let mut just_finished = false;
        if self.direction != 0 {
            let duration_ms = if self.direction > 0 {
                TRANSITION_ENTER_MS
            } else {
                TRANSITION_EXIT_MS
            } as f32;
            self.progress += dt_seconds * 1000.0 / duration_ms * self.direction as f32;
            if self.direction > 0 && self.progress >= 1.0 {
                self.progress = 1.0;
                self.direction = 0;
                just_finished = true;
            } else if self.direction < 0 && self.progress <= 0.0 {
                self.progress = 0.0;
                self.direction = 0;
                just_finished = true;
            }
        }
        (self.visual(), just_finished)
    }

    /// 缓动后的视觉进度：入场 ease-out（先快后慢落定），退场 ease-in（加速消失）。
    pub fn visual(&self) -> f32 {
        let t = self.progress.clamp(0.0, 1.0);
        if self.direction < 0 {
            t * t
        } else {
            1.0 - (1.0 - t).powi(3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_cancels_exit_before_its_first_frame() {
        let mut transition = Transition::new(true);
        assert!(transition.hide());
        transition.show();
        transition.advance(1.0);
        assert_eq!(transition.visual(), 1.0);
        assert!(!transition.is_animating());
        assert!(!transition.is_fully_hidden());
    }

    #[test]
    fn enter_animates_from_zero_and_finishes() {
        let mut t = Transition::new(false);
        t.show();
        assert!(t.is_animating());
        assert_eq!(t.visual(), 0.0);
        let mut finished = false;
        while !finished {
            finished = t.advance(0.016).1;
        }
        assert_eq!(t.visual(), 1.0);
        assert!(!t.is_animating());
    }

    #[test]
    fn exit_animates_and_reports_completion() {
        let mut t = Transition::new(true);
        assert!(t.hide());
        assert!(t.is_animating());
        let mut finished = false;
        while !finished {
            finished = t.advance(0.016).1;
        }
        assert!(t.is_fully_hidden());
        assert_eq!(t.visual(), 0.0);
    }

    #[test]
    fn hide_on_fully_hidden_is_a_noop_signal() {
        let mut t = Transition::new(false);
        assert!(!t.hide(), "已完全隐藏时不应再走动画");
    }

    #[test]
    fn show_during_exit_reverses_from_current_progress() {
        let mut t = Transition::new(true);
        t.hide();
        // 退场播到约一半。
        t.advance(0.07);
        let mid = t.visual();
        assert!(mid > 0.0 && mid < 1.0);
        t.show();
        assert!(t.is_animating());
        // 反转后继续向上走而不是跳变。
        let (next, _) = t.advance(0.016);
        assert!(next > 0.0 && next <= 1.0);
    }

    #[test]
    fn state_change_while_shown_does_not_replay() {
        let mut t = Transition::new(true);
        t.show();
        assert!(!t.is_animating(), "完全显示时 show 是空操作");
    }

    #[test]
    fn enter_duration_is_respected() {
        let mut t = Transition::new(false);
        t.show();
        // 恰好播完整个入场时长后应到达终点。
        let _ = t.advance(TRANSITION_ENTER_MS as f32 / 1000.0);
        assert_eq!(t.visual(), 1.0);
    }
}

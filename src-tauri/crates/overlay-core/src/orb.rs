/// 拖拽阈值（CSS 像素），与 interaction.ts 的 ORB_DRAG_THRESHOLD 一致。
pub const ORB_DRAG_THRESHOLD: f64 = 5.0;

/// 移植自 interaction.ts 的 shouldStartOrbDrag。
pub fn should_start_orb_drag(delta_x: f64, delta_y: f64) -> bool {
    delta_x.hypot(delta_y) >= ORB_DRAG_THRESHOLD
}

/// 左键动作，与 interaction.ts 的 OrbClickAction 一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbClickAction {
    Activate,
    Stop,
    Submit,
    ShowError,
}

/// 移植自 interaction.ts 的 floatingOrbClickAction。
pub fn orb_click_action(phase: &str, can_submit: bool) -> Option<OrbClickAction> {
    match phase {
        "idle" | "armed" => Some(OrbClickAction::Activate),
        "recording" => Some(OrbClickAction::Stop),
        "success" if can_submit => Some(OrbClickAction::Submit),
        "error" => Some(OrbClickAction::ShowError),
        _ => None,
    }
}

/// 右键动作，与 interaction.ts 的 OrbContextAction 一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbContextAction {
    Cancel,
    DismissSubmit,
    DismissError,
    Menu,
}

/// 移植自 interaction.ts 的 floatingOrbContextAction。
pub fn orb_context_action(
    phase: &str,
    can_submit: bool,
    transient: bool,
) -> Option<OrbContextAction> {
    if phase == "error" {
        return Some(OrbContextAction::DismissError);
    }
    if phase == "recording" {
        return Some(OrbContextAction::Cancel);
    }
    if phase == "success" && can_submit {
        return Some(OrbContextAction::DismissSubmit);
    }
    if phase == "idle" && !transient {
        return Some(OrbContextAction::Menu);
    }
    None
}

/// 波形柱数量，与 floating-orb/waveform.ts 的 WAVE_BAR_COUNT 一致。
pub const ORB_WAVE_BAR_COUNT: usize = 5;

/// 每根波形柱的基础高度占比，与 floating-orb.css 的 --bar-height 对应
/// （第 3 根使用默认的 74%）。
pub const ORB_WAVE_BAR_HEIGHTS: [f32; ORB_WAVE_BAR_COUNT] = [0.47, 0.64, 0.74, 0.64, 0.47];

/// 波形柱最小缩放，与 OrbWaveform 的 `max(0.18, level)` 一致。
pub const ORB_WAVE_BAR_MIN_SCALE: f32 = 0.18;

/// 与 floating-orb.tsx 的波形投影一致：peaks 逐个过 floatingOrbWaveScale 后取
/// 最后 WAVE_BAR_COUNT 个，不足的位置用整体 level（同样过曲线）补齐。
pub fn orb_wave_levels(level: f32, peaks: &[f32]) -> [f32; ORB_WAVE_BAR_COUNT] {
    use crate::indicator::wave_scale;
    let mut levels_out = [wave_scale(level); ORB_WAVE_BAR_COUNT];
    let take = peaks.len().min(ORB_WAVE_BAR_COUNT);
    for (index, value) in peaks[peaks.len() - take..].iter().enumerate() {
        levels_out[index] = wave_scale(*value);
    }
    levels_out
}

/// 悬浮球交给主应用的动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbAction {
    Click(OrbClickAction),
    Context(OrbContextAction),
    DragStarted,
    DragFinished,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_action_matches_interaction_ts() {
        assert_eq!(
            orb_click_action("idle", false),
            Some(OrbClickAction::Activate)
        );
        assert_eq!(
            orb_click_action("armed", false),
            Some(OrbClickAction::Activate)
        );
        assert_eq!(
            orb_click_action("recording", false),
            Some(OrbClickAction::Stop)
        );
        assert_eq!(
            orb_click_action("success", true),
            Some(OrbClickAction::Submit)
        );
        assert_eq!(orb_click_action("success", false), None);
        assert_eq!(
            orb_click_action("error", false),
            Some(OrbClickAction::ShowError)
        );
        for phase in [
            "processing",
            "smartProcessing",
            "submitting",
            "moving",
            "positioning",
            "busy",
            "fallback",
            "cancelled",
            "submitted",
        ] {
            assert_eq!(orb_click_action(phase, true), None, "{phase}");
        }
    }

    #[test]
    fn context_action_matches_interaction_ts() {
        assert_eq!(
            orb_context_action("error", false, false),
            Some(OrbContextAction::DismissError)
        );
        assert_eq!(
            orb_context_action("recording", false, false),
            Some(OrbContextAction::Cancel)
        );
        assert_eq!(
            orb_context_action("success", true, false),
            Some(OrbContextAction::DismissSubmit)
        );
        assert_eq!(orb_context_action("success", false, false), None);
        assert_eq!(
            orb_context_action("idle", false, false),
            Some(OrbContextAction::Menu)
        );
        // 瞬时球（鼠标手势）的 idle 不打开菜单。
        assert_eq!(orb_context_action("idle", false, true), None);
        assert_eq!(orb_context_action("processing", false, false), None);
    }

    #[test]
    fn drag_threshold_uses_pointer_distance() {
        assert!(!should_start_orb_drag(3.0, 3.0));
        assert!(should_start_orb_drag(3.0, 4.0));
        assert!(should_start_orb_drag(5.0, 0.0));
        assert!(!should_start_orb_drag(4.9, 0.0));
    }

    #[test]
    fn wave_levels_take_the_last_peaks_and_fill_with_level() {
        use crate::indicator::wave_scale;
        assert_eq!(orb_wave_levels(0.25, &[]), [0.9; ORB_WAVE_BAR_COUNT]);
        let levels = orb_wave_levels(0.0, &[1.0, 0.0]);
        assert_eq!(levels[0], 1.0);
        assert_eq!(levels[1], 0.0);
        assert_eq!(levels[2], 0.0);
        // 6 个峰值取最后 5 个，并逐个过响度曲线。
        let levels = orb_wave_levels(0.0, &[0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
        assert_eq!(levels[0], wave_scale(0.2));
        assert_eq!(levels[4], 1.0);
    }
}

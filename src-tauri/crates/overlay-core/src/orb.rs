use crate::anim::Transition;
use crate::canvas::{
    Canvas, FontFamily, ParagraphAlign, SvgIcon, TextAlign, TextEngine, TextStyle,
};
use crate::geom::{Color, Point, PxRect, Rect, Stroke, Transform};
use crate::look::Look;
use crate::svg::SvgSeg;
use crate::window::*;
use std::time::Instant;

/// 创建时的占位边长；首次 configure 会立刻改成真实尺寸。
const DEFAULT_SIZE: i32 = 64;
/// 图标区占球边长的比例，与 .orb-icon-shell / .orb-waveform 的 43% / 54% 一致。
const ICON_RATIO: f32 = 0.43;
/// MDL2 字形的 em 方框留有内边距，字号略放大以接近 43% 的 SVG 视觉尺寸。
const GLYPH_RATIO: f32 = 0.46;
const WAVE_AREA_RATIO: f32 = 0.54;
const WAVE_AREA_RATIO_HOVER: f32 = 0.594;
const WAVE_BAR_RATIO: f32 = 0.1;
const WAVE_GAP_RATIO: f32 = 0.08;
const SPINNER_STEP_DEG: f32 = 6.0; // 60fps 下约 0.9s 一圈，与 CSS orb-spin 一致
/// 相位颜色过渡时长（非高亮 ↔ 高亮不跳变）。
const STYLE_BLEND_S: f32 = 0.15;
/// 波形柱平滑时间常数，与 CSS 的 70ms ease-out height 过渡等效。
const WAVE_SMOOTH_S: f32 = 0.070;

// Segoe MDL2 Assets 字形。
/// 麦克风图标（SVG path，24x24 viewBox），替代字体字形，线条更细腻。
const MIC_SVG_PATH: &str = "M12 15c1.66 0 2.99-1.34 2.99-3L15 6c0-1.66-1.34-3-3-3S9 4.34 9 6v6c0 1.66 1.34 3 3 3m6.08-3c-.42 0-.77.3-.83.71c-.37 2.61-2.72 4.39-5.25 4.39s-4.88-1.77-5.25-4.39a.84.84 0 0 0-.83-.71c-.52 0-.92.46-.85.97c.46 2.97 2.96 5.3 5.93 5.75V21c0 .55.45 1 1 1s1-.45 1-1v-2.28c2.96-.43 5.47-2.78 5.93-5.75a.857.857 0 0 0-.85-.97";
const GLYPH_CHECK: u16 = 0xE73E;
const GLYPH_WARNING: u16 = 0xE7BA;

// 颜色取自 ui/src/index.css 的暗色令牌与 floating-orb.css 的 color-mix 结果。
const BG: (u8, u8, u8) = (25, 25, 25); // #0f0f0f 96% + 白 4%
const BG_HOVER: (u8, u8, u8) = (20, 36, 56); // bg 78% + accent 22%
const BG_RECORDING_HOVER: (u8, u8, u8) = (20, 34, 52); // bg 80% + accent 20%
const FG: (u8, u8, u8) = (255, 255, 255);
const ACCENT: (u8, u8, u8) = (40, 110, 200); // --color-accent #286ec8
const ACCENT_LIGHT: (u8, u8, u8) = (113, 159, 219); // --color-accent-light #719fdb
const WARN: (u8, u8, u8) = (255, 209, 102); // fallback/busy
const ERROR_RGB: (u8, u8, u8) = (255, 107, 107);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbPhase {
    Idle,
    Armed,
    Moving,
    Positioning,
    Recording,
    Processing,
    SmartProcessing,
    Success,
    Fallback,
    Error,
    Cancelled,
    Busy,
    Submitting,
    Submitted,
}

impl OrbPhase {
    pub fn from_str(phase: &str) -> Option<Self> {
        Some(match phase {
            "idle" => Self::Idle,
            "armed" => Self::Armed,
            "moving" => Self::Moving,
            "positioning" => Self::Positioning,
            "recording" => Self::Recording,
            "processing" => Self::Processing,
            "smartProcessing" => Self::SmartProcessing,
            "success" => Self::Success,
            "fallback" => Self::Fallback,
            "error" => Self::Error,
            "cancelled" => Self::Cancelled,
            "busy" => Self::Busy,
            "submitting" => Self::Submitting,
            "submitted" => Self::Submitted,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Armed => "armed",
            Self::Moving => "moving",
            Self::Positioning => "positioning",
            Self::Recording => "recording",
            Self::Processing => "processing",
            Self::SmartProcessing => "smartProcessing",
            Self::Success => "success",
            Self::Fallback => "fallback",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::Busy => "busy",
            Self::Submitting => "submitting",
            Self::Submitted => "submitted",
        }
    }

    /// floating-orb.tsx 的 loading || busy：这些相位显示旋转弧线。
    pub fn is_spinner(self) -> bool {
        matches!(
            self,
            Self::Positioning
                | Self::Processing
                | Self::SmartProcessing
                | Self::Submitting
                | Self::Busy
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    SetRect {
        x: i32,
        y: i32,
        size: i32,
    },
    MoveTo {
        x: i32,
        y: i32,
    },
    SetOpacity(f32),
    Show,
    Hide,
    SetInteractive(bool),
    SetState {
        phase: OrbPhase,
        transient: bool,
        can_submit: bool,
    },
    SetWaveform {
        level: f32,
        peaks: Vec<f32>,
    },
}

#[derive(Clone, Copy, PartialEq)]
struct OrbStyle {
    icon: (u8, u8, u8),
    icon_alpha: f32,
    border: (u8, u8, u8),
    border_alpha: f32,
    background: (u8, u8, u8),
}

impl OrbStyle {
    fn lerp(self, other: OrbStyle, t: f32) -> OrbStyle {
        let mix = |a: (u8, u8, u8), b: (u8, u8, u8)| {
            (
                (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t).round() as u8,
                (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t).round() as u8,
                (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t).round() as u8,
            )
        };
        OrbStyle {
            icon: mix(self.icon, other.icon),
            icon_alpha: self.icon_alpha + (other.icon_alpha - self.icon_alpha) * t,
            border: mix(self.border, other.border),
            border_alpha: self.border_alpha + (other.border_alpha - self.border_alpha) * t,
            background: mix(self.background, other.background),
        }
    }
}

/// 各相位的颜色，与 floating-orb.css 的类规则一一对应。
fn phase_style(phase: OrbPhase, hovering: bool) -> OrbStyle {
    let base = OrbStyle {
        icon: FG,
        icon_alpha: 1.0,
        border: FG,
        border_alpha: 0.24, // 比 CSS 的 0.16 略强：没有 box-shadow 的深色小窗需要描边分辨
        background: BG,
    };
    match phase {
        OrbPhase::Idle if hovering => OrbStyle {
            icon: ACCENT_LIGHT,
            border: ACCENT,
            border_alpha: 0.55, // --accent-ring
            background: BG_HOVER,
            ..base
        },
        OrbPhase::Armed | OrbPhase::Success | OrbPhase::Submitted => OrbStyle {
            icon: ACCENT_LIGHT,
            border: ACCENT,
            border_alpha: 0.55,
            ..base
        },
        OrbPhase::Moving
        | OrbPhase::Processing
        | OrbPhase::SmartProcessing
        | OrbPhase::Submitting => OrbStyle {
            icon: ACCENT_LIGHT,
            ..base
        },
        OrbPhase::Recording => OrbStyle {
            icon: ACCENT_LIGHT,
            border: ACCENT,
            border_alpha: if hovering { 1.0 } else { 0.55 },
            background: if hovering { BG_RECORDING_HOVER } else { BG },
            ..base
        },
        OrbPhase::Busy | OrbPhase::Fallback => OrbStyle {
            icon: WARN,
            border: WARN,
            border_alpha: 0.38,
            ..base
        },
        OrbPhase::Error => OrbStyle {
            icon: ERROR_RGB,
            border: ERROR_RGB,
            border_alpha: 0.42,
            ..base
        },
        OrbPhase::Cancelled => OrbStyle {
            icon: FG,
            icon_alpha: 0.78, // --color-fg-muted
            ..base
        },
        _ => base,
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbHit {
    pub phase: OrbPhase,
    pub transient: bool,
    pub can_submit: bool,
    pub dpi: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbInput {
    Hover(bool),
}

#[derive(Debug, Default)]
pub struct OrbPointer {
    press: Option<(i32, i32)>,
    dragged: bool,
    hovering: bool,
}
impl PointerModel for OrbPointer {
    type Hit = OrbHit;
    type Input = OrbInput;
    type Action = OrbAction;
    fn event(
        &mut self,
        event: PointerEvent,
        hit: &OrbHit,
    ) -> Vec<PointerStep<OrbAction, OrbInput>> {
        let mut steps = Vec::new();
        match event {
            PointerEvent::LeftDown { screen, .. } => {
                self.press = Some(screen);
                self.dragged = false;
            }
            PointerEvent::Move { screen, .. } => {
                if !self.hovering {
                    self.hovering = true;
                    steps.push(PointerStep::Op(WindowOp::TrackMouseLeave));
                    steps.push(PointerStep::Model(OrbInput::Hover(true)));
                }
                if let Some((x, y)) = self.press {
                    if !self.dragged && hit.phase == OrbPhase::Idle && !hit.transient {
                        let dpr = f64::from(hit.dpi) / 96.0;
                        let dpr = if dpr > 0.0 { dpr } else { 1.0 };
                        if should_start_orb_drag(
                            f64::from(screen.0 - x) / dpr,
                            f64::from(screen.1 - y) / dpr,
                        ) {
                            self.dragged = true;
                            steps.push(PointerStep::Op(WindowOp::Action(OrbAction::DragStarted)));
                            steps.push(PointerStep::Op(WindowOp::BeginSystemDrag));
                        }
                    }
                }
            }
            PointerEvent::LeftUp { .. } => {
                let action = if self.dragged || self.press.is_none() {
                    None
                } else {
                    orb_click_action(hit.phase.as_str(), hit.can_submit)
                };
                self.press = None;
                self.dragged = false;
                if let Some(action) = action {
                    // 恢复焦点时转发的点击必须在本帧穿过球，先切穿透再派发异步动作。
                    if action == OrbClickAction::Activate {
                        steps.push(PointerStep::Op(WindowOp::SetInteractive(false)));
                    }
                    steps.push(PointerStep::Op(WindowOp::Action(OrbAction::Click(action))));
                }
            }
            PointerEvent::RightUp { .. } => {
                if let Some(action) =
                    orb_context_action(hit.phase.as_str(), hit.can_submit, hit.transient)
                {
                    steps.push(PointerStep::Op(WindowOp::Action(OrbAction::Context(
                        action,
                    ))));
                }
            }
            PointerEvent::Leave => {
                self.hovering = false;
                steps.push(PointerStep::Model(OrbInput::Hover(false)));
            }
            PointerEvent::CaptureLost => {
                self.press = None;
                self.dragged = false;
            }
        }
        steps
    }
}

pub struct Orb {
    phase: OrbPhase,
    transient: bool,
    can_submit: bool,
    hovering: bool,
    opacity: f32,
    wave_level: f32,
    wave_peaks: Vec<f32>,
    wave_display: [f32; ORB_WAVE_BAR_COUNT],
    style_from: OrbStyle,
    style_target: OrbStyle,
    style_blend_start: Option<Instant>,
    spinner_angle: f32,
    dpi: u32,
    bounds: PxRect,
    visible: bool,
    requested_visible: bool,
    transition: Transition,
    look: Look,
}
impl Orb {
    pub fn new(dpi: u32, look: &Look) -> Self {
        Self {
            phase: OrbPhase::Idle,
            transient: false,
            can_submit: false,
            hovering: false,
            opacity: 1.0,
            wave_level: 0.0,
            wave_peaks: Vec::new(),
            wave_display: [0.0; ORB_WAVE_BAR_COUNT],
            style_from: phase_style(OrbPhase::Idle, false),
            style_target: phase_style(OrbPhase::Idle, false),
            style_blend_start: None,
            spinner_angle: 0.0,
            dpi,
            bounds: PxRect::new(0, 0, DEFAULT_SIZE, DEFAULT_SIZE),
            visible: false,
            requested_visible: false,
            transition: Transition::new(false),
            look: *look,
        }
    }
    pub fn rect(&self) -> PxRect {
        self.bounds
    }
    /// 逻辑可见性在 Hide 时立即关闭，退场绘制仍可继续。
    pub fn is_visible(&self) -> bool {
        self.requested_visible
    }
    pub fn hovering(&self) -> bool {
        self.hovering
    }
    fn color8(&self, rgb: (u8, u8, u8), alpha: f32) -> Color {
        self.look.color(
            f32::from(rgb.0) / 255.0,
            f32::from(rgb.1) / 255.0,
            f32::from(rgb.2) / 255.0,
            alpha,
        )
    }
    fn blended_style(&self, now: Instant) -> OrbStyle {
        let Some(start) = self.style_blend_start else {
            return self.style_target;
        };
        let elapsed = now.saturating_duration_since(start).as_secs_f32();
        let t = (elapsed / STYLE_BLEND_S).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.style_from.lerp(self.style_target, eased)
    }
    fn style_blending(&self, now: Instant) -> bool {
        self.style_blend_start
            .is_some_and(|start| now.saturating_duration_since(start).as_secs_f32() < STYLE_BLEND_S)
    }
}
impl<T: TextEngine> OverlayModel<T> for Orb {
    type Command = Command;
    type Action = OrbAction;
    type Pointer = OrbPointer;
    fn apply(&mut self, command: Command, _: &Cx<'_, T>) -> Ops<OrbAction> {
        match command {
            Command::SetRect { x, y, size } => {
                self.bounds = PxRect::new(x, y, size, size);
                vec![WindowOp::SetBounds(self.bounds), WindowOp::Redraw]
            }
            Command::MoveTo { x, y } => {
                self.bounds.x = x;
                self.bounds.y = y;
                vec![WindowOp::MoveTo { x, y }]
            }
            Command::SetOpacity(value) => {
                self.opacity = value;
                vec![WindowOp::Redraw]
            }
            Command::Show => {
                self.visible = true;
                self.requested_visible = true;
                self.transition.show();
                vec![WindowOp::Redraw, WindowOp::RaiseTopmost]
            }
            Command::Hide => {
                self.requested_visible = false;
                if self.transition.hide() {
                    vec![WindowOp::SetInteractive(false)]
                } else {
                    self.visible = false;
                    vec![WindowOp::Hide]
                }
            }
            Command::SetInteractive(value) => vec![WindowOp::SetInteractive(value)],
            Command::SetState {
                phase,
                transient,
                can_submit,
            } => {
                self.phase = phase;
                self.transient = transient;
                self.can_submit = can_submit;
                vec![WindowOp::Redraw]
            }
            Command::SetWaveform { level, peaks } => {
                self.wave_level = level;
                self.wave_peaks = peaks;
                Vec::new()
            }
        }
    }
    fn input(&mut self, input: OrbInput, _: &Cx<'_, T>) -> Ops<OrbAction> {
        let OrbInput::Hover(value) = input;
        self.hovering = value;
        vec![WindowOp::Redraw]
    }
    fn window_event(&mut self, event: WindowEvent, _: &Cx<'_, T>) -> Ops<OrbAction> {
        match event {
            WindowEvent::Moved { x, y } => {
                self.bounds.x = x;
                self.bounds.y = y;
                Vec::new()
            }
            WindowEvent::SystemDragEnded { bounds } => {
                self.bounds = bounds;
                vec![WindowOp::Action(OrbAction::DragFinished)]
            }
            WindowEvent::DpiChanged { dpi, suggested } => {
                let old_dpi = self.dpi.max(1);
                if dpi == 0 || dpi == old_dpi {
                    return Vec::new();
                }
                self.dpi = dpi;
                let logical = f64::from(self.bounds.width) * 96.0 / f64::from(old_dpi);
                let size = (logical * f64::from(dpi) / 96.0).round().max(1.0) as i32;
                self.bounds = PxRect::new(suggested.x, suggested.y, size, size);
                vec![WindowOp::SetBounds(self.bounds), WindowOp::Redraw]
            }
            WindowEvent::AreaChanged => Vec::new(),
        }
    }
    fn set_look(&mut self, look: &Look) -> Ops<OrbAction> {
        self.look = *look;
        vec![WindowOp::Redraw]
    }
    fn on_frame(&mut self, dt: f32, cx: &Cx<'_, T>) -> Ops<OrbAction> {
        let target = orb_wave_levels(self.wave_level, &self.wave_peaks);
        let k = 1.0 - (-dt / WAVE_SMOOTH_S).exp();
        for (display, target) in self.wave_display.iter_mut().zip(target) {
            *display += (target - *display) * k;
        }
        if !self.style_blending(cx.now) {
            self.style_blend_start = None;
        }
        self.spinner_angle = (self.spinner_angle + SPINNER_STEP_DEG) % 360.0;
        let (_, finished) = self.transition.tick(cx.now);
        let mut ops = Vec::new();
        if finished && self.transition.is_fully_hidden() {
            self.visible = false;
            ops.push(WindowOp::Hide);
        }
        ops.push(WindowOp::Redraw);
        ops
    }
    fn wants_frames(&self, _: Instant) -> bool {
        (self.visible && (self.phase.is_spinner() || self.phase == OrbPhase::Recording))
            || self.transition.is_animating()
            // 到期也要保留结束帧，由 on_frame 清理过渡并重绘最终颜色。
            || self.style_blend_start.is_some()
    }
    fn prepare_frame(&mut self, cx: &Cx<'_, T>) -> Option<FrameSpec> {
        if !self.visible {
            return None;
        }
        if cx.dpi != 0 {
            self.dpi = cx.dpi;
        }
        if self.phase != OrbPhase::Positioning {
            let desired = phase_style(self.phase, self.hovering);
            if desired != self.style_target {
                self.style_from = self.blended_style(cx.now);
                self.style_target = desired;
                self.style_blend_start = Some(cx.now);
            }
        }
        Some(FrameSpec {
            bounds: self.bounds,
            dpi: self.dpi,
            alpha: self.opacity.clamp(0.0, 1.0) * self.transition.visual(),
        })
    }
    fn hit(&self) -> OrbHit {
        OrbHit {
            phase: self.phase,
            transient: self.transient,
            can_submit: self.can_submit,
            dpi: self.dpi,
        }
    }
    fn draw<C: Canvas<Text = T>>(&self, canvas: &mut C, now: Instant) {
        self.draw_face(canvas, now);
    }
}
impl Orb {
    fn draw_face<C: Canvas>(&self, canvas: &mut C, now: Instant) {
        if self.phase == OrbPhase::Positioning {
            return;
        }
        let logical = self.bounds.width as f32 * 96.0 / (self.dpi.max(1) as f32);
        let stroke = (logical * 0.03).clamp(1.25, 2.0);
        let style = self.blended_style(now);
        let center = logical / 2.0;
        let radius = center - stroke / 2.0;
        canvas.fill_circle(
            Point::new(center, center),
            radius,
            self.color8(style.background, 1.0),
        );
        canvas.stroke_circle(
            Point::new(center, center),
            radius,
            self.color8(style.border, style.border_alpha),
            Stroke::plain(stroke),
        );
        let area = (
            (logical - logical * ICON_RATIO) / 2.0,
            (logical - logical * ICON_RATIO) / 2.0,
            logical * ICON_RATIO,
        );
        match self.phase {
            OrbPhase::Recording => self.draw_waveform(canvas, logical),
            phase if phase.is_spinner() => self.draw_spinner(canvas, logical, style),
            OrbPhase::Success => {
                if self.can_submit && self.hovering {
                    self.draw_enter_arrow(canvas, area, style);
                } else {
                    self.draw_glyph(canvas, GLYPH_CHECK, logical, style);
                    if self.can_submit {
                        self.draw_submit_badge(canvas, logical);
                    }
                }
            }
            OrbPhase::Submitted => self.draw_enter_arrow(canvas, area, style),
            OrbPhase::Fallback => self.draw_clipboard(canvas, area, style),
            OrbPhase::Cancelled => {
                self.stroke_polyline(canvas, area, &[(6.0, 6.0), (18.0, 18.0)], style);
                self.stroke_polyline(canvas, area, &[(18.0, 6.0), (6.0, 18.0)], style);
            }
            OrbPhase::Error => self.draw_glyph(canvas, GLYPH_WARNING, logical, style),
            _ => {
                let area = logical * GLYPH_RATIO;
                let offset = (logical - area) / 2.0;
                let scale = area / 24.0;
                canvas.save();
                canvas.concat_transform(Transform {
                    scale,
                    dx: offset,
                    dy: offset,
                });
                canvas.fill_icon(
                    &SvgIcon {
                        key: "orb-mic",
                        d: MIC_SVG_PATH,
                    },
                    self.color8(style.icon, style.icon_alpha),
                );
                canvas.restore();
            }
        }
    }
    fn draw_glyph<C: Canvas>(&self, canvas: &mut C, glyph: u16, logical: f32, style: OrbStyle) {
        let text = char::from_u32(u32::from(glyph)).unwrap().to_string();
        let text_style = TextStyle {
            family: FontFamily::SymbolGlyphs,
            size: logical * GLYPH_RATIO,
            weight: 400,
            align: TextAlign::Center,
            paragraph: ParagraphAlign::Center,
            wrap: true,
            line_spacing: None,
        };
        if let Some(layout) = canvas.text().layout(&text, &text_style, logical, logical) {
            canvas.draw_text(
                &layout,
                Point::new(0.0, 0.0),
                self.color8(style.icon, style.icon_alpha),
                None,
            );
        }
    }
    fn draw_waveform<C: Canvas>(&self, canvas: &mut C, logical: f32) {
        let area = logical
            * if self.hovering {
                WAVE_AREA_RATIO_HOVER
            } else {
                WAVE_AREA_RATIO
            };
        let area_left = (logical - area) / 2.0;
        let area_top = (logical - area) / 2.0;
        let dpr = self.dpi as f32 / 96.0;
        let dpr = if dpr > 0.0 { dpr } else { 1.0 };
        let bar = ((area * dpr * WAVE_BAR_RATIO).round() as i32).max(1) as f32 / dpr;
        let gap = ((area * dpr * WAVE_GAP_RATIO).round() as i32).max(1) as f32 / dpr;
        let total = ORB_WAVE_BAR_COUNT as f32 * bar + (ORB_WAVE_BAR_COUNT - 1) as f32 * gap;
        let start = area_left + ((area * dpr - total * dpr) / 2.0).round() / dpr;
        for (index, level) in self.wave_display.iter().enumerate() {
            let scale = level.max(ORB_WAVE_BAR_MIN_SCALE);
            let mut height = area * ORB_WAVE_BAR_HEIGHTS[index] * scale;
            height = height.max(bar);
            let left = start + index as f32 * (bar + gap);
            let top = area_top + (area - height) / 2.0;
            canvas.fill_rounded_rect(
                Rect::new(left, top, left + bar, top + height),
                bar / 2.0,
                self.color8(ACCENT, 1.0),
            );
        }
    }
    fn draw_spinner<C: Canvas>(&self, canvas: &mut C, logical: f32, style: OrbStyle) {
        let side = logical * ICON_RATIO;
        let stroke = 1.8f32;
        let radius = side / 2.0 - stroke / 2.0;
        let center = logical / 2.0;
        let point =
            |angle: f32| Point::new(center + radius * angle.cos(), center + radius * angle.sin());
        let start = self.spinner_angle.to_radians();
        let sweep = 100.0f32.to_radians();
        canvas.stroke_circle(
            Point::new(center, center),
            radius,
            self.color8(style.icon, 0.2 * style.icon_alpha),
            Stroke::plain(stroke),
        );
        let from = point(start);
        let to = point(start + sweep);
        canvas.stroke_path(
            &[
                SvgSeg::Move(from.x, from.y),
                SvgSeg::Arc {
                    x: to.x,
                    y: to.y,
                    rx: radius,
                    ry: radius,
                    rotation: 0.0,
                    large: false,
                    sweep: true,
                },
            ],
            self.color8(style.icon, style.icon_alpha),
            Stroke::plain(stroke),
        );
    }
    fn stroke_polyline<C: Canvas>(
        &self,
        canvas: &mut C,
        area: (f32, f32, f32),
        points: &[(f32, f32)],
        style: OrbStyle,
    ) {
        let width = 1.8 / 24.0 * area.2;
        for pair in points.windows(2) {
            canvas.line(
                lucide(area, pair[0].0, pair[0].1),
                lucide(area, pair[1].0, pair[1].1),
                self.color8(style.icon, style.icon_alpha),
                Stroke::plain(width),
            );
        }
    }
    fn draw_enter_arrow<C: Canvas>(&self, canvas: &mut C, area: (f32, f32, f32), style: OrbStyle) {
        self.stroke_polyline(
            canvas,
            area,
            &[(20.0, 4.0), (20.0, 11.0), (16.0, 15.0), (4.0, 15.0)],
            style,
        );
        self.stroke_polyline(
            canvas,
            area,
            &[(9.0, 10.0), (4.0, 15.0), (9.0, 20.0)],
            style,
        );
    }
    fn draw_clipboard<C: Canvas>(&self, canvas: &mut C, area: (f32, f32, f32), style: OrbStyle) {
        let width = 1.8 / 24.0 * area.2;
        let tl = lucide(area, 4.0, 5.0);
        let br = lucide(area, 20.0, 22.0);
        let radius = 2.0 / 24.0 * area.2;
        let outer = Rect::new(tl.x, tl.y, br.x, br.y);
        let tl = lucide(area, 8.0, 1.5);
        let br = lucide(area, 16.0, 5.5);
        let tab = Rect::new(tl.x, tl.y, br.x, br.y);
        canvas.fill_rounded_rect(tab, radius / 2.0, self.color8(style.background, 1.0));
        let color = self.color8(style.icon, style.icon_alpha);
        canvas.stroke_rounded_rect(outer, radius, color, Stroke::plain(width));
        canvas.stroke_rounded_rect(tab, radius / 2.0, color, Stroke::plain(width));
    }
    fn draw_submit_badge<C: Canvas>(&self, canvas: &mut C, logical: f32) {
        let radius = logical * 0.17;
        let center = Point::new(
            logical - radius - logical * 0.01,
            logical - radius - logical * 0.01,
        );
        canvas.fill_circle(center, radius, self.color8(ACCENT, 1.0));
        let side = radius * 1.2;
        let area = (center.x - side / 2.0, center.y - side / 2.0, side);
        self.draw_enter_arrow(
            canvas,
            area,
            OrbStyle {
                icon: FG,
                icon_alpha: 1.0,
                border: FG,
                border_alpha: 1.0,
                background: ACCENT,
            },
        );
    }
}
fn lucide(area: (f32, f32, f32), x: f32, y: f32) -> Point {
    Point::new(area.0 + x / 24.0 * area.2, area.1 + y / 24.0 * area.2)
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::testing::{Draw, FakeText, RecordingCanvas};
    use std::time::Duration;

    const PHASES: [OrbPhase; 14] = [
        OrbPhase::Idle,
        OrbPhase::Armed,
        OrbPhase::Moving,
        OrbPhase::Positioning,
        OrbPhase::Recording,
        OrbPhase::Processing,
        OrbPhase::SmartProcessing,
        OrbPhase::Success,
        OrbPhase::Fallback,
        OrbPhase::Error,
        OrbPhase::Cancelled,
        OrbPhase::Busy,
        OrbPhase::Submitting,
        OrbPhase::Submitted,
    ];
    fn hit(phase: OrbPhase, dpi: u32) -> OrbHit {
        OrbHit {
            phase,
            dpi,
            transient: false,
            can_submit: false,
        }
    }
    fn down(pointer: &mut OrbPointer, hit: &OrbHit) {
        assert!(pointer
            .event(
                PointerEvent::LeftDown {
                    client: (9, 8),
                    screen: (100, 200)
                },
                hit
            )
            .is_empty());
    }
    fn move_to(
        pointer: &mut OrbPointer,
        hit: &OrbHit,
        screen: (i32, i32),
    ) -> Vec<PointerStep<OrbAction, OrbInput>> {
        pointer.event(
            PointerEvent::Move {
                client: (-50, 700),
                screen,
            },
            hit,
        )
    }
    fn cx(now: Instant, dpi: u32) -> Cx<'static, FakeText> {
        Cx {
            now,
            dpi,
            area: PxRect::default(),
            text: &FakeText,
        }
    }
    fn state(model: &mut Orb, phase: OrbPhase, cx: &Cx<'_, FakeText>) {
        model.apply(
            Command::SetState {
                phase,
                transient: false,
                can_submit: false,
            },
            cx,
        );
    }
    fn settle(model: &mut Orb, now: Instant) {
        let context = cx(now, 96);
        model.apply(Command::Show, &context);
        model.prepare_frame(&context);
        model.on_frame(0.016, &context);
        model.on_frame(0.320, &cx(now + Duration::from_millis(320), 96));
        model.prepare_frame(&cx(now + Duration::from_millis(320), 96));
    }

    #[test]
    fn phases_roundtrip_and_spinner_membership() {
        for phase in PHASES {
            assert_eq!(OrbPhase::from_str(phase.as_str()), Some(phase));
        }
        assert_eq!(OrbPhase::from_str("unknown"), None);
        assert_eq!(
            PHASES
                .into_iter()
                .filter(|p| p.is_spinner())
                .collect::<Vec<_>>(),
            vec![
                OrbPhase::Positioning,
                OrbPhase::Processing,
                OrbPhase::SmartProcessing,
                OrbPhase::Busy,
                OrbPhase::Submitting
            ]
        );
    }
    #[test]
    fn pointer_hover_tracks_once_and_leave_rearms_tracking() {
        let mut pointer = OrbPointer::default();
        let hit = hit(OrbPhase::Idle, 96);
        let enter = vec![
            PointerStep::Op(WindowOp::TrackMouseLeave),
            PointerStep::Model(OrbInput::Hover(true)),
        ];
        assert_eq!(move_to(&mut pointer, &hit, (0, 0)), enter);
        assert!(move_to(&mut pointer, &hit, (1, 1)).is_empty());
        assert_eq!(
            pointer.event(PointerEvent::Leave, &hit),
            vec![PointerStep::Model(OrbInput::Hover(false))]
        );
        assert_eq!(move_to(&mut pointer, &hit, (2, 2)), enter);
    }
    #[test]
    fn pointer_drag_uses_screen_distance_and_dpi_scaled_five_pixel_threshold() {
        for (dpi, below, threshold) in [
            (0, 4, 5),
            (96, 4, 5),
            (120, 6, 7),
            (144, 7, 8),
            (168, 8, 9),
            (192, 9, 10),
        ] {
            let mut pointer = OrbPointer::default();
            let hit = hit(OrbPhase::Idle, dpi);
            move_to(&mut pointer, &hit, (100, 200));
            down(&mut pointer, &hit);
            assert!(
                move_to(&mut pointer, &hit, (100 + below, 200)).is_empty(),
                "dpi={dpi}"
            );
            assert_eq!(
                move_to(&mut pointer, &hit, (100 + threshold, 200)),
                vec![
                    PointerStep::Op(WindowOp::Action(OrbAction::DragStarted)),
                    PointerStep::Op(WindowOp::BeginSystemDrag)
                ],
                "dpi={dpi}"
            );
            assert!(move_to(&mut pointer, &hit, (500, 600)).is_empty());
            assert!(pointer
                .event(PointerEvent::LeftUp { client: (0, 0) }, &hit)
                .is_empty());
        }
        let mut pointer = OrbPointer::default();
        let hit = hit(OrbPhase::Idle, 96);
        down(&mut pointer, &hit);
        let steps = move_to(&mut pointer, &hit, (103, 204));
        assert_eq!(
            steps,
            vec![
                PointerStep::Op(WindowOp::TrackMouseLeave),
                PointerStep::Model(OrbInput::Hover(true)),
                PointerStep::Op(WindowOp::Action(OrbAction::DragStarted)),
                PointerStep::Op(WindowOp::BeginSystemDrag)
            ]
        );
    }
    #[test]
    fn pointer_drag_only_allows_idle_nontransient() {
        for phase in PHASES {
            for transient in [false, true] {
                let hit = OrbHit {
                    transient,
                    ..hit(phase, 96)
                };
                let mut pointer = OrbPointer::default();
                move_to(&mut pointer, &hit, (0, 0));
                down(&mut pointer, &hit);
                let steps = move_to(&mut pointer, &hit, (500, 500));
                assert_eq!(
                    !steps.is_empty(),
                    phase == OrbPhase::Idle && !transient,
                    "{phase:?}/{transient}"
                );
            }
        }
    }
    #[test]
    fn pointer_release_requires_press_and_capture_lost_clears_it() {
        let hit = hit(OrbPhase::Idle, 96);
        let mut pointer = OrbPointer::default();
        assert!(pointer
            .event(PointerEvent::LeftUp { client: (0, 0) }, &hit)
            .is_empty());
        down(&mut pointer, &hit);
        assert!(pointer.event(PointerEvent::CaptureLost, &hit).is_empty());
        assert!(pointer
            .event(PointerEvent::LeftUp { client: (0, 0) }, &hit)
            .is_empty());
        down(&mut pointer, &hit);
        move_to(&mut pointer, &hit, (110, 200));
        pointer.event(PointerEvent::CaptureLost, &hit);
        assert!(pointer
            .event(PointerEvent::LeftUp { client: (0, 0) }, &hit)
            .is_empty());
        assert!(move_to(&mut pointer, &hit, (600, 600)).is_empty());
    }
    #[test]
    fn pointer_clicks_follow_current_phase_and_activate_cuts_through_before_action() {
        for phase in PHASES {
            for can_submit in [false, true] {
                let hit = OrbHit {
                    can_submit,
                    ..hit(phase, 96)
                };
                let mut pointer = OrbPointer::default();
                down(&mut pointer, &hit);
                // 捕获期间抬起点在球外仍算点击。
                let steps = pointer.event(
                    PointerEvent::LeftUp {
                        client: (-100, -100),
                    },
                    &hit,
                );
                let mut expected = Vec::new();
                if let Some(action) = orb_click_action(phase.as_str(), can_submit) {
                    if action == OrbClickAction::Activate {
                        expected.push(PointerStep::Op(WindowOp::SetInteractive(false)));
                    }
                    expected.push(PointerStep::Op(WindowOp::Action(OrbAction::Click(action))));
                }
                assert_eq!(steps, expected, "{phase:?}/{can_submit}");
                assert!(pointer
                    .event(PointerEvent::LeftUp { client: (0, 0) }, &hit)
                    .is_empty());
            }
        }
        let mut pointer = OrbPointer::default();
        down(&mut pointer, &hit(OrbPhase::Idle, 96));
        assert_eq!(
            pointer.event(
                PointerEvent::LeftUp { client: (0, 0) },
                &hit(OrbPhase::Recording, 96)
            ),
            vec![PointerStep::Op(WindowOp::Action(OrbAction::Click(
                OrbClickAction::Stop
            )))]
        );
    }
    #[test]
    fn pointer_right_click_maps_every_phase_without_requiring_left_press() {
        for phase in PHASES {
            for transient in [false, true] {
                for can_submit in [false, true] {
                    let hit = OrbHit {
                        phase,
                        transient,
                        can_submit,
                        dpi: 96,
                    };
                    let mut pointer = OrbPointer::default();
                    let expected = match phase {
                        OrbPhase::Error => Some(OrbContextAction::DismissError),
                        OrbPhase::Recording => Some(OrbContextAction::Cancel),
                        OrbPhase::Success if can_submit => Some(OrbContextAction::DismissSubmit),
                        OrbPhase::Idle if !transient => Some(OrbContextAction::Menu),
                        _ => None,
                    };
                    assert_eq!(
                        pointer.event(PointerEvent::RightUp { client: (0, 0) }, &hit),
                        expected
                            .into_iter()
                            .map(|a| PointerStep::Op(WindowOp::Action(OrbAction::Context(a))))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    #[test]
    fn model_commands_return_ordered_ops_and_move_does_not_redraw() {
        let now = Instant::now();
        let context = cx(now, 96);
        let mut model = Orb::new(96, &Look::WINDOWS_LEGACY);
        let bounds = PxRect::new(-400, 50, 81, 81);
        assert_eq!(
            model.apply(
                Command::SetRect {
                    x: bounds.x,
                    y: bounds.y,
                    size: 81
                },
                &context
            ),
            vec![WindowOp::SetBounds(bounds), WindowOp::Redraw]
        );
        assert_eq!(model.rect(), bounds);
        assert_eq!(
            model.apply(Command::MoveTo { x: -800, y: 60 }, &context),
            vec![WindowOp::MoveTo { x: -800, y: 60 }]
        );
        assert_eq!(model.rect(), PxRect::new(-800, 60, 81, 81));
        assert!(model.prepare_frame(&context).is_none());
        assert_eq!(
            model.apply(Command::SetOpacity(0.6), &context),
            vec![WindowOp::Redraw]
        );
        assert_eq!(
            model.apply(Command::Show, &context),
            vec![WindowOp::Redraw, WindowOp::RaiseTopmost]
        );
        assert!(model.is_visible());
        assert_eq!(
            model.apply(Command::SetInteractive(false), &context),
            vec![WindowOp::SetInteractive(false)]
        );
        assert!(model
            .apply(
                Command::SetWaveform {
                    level: 0.3,
                    peaks: vec![0.1]
                },
                &context
            )
            .is_empty());
        assert_eq!(model.prepare_frame(&context).unwrap().alpha, 0.0);
    }
    #[test]
    fn dpi_change_preserves_logical_size_and_uses_suggested_origin() {
        for (old, new, size) in [(96, 144, 65), (144, 120, 97), (168, 96, 113), (0, 192, 1)] {
            let mut model = Orb::new(old, &Look::WINDOWS_LEGACY);
            let context = cx(Instant::now(), new);
            model.apply(Command::SetRect { x: 30, y: 40, size }, &context);
            let expected = ((f64::from(size) * 96.0 / f64::from(old.max(1))) * f64::from(new)
                / 96.0)
                .round()
                .max(1.0) as i32;
            let bounds = PxRect::new(-1300, -200, expected, expected);
            assert_eq!(
                model.window_event(
                    WindowEvent::DpiChanged {
                        dpi: new,
                        suggested: PxRect::new(-1300, -200, 999, 888)
                    },
                    &context
                ),
                vec![WindowOp::SetBounds(bounds), WindowOp::Redraw]
            );
            assert_eq!(model.rect(), bounds);
            assert!(model
                .window_event(
                    WindowEvent::DpiChanged {
                        dpi: new,
                        suggested: PxRect::default()
                    },
                    &context
                )
                .is_empty());
            assert!(model
                .window_event(
                    WindowEvent::DpiChanged {
                        dpi: 0,
                        suggested: PxRect::default()
                    },
                    &context
                )
                .is_empty());
        }
    }
    #[test]
    fn moved_and_system_drag_end_update_rect_before_drag_finished() {
        let mut model = Orb::new(96, &Look::WINDOWS_LEGACY);
        let context = cx(Instant::now(), 96);
        assert!(model
            .window_event(WindowEvent::Moved { x: -80, y: -20 }, &context)
            .is_empty());
        assert_eq!(model.rect(), PxRect::new(-80, -20, 64, 64));
        let bounds = PxRect::new(-90, 200, 96, 96);
        assert_eq!(
            model.window_event(WindowEvent::SystemDragEnded { bounds }, &context),
            vec![WindowOp::Action(OrbAction::DragFinished)]
        );
        assert_eq!(model.rect(), bounds);
    }
    #[test]
    fn hidden_phase_and_positioning_defer_style_target_until_visible_prepare() {
        let now = Instant::now();
        let mut model = Orb::new(96, &Look::WINDOWS_LEGACY);
        let context = cx(now, 96);
        state(&mut model, OrbPhase::Error, &context);
        assert!(model.style_target == phase_style(OrbPhase::Idle, false));
        assert!(model.prepare_frame(&context).is_none());
        model.apply(Command::Show, &context);
        assert_eq!(model.style_blend_start, None);
        model.prepare_frame(&context);
        assert!(model.style_target == phase_style(OrbPhase::Error, false));
        assert_eq!(model.style_blend_start, Some(now));
        assert!(model.blended_style(now) == phase_style(OrbPhase::Idle, false));
        let later = now + Duration::from_secs(1);
        state(&mut model, OrbPhase::Positioning, &cx(later, 96));
        model.prepare_frame(&cx(later, 96));
        assert!(model.style_target == phase_style(OrbPhase::Error, false));
        let mut canvas = RecordingCanvas::new(96);
        model.draw(&mut canvas, later);
        assert!(canvas.calls.is_empty());
    }
    #[test]
    fn hide_show_before_first_exit_frame_stays_visible_and_exit_finishes_once() {
        let now = Instant::now();
        let mut model = Orb::new(96, &Look::WINDOWS_LEGACY);
        assert_eq!(
            model.apply(Command::Hide, &cx(now, 96)),
            vec![WindowOp::Hide]
        );
        settle(&mut model, now);
        let later = now + Duration::from_secs(1);
        assert_eq!(
            model.apply(Command::Hide, &cx(later, 96)),
            vec![WindowOp::SetInteractive(false)]
        );
        assert!(!model.is_visible());
        model.apply(Command::Show, &cx(later, 96));
        model.on_frame(0.016, &cx(later, 96));
        assert_eq!(model.prepare_frame(&cx(later, 96)).unwrap().alpha, 1.0);
        assert!(!<Orb as OverlayModel<FakeText>>::wants_frames(
            &model, later
        ));
        model.apply(Command::Hide, &cx(later, 96));
        model.on_frame(0.016, &cx(later, 96));
        assert_eq!(
            model.on_frame(0.140, &cx(later + Duration::from_millis(140), 96)),
            vec![WindowOp::Hide, WindowOp::Redraw]
        );
        assert!(model.prepare_frame(&cx(later, 96)).is_none());
        assert!(!<Orb as OverlayModel<FakeText>>::wants_frames(
            &model, later
        ));
    }
    #[test]
    fn spinner_advances_six_degrees_per_frame_in_every_phase_not_per_dt() {
        let now = Instant::now();
        let mut model = Orb::new(96, &Look::WINDOWS_LEGACY);
        for (index, phase) in PHASES.into_iter().enumerate() {
            state(&mut model, phase, &cx(now, 96));
            model.on_frame(if index % 2 == 0 { 0.001 } else { 10.0 }, &cx(now, 96));
            assert_eq!(model.spinner_angle, (index + 1) as f32 * 6.0);
        }
        model.spinner_angle = 354.0;
        model.on_frame(0.0, &cx(now, 96));
        assert_eq!(model.spinner_angle, 0.0);
    }
    #[test]
    fn recording_wave_smoothing_uses_first_frame_delta_and_hover_redraws() {
        let now = Instant::now();
        let mut model = Orb::new(96, &Look::WINDOWS_LEGACY);
        state(&mut model, OrbPhase::Recording, &cx(now, 96));
        model.apply(Command::Show, &cx(now, 96));
        model.apply(
            Command::SetWaveform {
                level: 1.0,
                peaks: vec![],
            },
            &cx(now, 96),
        );
        model.on_frame(FIRST_FRAME_DT, &cx(now, 96));
        let expected = 1.0 - (-FIRST_FRAME_DT / WAVE_SMOOTH_S).exp();
        assert_eq!(model.wave_display, [expected; 5]);
        assert_eq!(
            model.input(OrbInput::Hover(true), &cx(now, 96)),
            vec![WindowOp::Redraw]
        );
        assert!(model.hovering());
        assert!(<Orb as OverlayModel<FakeText>>::wants_frames(&model, now));
    }
    #[test]
    fn legacy_draw_uses_mic_transform_mdl2_glyphs_and_clipboard_fill_first() {
        let now = Instant::now();
        let mut model = Orb::new(96, &Look::WINDOWS_LEGACY);
        settle(&mut model, now);
        let mut canvas = RecordingCanvas::new(96);
        model.draw(&mut canvas, now);
        assert!(
            matches!(canvas.calls[0],Draw::FillCircle(_,_,color) if color==Look::WINDOWS_LEGACY.color(25.0/255.0,25.0/255.0,25.0/255.0,1.0))
        );
        assert!(
            matches!(canvas.calls[1],Draw::StrokeCircle(_,_,color,_) if color==Look::WINDOWS_LEGACY.color(1.0,1.0,1.0,0.24))
        );
        assert!(matches!(canvas.calls[2], Draw::Save));
        assert!(matches!(canvas.calls[3], Draw::Transform(_)));
        assert!(matches!(canvas.calls[4], Draw::Icon("orb-mic", _, None)));
        assert!(matches!(canvas.calls[5], Draw::Restore));
        for (phase, glyph) in [
            (OrbPhase::Success, "\u{E73E}"),
            (OrbPhase::Error, "\u{E7BA}"),
        ] {
            state(&mut model, phase, &cx(now, 96));
            model.prepare_frame(&cx(now, 96));
            canvas.calls.clear();
            model.draw(&mut canvas, now);
            assert!(matches!(&canvas.calls[2],Draw::Text(text,_,_,None) if text==glyph));
        }
        state(&mut model, OrbPhase::Fallback, &cx(now, 96));
        model.prepare_frame(&cx(now, 96));
        canvas.calls.clear();
        model.draw(&mut canvas, now);
        assert!(matches!(canvas.calls[2], Draw::FillRounded(_, _, _)));
        assert!(matches!(canvas.calls[3], Draw::StrokeRounded(_, _, _, _)));
        assert!(matches!(canvas.calls[4], Draw::StrokeRounded(_, _, _, _)));
    }

    #[test]
    fn expired_hover_transition_keeps_frames_until_final_color_is_drawn() {
        let now = Instant::now();
        let mut model = Orb::new(96, &Look::WINDOWS_LEGACY);
        settle(&mut model, now);
        let start = now + Duration::from_secs(1);
        let mut pointer = OrbPointer::default();
        let steps = pointer.event(
            PointerEvent::Move {
                client: (32, 32),
                screen: (32, 32),
            },
            &<Orb as OverlayModel<FakeText>>::hit(&model),
        );
        for step in steps {
            if let PointerStep::Model(input) = step {
                model.input(input, &cx(start, 96));
            }
        }
        model.prepare_frame(&cx(start, 96));
        let mut clock = FrameClock::new();
        assert_eq!(
            clock.sync(<Orb as OverlayModel<FakeText>>::wants_frames(&model, start)),
            Some(true)
        );
        let middle = start + Duration::from_millis(75);
        model.on_frame(0.075, &cx(middle, 96));
        model.prepare_frame(&cx(middle, 96));
        let mut canvas = RecordingCanvas::new(96);
        model.draw(&mut canvas, middle);
        assert!(
            matches!(canvas.calls[0],Draw::FillCircle(_,_,color) if color!=model.color8(BG_HOVER,1.0))
        );
        let end = start + Duration::from_millis(151);
        // 已悬停的 Move 没有模型输入，宿主仍会查询 wants_frames 来同步定时器。
        assert!(pointer
            .event(
                PointerEvent::Move {
                    client: (33, 32),
                    screen: (33, 32)
                },
                &<Orb as OverlayModel<FakeText>>::hit(&model)
            )
            .is_empty());
        assert_eq!(
            clock.sync(<Orb as OverlayModel<FakeText>>::wants_frames(&model, end)),
            None
        );
        assert!(clock.is_active());
        assert_eq!(model.on_frame(0.076, &cx(end, 96)), vec![WindowOp::Redraw]);
        model.prepare_frame(&cx(end, 96));
        canvas.calls.clear();
        model.draw(&mut canvas, end);
        assert!(
            matches!(canvas.calls[0],Draw::FillCircle(_,_,color) if color==model.color8(BG_HOVER,1.0))
        );
        assert_eq!(model.style_blend_start, None);
        assert_eq!(
            clock.sync(<Orb as OverlayModel<FakeText>>::wants_frames(&model, end)),
            Some(false)
        );
    }
}

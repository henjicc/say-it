//! Windows 原生悬浮球原型。
//!
//! 目标与 native_indicator 相同：悬浮球不再占用常驻 WebView 进程。圆形窗口
//! 画在 UpdateLayeredWindow 分层窗口的透明背景上，按像素 alpha 命中测试天然
//! 获得圆形可点区域；「非交互态点击穿透」用动态切换 WS_EX_TRANSPARENT 实现，
//! 等价于 WebView 路径的 set_ignore_cursor_events。
//!
//! 视觉规格以 ui/src/floating-orb.css 为准；点击/右键动作映射与拖拽阈值
//! 移植自 ui/src/floating-orb/interaction.ts；波形柱布局来自 OrbWaveform.tsx。
//! 分层窗口/D2D 渲染目标/DIB/命令队列/UI 线程等基础设施见 native_overlay.rs。

use std::sync::OnceLock;

/// Windows 上默认启用原生悬浮球；`SAYIT_NATIVE_ORB=0` 回退 WebView。
pub(crate) fn native_orb_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        cfg!(windows)
            && std::env::var("SAYIT_NATIVE_ORB")
                .map(|value| value != "0")
                .unwrap_or(true)
    })
}

/// 拖拽阈值（CSS 像素），与 interaction.ts 的 ORB_DRAG_THRESHOLD 一致。
#[cfg(any(windows, test))]
pub(crate) const ORB_DRAG_THRESHOLD: f64 = 5.0;

/// 移植自 interaction.ts 的 shouldStartOrbDrag。
#[cfg(any(windows, test))]
pub(crate) fn should_start_orb_drag(delta_x: f64, delta_y: f64) -> bool {
    delta_x.hypot(delta_y) >= ORB_DRAG_THRESHOLD
}

/// 左键动作，与 interaction.ts 的 OrbClickAction 一致。
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OrbClickAction {
    Activate,
    Stop,
    Submit,
    ShowError,
}

/// 移植自 interaction.ts 的 floatingOrbClickAction。
#[cfg(any(windows, test))]
pub(crate) fn orb_click_action(phase: &str, can_submit: bool) -> Option<OrbClickAction> {
    match phase {
        "idle" | "armed" => Some(OrbClickAction::Activate),
        "recording" => Some(OrbClickAction::Stop),
        "success" if can_submit => Some(OrbClickAction::Submit),
        "error" => Some(OrbClickAction::ShowError),
        _ => None,
    }
}

/// 右键动作，与 interaction.ts 的 OrbContextAction 一致。
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OrbContextAction {
    Cancel,
    DismissSubmit,
    DismissError,
    Menu,
}

/// 移植自 interaction.ts 的 floatingOrbContextAction。
#[cfg(any(windows, test))]
pub(crate) fn orb_context_action(
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
#[cfg(any(windows, test))]
pub(crate) const ORB_WAVE_BAR_COUNT: usize = 5;

/// 每根波形柱的基础高度占比，与 floating-orb.css 的 --bar-height 对应
/// （第 3 根使用默认的 74%）。
#[cfg(any(windows, test))]
const ORB_WAVE_BAR_HEIGHTS: [f32; ORB_WAVE_BAR_COUNT] = [0.47, 0.64, 0.74, 0.64, 0.47];

/// 波形柱最小缩放，与 OrbWaveform 的 `max(0.18, level)` 一致。
#[cfg(any(windows, test))]
const ORB_WAVE_BAR_MIN_SCALE: f32 = 0.18;

/// 与 floating-orb.tsx 的波形投影一致：peaks 逐个过 floatingOrbWaveScale 后取
/// 最后 WAVE_BAR_COUNT 个，不足的位置用整体 level（同样过曲线）补齐。
#[cfg(any(windows, test))]
pub(crate) fn orb_wave_levels(level: f32, peaks: &[f32]) -> [f32; ORB_WAVE_BAR_COUNT] {
    use super::native_indicator::wave_scale;
    let mut levels_out = [wave_scale(level); ORB_WAVE_BAR_COUNT];
    let take = peaks.len().min(ORB_WAVE_BAR_COUNT);
    for (index, value) in peaks[peaks.len() - take..].iter().enumerate() {
        levels_out[index] = wave_scale(*value);
    }
    levels_out
}

pub(crate) fn native_orb_attach(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        let _ = imp::APP.set(app.clone());
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// 原生悬浮球的 UI 线程是否已经启动（窗口是否已创建）。
pub(crate) fn native_orb_started() -> bool {
    #[cfg(windows)]
    {
        imp::UI.get().is_some_and(Option::is_some)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// 首次创建原生悬浮球窗口并设置矩形与整体透明度（物理像素 / 百分比）。
pub(crate) fn native_orb_configure(x: i32, y: i32, size: i32, opacity_percent: u8) {
    #[cfg(windows)]
    {
        imp::set_last_rect(x, y, size);
        imp::post(imp::Command::SetRect { x, y, size });
        imp::post(imp::Command::SetOpacity(f32::from(opacity_percent) / 100.0));
    }
    #[cfg(not(windows))]
    let _ = (x, y, size, opacity_percent);
}

pub(crate) fn native_orb_set_rect(x: i32, y: i32, size: i32) {
    #[cfg(windows)]
    {
        imp::set_last_rect(x, y, size);
        imp::post_if_started(imp::Command::SetRect { x, y, size });
    }
    #[cfg(not(windows))]
    let _ = (x, y, size);
}

pub(crate) fn native_orb_move_to(x: i32, y: i32) {
    #[cfg(windows)]
    {
        imp::move_last_rect(x, y);
        imp::post_if_started(imp::Command::MoveTo { x, y });
    }
    #[cfg(not(windows))]
    let _ = (x, y);
}

pub(crate) fn native_orb_set_opacity(percent: u8) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetOpacity(f32::from(percent) / 100.0));
    #[cfg(not(windows))]
    let _ = percent;
}

pub(crate) fn native_orb_show() {
    #[cfg(windows)]
    {
        imp::VISIBLE.store(true, std::sync::atomic::Ordering::Release);
        imp::post_if_started(imp::Command::Show);
    }
}

pub(crate) fn native_orb_hide() {
    #[cfg(windows)]
    {
        imp::VISIBLE.store(false, std::sync::atomic::Ordering::Release);
        imp::post_if_started(imp::Command::Hide);
    }
}

/// 等价于 WebView 的 `set_ignore_cursor_events(!interactive)`。
pub(crate) fn native_orb_set_interactive(interactive: bool) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetInteractive(interactive));
    #[cfg(not(windows))]
    let _ = interactive;
}

/// 未知 phase 静默丢弃，与 native_indicator 的状态过滤一致。
pub(crate) fn native_orb_set_state(phase: &str, transient: bool, can_submit: bool) {
    #[cfg(windows)]
    if let Some(phase) = imp::OrbPhase::from_str(phase) {
        imp::post_if_started(imp::Command::SetState {
            phase,
            transient,
            can_submit,
        });
    }
    #[cfg(not(windows))]
    let _ = (phase, transient, can_submit);
}

pub(crate) fn native_orb_set_waveform(level: f32, peaks: Vec<f32>) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetWaveform { level, peaks });
    #[cfg(not(windows))]
    let _ = (level, peaks);
}

/// 最近一次已知的窗口矩形（物理像素 x/y/边长），由命令投递方与拖拽
/// 结束后的窗口过程共同维护，供任意线程同步读取。
pub(crate) fn native_orb_rect() -> Option<(i32, i32, i32)> {
    #[cfg(windows)]
    {
        imp::last_rect()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub(crate) fn native_orb_is_visible() -> bool {
    #[cfg(windows)]
    {
        imp::VISIBLE.load(std::sync::atomic::Ordering::Acquire)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// 与 WebView 路径的 is_cursor_over_floating_orb 同语义：真实指针位置
/// 是否落在当前可见的悬浮球矩形内。
pub(crate) fn native_orb_cursor_over() -> bool {
    #[cfg(windows)]
    {
        imp::cursor_over()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod imp {
    use super::{
        orb_click_action, orb_context_action, orb_wave_levels, should_start_orb_drag,
        OrbClickAction, OrbContextAction, ORB_WAVE_BAR_COUNT, ORB_WAVE_BAR_HEIGHTS,
        ORB_WAVE_BAR_MIN_SCALE,
    };
    use super::super::native_overlay::{
        create_d2d_factory, create_dwrite_factory, create_text_format, rect_f, rgba, window_dpi,
        LayeredSurface, OverlayThread, Transition,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Direct2D::Common::{
        D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D_POINT_2F, D2D_SIZE_F,
    };
    use windows::Win32::Graphics::Direct2D::{
        ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush, D2D1_ARC_SEGMENT,
        D2D1_ARC_SIZE_SMALL, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT,
        D2D1_SWEEP_DIRECTION_CLOCKWISE,
    };
    use windows::Win32::Graphics::DirectWrite::{
        IDWriteFactory, IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowLongPtrW, GetWindowRect, KillTimer,
        LoadCursorW, RegisterClassW, SendMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos,
        ShowWindow, GWLP_USERDATA, GWL_EXSTYLE, HTCAPTION, HWND_TOPMOST, IDC_ARROW,
        SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, WM_CAPTURECHANGED, WM_DESTROY,
        WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOVE, WM_NCLBUTTONDOWN,
        WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };

    // WM_MOUSELEAVE 在 windows crate 里属于 Win32_UI_Controls 特性，为这一个常量
    // 引入整个特性不值得，按 winuser.h 的定义写在这里。
    const WM_MOUSELEAVE_MSG: u32 = 0x02A3;

    const LOG_TAG: &str = "native-orb";
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
    const TIMER_ID: usize = 1;
    const TIMER_MS: u32 = 16; // ~60fps：spinner/波形/颜色过渡/出场动画共用
    /// 相位颜色过渡时长（非高亮 ↔ 高亮不跳变）。
    const STYLE_BLEND_S: f32 = 0.15;
    /// 波形柱平滑时间常数，与 CSS 的 70ms ease-out height 过渡等效。
    const WAVE_SMOOTH_S: f32 = 0.070;

    // Segoe MDL2 Assets 字形。
    const GLYPH_MIC: u16 = 0xE720;
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

    fn color8(rgb: (u8, u8, u8), alpha: f32) -> D2D1_COLOR_F {
        rgba(
            f32::from(rgb.0) / 255.0,
            f32::from(rgb.1) / 255.0,
            f32::from(rgb.2) / 255.0,
            alpha,
        )
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum OrbPhase {
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
        pub(super) fn from_str(phase: &str) -> Option<Self> {
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

        fn as_str(self) -> &'static str {
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
        fn is_spinner(self) -> bool {
            matches!(
                self,
                Self::Positioning | Self::Processing | Self::SmartProcessing | Self::Submitting | Self::Busy
            )
        }
    }

    pub(super) enum Command {
        SetRect { x: i32, y: i32, size: i32 },
        MoveTo { x: i32, y: i32 },
        SetOpacity(f32),
        Show,
        Hide,
        SetInteractive(bool),
        SetState {
            phase: OrbPhase,
            transient: bool,
            can_submit: bool,
        },
        SetWaveform { level: f32, peaks: Vec<f32> },
    }

    pub(super) static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
    pub(super) static UI: OnceLock<Option<OverlayThread<Command>>> = OnceLock::new();
    pub(super) static VISIBLE: AtomicBool = AtomicBool::new(false);
    static LAST_RECT: Mutex<Option<(i32, i32, i32)>> = Mutex::new(None);

    /// configure 走这里：第一次投递顺便启动 UI 线程。
    pub(super) fn post(command: Command) {
        let Some(shared) = UI
            .get_or_init(|| {
                OverlayThread::start("sayit-native-orb", create_window, |hwnd, command| {
                    with_state(hwnd, |state| state.apply(command));
                })
                .ok()
            })
            .as_ref()
        else {
            return;
        };
        shared.post(command);
    }

    /// 其余命令只在窗口已创建时有意义；线程不存在时静默丢弃（窗口本来也不可见）。
    pub(super) fn post_if_started(command: Command) {
        if let Some(Some(shared)) = UI.get() {
            shared.post(command);
        }
    }

    pub(super) fn set_last_rect(x: i32, y: i32, size: i32) {
        if let Ok(mut slot) = LAST_RECT.lock() {
            *slot = Some((x, y, size));
        }
    }

    pub(super) fn move_last_rect(x: i32, y: i32) {
        if let Ok(mut slot) = LAST_RECT.lock() {
            if let Some(rect) = slot.as_mut() {
                rect.0 = x;
                rect.1 = y;
            }
        }
    }

    pub(super) fn last_rect() -> Option<(i32, i32, i32)> {
        LAST_RECT.lock().ok().and_then(|slot| *slot)
    }

    pub(super) fn cursor_over() -> bool {
        if !VISIBLE.load(Ordering::Acquire) {
            return false;
        }
        let Some((x, y, size)) = last_rect() else {
            return false;
        };
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return false;
        }
        cursor.x >= x && cursor.x < x + size && cursor.y >= y && cursor.y < y + size
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
            border_alpha: 0.16, // --color-line-strong
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

    /// 绘制所需的全部输入；与渲染面分离，原因同 native_indicator 的 IndicatorView。
    struct OrbView {
        phase: OrbPhase,
        transient: bool,
        can_submit: bool,
        hovering: bool,
        opacity: f32,
        wave_level: f32,
        wave_peaks: Vec<f32>,
        /// 平滑后的波形柱高（CSS 70ms ease-out height 过渡等效）。
        wave_display: [f32; ORB_WAVE_BAR_COUNT],
        /// 相位/悬停颜色的 150ms 过渡。
        style_from: OrbStyle,
        style_target: OrbStyle,
        style_blend_start: Option<Instant>,
        spinner_angle: f32,
        dpi: u32,
        size_px: i32,
        dwrite: IDWriteFactory,
        icon_format: Option<(f32, IDWriteTextFormat)>,
    }

    struct WindowState {
        hwnd: HWND,
        x: i32,
        y: i32,
        visible: bool,
        timer_active: bool,
        transition: Transition,
        last_frame: Option<Instant>,
        press: Option<(i32, i32)>,
        dragged: bool,
        d2d: ID2D1Factory,
        surface: LayeredSurface,
        view: OrbView,
    }

    impl WindowState {
        fn apply(&mut self, command: Command) {
            match command {
                Command::SetRect { x, y, size } => {
                    let resized = size != self.view.size_px;
                    self.x = x;
                    self.y = y;
                    self.view.size_px = size;
                    unsafe {
                        let _ = SetWindowPos(
                            self.hwnd,
                            HWND_TOPMOST,
                            x,
                            y,
                            size,
                            size,
                            SWP_NOACTIVATE,
                        );
                    }
                    if resized {
                        self.surface.discard_dib();
                        self.view.icon_format = None;
                    }
                    self.render();
                }
                Command::MoveTo { x, y } => {
                    self.x = x;
                    self.y = y;
                    unsafe {
                        let _ = SetWindowPos(
                            self.hwnd,
                            HWND_TOPMOST,
                            x,
                            y,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                }
                Command::SetOpacity(opacity) => {
                    self.view.opacity = opacity;
                    self.render();
                }
                Command::Show => {
                    self.visible = true;
                    // 已完全显示时是空操作，不会重播动画。
                    self.transition.show();
                    self.render();
                    unsafe {
                        let _ = SetWindowPos(
                            self.hwnd,
                            HWND_TOPMOST,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                    self.sync_timer();
                }
                Command::Hide => {
                    // 有可见内容时先播退场动画，真正隐藏推迟到动画结束
                    // （WM_TIMER 里收尾）；期间不再接受点击。
                    if self.transition.hide() {
                        apply_interactive_style(self.hwnd, false);
                    } else {
                        self.visible = false;
                        self.surface.mark_hidden();
                        unsafe {
                            let _ = ShowWindow(self.hwnd, SW_HIDE);
                        }
                    }
                    self.sync_timer();
                }
                Command::SetInteractive(interactive) => {
                    apply_interactive_style(self.hwnd, interactive);
                }
                Command::SetState {
                    phase,
                    transient,
                    can_submit,
                } => {
                    self.view.phase = phase;
                    self.view.transient = transient;
                    self.view.can_submit = can_submit;
                    self.sync_timer();
                    self.render();
                }
                Command::SetWaveform { level, peaks } => {
                    // 波形数据只更新缓存，重绘交给 30fps 定时器。
                    self.view.wave_level = level;
                    self.view.wave_peaks = peaks;
                    self.sync_timer();
                }
            }
        }

        fn sync_timer(&mut self) {
            let want = (self.visible
                && (self.view.phase.is_spinner() || self.view.phase == OrbPhase::Recording))
                || self.transition.is_animating()
                || self.view.style_blending();
            unsafe {
                if want && !self.timer_active {
                    // 停顿后首帧 dt 置零，波形平滑不会因定时器重启跳变。
                    self.last_frame = None;
                    SetTimer(self.hwnd, TIMER_ID, TIMER_MS, None);
                    self.timer_active = true;
                } else if !want && self.timer_active {
                    let _ = KillTimer(self.hwnd, TIMER_ID);
                    self.timer_active = false;
                }
            }
        }

        fn render(&mut self) {
            if !self.visible {
                return;
            }
            let dpi = window_dpi(self.hwnd);
            if dpi != 0 {
                self.view.dpi = dpi;
            }
            // 外观设置的不透明度 × 出场/退场过渡进度。
            let alpha = (self.view.opacity.clamp(0.0, 1.0) * self.transition.visual() * 255.0)
                .round() as u8;
            let Self {
                view,
                d2d,
                surface,
                x,
                y,
                ..
            } = self;
            let size = view.size_px;
            surface.render(d2d, view.dpi, *x, *y, size, size, alpha, LOG_TAG, |target, brush| {
                view.draw(d2d, target, brush);
            });
        }

        /// DPI 变化：保持逻辑尺寸不变，按新 DPI 重新换算物理边长。
        unsafe fn on_dpi_changed(&mut self, dpi: u32, lparam: LPARAM) {
            let old_dpi = self.view.dpi.max(1);
            if dpi == 0 || dpi == old_dpi {
                return;
            }
            self.view.dpi = dpi;
            let logical = f64::from(self.view.size_px) * 96.0 / f64::from(old_dpi);
            let size = (logical * f64::from(dpi) / 96.0).round().max(1.0) as i32;
            self.view.size_px = size;
            let suggested = &*(lparam.0 as *const RECT);
            self.x = suggested.left;
            self.y = suggested.top;
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                self.x,
                self.y,
                size,
                size,
                SWP_NOACTIVATE,
            );
            self.surface.discard_dib();
            self.view.icon_format = None;
            set_last_rect(self.x, self.y, size);
            self.render();
        }
    }

    impl OrbView {
        /// 目标样式变化时开启 150ms 过渡；当前显示样式含过渡中插值。
        fn current_style(&mut self) -> OrbStyle {
            let desired = phase_style(self.phase, self.hovering);
            if desired != self.style_target {
                self.style_from = self.blended_style();
                self.style_target = desired;
                self.style_blend_start = Some(Instant::now());
            }
            self.blended_style()
        }

        fn blended_style(&self) -> OrbStyle {
            let Some(start) = self.style_blend_start else {
                return self.style_target;
            };
            let t = (start.elapsed().as_secs_f32() / STYLE_BLEND_S).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - t).powi(3);
            self.style_from.lerp(self.style_target, eased)
        }

        fn style_blending(&self) -> bool {
            self.style_blend_start
                .is_some_and(|start| start.elapsed().as_secs_f32() < STYLE_BLEND_S)
        }

        /// 每帧推进：波形柱平滑 + 颜色过渡收尾。
        fn tick_animations(&mut self, dt: f32) {
            let target = orb_wave_levels(self.wave_level, &self.wave_peaks);
            let k = 1.0 - (-dt / WAVE_SMOOTH_S).exp();
            for (display, target) in self.wave_display.iter_mut().zip(target) {
                *display += (target - *display) * k;
            }
            if !self.style_blending() {
                self.style_blend_start = None;
            }
        }

        fn draw(
            &mut self,
            d2d: &ID2D1Factory,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
        ) {
            // positioning 态整体透明（CSS: .floating-orb.positioning { opacity: 0 }）。
            if self.phase == OrbPhase::Positioning {
                return;
            }
            let logical = self.size_px as f32 * 96.0 / (self.dpi.max(1) as f32);
            // 描边与 CSS 的 clamp(1.25px, 3vmin, 2px) 一致；vmin 即球的边长。
            let stroke = (logical * 0.03).clamp(1.25, 2.0);
            let style = self.current_style();
            let center = logical / 2.0;
            let radius = center - stroke / 2.0;
            let ellipse = D2D1_ELLIPSE {
                point: D2D_POINT_2F {
                    x: center,
                    y: center,
                },
                radiusX: radius,
                radiusY: radius,
            };
            unsafe {
                brush.SetColor(&color8(style.background, 1.0));
                target.FillEllipse(&ellipse, brush);
                brush.SetColor(&color8(style.border, style.border_alpha));
                target.DrawEllipse(&ellipse, brush, stroke, None);
            }
            let icon_area = (
                (logical - logical * ICON_RATIO) / 2.0,
                (logical - logical * ICON_RATIO) / 2.0,
                logical * ICON_RATIO,
            );
            match self.phase {
                OrbPhase::Recording => self.draw_waveform(target, brush, logical),
                phase if phase.is_spinner() => {
                    self.draw_spinner(d2d, target, brush, logical, style)
                }
                OrbPhase::Success => {
                    if self.can_submit && self.hovering {
                        // 与 CSS 一致：悬停时把对勾换成整幅回车箭头。
                        draw_enter_arrow(target, brush, icon_area, style);
                    } else {
                        self.draw_glyph(target, brush, GLYPH_CHECK, logical, style);
                        if self.can_submit {
                            draw_submit_badge(target, brush, logical);
                        }
                    }
                }
                OrbPhase::Submitted => draw_enter_arrow(target, brush, icon_area, style),
                OrbPhase::Fallback => draw_clipboard(target, brush, icon_area, style),
                OrbPhase::Cancelled => draw_cross(target, brush, icon_area, style),
                OrbPhase::Error => self.draw_glyph(target, brush, GLYPH_WARNING, logical, style),
                // idle/armed/moving 都是麦克风。
                _ => self.draw_glyph(target, brush, GLYPH_MIC, logical, style),
            }
        }

        fn draw_glyph(
            &mut self,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            glyph: u16,
            logical: f32,
            style: OrbStyle,
        ) {
            let size = logical * GLYPH_RATIO;
            if self
                .icon_format
                .as_ref()
                .map_or(true, |(cached, _)| (*cached - size).abs() > 0.5)
            {
                // 系统自带的 Segoe MDL2 Assets；创建失败时保留 None，本帧不画图标。
                self.icon_format =
                    create_text_format(&self.dwrite, "Segoe MDL2 Assets", size, true, true)
                        .ok()
                        .map(|format| (size, format));
            }
            let Some((_, format)) = &self.icon_format else {
                return;
            };
            unsafe {
                brush.SetColor(&color8(style.icon, style.icon_alpha));
                target.DrawText(
                    &[glyph],
                    format,
                    &rect_f(0.0, 0.0, logical, logical),
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }

        fn draw_waveform(
            &self,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            logical: f32,
        ) {
            let area = logical
                * if self.hovering {
                    WAVE_AREA_RATIO_HOVER
                } else {
                    WAVE_AREA_RATIO
                };
            let area_left = (logical - area) / 2.0;
            let area_top = (logical - area) / 2.0;
            // 柱宽/间距按物理像素取整再换回 DIP，与 floatingOrbWaveLayout 的
            // 像素对齐一致，避免不同缩放比例下细柱粗细不一。
            let dpr = self.dpi as f32 / 96.0;
            let dpr = if dpr > 0.0 { dpr } else { 1.0 };
            let bar = ((area * dpr * WAVE_BAR_RATIO).round() as i32).max(1) as f32 / dpr;
            let gap = ((area * dpr * WAVE_GAP_RATIO).round() as i32).max(1) as f32 / dpr;
            let total = ORB_WAVE_BAR_COUNT as f32 * bar + (ORB_WAVE_BAR_COUNT - 1) as f32 * gap;
            let start = area_left + ((area * dpr - total * dpr) / 2.0).round() / dpr;
            // 用 tick_animations 平滑后的柱高（CSS 70ms ease-out 过渡等效）。
            let levels = self.wave_display;
            unsafe {
                brush.SetColor(&color8(ACCENT, 1.0));
                for (index, level) in levels.iter().enumerate() {
                    let scale = level.max(ORB_WAVE_BAR_MIN_SCALE);
                    let mut height = area * ORB_WAVE_BAR_HEIGHTS[index] * scale;
                    // CSS 的 min-height: 柱宽——低响度时收成小圆点。
                    height = height.max(bar);
                    let left = start + index as f32 * (bar + gap);
                    let top = area_top + (area - height) / 2.0;
                    target.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: rect_f(left, top, left + bar, top + height),
                            radiusX: bar / 2.0,
                            radiusY: bar / 2.0,
                        },
                        brush,
                    );
                }
            }
        }

        fn draw_spinner(
            &self,
            d2d: &ID2D1Factory,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            logical: f32,
            style: OrbStyle,
        ) {
            let side = logical * ICON_RATIO;
            let stroke = 1.8f32;
            let radius = side / 2.0 - stroke / 2.0;
            let center = logical / 2.0;
            let point = |angle: f32| D2D_POINT_2F {
                x: center + radius * angle.cos(),
                y: center + radius * angle.sin(),
            };
            let start = self.spinner_angle.to_radians();
            let sweep = 100.0f32.to_radians();
            unsafe {
                // CSS：整环 currentColor 20% 淡色底 + 一段亮色弧旋转。
                brush.SetColor(&color8(style.icon, 0.2 * style.icon_alpha));
                target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: center,
                            y: center,
                        },
                        radiusX: radius,
                        radiusY: radius,
                    },
                    brush,
                    stroke,
                    None,
                );
                let Ok(geometry) = d2d.CreatePathGeometry() else {
                    return;
                };
                let Ok(sink) = geometry.Open() else {
                    return;
                };
                sink.BeginFigure(point(start), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: point(start + sweep),
                    size: D2D_SIZE_F {
                        width: radius,
                        height: radius,
                    },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                let _ = sink.Close();
                drop(sink);
                brush.SetColor(&color8(style.icon, style.icon_alpha));
                target.DrawGeometry(&geometry, brush, stroke, None);
            }
        }
    }

    /// lucide 图标统一 24x24 视图盒；area 为（left, top, side）。
    fn lucide(area: (f32, f32, f32), x: f32, y: f32) -> D2D_POINT_2F {
        D2D_POINT_2F {
            x: area.0 + x / 24.0 * area.2,
            y: area.1 + y / 24.0 * area.2,
        }
    }

    fn stroke_polyline(
        target: &ID2D1DCRenderTarget,
        brush: &ID2D1SolidColorBrush,
        area: (f32, f32, f32),
        points: &[(f32, f32)],
        style: OrbStyle,
    ) {
        let width = 1.8 / 24.0 * area.2;
        unsafe {
            brush.SetColor(&color8(style.icon, style.icon_alpha));
            for segment in points.windows(2) {
                target.DrawLine(
                    lucide(area, segment[0].0, segment[0].1),
                    lucide(area, segment[1].0, segment[1].1),
                    brush,
                    width,
                    None,
                );
            }
        }
    }

    /// lucide CornerDownLeft。
    fn draw_enter_arrow(
        target: &ID2D1DCRenderTarget,
        brush: &ID2D1SolidColorBrush,
        area: (f32, f32, f32),
        style: OrbStyle,
    ) {
        stroke_polyline(
            target,
            brush,
            area,
            &[(20.0, 4.0), (20.0, 11.0), (16.0, 15.0), (4.0, 15.0)],
            style,
        );
        stroke_polyline(
            target,
            brush,
            area,
            &[(9.0, 10.0), (4.0, 15.0), (9.0, 20.0)],
            style,
        );
    }

    /// lucide X。
    fn draw_cross(
        target: &ID2D1DCRenderTarget,
        brush: &ID2D1SolidColorBrush,
        area: (f32, f32, f32),
        style: OrbStyle,
    ) {
        stroke_polyline(target, brush, area, &[(6.0, 6.0), (18.0, 18.0)], style);
        stroke_polyline(target, brush, area, &[(18.0, 6.0), (6.0, 18.0)], style);
    }

    /// lucide Clipboard：外框圆角矩形 + 顶部夹子。
    fn draw_clipboard(
        target: &ID2D1DCRenderTarget,
        brush: &ID2D1SolidColorBrush,
        area: (f32, f32, f32),
        style: OrbStyle,
    ) {
        let width = 1.8 / 24.0 * area.2;
        let top_left = lucide(area, 4.0, 5.0);
        let bottom_right = lucide(area, 20.0, 22.0);
        let radius = 2.0 / 24.0 * area.2;
        let outer = D2D1_ROUNDED_RECT {
            rect: rect_f(top_left.x, top_left.y, bottom_right.x, bottom_right.y),
            radiusX: radius,
            radiusY: radius,
        };
        let tab_tl = lucide(area, 8.0, 1.5);
        let tab_br = lucide(area, 16.0, 5.5);
        let tab = D2D1_ROUNDED_RECT {
            rect: rect_f(tab_tl.x, tab_tl.y, tab_br.x, tab_br.y),
            radiusX: radius / 2.0,
            radiusY: radius / 2.0,
        };
        unsafe {
            // 夹子先用背景色填实，盖住外框顶边。
            brush.SetColor(&color8(style.background, 1.0));
            target.FillRoundedRectangle(&tab, brush);
            brush.SetColor(&color8(style.icon, style.icon_alpha));
            target.DrawRoundedRectangle(&outer, brush, width, None);
            target.DrawRoundedRectangle(&tab, brush, width, None);
        }
    }

    /// success + canSubmit 未悬停时的回车角标：右下角实心小圆 + 回车箭头。
    fn draw_submit_badge(target: &ID2D1DCRenderTarget, brush: &ID2D1SolidColorBrush, logical: f32) {
        let radius = logical * 0.17;
        let center = D2D_POINT_2F {
            x: logical - radius - logical * 0.01,
            y: logical - radius - logical * 0.01,
        };
        unsafe {
            brush.SetColor(&color8(ACCENT, 1.0));
            target.FillEllipse(
                &D2D1_ELLIPSE {
                    point: center,
                    radiusX: radius,
                    radiusY: radius,
                },
                brush,
            );
        }
        let side = radius * 1.2;
        let area = (center.x - side / 2.0, center.y - side / 2.0, side);
        let style = OrbStyle {
            icon: FG,
            icon_alpha: 1.0,
            border: FG,
            border_alpha: 1.0,
            background: ACCENT,
        };
        draw_enter_arrow(target, brush, area, style);
    }

    /// interactive=false 等价于 WebView 的 set_ignore_cursor_events(true)。
    /// 抽成自由函数：窗口过程在点击发生的当帧就要同步切换，不能只走命令队列。
    fn apply_interactive_style(hwnd: HWND, interactive: bool) {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            // 点击穿透等价于 WebView 的 set_ignore_cursor_events(true)。
            let next = if interactive {
                style & !(WS_EX_TRANSPARENT.0 as isize)
            } else {
                style | (WS_EX_TRANSPARENT.0 as isize)
            };
            if next != style {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
        }
    }

    fn run_click_action(hwnd: HWND, action: OrbClickAction) {
        use crate::desktop::floating_orb as orb;
        let Some(app) = APP.get().cloned() else { return };
        match action {
            OrbClickAction::Activate => {
                // 点击被接受的当帧就让球进入穿透态：activate 流程里用于恢复焦点的
                // 转发点击必须穿过悬浮球落到下方的输入窗口；若等异步命令队列切换，
                // 转发点击可能抢在切换前落回球上被吃掉，焦点回不来、文本也就粘贴不上。
                apply_interactive_style(hwnd, false);
                tauri::async_runtime::spawn(async move {
                    if orb::floating_orb_activate(app).await.is_err() {
                        // 激活在早期失败（尚未进入任何相位切换）时恢复可交互，
                        // 否则悬浮球会永远停在穿透态，再也点不中。
                        crate::desktop::native_orb::native_orb_set_interactive(true);
                    }
                });
            }
            OrbClickAction::Stop => {
                tauri::async_runtime::spawn(async move {
                    let _ = orb::floating_orb_stop(app).await;
                });
            }
            OrbClickAction::Submit => {
                tauri::async_runtime::spawn(async move {
                    let _ = orb::floating_orb_submit_enter(app).await;
                });
            }
            // 原型期差异：WebView 里左键 error 会打开错误详情对话框；
            // 原生改为打开主窗口。
            OrbClickAction::ShowError => {
                tauri::async_runtime::spawn(async move {
                    let _ = orb::floating_orb_open_main_window(app).await;
                });
            }
        }
    }

    fn run_context_action(action: OrbContextAction) {
        use crate::desktop::floating_orb as orb;
        let Some(app) = APP.get().cloned() else { return };
        match action {
            OrbContextAction::DismissError => {
                tauri::async_runtime::spawn(async move {
                    let _ = orb::floating_orb_dismiss_error(app).await;
                });
            }
            OrbContextAction::Cancel => {
                tauri::async_runtime::spawn(async move {
                    let _ = orb::floating_orb_cancel(app).await;
                });
            }
            OrbContextAction::DismissSubmit => {
                let _ = orb::floating_orb_dismiss_submit_enter(app);
            }
            // show_floating_orb_menu 是 async 命令（Windows 上窗口操作不能在
            // WebView2 同步 IPC 回调里）；这里从原生 UI 线程调用，放到 blocking 线程池。
            OrbContextAction::Menu => {
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = orb::show_floating_orb_menu(app);
                });
            }
        }
    }

    fn with_state<R>(hwnd: HWND, f: impl FnOnce(&mut WindowState) -> R) -> Option<R> {
        unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if ptr.is_null() {
                None
            } else {
                Some(f(&mut *ptr))
            }
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_TIMER => {
                with_state(hwnd, |state| {
                    let now = Instant::now();
                    let dt = state
                        .last_frame
                        .replace(now)
                        .map(|last| now.saturating_duration_since(last).as_secs_f32())
                        .unwrap_or(TIMER_MS as f32 / 1000.0);
                    state.view.tick_animations(dt);
                    state.view.spinner_angle = (state.view.spinner_angle + SPINNER_STEP_DEG) % 360.0;
                    let (_, finished) = state.transition.tick();
                    if finished && state.transition.is_fully_hidden() {
                        // 退场动画播完才真正隐藏。
                        state.visible = false;
                        state.surface.mark_hidden();
                        let _ = ShowWindow(state.hwnd, SW_HIDE);
                    }
                    state.render();
                    state.sync_timer();
                });
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);
                let start_drag = with_state(hwnd, |state| {
                    if !state.view.hovering {
                        state.view.hovering = true;
                        // 只注册一次离开追踪，离开前不再重复。
                        let mut track = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut track);
                        state.sync_timer();
                        state.render();
                    }
                    let Some((start_x, start_y)) = state.press else {
                        return false;
                    };
                    // 与 WebView 一致：只有 idle 且非瞬时态允许拖拽。
                    if state.dragged
                        || state.view.phase != OrbPhase::Idle
                        || state.view.transient
                    {
                        return false;
                    }
                    // 阈值按 CSS 像素判断（与 WebView 的 screenX 一致）。
                    let dpr = f64::from(state.view.dpi) / 96.0;
                    let dpr = if dpr > 0.0 { dpr } else { 1.0 };
                    should_start_orb_drag(
                        f64::from(cursor.x - start_x) / dpr,
                        f64::from(cursor.y - start_y) / dpr,
                    )
                });
                if start_drag == Some(true) {
                    with_state(hwnd, |state| state.dragged = true);
                    if let Some(app) = APP.get() {
                        crate::desktop::floating_orb::native_orb_drag_started(app);
                    }
                    // 经典做法：转成标题栏按下，让系统接管移动；
                    // SendMessage 会阻塞到拖拽结束（期间窗口过程被重入，
                    // 因此上面不能持有 WindowState 借用）。
                    let _ = ReleaseCapture();
                    SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
                    let mut rect = RECT::default();
                    if GetWindowRect(hwnd, &mut rect).is_ok() {
                        let size = rect.right - rect.left;
                        with_state(hwnd, |state| {
                            state.x = rect.left;
                            state.y = rect.top;
                            state.view.size_px = size;
                            state.press = None;
                            state.dragged = false;
                        });
                        set_last_rect(rect.left, rect.top, size);
                    }
                    if let Some(app) = APP.get() {
                        crate::desktop::floating_orb::native_orb_drag_finished(app);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);
                with_state(hwnd, |state| {
                    state.press = Some((cursor.x, cursor.y));
                    state.dragged = false;
                });
                SetCapture(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let action = with_state(hwnd, |state| {
                    // 拖拽后的抬起不触发点击（与 shouldHandleOrbClick 一致）；
                    // 按下点在球外、抬起才进入球内的也不算点击。
                    let action = if state.dragged || state.press.is_none() {
                        None
                    } else {
                        orb_click_action(state.view.phase.as_str(), state.view.can_submit)
                    };
                    state.press = None;
                    state.dragged = false;
                    action
                });
                let _ = ReleaseCapture();
                if let Some(Some(action)) = action {
                    run_click_action(hwnd, action);
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let action = with_state(hwnd, |state| {
                    orb_context_action(
                        state.view.phase.as_str(),
                        state.view.can_submit,
                        state.view.transient,
                    )
                });
                if let Some(action) = action.flatten() {
                    run_context_action(action);
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE_MSG => {
                with_state(hwnd, |state| {
                    state.view.hovering = false;
                    state.sync_timer();
                    state.render();
                });
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                // 捕获被抢走（含拖拽转系统移动循环）时取消未完成的按下，
                // 避免之后的抬起误触发点击。
                with_state(hwnd, |state| {
                    state.press = None;
                    state.dragged = false;
                });
                LRESULT(0)
            }
            WM_MOVE => {
                // WS_POPUP 没有非客户区，WM_MOVE 的坐标即屏幕坐标。
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                with_state(hwnd, |state| {
                    state.x = x;
                    state.y = y;
                });
                move_last_rect(x, y);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let dpi = (wparam.0 & 0xFFFF) as u32;
                with_state(hwnd, |state| state.on_dpi_changed(dpi, lparam));
                LRESULT(0)
            }
            WM_DESTROY => {
                let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut WindowState;
                if !ptr.is_null() {
                    drop(Box::from_raw(ptr));
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    fn create_window() -> Option<HWND> {
        unsafe {
            let instance = match GetModuleHandleW(None) {
                Ok(instance) => instance,
                Err(error) => {
                    eprintln!("[native-orb] 读取模块句柄失败：{error}");
                    return None;
                }
            };
            let class_name = w!("SayItNativeFloatingOrb");
            // 类光标必须显式给箭头：NULL 时悬停会沿用进入窗口前的光标
            // （常见就是"后台忙碌"转圈），看起来像悬浮球卡住了。
            let arrow_cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                hCursor: arrow_cursor,
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                eprintln!("[native-orb] 注册窗口类失败");
                return None;
            }
            let d2d = match create_d2d_factory() {
                Ok(factory) => factory,
                Err(error) => {
                    eprintln!("[native-orb] {error}");
                    return None;
                }
            };
            let dwrite = match create_dwrite_factory() {
                Ok(factory) => factory,
                Err(error) => {
                    eprintln!("[native-orb] {error}");
                    return None;
                }
            };
            let dpi = GetDpiForSystem();
            // 不加 WS_EX_TRANSPARENT：悬浮球要接收点击；点击穿透按相位动态切换。
            let hwnd = match CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name,
                w!("说吧！语音输入悬浮球"),
                WS_POPUP,
                0,
                0,
                DEFAULT_SIZE,
                DEFAULT_SIZE,
                None,
                None,
                instance,
                None,
            ) {
                Ok(hwnd) => hwnd,
                Err(error) => {
                    eprintln!("[native-orb] 创建悬浮球窗口失败：{error}");
                    return None;
                }
            };
            let state = Box::new(WindowState {
                hwnd,
                x: 0,
                y: 0,
                visible: false,
                timer_active: false,
                transition: Transition::new(false),
                last_frame: None,
                press: None,
                dragged: false,
                d2d,
                surface: LayeredSurface::new(hwnd),
                view: OrbView {
                    phase: OrbPhase::Idle,
                    transient: false,
                    can_submit: false,
                    hovering: false,
                    wave_display: [0.0; ORB_WAVE_BAR_COUNT],
                    style_from: phase_style(OrbPhase::Idle, false),
                    style_target: phase_style(OrbPhase::Idle, false),
                    style_blend_start: None,
                    opacity: 1.0,
                    wave_level: 0.0,
                    wave_peaks: Vec::new(),
                    spinner_angle: 0.0,
                    dpi,
                    size_px: DEFAULT_SIZE,
                    dwrite,
                    icon_format: None,
                },
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            Some(hwnd)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_action_matches_interaction_ts() {
        assert_eq!(orb_click_action("idle", false), Some(OrbClickAction::Activate));
        assert_eq!(orb_click_action("armed", false), Some(OrbClickAction::Activate));
        assert_eq!(orb_click_action("recording", false), Some(OrbClickAction::Stop));
        assert_eq!(orb_click_action("success", true), Some(OrbClickAction::Submit));
        assert_eq!(orb_click_action("success", false), None);
        assert_eq!(orb_click_action("error", false), Some(OrbClickAction::ShowError));
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
        use super::super::native_indicator::wave_scale;
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

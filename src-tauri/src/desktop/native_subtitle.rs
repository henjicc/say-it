//! Windows 原生实时字幕条。
//!
//! 复刻 indicator.css 的 subtitle-mode 与 IndicatorApp.tsx 的 useTextTrack：
//! 原文/译文两条独立文本轨道（stable 前缀 + ≤10 字符新增淡入 + 滚动/替换定位）、
//! 悬停控制条（锁定/重置/关闭）、未锁定时按住拖拽（位置不持久化，重置回到
//! 配置位）。窗口可交互（不加 WS_EX_TRANSPARENT，与原生听写胶囊相反）。
//!
//! 分层窗口/D2D 渲染目标/DIB/命令队列/UI 线程等基础设施见 native_overlay.rs；
//! 与原生胶囊之间的「当前谁拥有指示窗通道」路由见 indicator.rs。

use std::sync::OnceLock;

/// Windows 上默认启用原生字幕条；`SAYIT_NATIVE_SUBTITLE=0` 回退 WebView。
pub(crate) fn native_subtitle_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        cfg!(windows)
            && std::env::var("SAYIT_NATIVE_SUBTITLE")
                .map(|value| value != "0")
                .unwrap_or(true)
    })
}

/// 字幕配置，与 sync_presentation 发出的 dictation-indicator-config.subtitle 同构。
/// 字段兜底值与 IndicatorApp.tsx 的默认值一致。
#[cfg(any(windows, test))]
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SubtitleConfig {
    pub display_mode: String,
    pub font_family: String,
    pub font_size: f64,
    pub line_count: u32,
    pub text_color: String,
    pub background_color: String,
    pub rounded: f64,
    pub width: f64,
    pub window_width: f64,
    pub window_height: f64,
    pub anchor: String,
    pub offset_y: f64,
    pub motion_enabled: bool,
    pub motion_duration_ms: u32,
    pub motion_easing: String,
    pub fade_enabled: bool,
    pub fade_duration_ms: u32,
    pub fade_easing: String,
    pub translation_enabled: bool,
    pub translation_layout: String,
    pub translation_order: String,
}

#[cfg(any(windows, test))]
impl Default for SubtitleConfig {
    fn default() -> Self {
        Self {
            display_mode: "scroll".into(),
            font_family: "Microsoft YaHei".into(),
            font_size: 28.0,
            line_count: 2,
            text_color: "#fff".into(),
            background_color: "rgba(5, 7, 10, 0.72)".into(),
            rounded: 18.0,
            width: 880.0,
            window_width: 880.0,
            window_height: 134.0,
            anchor: "bottom".into(),
            offset_y: 36.0,
            motion_enabled: true,
            motion_duration_ms: 120,
            motion_easing: "ease-out".into(),
            fade_enabled: true,
            fade_duration_ms: 180,
            fade_easing: "ease-out".into(),
            translation_enabled: false,
            translation_layout: "bilingual".into(),
            translation_order: "translationFirst".into(),
        }
    }
}

/// CSS 只给 ≤10 字符的短追加挂 .fresh 淡入；长追加直接显示。
#[cfg(any(windows, test))]
pub(crate) const FRESH_FADE_MAX_CHARS: usize = 10;

/// 渲染文本上限，与 IndicatorApp.tsx 的 MAX_RENDER_CHARS 一致。
#[cfg(any(windows, test))]
pub(crate) const MAX_RENDER_CHARS: usize = 20000;

/// useTextTrack 的末端重叠搜索范围（OVERLAP_SEARCH_MAX）。
#[cfg(any(windows, test))]
const OVERLAP_SEARCH_MAX: usize = 200;

/// 解析 #rgb / #rrggbb / rgb(r,g,b) / rgba(r,g,b,a)（a 为 0..1 浮点）。
#[cfg(any(windows, test))]
pub(crate) fn parse_css_color(value: &str) -> Option<(u8, u8, u8, f32)> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let (r, g, b) = match hex.len() {
            3 => (
                u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?,
                u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?,
                u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?,
            ),
            6 => (
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            ),
            _ => return None,
        };
        return Some((r, g, b, 1.0));
    }
    let (rgba_mode, inner) = if let Some(inner) = value
        .strip_prefix("rgba(")
        .and_then(|v| v.strip_suffix(')'))
    {
        (true, inner)
    } else if let Some(inner) = value.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        (false, inner)
    } else {
        return None;
    };
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() < 3 || (rgba_mode && parts.len() < 4) {
        return None;
    }
    let channel = |index: usize| parts.get(index)?.trim().parse::<f64>().ok();
    let alpha = if rgba_mode {
        channel(3)? as f32
    } else {
        1.0
    };
    Some((
        channel(0)?.round().clamp(0.0, 255.0) as u8,
        channel(1)?.round().clamp(0.0, 255.0) as u8,
        channel(2)?.round().clamp(0.0, 255.0) as u8,
        alpha.clamp(0.0, 1.0),
    ))
}

/// useTextTrack 的 stable 前缀 + 末端重叠搜索。JS 按 UTF-16 码元切片；
/// 这里按 char 遍历，BMP 文本完全一致，astral 字符不会落到半个码元上。
/// 返回值是 next 中「稳定前缀」的字符数。
#[cfg(any(windows, test))]
pub(crate) fn overlap_chars(prev: &str, next: &str) -> usize {
    let prev: Vec<char> = prev.chars().collect();
    let next: Vec<char> = next.chars().collect();
    let min = prev.len().min(next.len());
    let mut prefix = 0;
    while prefix < min && prev[prefix] == next[prefix] {
        prefix += 1;
    }
    let mut overlap = prefix;
    let floor = prefix.max(min.saturating_sub(OVERLAP_SEARCH_MAX));
    let mut k = min;
    while k > floor {
        // prev.endsWith(next[..k])
        if prev[prev.len() - k..] == next[..k] {
            overlap = k;
            break;
        }
        k -= 1;
    }
    overlap
}

/// nextText.slice(-MAX_RENDER_CHARS).replace(/^\s+/, "") 的 char 安全版本。
#[cfg(any(windows, test))]
pub(crate) fn trim_render_text(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_RENDER_CHARS {
        return text.to_string();
    }
    text.chars()
        .skip(count - MAX_RENDER_CHARS)
        .collect::<String>()
        .trim_start()
        .to_string()
}

/// 滚动累积模式：内容底对齐，超高部分整体上移（translateY(-overflow)）。
#[cfg(any(windows, test))]
pub(crate) fn scroll_offset_y(content_height: f32, flow_height: f32) -> f32 {
    (content_height - flow_height).max(0.0)
}

/// 单句替换模式：内容不宽于框时居中，超宽时贴右显示最新内容（translateX 左移）。
#[cfg(any(windows, test))]
pub(crate) fn replace_offset_x(content_width: f32, flow_width: f32) -> f32 {
    let overflow = content_width - flow_width;
    if overflow > 0.0 {
        -overflow
    } else {
        (flow_width - content_width) / 2.0
    }
}

/// CSS ease-* 的三次贝塞尔曲线；按时间 x 反解曲线参数后再取 y。
#[cfg(any(windows, test))]
pub(crate) fn ease_value(name: &str, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if name == "linear" || t == 0.0 || t == 1.0 {
        return t;
    }
    let (x1, x2) = match name {
        "ease-in" => (0.42, 1.0),
        "ease-in-out" => (0.42, 0.58),
        _ => (0.0, 0.58),
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..20 {
        let u = (low + high) * 0.5;
        let x = 3.0 * (1.0 - u) * (1.0 - u) * u * x1 + 3.0 * (1.0 - u) * u * u * x2 + u * u * u;
        if x < t {
            low = u;
        } else {
            high = u;
        }
    }
    let u = (low + high) * 0.5;
    3.0 * (1.0 - u) * u * u + u * u * u
}

/// 行高，与 IndicatorApp 的 Math.round(fontSize * 1.38) 一致。
#[cfg(any(windows, test))]
pub(crate) fn line_height(font_size: f64) -> f32 {
    (font_size * 1.38).round() as f32
}

/// 内容块高度：原文 lineHeight*lines+28、译文 +20（indicator.css 的 padding 差）。
#[cfg(any(windows, test))]
pub(crate) fn block_height(line_height: f32, lines: u32, translation: bool) -> f32 {
    line_height * lines.max(1) as f32 + if translation { 20.0 } else { 28.0 }
}

/// 底部对齐堆叠（#wrap justify-end，gap 10）：输入自上而下各块高度，
/// 返回各块顶部 y。
#[cfg(any(windows, test))]
pub(crate) fn stack_from_bottom(window_height: f32, heights: &[f32], gap: f32) -> Vec<f32> {
    let total: f32 =
        heights.iter().sum::<f32>() + gap * heights.len().saturating_sub(1) as f32;
    let mut y = window_height - total;
    heights
        .iter()
        .map(|height| {
            let top = y;
            y += height + gap;
            top
        })
        .collect()
}

/// 窗口定位，与 indicator.rs fallback_indicator_position 同一数学：
/// 水平居中，anchor 决定垂直位置，margin 为 offsetY（物理像素）。
#[cfg(any(windows, test))]
pub(crate) fn window_position(
    area_x: i32,
    area_y: i32,
    area_w: i32,
    area_h: i32,
    win_w: i32,
    win_h: i32,
    anchor: &str,
    margin: i32,
) -> (i32, i32) {
    let x = area_x + (area_w - win_w) / 2;
    let y = match anchor {
        "top" => area_y + margin,
        "center" => area_y + (area_h - win_h) / 2 + margin,
        _ => area_y + area_h - win_h - margin,
    };
    (x, y)
}

pub(crate) fn native_subtitle_attach(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        let _ = imp::APP.set(app.clone());
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// 送字幕配置（dictation-indicator-config.subtitle 的 json 结构）。
pub(crate) fn native_subtitle_set_config(config: serde_json::Value) {
    #[cfg(windows)]
    if let Ok(config) = serde_json::from_value::<SubtitleConfig>(config) {
        imp::post(imp::Command::SetConfig(Box::new(config)));
    }
    #[cfg(not(windows))]
    let _ = config;
}

/// 窗口几何（逻辑像素）：尺寸 + 锚点。调用方已完成取值裁剪（见 set_indicator_layout）。
pub(crate) fn native_subtitle_set_layout(width: f64, height: f64, anchor: &str, offset_y: f64) {
    #[cfg(windows)]
    imp::post(imp::Command::SetLayout {
        width: width as f32,
        height: height as f32,
        anchor: anchor.to_string(),
        offset_y: offset_y as f32,
    });
    #[cfg(not(windows))]
    let _ = (width, height, anchor, offset_y);
}

pub(crate) fn native_subtitle_show() {
    #[cfg(windows)]
    imp::post(imp::Command::Show);
}

pub(crate) fn native_subtitle_hide() {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::Hide);
}

pub(crate) fn native_subtitle_set_text(text: String, fade: bool) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetText { text, fade });
    #[cfg(not(windows))]
    let _ = (text, fade);
}

pub(crate) fn native_subtitle_set_translation(text: String) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetTranslation(text));
    #[cfg(not(windows))]
    let _ = text;
}

#[cfg(windows)]
mod imp {
    use super::super::native_overlay::{
        create_d2d_factory, create_dc_render_target, create_dwrite_factory, create_text_format_weight, rect_f,
        svg_path_geometry_stroke, window_dpi, Dib, LayeredSurface, OverlayThread, Transition,
    };
    use super::{
        block_height, ease_value, line_height, overlap_chars, parse_css_color, replace_offset_x,
        scroll_offset_y, stack_from_bottom, trim_render_text, window_position, SubtitleConfig,
        FRESH_FADE_MAX_CHARS,
    };
    use crate::desktop::native_orb::should_start_orb_drag;
    use std::sync::OnceLock;
    use std::time::Instant;
    use windows::core::w;
    use windows::Foundation::Numerics::Matrix3x2;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_U};
    use windows::Win32::Graphics::Direct2D::{
        ID2D1DCRenderTarget, ID2D1Factory, ID2D1PathGeometry, ID2D1SolidColorBrush,
        ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_CAP_STYLE_ROUND,
        D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_LINE_JOIN_ROUND,
        D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, ID2D1Bitmap,
        D2D1_BITMAP_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    };
    use windows::Win32::Graphics::DirectWrite::{
        IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_LINE_SPACING_METHOD_UNIFORM, DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
        DWRITE_WORD_WRAPPING_NO_WRAP,
    };
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowLongPtrW, KillTimer, LoadCursorW,
        RegisterClassW, SendMessageW, SetCursor, SetTimer, SetWindowLongPtrW, SetWindowPos,
        ShowWindow, GWLP_USERDATA, HCURSOR, HTCAPTION, HTCLIENT, HWND_TOPMOST, IDC_ARROW,
        IDC_SIZEALL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, WM_DESTROY, WM_DPICHANGED,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOVE, WM_NCLBUTTONDOWN, WM_SETCURSOR,
        WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    };

    // WM_MOUSELEAVE 在 windows crate 里属于 Win32_UI_Controls 特性，为这一个常量
    // 引入整个特性不值得，按 winuser.h 的定义写在这里。
    const WM_MOUSELEAVE_MSG: u32 = 0x02A3;

    const LOG_TAG: &str = "native-subtitle";
    const FRAME_TIMER_ID: usize = 1;
    const FRAME_MS: u32 = 16; // ~60fps：淡入/位移/控制条/过渡动画共用
    const SWAP_IN_S: f32 = 0.32; // indicator.css 的 swapIn
    const CONTROLS_FADE_S: f32 = 0.12; // .subtitle-controls 的 opacity 过渡
    /// UNIFORM 行距下基线在行框中的位置（贴近 CSS line-height 的居中效果）。
    const BASELINE_RATIO: f32 = 0.8;

    const BLOCK_GAP: f32 = 10.0;
    const PAD_X: f32 = 22.0;
    const PAD_TOP_MAIN: f32 = 14.0;
    const PAD_TOP_TRANSLATION: f32 = 10.0;
    /// 控制条：top 5 right 8，3 个 22px 按钮，gap 8，16px 图标。
    const CONTROL_BUTTON: f32 = 22.0;
    const CONTROL_GAP: f32 = 8.0;
    const CONTROL_TOP: f32 = 5.0;
    const CONTROL_RIGHT: f32 = 8.0;
    const ICON_SIZE: f32 = 16.0;
    const ICON_STROKE: f32 = 1.45;
    const CONTROL_COUNT: usize = 3;
    const BORDER: f32 = 1.0;

    // D2D 画刷输入使用 straight alpha；仅 DIB/位图存储使用预乘 alpha。
    fn brush_color(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r, g, b, a }
    }

    /// 出场/退场位移幅度，与 indicator.css 的 wrapIn（translateY(10px)）一致。
    const TRANSITION_SLIDE_PX: f32 = 10.0;

    // lucide 图标路径（24x24 视图盒，stroke 风格）。
    const ICON_LOCK_SHACKLE: &str = "M7 11V7a5 5 0 0 1 10 0v4";
    const ICON_UNLOCK_SHACKLE: &str = "M7 11V7a5 5 0 0 1 9.9-1";
    const ICON_ROTATE: &str = "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8M21 3v5h-5";
    const ICON_CLOSE: &str = "M18 6L6 18M6 6L18 18";

    pub(super) enum Command {
        SetConfig(Box<SubtitleConfig>),
        SetLayout {
            width: f32,
            height: f32,
            anchor: String,
            offset_y: f32,
        },
        SetText {
            text: String,
            fade: bool,
        },
        SetTranslation(String),
        Show,
        Hide,
    }

    pub(super) static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
    static UI: OnceLock<Option<OverlayThread<Command>>> = OnceLock::new();

    /// 配置/布局/显示命令携带建窗所需信息，首次投递顺便启动 UI 线程。
    pub(super) fn post(command: Command) {
        let Some(shared) = UI
            .get_or_init(|| {
                OverlayThread::start("sayit-native-subtitle", create_window, |hwnd, command| {
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

    /// 文本/隐藏命令只在窗口已创建时有意义（字幕没开过就没什么可清可藏的）。
    pub(super) fn post_if_started(command: Command) {
        if let Some(Some(shared)) = UI.get() {
            shared.post(command);
        }
    }

    /// 一路文本轨道（useTextTrack 移植）：原文/译文各一份。
    struct TextTrack {
        displayed: String,
        layout: Option<IDWriteTextLayout>,
        content_size: (f32, f32),
        /// 「新增内容」的起始字符索引；usize::MAX 表示本轮没有新增区间。
        fresh_from: usize,
        fresh_started: Option<Instant>,
        /// swapText（fade=true）：整段换新并播 swapIn。
        swap_started: Option<Instant>,
        offset: f32,
        offset_from: f32,
        offset_target: f32,
        offset_anim: Option<Instant>,
    }

    impl Default for TextTrack {
        fn default() -> Self {
            Self {
                displayed: String::new(),
                layout: None,
                content_size: (0.0, 0.0),
                fresh_from: usize::MAX,
                fresh_started: None,
                swap_started: None,
                offset: 0.0,
                offset_from: 0.0,
                offset_target: 0.0,
                offset_anim: None,
            }
        }
    }

    impl TextTrack {
        fn reset(&mut self) {
            *self = Self::default();
        }

        fn has_text(&self) -> bool {
            !self.displayed.is_empty()
        }

        fn animating(&self) -> bool {
            self.fresh_started.is_some() || self.swap_started.is_some() || self.offset_anim.is_some()
        }
    }

    struct Icons {
        lock_shackle: Option<ID2D1PathGeometry>,
        unlock_shackle: Option<ID2D1PathGeometry>,
        rotate: Option<ID2D1PathGeometry>,
        close: Option<ID2D1PathGeometry>,
        stroke_style: Option<ID2D1StrokeStyle>,
    }

    /// 绘制所需的全部输入；与渲染面（LayeredSurface）分离，原因同 native_indicator。
    struct SubtitleView {
        config: SubtitleConfig,
        // SetLayout 是窗口几何的权威；config 里的同名字段供「重置位置」恢复。
        layout_w: f32,
        layout_h: f32,
        anchor: String,
        offset_y: f32,
        locked: bool,
        tracks: [TextTrack; 2],
        format: Option<(String, f32, IDWriteTextFormat)>,
        hovering_block: bool,
        hover_button: Option<usize>,
        hover_tracking: bool,
        controls_alpha: f32,
        controls_anim: Option<(Instant, f32, f32)>,
        dpi: u32,
        dwrite: IDWriteFactory,
        icons: Icons,
    }

    struct WindowState {
        hwnd: HWND,
        x: i32,
        y: i32,
        size: (i32, i32),
        visible: bool,
        dragging: bool,
        press: Option<PressState>,
        view: SubtitleView,
        d2d: ID2D1Factory,
        surface: LayeredSurface,
        transition: Transition,
        timer_active: bool,
        last_frame: Option<Instant>,
        cursor_arrow: HCURSOR,
        cursor_move: HCURSOR,
    }

    #[derive(Clone, Copy)]
    struct PressState {
        screen: (i32, i32),
        on_button: Option<usize>,
        draggable: bool,
    }

    impl WindowState {
        fn apply(&mut self, command: Command) {
            match command {
                Command::SetConfig(config) => {
                    let font_changed = self.view.config.font_family != config.font_family
                        || self.view.config.font_size != config.font_size;
                    let mode_changed = self.view.config.display_mode != config.display_mode;
                    let flow_changed = self.view.config.width != config.width
                        || self.view.config.line_count != config.line_count;
                    self.view.config = *config;
                    if font_changed {
                        self.view.format = None;
                    }
                    if font_changed || mode_changed || flow_changed {
                        self.view.reflow_tracks();
                    }
                    self.render();
                }
                Command::SetLayout {
                    width,
                    height,
                    anchor,
                    offset_y,
                } => {
                    let width_changed = self.view.layout_w != width;
                    self.view.layout_w = width;
                    self.view.layout_h = height;
                    self.view.anchor = anchor;
                    self.view.offset_y = offset_y;
                    if width_changed {
                        self.view.reflow_tracks();
                    }
                    self.apply_configured_placement();
                }
                Command::SetText { text, fade } => {
                    self.view.set_track_text(0, &text, fade);
                    self.render();
                }
                Command::SetTranslation(text) => {
                    self.view.set_track_text(1, &text, false);
                    self.render();
                }
                Command::Show => {
                    self.visible = true;
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
                }
                Command::Hide => {
                    self.begin_exit();
                }
            }
            // 样式切换类动画的定时器状态必须在 render 之后同步（先渲染出新态，
            // 再决定下一帧还要不要跑）。
            self.sync_timer();
        }

        /// 把窗口摆回「配置位」（anchor + offsetY），SetLayout 与重置按钮共用。
        fn apply_configured_placement(&mut self) {
            let (x, y, width, height) = configured_placement(
                self.view.dpi,
                self.view.layout_w,
                self.view.layout_h,
                &self.view.anchor,
                self.view.offset_y,
            );
            if (width, height) != self.size {
                self.size = (width, height);
                self.surface.discard_dib();
            }
            self.x = x;
            self.y = y;
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    x,
                    y,
                    width,
                    height,
                    SWP_NOACTIVATE,
                );
            }
            self.render();
        }

        fn begin_exit(&mut self) {
            if !self.transition.hide() {
                self.finish_hide();
            }
        }

        /// 退场动画播完（或本就没有可见内容）才真正隐藏并清空轨道，
        /// 与 IndicatorApp 在 hidden 时 resetText 一致。
        fn finish_hide(&mut self) {
            self.visible = false;
            self.view.tracks[0].reset();
            self.view.tracks[1].reset();
            self.view.hovering_block = false;
            self.view.hover_button = None;
            self.view.controls_alpha = 0.0;
            self.view.controls_anim = None;
            self.surface.mark_hidden();
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }

        fn sync_timer(&mut self) {
            let want = self.visible
                && (self.transition.is_animating()
                    || self.view.tracks[0].animating()
                    || self.view.tracks[1].animating()
                    || self.view.controls_anim.is_some());
            unsafe {
                if want && !self.timer_active {
                    // 停顿后首帧 dt 置零，动画不会因定时器重启跳变。
                    self.last_frame = None;
                    SetTimer(self.hwnd, FRAME_TIMER_ID, FRAME_MS, None);
                    self.timer_active = true;
                } else if !want && self.timer_active {
                    let _ = KillTimer(self.hwnd, FRAME_TIMER_ID);
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
            self.view.prepare_layouts();
            let visual = self.transition.visual();
            let scale = if self.view.dpi == 0 {
                1.0
            } else {
                self.view.dpi as f32 / 96.0
            };
            let slide = ((1.0 - visual) * TRANSITION_SLIDE_PX * scale).round() as i32;
            let alpha = (visual * 255.0).round() as u8;
            let (width, height) = self.size;
            let Self {
                view,
                d2d,
                surface,
                x,
                y,
                ..
            } = self;
            surface.render(
                d2d,
                view.dpi,
                *x,
                *y + slide,
                width,
                height,
                alpha,
                LOG_TAG,
                |target, brush| view.draw(target, brush),
            );
        }

        fn tick(&mut self) {
            let now = Instant::now();
            let dt = self
                .last_frame
                .replace(now)
                .map(|last| now.saturating_duration_since(last).as_secs_f32())
                .unwrap_or(FRAME_MS as f32 / 1000.0);
            self.view.tick(dt);
            let (_, finished) = self.transition.tick();
            if finished && self.transition.is_fully_hidden() {
                self.finish_hide();
            }
            self.render();
            self.sync_timer();
        }
    }

    impl SubtitleView {
        fn line_height(&self) -> f32 {
            line_height(self.config.font_size)
        }

        fn block_width(&self) -> f32 {
            // width: min(config.width, 窗口宽)
            (self.config.width as f32).min(self.layout_w)
        }

        fn flow_width(&self) -> f32 {
            (self.block_width() - (PAD_X + BORDER) * 2.0).max(1.0)
        }

        fn flow_height(&self) -> f32 {
            self.line_height() * self.config.line_count.max(1) as f32
        }

        fn is_replace(&self) -> bool {
            self.config.display_mode == "replace"
        }

        fn show_translation(&self) -> bool {
            // #translation-text.empty { display: none }：译文块只在双语且有内容时出现。
            self.config.translation_enabled
                && self.config.translation_layout == "bilingual"
                && self.tracks[1].has_text()
        }

        /// 当前应显示的块（轨道索引 + 矩形），自上而下；底部对齐、gap 10。
        fn blocks(&self) -> Vec<(usize, D2D_RECT_F)> {
            let line_h = self.line_height();
            let lines = self.config.line_count.max(1);
            let translation_first = self.config.translation_order == "translationFirst";
            let show_translation = self.show_translation();
            let mut order: Vec<(usize, f32)> = Vec::with_capacity(2);
            let original = (0usize, block_height(line_h, lines, false));
            let translation = (1usize, block_height(line_h, lines, true));
            if translation_first {
                if show_translation {
                    order.push(translation);
                }
                order.push(original);
            } else {
                order.push(original);
                if show_translation {
                    order.push(translation);
                }
            }
            let heights: Vec<f32> = order.iter().map(|(_, h)| *h).collect();
            let tops = stack_from_bottom(self.layout_h, &heights, BLOCK_GAP);
            let width = self.block_width();
            let left = (self.layout_w - width) / 2.0;
            order
                .iter()
                .zip(tops)
                .map(|((track, height), top)| {
                    (*track, rect_f(left, top, left + width, top + height))
                })
                .collect()
        }

        fn original_block_rect(&self) -> D2D_RECT_F {
            self.blocks()
                .into_iter()
                .find(|(track, _)| *track == 0)
                .map(|(_, rect)| rect)
                .unwrap_or_else(|| rect_f(0.0, 0.0, 0.0, 0.0))
        }

        /// 控制条三个按钮的矩形（lock/reset/close，从左到右）。
        fn control_rects(&self) -> Option<[D2D_RECT_F; CONTROL_COUNT]> {
            // 只有原文有文本时才出现（IndicatorApp: original.hasText）。
            if !self.tracks[0].has_text() {
                return None;
            }
            let block = self.original_block_rect();
            let total = CONTROL_BUTTON * CONTROL_COUNT as f32
                + CONTROL_GAP * (CONTROL_COUNT - 1) as f32;
            let left = block.right - CONTROL_RIGHT - total;
            let top = block.top + CONTROL_TOP;
            let mut rects = [rect_f(0.0, 0.0, 0.0, 0.0); CONTROL_COUNT];
            for (index, rect) in rects.iter_mut().enumerate() {
                let x = left + index as f32 * (CONTROL_BUTTON + CONTROL_GAP);
                *rect = rect_f(x, top, x + CONTROL_BUTTON, top + CONTROL_BUTTON);
            }
            Some(rects)
        }

        fn button_at(&self, point: (f32, f32)) -> Option<usize> {
            let rects = self.control_rects()?;
            rects.iter().position(|rect| {
                point.0 >= rect.left
                    && point.0 < rect.right
                    && point.1 >= rect.top
                    && point.1 < rect.bottom
            })
        }

        fn animate_controls(&mut self, target: f32) {
            if (self.controls_alpha - target).abs() < 0.001 && self.controls_anim.is_none() {
                return;
            }
            self.controls_anim = Some((Instant::now(), self.controls_alpha, target));
        }

        fn ensure_format(&mut self) -> Option<IDWriteTextFormat> {
            let family = self.config.font_family.clone();
            let size = self.config.font_size as f32;
            if self
                .format
                .as_ref()
                .map_or(true, |(f, s, _)| f != &family || *s != size)
            {
                self.format = create_text_format_weight(
                    &self.dwrite,
                    &family,
                    size,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    true,
                    false,
                )
                .ok()
                .map(|format| (family, size, format));
            }
            self.format.as_ref().map(|(_, _, format)| format.clone())
        }

        /// 可视区变化时连同测量和滚动目标一起更新，避免沿用旧宽度/行数的布局。
        fn reflow_tracks(&mut self) {
            for index in 0..2 {
                self.tracks[index].layout = None;
                self.update_track_offset(index, false);
            }
        }

        /// 为两条轨道建好文本布局（文本/字体变化后 layout 置空，渲染前重建）。
        fn prepare_layouts(&mut self) {
            for index in 0..2 {
                if self.tracks[index].layout.is_some() || self.tracks[index].displayed.is_empty()
                {
                    continue;
                }
                let Some(format) = self.ensure_format() else {
                    continue;
                };
                let Some(layout) = build_text_layout(
                    &self.dwrite,
                    &format,
                    &self.tracks[index].displayed.clone(),
                    self.flow_width(),
                    self.flow_height(),
                    self.is_replace(),
                    self.line_height(),
                ) else {
                    continue;
                };
                self.tracks[index].content_size = measure_layout(&layout);
                self.tracks[index].layout = Some(layout);
            }
        }

        fn set_track_text(&mut self, index: usize, text: &str, fade: bool) {
            let next = trim_render_text(text);
            if next.is_empty() {
                self.tracks[index].reset();
                if index == 0 {
                    // 原文清空后控制条也失去存在条件。
                    self.animate_controls(0.0);
                }
                return;
            }
            let was_empty = self.tracks[index].displayed.is_empty();
            {
                let track = &mut self.tracks[index];
                if fade {
                    // swapText：整段换新，不做前缀 diff，播 swapIn。
                    track.fresh_from = usize::MAX;
                    track.fresh_started = None;
                    track.swap_started = Some(Instant::now());
                } else {
                    let overlap = overlap_chars(&track.displayed, &next);
                    let fresh_len = next.chars().count().saturating_sub(overlap);
                    track.fresh_from = overlap;
                    track.fresh_started = (fresh_len > 0
                        && fresh_len <= FRESH_FADE_MAX_CHARS
                        && self.config.fade_enabled
                        && self.config.fade_duration_ms > 0)
                    .then(Instant::now);
                    track.swap_started = None;
                }
                track.displayed = next;
                track.layout = None;
            }
            self.update_track_offset(index, was_empty);
        }

        /// 内容尺寸变化后更新位移目标；首个词出现时不播位移动画直接落位
        /// （对照 applyScroll(skipAnimation=true)）。
        fn update_track_offset(&mut self, index: usize, first_paint: bool) {
            if self.tracks[index].displayed.is_empty() {
                return;
            }
            // 测量需要先建布局。
            if self.tracks[index].layout.is_none() {
                let Some(format) = self.ensure_format() else {
                    return;
                };
                let text = self.tracks[index].displayed.clone();
                let Some(layout) = build_text_layout(
                    &self.dwrite,
                    &format,
                    &text,
                    self.flow_width(),
                    self.flow_height(),
                    self.is_replace(),
                    self.line_height(),
                ) else {
                    return;
                };
                self.tracks[index].content_size = measure_layout(&layout);
                self.tracks[index].layout = Some(layout);
            }
            let (content_w, content_h) = self.tracks[index].content_size;
            let target = if self.is_replace() {
                replace_offset_x(content_w, self.flow_width())
            } else {
                scroll_offset_y(content_h, self.flow_height())
            };
            let track = &mut self.tracks[index];
            if first_paint
                || !self.config.motion_enabled
                || self.config.motion_duration_ms == 0
            {
                track.offset = target;
                track.offset_from = target;
                track.offset_target = target;
                track.offset_anim = None;
            } else if (target - track.offset_target).abs() > 0.5 {
                track.offset_from = track.offset;
                track.offset_target = target;
                track.offset_anim = Some(Instant::now());
            }
        }

        fn tick(&mut self, _dt: f32) {
            for index in 0..2 {
                let fade_ms = self.config.fade_duration_ms.max(1) as f32;
                let motion_ms = self.config.motion_duration_ms.max(1) as f32;
                let track = &mut self.tracks[index];
                if track
                    .fresh_started
                    .is_some_and(|start| start.elapsed().as_secs_f32() * 1000.0 >= fade_ms)
                {
                    track.fresh_started = None;
                    // 淡入完成后重建布局，摘掉新增区间的画刷效果。
                    track.layout = None;
                }
                if track
                    .swap_started
                    .is_some_and(|start| start.elapsed().as_secs_f32() >= SWAP_IN_S)
                {
                    track.swap_started = None;
                }
                if let Some(start) = track.offset_anim {
                    let t = (start.elapsed().as_secs_f32() * 1000.0 / motion_ms).min(1.0);
                    let eased = ease_value(&self.config.motion_easing, t);
                    track.offset =
                        track.offset_from + (track.offset_target - track.offset_from) * eased;
                    if t >= 1.0 {
                        track.offset = track.offset_target;
                        track.offset_anim = None;
                    }
                }
            }
            if let Some((start, from, to)) = self.controls_anim {
                let t = (start.elapsed().as_secs_f32() / CONTROLS_FADE_S).min(1.0);
                self.controls_alpha = from + (to - from) * ease_value("ease-out", t);
                if t >= 1.0 {
                    self.controls_alpha = to;
                    self.controls_anim = None;
                }
            }
        }

        fn draw(&self, target: &ID2D1DCRenderTarget, brush: &ID2D1SolidColorBrush) {
            let blocks = self.blocks();
            for (track, rect) in &blocks {
                self.draw_block(target, brush, *track, *rect);
            }
            self.draw_controls(target, brush);
        }

        fn draw_block(
            &self,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            index: usize,
            rect: D2D_RECT_F,
        ) {
            let translation = index == 1;
            // 译文块整体 opacity 0.92，乘进所有颜色。
            let alpha_scale = if translation { 0.92 } else { 1.0 };
            let (bg_r, bg_g, bg_b, bg_a) =
                parse_css_color(&self.config.background_color).unwrap_or((5, 7, 10, 0.72));
            let radius = self.config.rounded as f32;
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: radius,
                radiusY: radius,
            };
            unsafe {
                brush.SetColor(&brush_color(
                    bg_r as f32 / 255.0,
                    bg_g as f32 / 255.0,
                    bg_b as f32 / 255.0,
                    bg_a * alpha_scale,
                ));
                target.FillRoundedRectangle(&rounded, brush);
                brush.SetColor(&brush_color(
                    1.0,
                    1.0,
                    1.0,
                    if translation {
                        0.10 * alpha_scale
                    } else {
                        0.08
                    },
                ));
                target.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: rect_f(
                            rect.left + 0.5,
                            rect.top + 0.5,
                            rect.right - 0.5,
                            rect.bottom - 0.5,
                        ),
                        radiusX: (radius - 0.5).max(0.0),
                        radiusY: (radius - 0.5).max(0.0),
                    },
                    brush,
                    BORDER,
                    None,
                );
            }
            let track = &self.tracks[index];
            let (Some(layout), true) = (&track.layout, track.has_text()) else {
                return;
            };
            let pad_top = if translation {
                PAD_TOP_TRANSLATION
            } else {
                PAD_TOP_MAIN
            };
            let flow = rect_f(
                rect.left + PAD_X + BORDER,
                rect.top + pad_top + BORDER,
                rect.right - PAD_X - BORDER,
                rect.top + pad_top + BORDER + self.flow_height(),
            );
            let (text_r, text_g, text_b, text_a) =
                parse_css_color(&self.config.text_color).unwrap_or((255, 255, 255, 1.0));
            // swapIn 的整段淡入和 6px→0 的模糊共用同一条 CSS 曲线。
            let swap_alpha = track
                .swap_started
                .map(|start| {
                    ease_value(
                        "ease-out",
                        (start.elapsed().as_secs_f32() / SWAP_IN_S).min(1.0),
                    )
                })
                .unwrap_or(1.0);
            let base_alpha = text_a * alpha_scale * swap_alpha;
            // fresh 淡入：新增 UTF-16 范围单独挂低透明度画刷。
            if let Some(start) = track.fresh_started {
                let fade = ease_value(
                    &self.config.fade_easing,
                    (start.elapsed().as_secs_f32() * 1000.0 / self.config.fade_duration_ms.max(1) as f32)
                        .min(1.0),
                );
                let total_chars = track.displayed.chars().count();
                if track.fresh_from < total_chars {
                    let utf16_start: u32 = track
                        .displayed
                        .chars()
                        .take(track.fresh_from)
                        .map(|c| c.len_utf16() as u32)
                        .sum();
                    let total: u32 = track.displayed.encode_utf16().count() as u32;
                    if utf16_start < total {
                        if let Ok(fresh_brush) = unsafe {
                            target.CreateSolidColorBrush(
                                &brush_color(
                                    text_r as f32 / 255.0,
                                    text_g as f32 / 255.0,
                                    text_b as f32 / 255.0,
                                    base_alpha * fade,
                                ),
                                None,
                            )
                        } {
                            unsafe {
                                let _ = layout.SetDrawingEffect(
                                    &fresh_brush,
                                    DWRITE_TEXT_RANGE {
                                        startPosition: utf16_start,
                                        length: total - utf16_start,
                                    },
                                );
                            }
                        }
                    }
                }
            }
            let (dx, dy) = if self.is_replace() {
                (track.offset, 0.0)
            } else {
                (0.0, -track.offset)
            };
            let text_color = brush_color(
                text_r as f32 / 255.0,
                text_g as f32 / 255.0,
                text_b as f32 / 255.0,
                base_alpha,
            );
            let blurred = if swap_alpha < 0.999 {
                match blurred_text_bitmap(
                    target,
                    layout,
                    &flow,
                    (dx, dy),
                    &text_color,
                    6.0 * (1.0 - swap_alpha),
                ) {
                    Ok(bitmap) => Some(bitmap),
                    Err(error) => {
                        eprintln!("[{LOG_TAG}] 绘制字幕模糊失败，回退清晰文本：{error}");
                        None
                    }
                }
            } else {
                None
            };
            unsafe {
                brush.SetColor(&text_color);
                target.PushAxisAlignedClip(&flow, D2D1_ANTIALIAS_MODE_ALIASED);
                if let Some((bitmap, destination)) = blurred {
                    target.DrawBitmap(
                        &bitmap,
                        Some(&destination),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                } else {
                    target.DrawTextLayout(
                        D2D_POINT_2F {
                            x: flow.left + dx,
                            y: flow.top + dy,
                        },
                        layout,
                        brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
                target.PopAxisAlignedClip();
            }
        }

        fn draw_controls(&self, target: &ID2D1DCRenderTarget, brush: &ID2D1SolidColorBrush) {
            if self.controls_alpha <= 0.001 {
                return;
            }
            let Some(rects) = self.control_rects() else {
                return;
            };
            for (index, rect) in rects.iter().enumerate() {
                let hovered = self.hover_button == Some(index);
                let active = index == 0 && self.locked;
                // 基础态 color rgba(255,255,255,0.78) + opacity 0.82；
                // hover/激活态纯白不透明。
                let alpha = if hovered || active {
                    1.0
                } else {
                    0.78 * 0.82
                } * self.controls_alpha;
                let center_x = (rect.left + rect.right) / 2.0;
                let center_y = (rect.top + rect.bottom) / 2.0;
                self.draw_icon(target, brush, index, center_x, center_y, alpha);
            }
        }

        /// 24x24 视图盒的 lucide 图标，经变换绘制到 16px 图标位。
        fn draw_icon(
            &self,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            index: usize,
            center_x: f32,
            center_y: f32,
            alpha: f32,
        ) {
            let scale = ICON_SIZE / 24.0;
            let transform = Matrix3x2 {
                M11: scale,
                M12: 0.0,
                M21: 0.0,
                M22: scale,
                M31: center_x - ICON_SIZE / 2.0,
                M32: center_y - ICON_SIZE / 2.0,
            };
            let identity = Matrix3x2::identity();
            unsafe {
                target.SetTransform(&transform);
                brush.SetColor(&brush_color(1.0, 1.0, 1.0, alpha));
                match index {
                    // Lock/LockOpen：圆角矩形锁体 + 锁梁。
                    0 => {
                        target.DrawRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: rect_f(3.0, 11.0, 21.0, 22.0),
                                radiusX: 2.0,
                                radiusY: 2.0,
                            },
                            brush,
                            ICON_STROKE,
                            self.icons.stroke_style.as_ref(),
                        );
                        let shackle = if self.locked {
                            &self.icons.lock_shackle
                        } else {
                            &self.icons.unlock_shackle
                        };
                        if let Some(shackle) = shackle {
                            target.DrawGeometry(
                                shackle,
                                brush,
                                ICON_STROKE,
                                self.icons.stroke_style.as_ref(),
                            );
                        }
                    }
                    1 => {
                        if let Some(rotate) = &self.icons.rotate {
                            target.DrawGeometry(
                                rotate,
                                brush,
                                ICON_STROKE,
                                self.icons.stroke_style.as_ref(),
                            );
                        }
                    }
                    _ => {
                        if let Some(close) = &self.icons.close {
                            target.DrawGeometry(
                                close,
                                brush,
                                ICON_STROKE,
                                self.icons.stroke_style.as_ref(),
                            );
                        }
                    }
                }
                target.SetTransform(&identity);
            }
        }
    }

    /// DC 渲染目标没有 D2D effect 管线；仅在整段换新动画期间创建软件临时画布。
    /// 三倍 sigma 留白让可视区外的字形也能贡献模糊像素，最后仍按 flow 裁剪。
    fn blurred_text_bitmap(
        target: &ID2D1DCRenderTarget,
        layout: &IDWriteTextLayout,
        flow: &D2D_RECT_F,
        offset: (f32, f32),
        color: &D2D1_COLOR_F,
        sigma: f32,
    ) -> windows::core::Result<(ID2D1Bitmap, D2D_RECT_F)> {
        const PAD: f32 = 18.0;
        unsafe {
            let (mut dpi_x, mut dpi_y) = (96.0, 96.0);
            target.GetDpi(&mut dpi_x, &mut dpi_y);
            let scale = dpi_x / 96.0;
            let width = ((flow.right - flow.left + 2.0 * PAD) * scale).ceil() as u32;
            let height = ((flow.bottom - flow.top + 2.0 * PAD) * scale).ceil() as u32;
            let dib = Dib::create(width as i32, height as i32)
                .ok_or_else(windows::core::Error::from_win32)?;
            let scratch = create_dc_render_target(&target.GetFactory()?, dpi_x)?;
            scratch.BindDC(
                dib.dc(),
                &RECT {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
            )?;
            let brush = scratch.CreateSolidColorBrush(color, None)?;
            scratch.BeginDraw();
            scratch.Clear(Some(&brush_color(0.0, 0.0, 0.0, 0.0)));
            scratch.DrawTextLayout(
                D2D_POINT_2F {
                    x: PAD + offset.0,
                    y: PAD + offset.1,
                },
                layout,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            scratch.EndDraw(None, None)?;
            // BGRA 与 RGBA 的通道顺序不影响逐通道卷积；保留预乘 alpha 避免黑边。
            let pixels = image::RgbaImage::from_raw(width, height, dib.pixels().to_vec())
                .expect("DIB byte length matches its dimensions");
            let blurred = image::imageops::fast_blur(&pixels, sigma * scale);
            let bitmap = target.CreateBitmap(
                D2D_SIZE_U { width, height },
                Some(blurred.as_ptr().cast()),
                width * 4,
                &D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: dpi_x,
                    dpiY: dpi_y,
                },
            )?;
            Ok((
                bitmap,
                rect_f(
                    flow.left - PAD,
                    flow.top - PAD,
                    flow.left - PAD + width as f32 / scale,
                    flow.top - PAD + height as f32 / scale,
                ),
            ))
        }
    }

    fn build_text_layout(
        dwrite: &IDWriteFactory,
        format: &IDWriteTextFormat,
        text: &str,
        flow_width: f32,
        flow_height: f32,
        replace: bool,
        line_height: f32,
    ) -> Option<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = unsafe {
            dwrite.CreateTextLayout(
                &wide,
                format,
                // 替换模式强制单行不换行，宽度不受限，量出自然宽度做左右平移。
                if replace { 1e6 } else { flow_width },
                if replace { line_height * 4.0 } else { flow_height * 100.0 },
            )
        }
        .ok()?;
        unsafe {
            if replace {
                layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).ok()?;
                // 单行的居中/贴右由 replace_offset_x 完成；若继承格式的居中对齐，
                // 字形会落在百万 DIP 布局框中部，整体平移后仍在可视区之外。
                layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING).ok()?;
            }
            // 贴近 CSS line-height 的行框效果。
            layout.SetLineSpacing(
                DWRITE_LINE_SPACING_METHOD_UNIFORM,
                line_height,
                line_height * BASELINE_RATIO,
            ).ok()?;
        }
        Some(layout)
    }

    fn measure_layout(layout: &IDWriteTextLayout) -> (f32, f32) {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        if unsafe { layout.GetMetrics(&mut metrics) }.is_err() {
            return (0.0, 0.0);
        }
        (metrics.width, metrics.height)
    }

    #[cfg(test)]
    mod layout_tests {
        use super::*;
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;

        // 真正读取软件 D2D 输出，防止“布局创建成功”掩盖画布里没有文字的回归。
        fn assert_visible_text_pixels(state: &mut WindowState, name: &str) {
            state.view.prepare_layouts();
            let width = state.view.layout_w as i32;
            let height = state.view.layout_h as i32;
            let dib = Dib::create(width, height).unwrap();
            unsafe {
                let target = create_dc_render_target(&state.d2d, 96.0).unwrap();
                target
                    .BindDC(
                        dib.dc(),
                        &RECT {
                            left: 0,
                            top: 0,
                            right: width,
                            bottom: height,
                        },
                    )
                    .unwrap();
                let brush = target
                    .CreateSolidColorBrush(&brush_color(1.0, 1.0, 1.0, 1.0), None)
                    .unwrap();
                if state.view.tracks[0].swap_started.is_some() {
                    blurred_text_bitmap(
                        &target,
                        state.view.tracks[0].layout.as_ref().unwrap(),
                        &rect_f(0.0, 0.0, state.view.flow_width(), state.view.flow_height()),
                        (0.0, 0.0),
                        &brush_color(1.0, 1.0, 1.0, 1.0),
                        6.0,
                    )
                    .unwrap();
                }
                target.BeginDraw();
                target.Clear(Some(&brush_color(0.0, 0.0, 0.0, 0.0)));
                state.view.draw(&target, &brush);
                target.EndDraw(None, None).unwrap();
                let pixels = dib.pixels();
                for (index, rect) in state.view.blocks() {
                    let pad = if index == 0 {
                        PAD_TOP_MAIN
                    } else {
                        PAD_TOP_TRANSLATION
                    };
                    let mut bright = 0;
                    for y in (rect.top + pad) as i32..(rect.top + pad + state.view.flow_height()) as i32
                    {
                        for x in (rect.left + PAD_X) as i32..(rect.right - PAD_X) as i32 {
                            let i = ((y * width + x) * 4) as usize;
                            if pixels[i..i + 3].iter().all(|c| *c > 150) {
                                bright += 1;
                            }
                        }
                    }
                    assert!(
                        bright > 30,
                        "{name}: track {index} has only {bright} text pixels"
                    );
                }
                if let Some(dir) = std::env::var_os("SAYIT_SUBTITLE_TEST_SNAPSHOTS") {
                    let mut image = pixels.to_vec();
                    for pixel in image.chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                    let dir = std::path::PathBuf::from(dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    image::save_buffer(
                        dir.join(format!("{name}.png")),
                        &image,
                        width as u32,
                        height as u32,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }

        #[test]
        fn window_commands_reflow_and_render_both_tracks_without_mouse_input() {
            struct TestWindow(HWND);
            impl Drop for TestWindow {
                fn drop(&mut self) {
                    unsafe {
                        let _ = DestroyWindow(self.0);
                    }
                }
            }
            let window = TestWindow(create_window().unwrap());
            with_state(window.0, |state| {
                let mut config = SubtitleConfig {
                    display_mode: "replace".into(),
                    width: 444.0,
                    window_width: 444.0,
                    window_height: 136.0,
                    line_count: 1,
                    font_size: 28.0,
                    translation_enabled: true,
                    motion_enabled: false,
                    fade_enabled: false,
                    ..SubtitleConfig::default()
                };
                state.apply(Command::SetLayout {
                    width: 444.0,
                    height: 136.0,
                    anchor: "center".into(),
                    offset_y: 0.0,
                });
                state.apply(Command::SetConfig(Box::new(config.clone())));
                state.apply(Command::SetText {
                    text: "原生字幕 Hello".into(),
                    fade: false,
                });
                state.apply(Command::SetTranslation("Bilingual translation".into()));
                assert_eq!(
                    state
                        .view
                        .blocks()
                        .iter()
                        .map(|(i, _)| *i)
                        .collect::<Vec<_>>(),
                    [1, 0]
                );
                assert_visible_text_pixels(state, "replace-bilingual-short");
                state.apply(Command::SetText {
                    text: "连续追加长句显示最新内容".repeat(12),
                    fade: false,
                });
                assert!(state.view.tracks[0].offset < 0.0);
                assert_visible_text_pixels(state, "replace-bilingual-long");
                config.display_mode = "scroll".into();
                config.line_count = 2;
                config.translation_order = "sourceFirst".into();
                state.apply(Command::SetLayout {
                    width: 444.0,
                    height: 214.0,
                    anchor: "center".into(),
                    offset_y: 0.0,
                });
                state.apply(Command::SetConfig(Box::new(config.clone())));
                assert_eq!(
                    state
                        .view
                        .blocks()
                        .iter()
                        .map(|(i, _)| *i)
                        .collect::<Vec<_>>(),
                    [0, 1]
                );
                assert_visible_text_pixels(state, "scroll-bilingual");
                let before = state.view.tracks[0].content_size.1;
                config.width = 260.0;
                state.apply(Command::SetConfig(Box::new(config.clone())));
                assert!(state.view.tracks[0].content_size.1 > before);
                let offset = state.view.tracks[0].offset;
                config.line_count = 1;
                state.apply(Command::SetConfig(Box::new(config.clone())));
                assert!((state.view.tracks[0].offset - offset - 39.0).abs() < 0.1);
                state.apply(Command::SetLayout {
                    width: 200.0,
                    height: 214.0,
                    anchor: "center".into(),
                    offset_y: 0.0,
                });
                assert_eq!(
                    unsafe { state.view.tracks[0].layout.as_ref().unwrap().GetMaxWidth() },
                    154.0
                );
                assert_visible_text_pixels(state, "scroll-resized");
                state.apply(Command::SetTranslation(String::new()));
                assert_eq!(state.view.blocks().len(), 1);
                config.fade_enabled = true;
                state.apply(Command::SetConfig(Box::new(config)));
                state.apply(Command::SetText {
                    text: "新字幕".into(),
                    fade: false,
                });
                assert!(state.view.tracks[0].fresh_started.is_some());
                // 用时钟注入覆盖淡入终点，不等待、不模拟鼠标。
                state.view.tracks[0].fresh_started =
                    Some(Instant::now() - std::time::Duration::from_secs(1));
                state.view.tick(0.0);
                assert!(state.view.tracks[0].fresh_started.is_none());
                assert_visible_text_pixels(state, "fresh-complete");
                state.apply(Command::SetText {
                    text: "整段换新".into(),
                    fade: true,
                });
                state.view.tracks[0].swap_started =
                    Some(Instant::now() - std::time::Duration::from_millis(240));
                assert_visible_text_pixels(state, "swap-blur");
                state.view.tracks[0].swap_started =
                    Some(Instant::now() - std::time::Duration::from_secs(1));
                state.view.tick(0.0);
                assert!(state.view.tracks[0].swap_started.is_none());
                assert_visible_text_pixels(state, "swap-complete");
            })
            .unwrap();
        }

        #[test]
        fn replace_text_stays_in_view_for_short_and_overflowing_lines() {
            let dwrite = create_dwrite_factory().unwrap();
            let format = create_text_format_weight(
                &dwrite,
                "Microsoft YaHei",
                28.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                true,
                false,
            )
            .unwrap();
            let flow_width = 400.0;
            let line_height = 39.0;
            for text in ["原生字幕 Hello", &"长句持续追加显示最新内容".repeat(12)] {
                let layout = build_text_layout(
                    &dwrite,
                    &format,
                    text,
                    flow_width,
                    line_height,
                    true,
                    line_height,
                )
                .unwrap();
                let mut metrics = DWRITE_TEXT_METRICS::default();
                unsafe {
                    layout.GetMetrics(&mut metrics).unwrap();
                }
                let offset = replace_offset_x(metrics.width, flow_width);
                let left = metrics.left + offset;
                let right = left + metrics.width;
                assert!(
                    left < flow_width && right > 0.0,
                    "text outside viewport: left={left}, right={right}, width={flow_width}"
                );
                if metrics.width > flow_width {
                    assert!((right - flow_width).abs() < 0.1);
                } else {
                    assert!((left - (flow_width - right)).abs() < 0.1);
                }
                assert_eq!(metrics.lineCount, 1);
                assert!((metrics.height - line_height).abs() < 0.1);
            }
        }

        #[test]
        fn scroll_text_keeps_centered_lines_and_reveals_the_latest_line() {
            let dwrite = create_dwrite_factory().unwrap();
            let format = create_text_format_weight(
                &dwrite,
                "Microsoft YaHei",
                28.0,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                true,
                false,
            )
            .unwrap();
            let layout = build_text_layout(
                &dwrite,
                &format,
                "第一行\n第二行\n最新一行",
                400.0,
                78.0,
                false,
                39.0,
            )
            .unwrap();
            let mut metrics = DWRITE_TEXT_METRICS::default();
            unsafe {
                layout.GetMetrics(&mut metrics).unwrap();
            }
            assert!((metrics.left - (400.0 - metrics.width) / 2.0).abs() < 0.1);
            assert_eq!(metrics.lineCount, 3);
            assert!((metrics.height - scroll_offset_y(metrics.height, 78.0) - 78.0).abs() < 0.1);
        }
    }

    /// 主显示器完整边界 + 锚点定位，物理像素（与 WebView 的 monitor.size 一致）。
    fn configured_placement(
        dpi: u32,
        layout_w: f32,
        layout_h: f32,
        anchor: &str,
        offset_y: f32,
    ) -> (i32, i32, i32, i32) {
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        let width = (layout_w * scale).round() as i32;
        let height = (layout_h * scale).round() as i32;
        // 主显示器的虚拟坐标原点恒为 (0,0)。
        let mut area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        unsafe {
            let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                area = info.rcMonitor;
            }
        }
        let margin = (offset_y * scale).round() as i32;
        let (x, y) = window_position(
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
            width,
            height,
            anchor,
            margin,
        );
        (x, y, width, height)
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

    /// 物理像素坐标（lparam）换算成 DIP。
    fn to_dip(dpi: u32, x: i32, y: i32) -> (f32, f32) {
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        (x as f32 / scale, y as f32 / scale)
    }

    fn run_button_action(index: usize, hwnd: HWND) {
        match index {
            // 锁定/解锁：只切换内部状态，图标与拖拽行为随之变化。
            0 => {
                with_state(hwnd, |state| {
                    state.view.locked = !state.view.locked;
                    state.render();
                    state.sync_timer();
                });
            }
            // 重置：恢复 config 的 anchor/offsetY 布局。
            1 => {
                with_state(hwnd, |state| {
                    let config = state.view.config.clone();
                    state.view.layout_w = config.window_width as f32;
                    state.view.layout_h = config.window_height as f32;
                    state.view.anchor = config.anchor.clone();
                    state.view.offset_y = config.offset_y as f32;
                    state.apply_configured_placement();
                    state.sync_timer();
                });
            }
            // 关闭：走字幕会话的停止逻辑（subtitle_stop 是 async 命令，
            // 从原生 UI 线程用 async_runtime 调用）。
            _ => {
                if let Some(app) = APP.get() {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::application::subtitles::subtitle_stop(app).await;
                    });
                }
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
                with_state(hwnd, |state| state.tick());
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);
                enum HoverAction {
                    None,
                    StartDrag,
                }
                let action = with_state(hwnd, |state| {
                    let point = to_dip(
                        state.view.dpi,
                        (lparam.0 & 0xFFFF) as i16 as i32,
                        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
                    );
                    let block = state.view.original_block_rect();
                    let over_block = point.0 >= block.left
                        && point.0 < block.right
                        && point.1 >= block.top
                        && point.1 < block.bottom;
                    if over_block && !state.view.hover_tracking {
                        // 只注册一次离开追踪，离开前不再重复。
                        let mut track = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut track);
                        state.view.hover_tracking = true;
                    }
                    let mut dirty = false;
                    if over_block != state.view.hovering_block {
                        state.view.hovering_block = over_block;
                        // 控制条只在悬停原文块时淡入（且原文有文本）。
                        let target = if over_block && state.view.tracks[0].has_text() {
                            1.0
                        } else {
                            0.0
                        };
                        state.view.animate_controls(target);
                        dirty = true;
                    }
                    let button = if over_block { state.view.button_at(point) } else { None };
                    if button != state.view.hover_button {
                        state.view.hover_button = button;
                        dirty = true;
                    }
                    if dirty {
                        state.render();
                        state.sync_timer();
                    }
                    // 拖拽阈值判定与悬浮球一致（阈值按 CSS 像素）。
                    let Some(press) = state.press else {
                        return HoverAction::None;
                    };
                    if state.dragging || !press.draggable {
                        return HoverAction::None;
                    }
                    let dpr = f64::from(state.view.dpi.max(96)) / 96.0;
                    if should_start_orb_drag(
                        f64::from(cursor.x - press.screen.0) / dpr,
                        f64::from(cursor.y - press.screen.1) / dpr,
                    ) {
                        HoverAction::StartDrag
                    } else {
                        HoverAction::None
                    }
                });
                if let Some(HoverAction::StartDrag) = action {
                    with_state(hwnd, |state| state.dragging = true);
                    // 经典做法：转成标题栏按下，让系统接管移动；SendMessage 会阻塞到
                    // 拖拽结束（期间窗口过程被重入，上面不能持有 WindowState 借用）。
                    let _ = ReleaseCapture();
                    SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
                    // 拖拽不持久化：位置留在窗口状态里，重置按钮随时可回到配置位。
                    with_state(hwnd, |state| {
                        state.press = None;
                        state.dragging = false;
                    });
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);
                with_state(hwnd, |state| {
                    let point = to_dip(
                        state.view.dpi,
                        (lparam.0 & 0xFFFF) as i16 as i32,
                        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
                    );
                    let block = state.view.original_block_rect();
                    let over_block = point.0 >= block.left
                        && point.0 < block.right
                        && point.1 >= block.top
                        && point.1 < block.bottom;
                    let on_button = state.view.button_at(point);
                    // 拖拽只从原文块发起（WebView 里 handleSubtitlePointerDown 只挂在
                    // #text 上），控制条按钮区域除外；锁定后禁止拖拽。
                    let draggable =
                        over_block && on_button.is_none() && !state.view.locked;
                    state.press = Some(PressState {
                        screen: (cursor.x, cursor.y),
                        on_button,
                        draggable,
                    });
                    state.dragging = false;
                });
                SetCapture(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let action = with_state(hwnd, |state| {
                    let point = to_dip(
                        state.view.dpi,
                        (lparam.0 & 0xFFFF) as i16 as i32,
                        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
                    );
                    let press = state.press.take();
                    state.dragging = false;
                    // 按下与抬起在同一个按钮上才算点击；拖拽结束不算。
                    press.and_then(|press| {
                        press
                            .on_button
                            .filter(|index| state.view.button_at(point) == Some(*index))
                    })
                });
                let _ = ReleaseCapture();
                if let Some(Some(button)) = action {
                    run_button_action(button, hwnd);
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE_MSG => {
                with_state(hwnd, |state| {
                    state.view.hovering_block = false;
                    state.view.hover_button = None;
                    state.view.hover_tracking = false;
                    state.view.animate_controls(0.0);
                    state.render();
                    state.sync_timer();
                });
                LRESULT(0)
            }
            WM_SETCURSOR => {
                // 只在客户区内自定义光标：按钮/锁定态用箭头，可拖拽区域用移动光标。
                if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                    let cursor = with_state(hwnd, |state| {
                        if state.dragging
                            || (state.view.hovering_block
                                && state.view.hover_button.is_none()
                                && !state.view.locked)
                        {
                            state.cursor_move
                        } else {
                            state.cursor_arrow
                        }
                    });
                    if let Some(cursor) = cursor {
                        SetCursor(cursor);
                        return LRESULT(1);
                    }
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_MOVE => {
                // WS_POPUP 没有非客户区，WM_MOVE 的坐标即屏幕坐标；拖拽后的位置
                // 必须跟住，否则下一次渲染会把窗口拉回旧坐标。
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                with_state(hwnd, |state| {
                    state.x = x;
                    state.y = y;
                });
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let dpi = (wparam.0 & 0xFFFF) as u32;
                with_state(hwnd, |state| {
                    if dpi != 0 && dpi != state.view.dpi {
                        state.view.dpi = dpi;
                        let scale = dpi as f32 / 96.0;
                        let width = (state.view.layout_w * scale).round() as i32;
                        let height = (state.view.layout_h * scale).round() as i32;
                        state.size = (width, height);
                        // 保持用户摆位（可能拖过），只按系统建议的矩形调整原点。
                        let suggested = &*(lparam.0 as *const RECT);
                        state.x = suggested.left;
                        state.y = suggested.top;
                        let _ = SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            state.x,
                            state.y,
                            width,
                            height,
                            SWP_NOACTIVATE,
                        );
                        // DPI 变化后物理尺寸改变，强制重建 DIB。
                        state.surface.discard_dib();
                        state.render();
                        state.sync_timer();
                    }
                });
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
                    eprintln!("[native-subtitle] 读取模块句柄失败：{error}");
                    return None;
                }
            };
            let cursor_arrow = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            let cursor_move = LoadCursorW(None, IDC_SIZEALL).unwrap_or_default();
            let class_name = w!("SayItNativeSubtitle");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                // 类光标给箭头；可拖拽区域在 WM_SETCURSOR 里换成移动光标。
                hCursor: cursor_arrow,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                eprintln!("[native-subtitle] 注册窗口类失败");
                return None;
            }
            let d2d = match create_d2d_factory() {
                Ok(factory) => factory,
                Err(error) => {
                    eprintln!("[native-subtitle] {error}");
                    return None;
                }
            };
            let dwrite = match create_dwrite_factory() {
                Ok(factory) => factory,
                Err(error) => {
                    eprintln!("[native-subtitle] {error}");
                    return None;
                }
            };
            let stroke_style = d2d
                .CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        startCap: D2D1_CAP_STYLE_ROUND,
                        endCap: D2D1_CAP_STYLE_ROUND,
                        dashCap: D2D1_CAP_STYLE_ROUND,
                        lineJoin: D2D1_LINE_JOIN_ROUND,
                        miterLimit: 10.0,
                        dashStyle: D2D1_DASH_STYLE_SOLID,
                        dashOffset: 0.0,
                    },
                    None,
                )
                .ok();
            let icons = Icons {
                lock_shackle: svg_path_geometry_stroke(&d2d, ICON_LOCK_SHACKLE).ok(),
                unlock_shackle: svg_path_geometry_stroke(&d2d, ICON_UNLOCK_SHACKLE).ok(),
                rotate: svg_path_geometry_stroke(&d2d, ICON_ROTATE).ok(),
                close: svg_path_geometry_stroke(&d2d, ICON_CLOSE).ok(),
                stroke_style,
            };
            let config = SubtitleConfig::default();
            let dpi = GetDpiForSystem();
            let (x, y, width, height) = configured_placement(
                dpi,
                config.window_width as f32,
                config.window_height as f32,
                &config.anchor,
                config.offset_y as f32,
            );
            // 不加 WS_EX_TRANSPARENT：字幕条要接收拖拽与按钮点击。
            let hwnd = match CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name,
                w!("说吧！实时字幕"),
                WS_POPUP,
                x,
                y,
                width,
                height,
                None,
                None,
                instance,
                None,
            ) {
                Ok(hwnd) => hwnd,
                Err(error) => {
                    eprintln!("[native-subtitle] 创建字幕窗口失败：{error}");
                    return None;
                }
            };
            let state = Box::new(WindowState {
                hwnd,
                x,
                y,
                size: (width, height),
                visible: false,
                dragging: false,
                press: None,
                view: SubtitleView {
                    layout_w: config.window_width as f32,
                    layout_h: config.window_height as f32,
                    anchor: config.anchor.clone(),
                    offset_y: config.offset_y as f32,
                    config,
                    locked: false,
                    tracks: [TextTrack::default(), TextTrack::default()],
                    format: None,
                    hovering_block: false,
                    hover_button: None,
                    hover_tracking: false,
                    controls_alpha: 0.0,
                    controls_anim: None,
                    dpi,
                    dwrite,
                    icons,
                },
                d2d,
                surface: LayeredSurface::new(hwnd),
                transition: Transition::new(false),
                timer_active: false,
                last_frame: None,
                cursor_arrow,
                cursor_move,
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
    fn css_color_parses_hex_and_rgba() {
        assert_eq!(parse_css_color("#fff"), Some((255, 255, 255, 1.0)));
        assert_eq!(parse_css_color("#286ec8"), Some((40, 110, 200, 1.0)));
        assert_eq!(
            parse_css_color("rgba(5, 7, 10, 0.72)"),
            Some((5, 7, 10, 0.72))
        );
        assert_eq!(parse_css_color("rgb(1, 2, 3)"), Some((1, 2, 3, 1.0)));
        assert_eq!(parse_css_color("  #FF4013  "), Some((255, 64, 19, 1.0)));
        assert_eq!(parse_css_color("not-a-color"), None);
        assert_eq!(parse_css_color("#12345"), None);
    }

    #[test]
    fn overlap_finds_stable_prefix() {
        assert_eq!(overlap_chars("你好", "你好世"), 2);
        assert_eq!(overlap_chars("", "你好"), 0);
        // 修订：前缀停在第一个不同字符，末端重叠搜索救不回就按前缀算。
        assert_eq!(overlap_chars("你好世界", "你好中国"), 2);
        // 替换模式的整句衔接：新句开头是旧句结尾时按末端重叠。
        assert_eq!(overlap_chars("abcdef", "cdefgh"), 4);
    }

    #[test]
    fn trim_limits_render_text_from_the_tail() {
        let short = "短文本";
        assert_eq!(trim_render_text(short), short);
        let long = "字".repeat(MAX_RENDER_CHARS + 10);
        let trimmed = trim_render_text(&long);
        assert_eq!(trimmed.chars().count(), MAX_RENDER_CHARS);
        let padded = format!("{}\n   ", "字".repeat(MAX_RENDER_CHARS));
        assert!(!trim_render_text(&padded).starts_with(char::is_whitespace));
    }

    #[test]
    fn scroll_and_replace_offsets_match_the_webview_math() {
        assert_eq!(scroll_offset_y(100.0, 78.0), 22.0);
        assert_eq!(scroll_offset_y(50.0, 78.0), 0.0);
        // 未超宽居中，超宽贴右。
        assert_eq!(replace_offset_x(100.0, 400.0), 150.0);
        assert_eq!(replace_offset_x(500.0, 400.0), -100.0);
    }

    #[test]
    fn block_layout_matches_indicator_css() {
        assert_eq!(line_height(28.0), 39.0);
        assert_eq!(block_height(39.0, 2, false), 106.0);
        assert_eq!(block_height(39.0, 2, true), 98.0);
        // 底部对齐堆叠：块间 gap 10，整体贴窗口底。
        let tops = stack_from_bottom(214.0, &[106.0, 98.0], 10.0);
        assert_eq!(tops, vec![0.0, 116.0]);
        let single = stack_from_bottom(134.0, &[106.0], 10.0);
        assert_eq!(single, vec![28.0]);
    }

    #[test]
    fn window_position_matches_fallback_indicator_position() {
        // 与 indicator.rs 的单测数值一致。
        assert_eq!(window_position(-1920, 0, 1920, 1080, 460, 188, "bottom", 36), (-1190, 856));
        assert_eq!(window_position(200, -900, 1600, 900, 400, 180, "top", 24), (800, -876));
        assert_eq!(window_position(200, -900, 1600, 900, 400, 180, "center", 24), (800, -516));
    }

    #[test]
    fn easing_covers_the_css_keyword_set() {
        assert_eq!(ease_value("linear", 0.5), 0.5);
        assert_eq!(ease_value("ease-in", 0.0), 0.0);
        assert_eq!(ease_value("ease-out", 1.0), 1.0);
        assert!((ease_value("ease-in-out", 0.5) - 0.5).abs() < 0.00001);
        assert!((ease_value("ease-out", 0.5) - 0.684643).abs() < 0.00001);
        assert!((ease_value("ease-in", 0.5) - 0.315357).abs() < 0.00001);
        assert_eq!(ease_value("unknown-keyword", 1.0), 1.0);
        assert!(ease_value("ease-out", 0.5) > 0.5);
    }

    #[test]
    fn config_deserializes_the_sync_presentation_payload() {
        let value = serde_json::json!({
            "displayMode": "replace", "fontFamily": "MiSans", "fontSize": 32.0,
            "lineCount": 1, "textColor": "#ffffff",
            "backgroundColor": "rgba(5, 7, 10, 0.72)",
            "rounded": 18, "width": 880.0, "windowWidth": 880.0, "windowHeight": 67.0,
            "anchor": "bottom", "offsetY": 64.0,
            "motionEnabled": false, "motionDurationMs": 120, "motionEasing": "ease-out",
            "fadeEnabled": true, "fadeDurationMs": 180, "fadeEasing": "linear",
            "translationEnabled": true, "translationLayout": "bilingual",
            "translationOrder": "translationFirst"
        });
        let config: SubtitleConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.display_mode, "replace");
        assert_eq!(config.font_size, 32.0);
        assert_eq!(config.line_count, 1);
        assert!(!config.motion_enabled);
        assert_eq!(config.translation_order, "translationFirst");
        // 缺字段时用与 IndicatorApp 一致的默认值。
        let sparse: SubtitleConfig = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(sparse.font_size, 28.0);
        assert_eq!(sparse.line_count, 2);
        assert!(sparse.motion_enabled);
        assert!(sparse.fade_enabled);
    }
}

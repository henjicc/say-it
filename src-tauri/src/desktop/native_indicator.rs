//! Windows 原生听写悬浮指示器原型。
//!
//! 目标是验证「听写悬浮窗不占用 WebView 进程」：现有指示器是一个常驻
//! WebviewWindow，每个 WebView 渲染进程闲时约 60MB 工作集。本模块用
//! UpdateLayeredWindow + Direct2D/DirectWrite 在独立 UI 线程上复刻听写模式
//! （recording/processing/smartProcessing/fallback/hidden + 文本 + 波形），
//! error 与 subtitle 两种状态仍由原 WebView 路径承载。
//!
//! 视觉规格以 ui/src/indicator/indicator.css 的 dictation-mode 为准。

use std::sync::OnceLock;

/// Windows 上默认启用原生听写指示器；`SAYIT_NATIVE_INDICATOR=0` 回退 WebView。
pub(crate) fn native_dictation_indicator_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        cfg!(windows)
            && std::env::var("SAYIT_NATIVE_INDICATOR")
                .map(|value| value != "0")
                .unwrap_or(true)
    })
}

/// 波形柱数量，与 IndicatorApp.tsx 的 INDICATOR_WAVE_BAR_COUNT 一致。
#[cfg(any(windows, test))]
pub(crate) const WAVE_BAR_COUNT: usize = 9;

/// 每根波形柱的基础高度占比，与 indicator.css 的 --bar-height 逐个对应。
#[cfg(any(windows, test))]
const WAVE_BAR_HEIGHTS: [f32; WAVE_BAR_COUNT] =
    [0.46, 0.58, 0.70, 0.82, 0.94, 0.82, 0.70, 0.58, 0.46];

/// 波形柱最小缩放，与 OrbWaveform 的 `max(0.18, level)` 一致。
#[cfg(any(windows, test))]
const WAVE_BAR_MIN_SCALE: f32 = 0.18;

/// 感知响度曲线，移植自 ui/src/floating-orb/interaction.ts 的 floatingOrbWaveScale。
#[cfg(any(windows, test))]
fn wave_scale(value: f32) -> f32 {
    // JS 侧 `Number(value) || 0` 会把 NaN 归 0。
    let normalized = if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    };
    (normalized.sqrt() * 1.8).min(1.0)
}

/// 把任意数量的峰值重采样到 9 根柱，移植自 IndicatorApp.tsx 的 resampleWaveLevels。
#[cfg(any(windows, test))]
fn resample_wave_levels(levels: &[f32], fallback: f32) -> [f32; WAVE_BAR_COUNT] {
    if levels.is_empty() {
        return [fallback; WAVE_BAR_COUNT];
    }
    if levels.len() == 1 {
        return [levels[0]; WAVE_BAR_COUNT];
    }
    let mut out = [0.0; WAVE_BAR_COUNT];
    for (index, slot) in out.iter_mut().enumerate() {
        let position = index as f32 * (levels.len() - 1) as f32 / (WAVE_BAR_COUNT - 1) as f32;
        let lower = position.floor() as usize;
        let upper = (levels.len() - 1).min(position.ceil() as usize);
        let progress = position - lower as f32;
        *slot = levels[lower] + (levels[upper] - levels[lower]) * progress;
    }
    out
}

pub(crate) fn native_indicator_prepare() {
    #[cfg(windows)]
    imp::post(imp::Command::Prepare);
}

/// 仅接受 recording/processing/smartProcessing/fallback；其余状态不在原生接管范围。
pub(crate) fn native_indicator_set_state(state: &str) {
    #[cfg(windows)]
    if let Some(state) = imp::NativeState::from_str(state) {
        imp::post(imp::Command::SetState(state));
    }
    #[cfg(not(windows))]
    let _ = state;
}

pub(crate) fn native_indicator_set_text(text: String) {
    #[cfg(windows)]
    imp::post(imp::Command::SetText(text));
    #[cfg(not(windows))]
    let _ = text;
}

pub(crate) fn native_indicator_set_waveform(level: f32, peaks: Vec<f32>) {
    #[cfg(windows)]
    imp::post(imp::Command::SetWaveform { level, peaks });
    #[cfg(not(windows))]
    let _ = (level, peaks);
}

pub(crate) fn native_indicator_hide() {
    #[cfg(windows)]
    imp::post(imp::Command::Hide);
}

#[cfg(windows)]
mod imp {
    use super::{
        resample_wave_levels, wave_scale, WAVE_BAR_COUNT, WAVE_BAR_HEIGHTS, WAVE_BAR_MIN_SCALE,
    };
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use windows::core::{w, HRESULT};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
    use windows::Win32::Graphics::Direct2D::Common::{
        D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
    };
    use windows::Win32::Graphics::Direct2D::{
        D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
        D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
        D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
        D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
        D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    };
    use windows::Win32::Graphics::DirectWrite::{
        DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
        DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_REGULAR,
        DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER,
        DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_WRAP,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW,
        MonitorFromPoint, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, HBITMAP,
        HDC, HGDIOBJ, MONITORINFO, AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, DIB_RGB_COLORS,
        MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowLongPtrW, KillTimer, PeekMessageW, PostThreadMessageW, RegisterClassW, SetTimer,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
        GWLP_USERDATA, HWND_TOPMOST, MSG, PM_NOREMOVE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_DESTROY, WM_DPICHANGED, WM_TIMER, WM_USER,
        WNDCLASSW, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };

    // 与 indicator.rs 的 DEFAULT_INDICATOR_WIDTH/HEIGHT 保持一致。
    const LOGICAL_WIDTH: f32 = 460.0;
    const LOGICAL_HEIGHT: f32 = 188.0;
    // 距主显示器工作区底边的逻辑像素，与非 macOS 的 DICTATION_INDICATOR_OFFSET_Y 一致。
    const OFFSET_Y: f32 = 36.0;

    // 以下几何与 indicator.css 的 dictation-mode 一一对应。
    const WRAP_PADDING_BOTTOM: f32 = 24.0;
    const STACK_GAP: f32 = 10.0;
    const TEXT_W: f32 = 396.0;
    const TEXT_H: f32 = 85.5; // 22.5 * 3 + 18
    const TEXT_PAD: f32 = 12.0;
    const TEXT_PAD_Y: f32 = 9.0;
    const TEXT_VISIBLE_H: f32 = 67.5; // 22.5 * 3
    const TEXT_RADIUS: f32 = 12.0;
    const PILL_W: f32 = 128.0;
    const PILL_H: f32 = 40.0;
    const PILL_RADIUS: f32 = 20.0;
    const FALLBACK_W: f32 = 396.0;
    const FALLBACK_H: f32 = 56.0;
    const FALLBACK_PAD: f32 = 14.0;
    const FALLBACK_ICON: f32 = 18.0;
    const FALLBACK_ICON_GAP: f32 = 10.0;
    const WAVE_AREA_W: f32 = 80.0;
    const WAVE_AREA_H: f32 = 26.0;
    const WAVE_BAR_RATIO: f32 = 0.03; // OrbWaveform dense 变体
    const WAVE_GAP_RATIO: f32 = 0.07;
    const DOT_RADIUS: f32 = 5.0;
    const DOT_LABEL_GAP: f32 = 8.0;

    const WAVE_TIMER_ID: usize = 1;
    const WAVE_TIMER_MS: u32 = 33; // ~30fps，仅录音且有波形时启用

    // 自定义消息：命令队列有内容，需要窗口线程消费。
    const WM_COMMAND_QUEUED: u32 = WM_USER + 0x0101;
    const RECREATE_TARGET: HRESULT = HRESULT(0x8899000Cu32 as i32);


    pub(super) enum Command {
        Prepare,
        SetState(NativeState),
        SetText(String),
        SetWaveform { level: f32, peaks: Vec<f32> },
        Hide,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum NativeState {
        Recording,
        Processing,
        SmartProcessing,
        Fallback,
    }

    impl NativeState {
        pub(super) fn from_str(state: &str) -> Option<Self> {
            match state {
                "recording" => Some(Self::Recording),
                "processing" => Some(Self::Processing),
                "smartProcessing" => Some(Self::SmartProcessing),
                "fallback" => Some(Self::Fallback),
                _ => None,
            }
        }

        fn label(self) -> &'static str {
            match self {
                Self::Recording => "聆听中…",
                Self::Processing => "识别中…",
                Self::SmartProcessing => "处理中…",
                Self::Fallback => "",
            }
        }
    }

    struct UiShared {
        thread_id: u32,
        queue: Mutex<VecDeque<Command>>,
    }

    static UI: OnceLock<Option<UiShared>> = OnceLock::new();

    /// 任意线程可调用：命令入队并唤醒 UI 线程。UI 线程启动失败时静默丢弃，
    /// 指示器只是不显示，不影响听写主流程。
    pub(super) fn post(command: Command) {
        let Some(shared) = UI.get_or_init(|| start_ui_thread().ok()).as_ref() else {
            return;
        };
        let Ok(mut queue) = shared.queue.lock() else {
            return;
        };
        queue.push_back(command);
        drop(queue);
        unsafe {
            let _ = PostThreadMessageW(
                shared.thread_id,
                WM_COMMAND_QUEUED,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }

    fn start_ui_thread() -> Result<UiShared, String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("sayit-native-indicator".into())
            .spawn(move || {
                // 先强制创建线程消息队列，否则 ready 之前到达的
                // PostThreadMessageW 会被系统直接丢弃。
                let mut message = MSG::default();
                unsafe {
                    let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                    let _ = ready_tx.send(GetCurrentThreadId());
                }
                ui_thread_main();
            })
            .map_err(|error| format!("创建原生指示器线程失败：{error}"))?;
        let thread_id = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "启动原生指示器线程超时".to_string())?;
        Ok(UiShared {
            thread_id,
            queue: Mutex::new(VecDeque::new()),
        })
    }

    /// 预乘 alpha 颜色。
    const fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: r * a,
            g: g * a,
            b: b * a,
            a,
        }
    }

    const fn rect_f(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
        D2D_RECT_F {
            left,
            top,
            right,
            bottom,
        }
    }

    /// 内存 DC + 顶向下 32bpp DIB section。GDI 句柄用 Drop 释放，
    /// 重复 show/hide 与尺寸重建不得泄漏。
    struct Dib {
        dc: HDC,
        bitmap: HBITMAP,
        old: HGDIOBJ,
        width: i32,
        height: i32,
    }

    impl Dib {
        fn create(width: i32, height: i32) -> Option<Self> {
            unsafe {
                let dc = CreateCompatibleDC(None);
                if dc.is_invalid() {
                    return None;
                }
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width,
                        // 负数表示顶向下，与 D2D 坐标方向一致。
                        biHeight: -height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits: *mut c_void = std::ptr::null_mut();
                let bitmap =
                    match CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                        Ok(bitmap) => bitmap,
                        Err(_) => {
                            let _ = DeleteDC(dc);
                            return None;
                        }
                    };
                let old = SelectObject(dc, bitmap);
                Some(Self {
                    dc,
                    bitmap,
                    old,
                    width,
                    height,
                })
            }
        }
    }

    impl Drop for Dib {
        fn drop(&mut self) {
            unsafe {
                SelectObject(self.dc, self.old);
                let _ = DeleteObject(self.bitmap);
                let _ = DeleteDC(self.dc);
            }
        }
    }

    struct WindowState {
        hwnd: HWND,
        state: Option<NativeState>,
        text: String,
        wave_active: bool,
        wave_level: f32,
        wave_peaks: Vec<f32>,
        timer_active: bool,
        /// UpdateLayeredWindow 不会把从未显示过的窗口摆上屏幕；
        /// 首次渲染后必须显式 ShowWindow 一次。
        shown: bool,
        dpi: u32,
        d2d: ID2D1Factory,
        dwrite: IDWriteFactory,
        body_format: IDWriteTextFormat,
        label_format: IDWriteTextFormat,
        fallback_format: IDWriteTextFormat,
        target: Option<ID2D1DCRenderTarget>,
        brush: Option<ID2D1SolidColorBrush>,
        dib: Option<Dib>,
    }

    impl WindowState {
        fn apply(&mut self, command: Command) {
            match command {
                // 与 WebView 的 prepare 语义一致：只重置内容，不改变可见性。
                Command::Prepare => {
                    self.text.clear();
                    self.wave_active = false;
                    self.wave_peaks.clear();
                    self.wave_level = 0.0;
                }
                Command::SetState(state) => {
                    self.state = Some(state);
                    // fallback 面板替代文本区与胶囊，与 WebView 行为一致。
                    if state == NativeState::Fallback {
                        self.text.clear();
                        self.wave_active = false;
                        self.wave_peaks.clear();
                    }
                    self.sync_timer();
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
                Command::SetText(text) => {
                    self.text = text;
                    if self.state.is_some() {
                        self.render();
                    }
                }
                Command::SetWaveform { level, peaks } => {
                    // 波形数据只更新缓存，重绘交给 30fps 定时器，避免按
                    // 音频回调频率重复 BindDC + UpdateLayeredWindow。
                    self.wave_active = true;
                    self.wave_level = level;
                    self.wave_peaks = peaks;
                    self.sync_timer();
                }
                Command::Hide => {
                    self.state = None;
                    self.text.clear();
                    self.wave_active = false;
                    self.wave_peaks.clear();
                    self.sync_timer();
                    self.shown = false;
                    unsafe {
                        let _ = ShowWindow(self.hwnd, SW_HIDE);
                    }
                }
            }
        }

        fn sync_timer(&mut self) {
            let want = self.state == Some(NativeState::Recording) && self.wave_active;
            unsafe {
                if want && !self.timer_active {
                    SetTimer(self.hwnd, WAVE_TIMER_ID, WAVE_TIMER_MS, None);
                    self.timer_active = true;
                } else if !want && self.timer_active {
                    let _ = KillTimer(self.hwnd, WAVE_TIMER_ID);
                    self.timer_active = false;
                }
            }
        }

        fn ensure_target(&mut self, dpi: u32) -> bool {
            if self.target.is_some() {
                return true;
            }
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: dpi as f32,
                dpiY: dpi as f32,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = unsafe { self.d2d.CreateDCRenderTarget(&props) };
            match target {
                Ok(target) => {
                    let brush = unsafe {
                        target.CreateSolidColorBrush(&rgba(0.0, 0.0, 0.0, 0.0), None)
                    };
                    match brush {
                        Ok(brush) => {
                            self.brush = Some(brush);
                            self.target = Some(target);
                            true
                        }
                        Err(error) => {
                            eprintln!("[native-indicator] 创建画刷失败：{error}");
                            false
                        }
                    }
                }
                Err(error) => {
                    eprintln!("[native-indicator] 创建 D2D 渲染目标失败：{error}");
                    false
                }
            }
        }

        fn render(&mut self) {
            if self.state.is_none() {
                return;
            }
            let dpi = unsafe {
                let dpi = GetDpiForWindow(self.hwnd);
                if dpi == 0 {
                    GetDpiForSystem()
                } else {
                    dpi
                }
            };
            self.dpi = dpi;
            let (x, y, width, height) = placement(dpi);
            if self
                .dib
                .as_ref()
                .map_or(true, |dib| dib.width != width || dib.height != height)
            {
                self.dib = Dib::create(width, height);
            }
            if !self.ensure_target(dpi) {
                return;
            }
            // COM 对象克隆只是 AddRef、HDC 是 Copy，换成局部值后不再借用 self，
            // EndDraw 失败时才能就地丢弃渲染目标。
            let (dib_dc, dib_width, dib_height, target, brush) = {
                let (Some(dib), Some(target), Some(brush)) =
                    (&self.dib, &self.target, &self.brush)
                else {
                    return;
                };
                (dib.dc, dib.width, dib.height, target.clone(), brush.clone())
            };
            let rect = RECT {
                left: 0,
                top: 0,
                right: dib_width,
                bottom: dib_height,
            };
            unsafe {
                target.SetDpi(dpi as f32, dpi as f32);
                if let Err(error) = target.BindDC(dib_dc, &rect) {
                    eprintln!("[native-indicator] 绑定绘制 DC 失败：{error}");
                    return;
                }
                target.BeginDraw();
                target.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
                self.draw_content(&target, &brush);
                match target.EndDraw(None, None) {
                    Ok(()) => {}
                    // 设备丢失：丢弃渲染目标，下一帧重建。
                    Err(error) if error.code() == RECREATE_TARGET => {
                        self.target = None;
                        self.brush = None;
                        return;
                    }
                    Err(error) => {
                        eprintln!("[native-indicator] 结束绘制失败：{error}");
                        return;
                    }
                }
                let destination = POINT { x, y };
                let size = SIZE {
                    cx: width,
                    cy: height,
                };
                let origin = POINT { x: 0, y: 0 };
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                if let Err(error) = UpdateLayeredWindow(
                    self.hwnd,
                    None,
                    Some(&destination),
                    Some(&size),
                    dib_dc,
                    Some(&origin),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                ) {
                    eprintln!("[native-indicator] 上屏失败：{error}");
                    return;
                }
                if !self.shown {
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                    self.shown = true;
                }
            }
        }

        fn draw_content(&self, target: &ID2D1DCRenderTarget, brush: &ID2D1SolidColorBrush) {
            let Some(state) = self.state else { return };
            // #wrap：纵向居中堆叠、底部对齐，padding-bottom 24。
            let content_bottom = LOGICAL_HEIGHT - WRAP_PADDING_BOTTOM;
            if state == NativeState::Fallback {
                self.draw_fallback(target, brush, content_bottom);
                return;
            }
            let pill_rect = rect_f(
                (LOGICAL_WIDTH - PILL_W) / 2.0,
                content_bottom - PILL_H,
                (LOGICAL_WIDTH + PILL_W) / 2.0,
                content_bottom,
            );
            if !self.text.is_empty() {
                let text_bottom = pill_rect.top - STACK_GAP;
                let text_rect = rect_f(
                    (LOGICAL_WIDTH - TEXT_W) / 2.0,
                    text_bottom - TEXT_H,
                    (LOGICAL_WIDTH + TEXT_W) / 2.0,
                    text_bottom,
                );
                self.draw_text_box(target, brush, text_rect);
            }
            self.draw_pill(target, brush, state, pill_rect);
        }

        fn fill_rounded(
            &self,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            rect: D2D_RECT_F,
            radius: f32,
            fill: D2D1_COLOR_F,
            border: D2D1_COLOR_F,
        ) {
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: radius,
                radiusY: radius,
            };
            unsafe {
                brush.SetColor(&fill);
                target.FillRoundedRectangle(&rounded, brush);
                brush.SetColor(&border);
                target.DrawRoundedRectangle(&rounded, brush, 1.0, None);
            }
        }

        fn draw_text_box(
            &self,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            rect: D2D_RECT_F,
        ) {
            self.fill_rounded(
                target,
                brush,
                rect,
                TEXT_RADIUS,
                rgba(12.0 / 255.0, 16.0 / 255.0, 24.0 / 255.0, 0.96),
                rgba(1.0, 1.0, 1.0, 0.08),
            );
            let wide: Vec<u16> = self.text.encode_utf16().collect();
            let inner_w = TEXT_W - TEXT_PAD * 2.0;
            let layout = unsafe {
                self.dwrite
                    .CreateTextLayout(&wide, &self.body_format, inner_w, 10_000.0)
            };
            let Ok(layout) = layout else {
                return;
            };
            let mut metrics = DWRITE_TEXT_METRICS::default();
            if unsafe { layout.GetMetrics(&mut metrics) }.is_err() {
                return;
            }
            let inner = rect_f(
                rect.left + TEXT_PAD,
                rect.top + TEXT_PAD_Y,
                rect.right - TEXT_PAD,
                rect.top + TEXT_PAD_Y + TEXT_VISIBLE_H,
            );
            // 滚动累积模式：超出可见高度时整体上移，保证最新内容留在底部可见区。
            let offset = (metrics.height - TEXT_VISIBLE_H).max(0.0);
            unsafe {
                target.PushAxisAlignedClip(&inner, D2D1_ANTIALIAS_MODE_ALIASED);
                brush.SetColor(&rgba(234.0 / 255.0, 240.0 / 255.0, 1.0, 1.0));
                target.DrawTextLayout(
                    D2D_POINT_2F {
                        x: inner.left,
                        y: inner.top - offset,
                    },
                    &layout,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                target.PopAxisAlignedClip();
            }
        }

        fn draw_pill(
            &self,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            state: NativeState,
            rect: D2D_RECT_F,
        ) {
            self.fill_rounded(
                target,
                brush,
                rect,
                PILL_RADIUS,
                rgba(12.0 / 255.0, 16.0 / 255.0, 24.0 / 255.0, 0.94),
                rgba(1.0, 1.0, 1.0, 0.08),
            );
            if state == NativeState::Recording && self.wave_active {
                self.draw_waveform(target, brush, rect);
                return;
            }
            // 状态点 + 标签，inline-flex 居中，间距 8px。
            let label = state.label();
            let wide: Vec<u16> = label.encode_utf16().collect();
            let label_w = unsafe {
                self.dwrite
                    .CreateTextLayout(&wide, &self.label_format, 1_000.0, 100.0)
                    .and_then(|layout| {
                        let mut metrics = DWRITE_TEXT_METRICS::default();
                        layout.GetMetrics(&mut metrics).map(|_| metrics.width)
                    })
            }
            .unwrap_or(0.0);
            let total = DOT_RADIUS * 2.0 + DOT_LABEL_GAP + label_w;
            let center_x = (rect.left + rect.right) / 2.0;
            let center_y = (rect.top + rect.bottom) / 2.0;
            let dot_color = if state == NativeState::Recording {
                rgba(1.0, 77.0 / 255.0, 79.0 / 255.0, 1.0)
            } else {
                rgba(109.0 / 255.0, 174.0 / 255.0, 1.0, 1.0)
            };
            unsafe {
                brush.SetColor(&dot_color);
                target.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: center_x - total / 2.0 + DOT_RADIUS,
                            y: center_y,
                        },
                        radiusX: DOT_RADIUS,
                        radiusY: DOT_RADIUS,
                    },
                    brush,
                );
                brush.SetColor(&rgba(1.0, 1.0, 1.0, 1.0));
                target.DrawText(
                    &wide,
                    &self.label_format,
                    &rect_f(
                        center_x - total / 2.0 + DOT_RADIUS * 2.0 + DOT_LABEL_GAP,
                        rect.top,
                        rect.right,
                        rect.bottom,
                    ),
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
            pill: D2D_RECT_F,
        ) {
            let area_left = pill.left + (PILL_W - WAVE_AREA_W) / 2.0;
            let area_top = pill.top + (PILL_H - WAVE_AREA_H) / 2.0;
            // 柱宽/间距按物理像素取整再换回 DIP，与 floatingOrbWaveLayout 的
            // 像素对齐一致，避免不同缩放比例下细柱粗细不一。
            let dpr = self.dpi as f32 / 96.0;
            let dpr = if dpr > 0.0 { dpr } else { 1.0 };
            let bar = ((WAVE_AREA_W * dpr * WAVE_BAR_RATIO).round() as i32).max(1) as f32 / dpr;
            let gap = ((WAVE_AREA_W * dpr * WAVE_GAP_RATIO).round() as i32).max(1) as f32 / dpr;
            let total = WAVE_BAR_COUNT as f32 * bar + (WAVE_BAR_COUNT - 1) as f32 * gap;
            let start = area_left + ((WAVE_AREA_W * dpr - total * dpr) / 2.0).round() / dpr;
            let levels = resample_wave_levels(&self.wave_peaks, wave_scale(self.wave_level));
            unsafe {
                brush.SetColor(&rgba(118.0 / 255.0, 167.0 / 255.0, 1.0, 1.0));
                for (index, level) in levels.iter().enumerate() {
                    let scale = level.max(WAVE_BAR_MIN_SCALE);
                    let mut height = WAVE_AREA_H * WAVE_BAR_HEIGHTS[index] * scale;
                    // CSS 的 min-height: 柱宽——低响度时收成小圆点。
                    height = height.max(bar);
                    let left = start + index as f32 * (bar + gap);
                    let top = area_top + (WAVE_AREA_H - height) / 2.0;
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

        fn draw_fallback(
            &self,
            target: &ID2D1DCRenderTarget,
            brush: &ID2D1SolidColorBrush,
            content_bottom: f32,
        ) {
            let rect = rect_f(
                (LOGICAL_WIDTH - FALLBACK_W) / 2.0,
                content_bottom - FALLBACK_H,
                (LOGICAL_WIDTH + FALLBACK_W) / 2.0,
                content_bottom,
            );
            self.fill_rounded(
                target,
                brush,
                rect,
                TEXT_RADIUS,
                rgba(12.0 / 255.0, 24.0 / 255.0, 42.0 / 255.0, 0.97),
                rgba(109.0 / 255.0, 174.0 / 255.0, 1.0, 0.34),
            );
            let icon_color = rgba(109.0 / 255.0, 174.0 / 255.0, 1.0, 1.0);
            let icon_cx = rect.left + FALLBACK_PAD + FALLBACK_ICON / 2.0;
            let icon_cy = (rect.top + rect.bottom) / 2.0;
            unsafe {
                // 简化图标：圆环 + 对勾，代替 ClipboardCheck。
                brush.SetColor(&icon_color);
                target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: icon_cx,
                            y: icon_cy,
                        },
                        radiusX: FALLBACK_ICON / 2.0 - 1.0,
                        radiusY: FALLBACK_ICON / 2.0 - 1.0,
                    },
                    brush,
                    1.8,
                    None,
                );
                let check = [
                    (icon_cx - 3.5, icon_cy + 0.5),
                    (icon_cx - 0.8, icon_cy + 3.2),
                    (icon_cx + 4.0, icon_cy - 3.0),
                ];
                target.DrawLine(
                    D2D_POINT_2F {
                        x: check[0].0,
                        y: check[0].1,
                    },
                    D2D_POINT_2F {
                        x: check[1].0,
                        y: check[1].1,
                    },
                    brush,
                    2.0,
                    None,
                );
                target.DrawLine(
                    D2D_POINT_2F {
                        x: check[1].0,
                        y: check[1].1,
                    },
                    D2D_POINT_2F {
                        x: check[2].0,
                        y: check[2].1,
                    },
                    brush,
                    2.0,
                    None,
                );
                brush.SetColor(&rgba(234.0 / 255.0, 242.0 / 255.0, 1.0, 1.0));
                target.DrawText(
                    &"已经把结果放到你的剪贴板里了，你粘贴就可以用了"
                        .encode_utf16()
                        .collect::<Vec<u16>>(),
                    &self.fallback_format,
                    &rect_f(
                        rect.left + FALLBACK_PAD + FALLBACK_ICON + FALLBACK_ICON_GAP,
                        rect.top,
                        rect.right - FALLBACK_PAD,
                        rect.bottom,
                    ),
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    /// 主显示器工作区底部居中，物理像素坐标。
    fn placement(dpi: u32) -> (i32, i32, i32, i32) {
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        let width = (LOGICAL_WIDTH * scale).round() as i32;
        let height = (LOGICAL_HEIGHT * scale).round() as i32;
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
                area = info.rcWork;
            }
        }
        let x = area.left + (area.right - area.left - width) / 2;
        let y = area.bottom - height - (OFFSET_Y * scale).round() as i32;
        (x, y, width, height)
    }

    fn create_text_format(
        dwrite: &IDWriteFactory,
        size: f32,
        centered: bool,
        vertical_center: bool,
    ) -> Result<IDWriteTextFormat, String> {
        unsafe {
            let format = dwrite
                .CreateTextFormat(
                    w!("Microsoft YaHei UI"),
                    None,
                    DWRITE_FONT_WEIGHT_REGULAR,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("zh-CN"),
                )
                .map_err(|error| format!("创建文字格式失败：{error}"))?;
            let _ = format.SetTextAlignment(if centered {
                DWRITE_TEXT_ALIGNMENT_CENTER
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            });
            let _ = format.SetParagraphAlignment(if vertical_center {
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER
            } else {
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR
            });
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            Ok(format)
        }
    }

    fn with_state(hwnd: HWND, f: impl FnOnce(&mut WindowState)) {
        unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !ptr.is_null() {
                f(&mut *ptr);
            }
        }
    }

    fn drain_commands(hwnd: HWND) {
        let Some(shared) = UI.get().and_then(Option::as_ref) else {
            return;
        };
        loop {
            let command = shared
                .queue
                .lock()
                .ok()
                .and_then(|mut queue| queue.pop_front());
            let Some(command) = command else { break };
            with_state(hwnd, |state| state.apply(command));
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_COMMAND_QUEUED => {
                drain_commands(hwnd);
                LRESULT(0)
            }
            WM_TIMER => {
                with_state(hwnd, |state| state.render());
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let dpi = (wparam.0 & 0xFFFF) as u32;
                with_state(hwnd, |state| {
                    if dpi != 0 {
                        state.dpi = dpi;
                    }
                    // DPI 变化后物理尺寸改变，强制重建 DIB。
                    state.dib = None;
                    state.render();
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

    fn ui_thread_main() {
        unsafe {
            let instance = match GetModuleHandleW(None) {
                Ok(instance) => instance,
                Err(error) => {
                    eprintln!("[native-indicator] 读取模块句柄失败：{error}");
                    return;
                }
            };
            let class_name = w!("SayItNativeDictationIndicator");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                eprintln!("[native-indicator] 注册窗口类失败");
                return;
            }
            let d2d: ID2D1Factory =
                match D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) {
                    Ok(factory) => factory,
                    Err(error) => {
                        eprintln!("[native-indicator] 创建 D2D 工厂失败：{error}");
                        return;
                    }
                };
            let dwrite: IDWriteFactory = match DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) {
                Ok(factory) => factory,
                Err(error) => {
                    eprintln!("[native-indicator] 创建 DirectWrite 工厂失败：{error}");
                    return;
                }
            };
            let (body_format, label_format, fallback_format) = match (
                create_text_format(&dwrite, 14.0, false, false),
                create_text_format(&dwrite, 13.0, false, true),
                create_text_format(&dwrite, 13.0, false, true),
            ) {
                (Ok(body), Ok(label), Ok(fallback)) => (body, label, fallback),
                _ => return,
            };
            let dpi = GetDpiForSystem();
            let (x, y, width, height) = placement(dpi);
            let hwnd = match CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                class_name,
                w!("说吧！语音输入"),
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
                    eprintln!("[native-indicator] 创建指示器窗口失败：{error}");
                    return;
                }
            };
            let state = Box::new(WindowState {
                hwnd,
                state: None,
                text: String::new(),
                wave_active: false,
                wave_level: 0.0,
                wave_peaks: Vec::new(),
                timer_active: false,
                shown: false,
                dpi,
                d2d,
                dwrite,
                body_format,
                label_format,
                fallback_format,
                target: None,
                brush: None,
                dib: None,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                // PostThreadMessageW 投递的是线程消息（hwnd 为空），不会经过
                // 窗口过程，必须在消息循环里直接认领，否则命令永远堆积。
                if message.hwnd.is_invalid() && message.message == WM_COMMAND_QUEUED {
                    drain_commands(hwnd);
                    continue;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            // 原型不做优雅退出；进程退出时这里一般不会执行到。
            let _ = DestroyWindow(hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_scale_matches_the_perceptual_loudness_curve() {
        assert_eq!(wave_scale(f32::NAN), 0.0);
        assert_eq!(wave_scale(-1.0), 0.0);
        assert_eq!(wave_scale(0.0), 0.0);
        assert_eq!(wave_scale(1.0), 1.0);
        assert_eq!(wave_scale(2.0), 1.0);
        assert!((wave_scale(0.25) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn resample_fills_fallback_when_no_levels() {
        assert_eq!(resample_wave_levels(&[], 0.5), [0.5; WAVE_BAR_COUNT]);
        assert_eq!(resample_wave_levels(&[0.3], 0.0), [0.3; WAVE_BAR_COUNT]);
    }

    #[test]
    fn resample_interpolates_like_the_webview() {
        let levels = [0.0, 1.0];
        let bars = resample_wave_levels(&levels, 0.0);
        assert_eq!(bars[0], 0.0);
        assert_eq!(bars[WAVE_BAR_COUNT - 1], 1.0);
        assert!((bars[4] - 0.5).abs() < 1e-6);

        let six = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
        let bars = resample_wave_levels(&six, 0.0);
        // 6 → 9：position = index * 5 / 8，索引 4 落在 2.5 处。
        assert!((bars[4] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn wave_bar_height_table_matches_css() {
        assert_eq!(WAVE_BAR_HEIGHTS.len(), WAVE_BAR_COUNT);
        assert_eq!(WAVE_BAR_HEIGHTS[0], 0.46);
        assert_eq!(WAVE_BAR_HEIGHTS[4], 0.94);
        assert!(WAVE_BAR_MIN_SCALE > 0.0);
    }
}

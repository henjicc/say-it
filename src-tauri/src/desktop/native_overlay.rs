//! Windows 原生分层窗口的共享基础设施。
//!
//! 听写指示器（native_indicator）与悬浮球（native_orb）共用同一套
//! 「独立 UI 线程 + 命令队列 + UpdateLayeredWindow + Direct2D/DirectWrite」
//! 骨架；本模块只承载与业务无关的部分，窗口样式、绘制内容与鼠标交互
//! 由各自的窗口模块实现。

#![cfg(windows)]

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_REGULAR,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BLENDFUNCTION, HBITMAP, HDC, HGDIOBJ, AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, ShowWindow,
    TranslateMessage, UpdateLayeredWindow, MSG, PM_NOREMOVE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_USER,
};

/// EndDraw 返回的设备丢失错误码：丢弃渲染目标，下一帧重建。
pub(crate) const RECREATE_TARGET: HRESULT = HRESULT(0x8899000Cu32 as i32);

/// 出场/退场过渡的时长。入场与 indicator.css 的 wrapIn（0.24s ease-out）一致。
pub(crate) const TRANSITION_ENTER_MS: u64 = 240;
pub(crate) const TRANSITION_EXIT_MS: u64 = 140;

/// 窗口出现/消失的过渡动画驱动器：只输出 0..1 的视觉进度，
/// 透明度与位移怎么用它由各自窗口决定。只认 Show/Hide 语义——
/// 状态内容更新（如录音中改文案）不重播动画。
pub(crate) struct Transition {
    /// 未缓动的线性进度：0 完全隐藏，1 完全显示。
    progress: f32,
    /// +1 入场中，-1 退场中，0 静止。
    direction: i8,
    last_tick: Option<Instant>,
}

impl Transition {
    pub(crate) fn new(shown: bool) -> Self {
        Self {
            progress: if shown { 1.0 } else { 0.0 },
            direction: 0,
            last_tick: None,
        }
    }

    /// 进入或回到显示态。已完全显示时是空操作；退场中途调用则从当前位置反向回播。
    pub(crate) fn show(&mut self) {
        if self.progress < 1.0 && self.direction != 1 {
            self.direction = 1;
            self.last_tick = None;
        }
    }

    /// 开始退场；返回当前是否有可见内容可退（没有则调用方应立即隐藏，不必等动画）。
    pub(crate) fn hide(&mut self) -> bool {
        if self.progress > 0.0 {
            self.direction = -1;
            self.last_tick = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.direction != 0
    }

    pub(crate) fn is_fully_hidden(&self) -> bool {
        self.progress <= 0.0 && self.direction <= 0
    }

    /// 推进一帧，返回（缓动后的视觉进度，本次是否刚好到达终点）。
    pub(crate) fn tick(&mut self) -> (f32, bool) {
        let now = Instant::now();
        let dt = self
            .last_tick
            .replace(now)
            .map(|last| now.saturating_duration_since(last).as_secs_f32())
            .unwrap_or(0.0);
        self.advance(dt)
    }

    /// 与 tick 分离的纯推进逻辑，便于单测注入固定帧间隔。
    pub(crate) fn advance(&mut self, dt_seconds: f32) -> (f32, bool) {
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
    pub(crate) fn visual(&self) -> f32 {
        let t = self.progress.clamp(0.0, 1.0);
        if self.direction < 0 {
            t * t
        } else {
            1.0 - (1.0 - t).powi(3)
        }
    }
}

/// 命令入队后投递给 UI 线程的线程消息。线程消息（hwnd 为空）不经过窗口过程，
/// 必须在消息循环里手动认领，否则命令永远堆积。
pub(crate) const WM_COMMAND_QUEUED: u32 = WM_USER + 0x0101;

/// 预乘 alpha 颜色。
pub(crate) const fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r * a,
        g: g * a,
        b: b * a,
        a,
    }
}

pub(crate) const fn rect_f(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}

/// 窗口当前 DPI，失败时退回系统 DPI。
pub(crate) fn window_dpi(hwnd: HWND) -> u32 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            GetDpiForSystem()
        } else {
            dpi
        }
    }
}

/// 内存 DC + 顶向下 32bpp DIB section。GDI 句柄用 Drop 释放，
/// 重复 show/hide 与尺寸重建不得泄漏。
pub(crate) struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl Dib {
    pub(crate) fn create(width: i32, height: i32) -> Option<Self> {
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
            let bitmap = match CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
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

    pub(crate) fn dc(&self) -> HDC {
        self.dc
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

/// 一个常驻的原生 UI 线程：持有一个分层窗口与一条命令队列。
pub(crate) struct OverlayThread<C> {
    thread_id: u32,
    queue: Arc<Mutex<VecDeque<C>>>,
}

impl<C: Send + 'static> OverlayThread<C> {
    /// `create_window` 在 UI 线程上执行（注册窗口类、创建窗口、挂好窗口状态），
    /// 返回窗口句柄；返回 None 表示创建失败，线程直接退出，后续命令静默丢弃。
    /// `on_command` 在 UI 线程上逐条消费命令队列。
    pub(crate) fn start(
        thread_name: &str,
        create_window: impl FnOnce() -> Option<HWND> + Send + 'static,
        on_command: fn(HWND, C),
    ) -> Result<Self, String> {
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let thread_queue = queue.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name(thread_name.to_string())
            .spawn(move || {
                // 先强制创建线程消息队列，否则 ready 之前到达的
                // PostThreadMessageW 会被系统直接丢弃。
                let mut message = MSG::default();
                unsafe {
                    let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                    let _ = ready_tx.send(GetCurrentThreadId());
                }
                let Some(hwnd) = create_window() else {
                    return;
                };
                message_loop(hwnd, &thread_queue, on_command);
            })
            .map_err(|error| format!("创建原生窗口线程失败：{error}"))?;
        let thread_id = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "启动原生窗口线程超时".to_string())?;
        Ok(Self { thread_id, queue })
    }

    /// 任意线程可调用：命令入队并唤醒 UI 线程。
    pub(crate) fn post(&self, command: C) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        queue.push_back(command);
        drop(queue);
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_COMMAND_QUEUED, WPARAM(0), LPARAM(0));
        }
    }
}

fn message_loop<C>(hwnd: HWND, queue: &Mutex<VecDeque<C>>, on_command: fn(HWND, C)) {
    let mut message = MSG::default();
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            // PostThreadMessageW 投递的是线程消息（hwnd 为空），不会经过
            // 窗口过程，必须在消息循环里直接认领。
            if message.hwnd.is_invalid() && message.message == WM_COMMAND_QUEUED {
                loop {
                    let command = queue.lock().ok().and_then(|mut queue| queue.pop_front());
                    let Some(command) = command else { break };
                    on_command(hwnd, command);
                }
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        // 原型不做优雅退出；进程退出时这里一般不会执行到。
        let _ = DestroyWindow(hwnd);
    }
}

/// 分层窗口的 D2D 渲染面：管理 DIB、DC 渲染目标与 UpdateLayeredWindow 上屏。
pub(crate) struct LayeredSurface {
    hwnd: HWND,
    dib: Option<Dib>,
    target: Option<ID2D1DCRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    /// UpdateLayeredWindow 不会把从未显示过的窗口摆上屏幕；
    /// 首次渲染后必须显式 ShowWindow 一次。
    shown: bool,
}

impl LayeredSurface {
    pub(crate) fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            dib: None,
            target: None,
            brush: None,
            shown: false,
        }
    }

    /// 隐藏窗口，下次 render 上屏后需要重新 ShowWindow 一次。
    pub(crate) fn mark_hidden(&mut self) {
        self.shown = false;
    }

    /// DPI 变化后物理尺寸改变，强制重建 DIB。
    pub(crate) fn discard_dib(&mut self) {
        self.dib = None;
    }

    fn ensure_target(&mut self, d2d: &ID2D1Factory, dpi: u32, log_tag: &str) -> bool {
        if self.target.is_some() {
            return true;
        }
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            // 软件渲染：悬浮球/指示器只有几 KB 的像素量，GPU 加速无收益，
            // 而硬件渲染目标会创建 D3D11 设备并把整套显卡驱动 DLL
            // （nvgpucomp64 等，映射上百 MB）拉进进程。
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi as f32,
            dpiY: dpi as f32,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let target = unsafe { d2d.CreateDCRenderTarget(&props) };
        match target {
            Ok(target) => {
                let brush = unsafe { target.CreateSolidColorBrush(&rgba(0.0, 0.0, 0.0, 0.0), None) };
                match brush {
                    Ok(brush) => {
                        self.brush = Some(brush);
                        self.target = Some(target);
                        true
                    }
                    Err(error) => {
                        eprintln!("[{log_tag}] 创建画刷失败：{error}");
                        false
                    }
                }
            }
            Err(error) => {
                eprintln!("[{log_tag}] 创建 D2D 渲染目标失败：{error}");
                false
            }
        }
    }

    /// 绘制一帧并上屏。坐标与尺寸为物理像素；D2D 内按 dpi 折算成 DIP 绘制。
    /// `constant_alpha` 乘到整窗不透明度上（对应外观设置的整体透明度）。
    pub(crate) fn render(
        &mut self,
        d2d: &ID2D1Factory,
        dpi: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        constant_alpha: u8,
        log_tag: &str,
        draw: impl FnOnce(&ID2D1DCRenderTarget, &ID2D1SolidColorBrush),
    ) {
        if self
            .dib
            .as_ref()
            .map_or(true, |dib| dib.width != width || dib.height != height)
        {
            self.dib = Dib::create(width, height);
        }
        if !self.ensure_target(d2d, dpi, log_tag) {
            return;
        }
        // COM 对象克隆只是 AddRef、HDC 是 Copy，换成局部值后不再借用 self，
        // EndDraw 失败时才能就地丢弃渲染目标。
        let (dib_dc, dib_width, dib_height, target, brush) = {
            let (Some(dib), Some(target), Some(brush)) = (&self.dib, &self.target, &self.brush)
            else {
                return;
            };
            (dib.dc(), dib.width, dib.height, target.clone(), brush.clone())
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
                eprintln!("[{log_tag}] 绑定绘制 DC 失败：{error}");
                return;
            }
            target.BeginDraw();
            target.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
            draw(&target, &brush);
            match target.EndDraw(None, None) {
                Ok(()) => {}
                Err(error) if error.code() == RECREATE_TARGET => {
                    self.target = None;
                    self.brush = None;
                    return;
                }
                Err(error) => {
                    eprintln!("[{log_tag}] 结束绘制失败：{error}");
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
                SourceConstantAlpha: constant_alpha,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            // 先 ShowWindow 再提交内容：对 SW_HIDE 过的窗口调用 UpdateLayeredWindow
            // 可能直接失败，若把 ShowWindow 放在 ULW 之后会永远卡在「窗口仍隐藏、
            // shown 标志未置位、每次渲染都失败」的死循环里。
            if !self.shown {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                self.shown = true;
            }
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
                eprintln!("[{log_tag}] 上屏失败：{error}");
                return;
            }
        }
    }
}

pub(crate) fn create_d2d_factory() -> Result<ID2D1Factory, String> {
    unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
        .map_err(|error| format!("创建 D2D 工厂失败：{error}"))
}

pub(crate) fn create_dwrite_factory() -> Result<IDWriteFactory, String> {
    unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
        .map_err(|error| format!("创建 DirectWrite 工厂失败：{error}"))
}

pub(crate) fn create_text_format(
    dwrite: &IDWriteFactory,
    family: &str,
    size: f32,
    centered: bool,
    vertical_center: bool,
) -> Result<IDWriteTextFormat, String> {
    let family_wide: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let format = dwrite
            .CreateTextFormat(
                PCWSTR(family_wide.as_ptr()),
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

#[cfg(test)]
mod tests {
    use super::{Transition, TRANSITION_ENTER_MS};

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

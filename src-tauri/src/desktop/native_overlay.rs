//! Windows 原生分层窗口的共享基础设施。
//!
//! 听写指示器（native_indicator）与悬浮球（native_orb）共用同一套
//! 「独立 UI 线程 + 命令队列 + UpdateLayeredWindow + Direct2D/DirectWrite」
//! 骨架；本模块只承载与业务无关的部分，窗口样式、绘制内容与鼠标交互
//! 由各自的窗口模块实现。

#![cfg(windows)]

use sayit_overlay_core::svg::{parse_svg_path, SvgSeg};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING,
    D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1PathGeometry, ID2D1SolidColorBrush,
    D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_REGULAR, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
#[cfg(test)]
use windows::Win32::Graphics::Gdi::GdiFlush;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, ShowWindow,
    TranslateMessage, UpdateLayeredWindow, MSG, PM_NOREMOVE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_USER,
};

/// EndDraw 返回的设备丢失错误码：丢弃渲染目标，下一帧重建。
pub(crate) const RECREATE_TARGET: HRESULT = HRESULT(0x8899000Cu32 as i32);

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

/// 悬浮窗动画计时的唯一时钟入口：生产恒为 `Instant::now()`；测试线程可用
/// `test_seams::set_now` 固定当前时刻，让动画只随注入的帧时刻推进。
pub(crate) fn overlay_now() -> Instant {
    #[cfg(test)]
    if let Some(now) = test_seams::now() {
        return now;
    }
    Instant::now()
}

/// 建窗时使用的系统 DPI；测试线程的 DPI 覆盖同样作用于此，保证首次布局与渲染一致。
pub(crate) fn system_dpi() -> u32 {
    #[cfg(test)]
    if let Some(dpi) = test_seams::dpi() {
        return dpi;
    }
    unsafe { GetDpiForSystem() }
}

/// 窗口当前 DPI，失败时退回系统 DPI。
pub(crate) fn window_dpi(hwnd: HWND) -> u32 {
    #[cfg(test)]
    if let Some(dpi) = test_seams::dpi() {
        return dpi;
    }
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
    bits: *const u8,
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
                bits: bits.cast(),
                width,
                height,
            })
        }
    }

    pub(crate) fn dc(&self) -> HDC {
        self.dc
    }

    /// 顶向下、预乘 alpha 的 BGRA 像素；调用方须先完成 D2D EndDraw。
    pub(crate) fn pixels(&self) -> &[u8] {
        // CreateDIBSection 成功后像素区由 bitmap 持有，借用不超过 Dib 生命周期。
        unsafe {
            std::slice::from_raw_parts(self.bits, self.width as usize * self.height as usize * 4)
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

/// 主画布与临时特效画布共用软件渲染，避免为小窗口加载 GPU 设备和驱动。
pub(crate) fn create_dc_render_target(
    d2d: &ID2D1Factory,
    dpi: f32,
) -> windows::core::Result<ID2D1DCRenderTarget> {
    unsafe {
        d2d.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        })
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
    /// 离屏模式下最近一次本应上屏的参数；隐藏后清空，表示窗口不可见。
    #[cfg(test)]
    presented: Option<test_seams::Presented>,
    /// 累计渲染尝试次数（含失败）；与 `presented.frame` 不等说明最近一次渲染没有出帧。
    #[cfg(test)]
    frames: u64,
}

impl LayeredSurface {
    pub(crate) fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            dib: None,
            target: None,
            brush: None,
            shown: false,
            #[cfg(test)]
            presented: None,
            #[cfg(test)]
            frames: 0,
        }
    }

    /// 隐藏窗口，下次 render 上屏后需要重新 ShowWindow 一次。
    pub(crate) fn mark_hidden(&mut self) {
        self.shown = false;
        #[cfg(test)]
        {
            self.presented = None;
        }
    }

    /// 离屏模式下最近一帧的上屏参数与 DIB 像素副本；窗口隐藏或尚未上屏时为 None。
    #[cfg(test)]
    pub(crate) fn capture(&self) -> Option<test_seams::Capture> {
        let presented = self.presented?;
        let dib = self.dib.as_ref()?;
        // 读 DIB 前把 GDI 批处理刷到位图，保证拿到的是完整的最近一帧。
        unsafe {
            let _ = GdiFlush();
        }
        Some(test_seams::Capture {
            presented,
            rendered: self.frames,
            pixels: dib.pixels().to_vec(),
        })
    }

    /// DPI 变化后物理尺寸改变，强制重建 DIB。
    pub(crate) fn discard_dib(&mut self) {
        self.dib = None;
    }

    fn ensure_target(&mut self, d2d: &ID2D1Factory, dpi: u32, log_tag: &str) -> bool {
        if self.target.is_some() {
            return true;
        }
        let target = create_dc_render_target(d2d, dpi as f32);
        match target {
            Ok(target) => {
                let brush =
                    unsafe { target.CreateSolidColorBrush(&rgba(0.0, 0.0, 0.0, 0.0), None) };
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
        #[cfg(test)]
        {
            self.frames += 1;
        }
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
            (
                dib.dc(),
                dib.width,
                dib.height,
                target.clone(),
                brush.clone(),
            )
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
            #[cfg(test)]
            if test_seams::offscreen() {
                self.presented = Some(test_seams::Presented {
                    x,
                    y,
                    width,
                    height,
                    alpha: constant_alpha,
                    frame: self.frames,
                });
                return;
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

/// 把 SVG path 的 d 字符串构建成 D2D 路径几何体（设备无关资源，可跨帧复用）。
pub(crate) fn svg_path_geometry(d2d: &ID2D1Factory, d: &str) -> Result<ID2D1PathGeometry, String> {
    build_svg_path_geometry(d2d, d, true)
}

/// 描边用途的 SVG 路径几何体：图形保持开放不闭合，供 DrawGeometry 描边。
/// lucide 图标都是 stroke 风格——若按填充语义闭合图形，X、箭头等开放路径
/// 会多出一条首尾相连的回连线。
pub(crate) fn svg_path_geometry_stroke(
    d2d: &ID2D1Factory,
    d: &str,
) -> Result<ID2D1PathGeometry, String> {
    build_svg_path_geometry(d2d, d, false)
}

fn build_svg_path_geometry(
    d2d: &ID2D1Factory,
    d: &str,
    filled: bool,
) -> Result<ID2D1PathGeometry, String> {
    let segments = parse_svg_path(d)?;
    let geometry = unsafe { d2d.CreatePathGeometry() }
        .map_err(|error| format!("创建路径几何体失败：{error}"))?;
    let sink =
        unsafe { geometry.Open() }.map_err(|error| format!("打开路径几何体失败：{error}"))?;
    unsafe {
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        let mut figure_open = false;
        for segment in segments {
            match segment {
                SvgSeg::Move(x, y) => {
                    if figure_open {
                        sink.EndFigure(if filled {
                            D2D1_FIGURE_END_CLOSED
                        } else {
                            D2D1_FIGURE_END_OPEN
                        });
                    }
                    sink.BeginFigure(
                        D2D_POINT_2F { x, y },
                        if filled {
                            D2D1_FIGURE_BEGIN_FILLED
                        } else {
                            D2D1_FIGURE_BEGIN_HOLLOW
                        },
                    );
                    figure_open = true;
                }
                SvgSeg::Line(x, y) => sink.AddLine(D2D_POINT_2F { x, y }),
                SvgSeg::Bezier(c1x, c1y, c2x, c2y, x, y) => {
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: c1x, y: c1y },
                        point2: D2D_POINT_2F { x: c2x, y: c2y },
                        point3: D2D_POINT_2F { x, y },
                    });
                }
                SvgSeg::Arc {
                    x,
                    y,
                    rx,
                    ry,
                    rotation,
                    large,
                    sweep,
                } => {
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: D2D_POINT_2F { x, y },
                        size: D2D_SIZE_F {
                            width: rx,
                            height: ry,
                        },
                        rotationAngle: rotation,
                        sweepDirection: if sweep {
                            D2D1_SWEEP_DIRECTION_CLOCKWISE
                        } else {
                            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                        },
                        arcSize: if large {
                            D2D1_ARC_SIZE_LARGE
                        } else {
                            D2D1_ARC_SIZE_SMALL
                        },
                    });
                }
                SvgSeg::Close => {
                    if figure_open {
                        sink.EndFigure(if filled {
                            D2D1_FIGURE_END_CLOSED
                        } else {
                            D2D1_FIGURE_END_OPEN
                        });
                        figure_open = false;
                    }
                }
            }
        }
        if figure_open {
            sink.EndFigure(if filled {
                D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
        }
        sink.Close()
            .map_err(|error| format!("关闭路径几何体失败：{error}"))?;
    }
    Ok(geometry)
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
    create_text_format_weight(
        dwrite,
        family,
        size,
        DWRITE_FONT_WEIGHT_REGULAR,
        centered,
        vertical_center,
    )
}

/// 带字重的文字格式（字幕正文字重 600）。
pub(crate) fn create_text_format_weight(
    dwrite: &IDWriteFactory,
    family: &str,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
    centered: bool,
    vertical_center: bool,
) -> Result<IDWriteTextFormat, String> {
    let family_wide: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let format = dwrite
            .CreateTextFormat(
                PCWSTR(family_wide.as_ptr()),
                None,
                weight,
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

/// 只服务测试的接缝：时钟、DPI 与离屏上屏开关。全部是线程局部的——窗口属于
/// 创建它的测试线程，并行测试互不干扰；未设置时一律走生产路径。
#[cfg(test)]
pub(crate) mod test_seams {
    use std::cell::Cell;
    use std::time::Instant;

    thread_local! {
        static NOW: Cell<Option<Instant>> = const { Cell::new(None) };
        static DPI: Cell<Option<u32>> = const { Cell::new(None) };
        static OFFSCREEN: Cell<bool> = const { Cell::new(false) };
    }

    /// 离屏模式下一帧本应交给 UpdateLayeredWindow 的参数（物理像素）。
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Presented {
        pub(crate) x: i32,
        pub(crate) y: i32,
        pub(crate) width: i32,
        pub(crate) height: i32,
        pub(crate) alpha: u8,
        /// 出这一帧的渲染尝试序号（渲染面上第几次 render，从 1 开始）。
        pub(crate) frame: u64,
    }

    /// 截取结果：上屏参数 + 顶向下、预乘 alpha 的 BGRA 像素。
    pub(crate) struct Capture {
        pub(crate) presented: Presented,
        /// 截取时渲染面累计的渲染尝试次数；大于 `presented.frame` 表示之后的渲染失败了，
        /// 像素是旧帧。
        pub(crate) rendered: u64,
        pub(crate) pixels: Vec<u8>,
    }

    pub(crate) fn now() -> Option<Instant> {
        NOW.with(Cell::get)
    }

    pub(crate) fn set_now(now: Instant) {
        NOW.with(|cell| cell.set(Some(now)));
    }

    pub(crate) fn dpi() -> Option<u32> {
        DPI.with(Cell::get)
    }

    pub(crate) fn offscreen() -> bool {
        OFFSCREEN.with(Cell::get)
    }

    /// 当前线程进入离屏模式并固定 DPI；时钟保持真实时间，直到调用 `set_now`。
    /// 必须在建窗之前创建，离开作用域时恢复全部接缝。
    pub(crate) struct OffscreenGuard(());

    impl OffscreenGuard {
        pub(crate) fn new(dpi: u32) -> Self {
            NOW.with(|cell| cell.set(None));
            DPI.with(|cell| cell.set(Some(dpi)));
            OFFSCREEN.with(|cell| cell.set(true));
            Self(())
        }
    }

    impl Drop for OffscreenGuard {
        fn drop(&mut self) {
            NOW.with(|cell| cell.set(None));
            DPI.with(|cell| cell.set(None));
            OFFSCREEN.with(|cell| cell.set(false));
        }
    }
}

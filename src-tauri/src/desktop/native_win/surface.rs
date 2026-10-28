#[cfg(test)]
use super::test_seams;
use super::{create_dc_render_target, rgba, Dib, RECREATE_TARGET};
use std::rc::Rc;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::{ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush};
#[cfg(test)]
use windows::Win32::Graphics::Gdi::GdiFlush;
use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION};
use windows::Win32::UI::WindowsAndMessaging::{
    ShowWindow, UpdateLayeredWindow, SW_SHOWNOACTIVATE, ULW_ALPHA,
};

/// 分层窗口的 D2D 渲染面：管理 DIB、DC 渲染目标与 UpdateLayeredWindow 上屏。
pub(crate) struct LayeredSurface {
    hwnd: HWND,
    dib: Option<Rc<Dib>>,
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
        if let Some(presentation) =
            self.draw_frame(d2d, dpi, x, y, width, height, constant_alpha, log_tag, draw)
        {
            presentation.present(log_tag);
        }
    }

    /// 仅绘制到 DIB，返回独立持有缓冲的上屏请求；调用方释放状态借用后执行 present。
    /// 离屏测试在此记录参数，不产生上屏请求。
    pub(crate) fn draw_frame(
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
    ) -> Option<Presentation> {
        #[cfg(test)]
        {
            self.frames += 1;
        }
        if self
            .dib
            .as_ref()
            .map_or(true, |dib| dib.width != width || dib.height != height)
        {
            self.dib = Dib::create(width, height).map(Rc::new);
        }
        if !self.ensure_target(d2d, dpi, log_tag) {
            return None;
        }
        // COM 对象克隆只是 AddRef、HDC 是 Copy，换成局部值后不再借用 self，
        // EndDraw 失败时才能就地丢弃渲染目标。
        let (dib_dc, dib_width, dib_height, target, brush) = {
            let (Some(dib), Some(target), Some(brush)) = (&self.dib, &self.target, &self.brush)
            else {
                return None;
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
                return None;
            }
            target.BeginDraw();
            target.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
            draw(&target, &brush);
            match target.EndDraw(None, None) {
                Ok(()) => {}
                Err(error) if error.code() == RECREATE_TARGET => {
                    self.target = None;
                    self.brush = None;
                    return None;
                }
                Err(error) => {
                    eprintln!("[{log_tag}] 结束绘制失败：{error}");
                    return None;
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
                return None;
            }
            let show = !self.shown;
            self.shown = true;
            Some(Presentation {
                hwnd: self.hwnd,
                dib: self.dib.as_ref()?.clone(),
                x,
                y,
                width,
                height,
                constant_alpha,
                show,
            })
        }
    }
}

/// 持有当帧 DIB，允许 ShowWindow / ULW 重入后重建下一帧缓冲而不悬空。
/// 此值不借用窗口状态；只能在状态借用结束后上屏。
pub(crate) struct Presentation {
    hwnd: HWND,
    dib: Rc<Dib>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    constant_alpha: u8,
    show: bool,
}
impl Presentation {
    pub(crate) fn present(self, log_tag: &str) {
        let Self {
            hwnd,
            dib,
            x,
            y,
            width,
            height,
            constant_alpha,
            show,
        } = self;
        let dib_dc = dib.dc();
        unsafe {
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
            if show {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            if let Err(error) = UpdateLayeredWindow(
                hwnd,
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

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;
use std::slice;
use std::time::Instant;

use image::{DynamicImage, RgbaImage};
use windows::Win32::Foundation::{HANDLE, HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmFlush, DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HGDIOBJ,
    SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsIconic, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE,
};

pub(crate) struct CapturedWindowImage {
    pub(crate) image: DynamicImage,
    pub(crate) elapsed_ms: u64,
}

struct HiddenWindowGuard {
    window: HWND,
    hidden: bool,
}

impl HiddenWindowGuard {
    fn new(window_handle: Option<isize>) -> Self {
        let window = HWND(window_handle.unwrap_or_default() as *mut c_void);
        let hidden = !window.0.is_null() && unsafe { IsWindowVisible(window).as_bool() };
        if hidden {
            unsafe {
                let _ = ShowWindow(window, SW_HIDE);
                let _ = DwmFlush();
            }
        }
        Self { window, hidden }
    }
}

impl Drop for HiddenWindowGuard {
    fn drop(&mut self) {
        if self.hidden {
            unsafe {
                let _ = ShowWindow(self.window, SW_SHOWNOACTIVATE);
                let _ = DwmFlush();
            }
        }
    }
}

pub(crate) fn capture_window(
    window_handle: isize,
    occluding_window_handle: Option<isize>,
) -> Result<CapturedWindowImage, String> {
    let started = Instant::now();
    let window = HWND(window_handle as *mut c_void);
    if window.0.is_null()
        || !unsafe { IsWindowVisible(window).as_bool() }
        || unsafe { IsIconic(window).as_bool() }
    {
        return Err("目标窗口不可见、已最小化或已经关闭".into());
    }

    // 调试窗口置顶时会遮住目标应用。只在同步拷贝屏幕像素期间隐藏它，
    // DwmFlush 确保合成器已经提交显隐状态，避免调试界面进入截图。
    let _hidden_window = HiddenWindowGuard::new(occluding_window_handle);
    let bounds = window_bounds(window)?;
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    if width <= 0 || height <= 0 {
        return Err("目标窗口截图尺寸无效".into());
    }

    let mut pixels = capture_screen_rect(bounds, width, height)?;
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let image = RgbaImage::from_raw(width as u32, height as u32, pixels)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "无法构造窗口截图".to_string())?;

    Ok(CapturedWindowImage {
        image,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn window_bounds(window: HWND) -> Result<RECT, String> {
    let mut bounds = RECT::default();
    let dwm_result = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    };
    if dwm_result.is_err() || bounds.right <= bounds.left || bounds.bottom <= bounds.top {
        unsafe { GetWindowRect(window, &mut bounds) }
            .map_err(|error| format!("读取目标窗口区域失败：{error}"))?;
    }
    Ok(bounds)
}

fn capture_screen_rect(bounds: RECT, width: i32, height: i32) -> Result<Vec<u8>, String> {
    let screen_dc = unsafe { GetDC(HWND::default()) };
    if screen_dc.is_invalid() {
        return Err("获取屏幕设备上下文失败".into());
    }

    let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
    if memory_dc.is_invalid() {
        unsafe {
            ReleaseDC(HWND::default(), screen_dc);
        }
        return Err("创建截图内存设备上下文失败".into());
    }

    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        // 负高度创建自上而下的 DIB，省去整张图的垂直翻转。
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut bits = ptr::null_mut::<c_void>();
    let bitmap_result = unsafe {
        CreateDIBSection(
            screen_dc,
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        )
    };
    let bitmap = match bitmap_result {
        Ok(bitmap) => bitmap,
        Err(error) => {
            unsafe {
                let _ = DeleteDC(memory_dc);
                ReleaseDC(HWND::default(), screen_dc);
            }
            return Err(format!("创建截图位图失败：{error}"));
        }
    };

    let old_object = unsafe { SelectObject(memory_dc, HGDIOBJ(bitmap.0)) };
    if old_object.is_invalid() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory_dc);
            ReleaseDC(HWND::default(), screen_dc);
        }
        return Err("将截图位图选入内存设备上下文失败".into());
    }
    let capture_result = unsafe {
        BitBlt(
            memory_dc,
            0,
            0,
            width,
            height,
            screen_dc,
            bounds.left,
            bounds.top,
            SRCCOPY | CAPTUREBLT,
        )
    };
    let pixels = if capture_result.is_ok() && !bits.is_null() {
        let byte_len = width as usize * height as usize * 4;
        Some(unsafe { slice::from_raw_parts(bits.cast::<u8>(), byte_len) }.to_vec())
    } else {
        None
    };

    unsafe {
        SelectObject(memory_dc, old_object);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory_dc);
        ReleaseDC(HWND::default(), screen_dc);
    }

    capture_result.map_err(|error| format!("拷贝目标窗口屏幕像素失败：{error}"))?;
    pixels.ok_or_else(|| "截图位图没有返回像素".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn top_down_bgra_pixels_convert_to_opaque_rgba() {
        let mut pixels = vec![3, 2, 1, 0, 30, 20, 10, 128];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        assert_eq!(pixels, vec![1, 2, 3, 255, 10, 20, 30, 255]);
    }
}

use std::sync::mpsc::{sync_channel, SyncSender};
use std::time::Instant;

use image::{DynamicImage, RgbaImage};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

pub(crate) struct CapturedWindowImage {
    pub(crate) image: DynamicImage,
    pub(crate) elapsed_ms: u64,
    pub(crate) compatibility_notes: Vec<String>,
}

struct OneFrameCapture {
    sender: SyncSender<Result<(u32, u32, Vec<u8>), String>>,
}

impl GraphicsCaptureApiHandler for OneFrameCapture {
    type Flags = SyncSender<Result<(u32, u32, Vec<u8>), String>>;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { sender: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        let result = (|| {
            let buffer = frame
                .buffer()
                .map_err(|error| format!("读取窗口截图像素失败：{error}"))?;
            let mut no_padding = Vec::new();
            let pixels = buffer.as_nopadding_buffer(&mut no_padding).to_vec();
            Ok((width, height, pixels))
        })();
        let _ = self.sender.send(result);
        capture_control.stop();
        Ok(())
    }
}

fn capture_frame(
    window_handle: isize,
    cursor: CursorCaptureSettings,
    border: DrawBorderSettings,
    secondary_windows: SecondaryWindowSettings,
) -> Result<(u32, u32, Vec<u8>), String> {
    let window = Window::from_raw_hwnd(window_handle as *mut std::ffi::c_void);
    if !window.is_valid() {
        return Err("目标窗口不可捕获或已经关闭".into());
    }

    let (sender, receiver) = sync_channel(1);
    let settings = Settings::new(
        window,
        cursor,
        border,
        secondary_windows,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        sender,
    );
    OneFrameCapture::start(settings).map_err(|error| format!("Graphics Capture 初始化失败：{error}"))?;
    receiver
        .recv()
        .map_err(|_| "窗口截图任务未返回图像".to_string())?
}

fn optional_settings(
    cursor_supported: bool,
    border_supported: bool,
    secondary_supported: bool,
) -> (
    CursorCaptureSettings,
    DrawBorderSettings,
    SecondaryWindowSettings,
    Vec<String>,
) {
    let mut notes = Vec::new();
    if !cursor_supported {
        notes.push("当前 Windows 不支持切换截图鼠标状态，已使用系统默认设置。".into());
    }
    if !border_supported {
        notes.push("当前 Windows 不支持关闭捕获边框，已使用系统默认边框。".into());
    }
    if !secondary_supported {
        notes.push("当前 Windows 不支持排除附属窗口，已使用系统默认设置。".into());
    }
    (
        if cursor_supported {
            CursorCaptureSettings::WithoutCursor
        } else {
            CursorCaptureSettings::Default
        },
        if border_supported {
            DrawBorderSettings::WithoutBorder
        } else {
            DrawBorderSettings::Default
        },
        if secondary_supported {
            SecondaryWindowSettings::Exclude
        } else {
            SecondaryWindowSettings::Default
        },
        notes,
    )
}

fn preferred_optional_settings() -> (
    CursorCaptureSettings,
    DrawBorderSettings,
    SecondaryWindowSettings,
    Vec<String>,
) {
    optional_settings(
        GraphicsCaptureApi::is_cursor_settings_supported().unwrap_or(false),
        GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false),
        GraphicsCaptureApi::is_secondary_windows_supported().unwrap_or(false),
    )
}

pub(crate) fn capture_window(window_handle: isize) -> Result<CapturedWindowImage, String> {
    let started = Instant::now();
    let (cursor, border, secondary_windows, mut compatibility_notes) =
        preferred_optional_settings();
    let (width, height, pixels) = match capture_frame(
        window_handle,
        cursor,
        border,
        secondary_windows,
    ) {
        Ok(frame) => frame,
        Err(first_error)
            if cursor != CursorCaptureSettings::Default
                || border != DrawBorderSettings::Default
                || secondary_windows != SecondaryWindowSettings::Default =>
        {
            compatibility_notes.push(
                "增强截图设置初始化失败，已自动使用系统默认设置重试。".into(),
            );
            capture_frame(
                window_handle,
                CursorCaptureSettings::Default,
                DrawBorderSettings::Default,
                SecondaryWindowSettings::Default,
            )
            .map_err(|fallback_error| {
                format!("{first_error}；默认设置重试失败：{fallback_error}")
            })?
        }
        Err(error) => return Err(error),
    };
    if width == 0 || height == 0 || pixels.len() != width as usize * height as usize * 4 {
        return Err("窗口截图尺寸无效".into());
    }
    let image = RgbaImage::from_raw(width, height, pixels)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "无法构造窗口截图".to_string())?;
    Ok(CapturedWindowImage {
        image,
        elapsed_ms: started.elapsed().as_millis() as u64,
        compatibility_notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_optional_capture_features_use_system_defaults() {
        let (cursor, border, secondary, notes) = optional_settings(false, false, false);
        assert_eq!(cursor, CursorCaptureSettings::Default);
        assert_eq!(border, DrawBorderSettings::Default);
        assert_eq!(secondary, SecondaryWindowSettings::Default);
        assert_eq!(notes.len(), 3);
    }

    #[test]
    fn supported_optional_capture_features_keep_privacy_preferences() {
        let (cursor, border, secondary, notes) = optional_settings(true, true, true);
        assert_eq!(cursor, CursorCaptureSettings::WithoutCursor);
        assert_eq!(border, DrawBorderSettings::WithoutBorder);
        assert_eq!(secondary, SecondaryWindowSettings::Exclude);
        assert!(notes.is_empty());
    }
}

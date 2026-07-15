use std::sync::mpsc::{sync_channel, SyncSender};
use std::time::Instant;

use image::{DynamicImage, RgbaImage};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

pub(crate) struct CapturedWindowImage {
    pub(crate) image: DynamicImage,
    pub(crate) elapsed_ms: u64,
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

pub(crate) fn capture_window(window_handle: isize) -> Result<CapturedWindowImage, String> {
    let started = Instant::now();
    let window = Window::from_raw_hwnd(window_handle as *mut std::ffi::c_void);
    if !window.is_valid() {
        return Err("目标窗口不可捕获或已经关闭".into());
    }

    let (sender, receiver) = sync_channel(1);
    let settings = Settings::new(
        window,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        sender,
    );
    OneFrameCapture::start(settings)
        .map_err(|error| format!("捕获目标窗口失败：{error}"))?;
    let (width, height, pixels) = receiver
        .recv()
        .map_err(|_| "窗口截图任务未返回图像".to_string())??;
    if width == 0 || height == 0 || pixels.len() != width as usize * height as usize * 4 {
        return Err("窗口截图尺寸无效".into());
    }
    let image = RgbaImage::from_raw(width, height, pixels)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "无法构造窗口截图".to_string())?;
    Ok(CapturedWindowImage {
        image,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

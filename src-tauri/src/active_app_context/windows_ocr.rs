use std::cell::Cell;

use image::DynamicImage;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine as WindowsOcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use super::model::{NormalizedRegion, OcrTextBlock};
use super::normalize::normalize_text;

const NO_LANGUAGE_PACK_MESSAGE: &str = "系统未安装可用的 OCR 语言包，请前往「设置-时间和语言-语言和区域」为对应语言添加「光学字符识别」组件，或切换为内置 OCR 引擎。";

thread_local! {
    // 每个使用 WinRT 的线程需要独立初始化一次；OCR 工作线程常驻应用生命周期，不需要匹配 RoUninitialize。
    static WINRT_APARTMENT_READY: Cell<bool> = const { Cell::new(false) };
}

fn ensure_apartment_initialized() {
    WINRT_APARTMENT_READY.with(|ready| {
        if !ready.get() {
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            ready.set(true);
        }
    });
}

fn to_software_bitmap(image: &DynamicImage) -> Result<SoftwareBitmap, String> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut bgra = rgba.into_raw();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let writer =
        DataWriter::new().map_err(|error| format!("创建系统 OCR 图像缓冲失败：{error}"))?;
    writer
        .WriteBytes(&bgra)
        .map_err(|error| format!("写入系统 OCR 图像数据失败：{error}"))?;
    let buffer = writer
        .DetachBuffer()
        .map_err(|error| format!("提取系统 OCR 图像缓冲失败：{error}"))?;
    SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
        BitmapAlphaMode::Ignore,
    )
    .map_err(|error| format!("构建系统 OCR 位图失败：{error}"))
}

pub(crate) fn recognize(image: &DynamicImage) -> Result<Vec<OcrTextBlock>, String> {
    ensure_apartment_initialized();
    let bitmap = to_software_bitmap(image)?;
    let engine = WindowsOcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|_| NO_LANGUAGE_PACK_MESSAGE.to_string())?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|error| format!("提交系统 OCR 识别失败：{error}"))?
        .get()
        .map_err(|error| format!("系统 OCR 识别失败：{error}"))?;

    let width = image.width().max(1) as f32;
    let height = image.height().max(1) as f32;
    let lines = result
        .Lines()
        .map_err(|error| format!("读取系统 OCR 识别行失败：{error}"))?;

    let mut blocks = Vec::new();
    for line in &lines {
        let text = line
            .Text()
            .map(|value| normalize_text(&value.to_string()))
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let Ok(words) = line.Words() else {
            continue;
        };
        let mut left = f32::MAX;
        let mut top = f32::MAX;
        let mut right = f32::MIN;
        let mut bottom = f32::MIN;
        for word in &words {
            let Ok(rect) = word.BoundingRect() else {
                continue;
            };
            left = left.min(rect.X);
            top = top.min(rect.Y);
            right = right.max(rect.X + rect.Width);
            bottom = bottom.max(rect.Y + rect.Height);
        }
        if !left.is_finite() || !top.is_finite() || !right.is_finite() || !bottom.is_finite() {
            continue;
        }
        let bounds = NormalizedRegion {
            left: left / width,
            top: top / height,
            right: right / width,
            bottom: bottom / height,
        }
        .clamped();
        blocks.push(OcrTextBlock {
            text,
            // 系统 OCR 不提供置信度分数；固定为满分，不影响排序或阅读顺序。
            confidence: 1.0,
            bounds,
        });
    }
    Ok(blocks)
}

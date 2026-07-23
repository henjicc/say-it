use super::OcrTextBlock;

/// 使用 macOS Vision 框架识别 PNG。Vision 原生返回归一化坐标，桥接层已把
/// 左下角原点转换为应用统一使用的左上角原点。
pub(crate) fn recognize_png(png: &[u8]) -> Result<Vec<OcrTextBlock>, String> {
    crate::macos_native::vision_ocr(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vision_accepts_packaged_png_image() {
        recognize_png(include_bytes!("../../icons/128x128.png"))
            .expect("macOS Vision should accept a valid packaged PNG");
    }
}

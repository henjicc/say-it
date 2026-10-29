/// 对预乘 BGRA/RGBA 像素逐通道模糊。尺寸与缓冲长度不符时返回 None。
pub fn fast_blur_premultiplied(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    sigma_px: f32,
) -> Option<Vec<u8>> {
    let len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if pixels.len() != len {
        return None;
    }
    let pixels = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(image::imageops::fast_blur(&pixels, sigma_px).into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blur_rejects_mismatched_dimensions_and_preserves_uniform_pixels() {
        assert!(fast_blur_premultiplied(vec![0; 3], 1, 1, 1.0).is_none());
        assert!(fast_blur_premultiplied(vec![0; 5], 1, 1, 1.0).is_none());
        let pixels = [10, 20, 30, 128].repeat(16);
        assert_eq!(
            fast_blur_premultiplied(pixels.clone(), 4, 4, 1.0),
            Some(pixels)
        );
    }
}

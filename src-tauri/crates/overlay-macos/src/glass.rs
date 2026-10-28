//! 遮罩在渲染线程生成，主线程只换 NSImage 与材质。
use objc2::{rc::Retained, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSImage, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindow, NSWorkspace,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGContext, CGImage, CGImageAlphaInfo, CGPath,
};
use sayit_overlay_core::{
    geom::{PxRect, Shape},
    look::{GlassMaterial, Look, Tone},
};
use std::ptr::{null, null_mut};

/// CGImage 不可变且独立持有像素；仅这个最终图像跨线程，CGContext/CT 对象始终留在渲染线程。
#[derive(Clone)]
pub(crate) struct Image(pub CFRetained<CGImage>);
// SAFETY: CGImage 是不可变 CoreGraphics 对象，retain/release 线程安全。
unsafe impl Send for Image {}

#[derive(Default)]
pub(crate) struct MaskCache {
    key: Option<(Vec<Shape>, PxRect, u32)>,
    image: Option<Image>,
}

impl MaskCache {
    pub fn get(&mut self, shapes: Vec<Shape>, crop: PxRect, dpi: u32) -> Option<Image> {
        let key = (shapes, crop, dpi);
        if self.key.as_ref() != Some(&key) {
            self.image = alpha_mask(&key.0, crop, dpi).map(Image);
            self.key = Some(key);
        }
        self.image.clone()
    }
}

pub(crate) fn valid_bitmap(width: i32, height: i32) -> bool {
    width > 0
        && height > 0
        && width <= 16384
        && height <= 16384
        && i64::from(width) * i64::from(height) <= 64 * 1024 * 1024
}

fn alpha_mask(shapes: &[Shape], crop: PxRect, dpi: u32) -> Option<CFRetained<CGImage>> {
    let ctx = alpha_context(shapes, crop, dpi)?;
    CGBitmapContextCreateImage(Some(&ctx))
}

fn alpha_context(shapes: &[Shape], crop: PxRect, dpi: u32) -> Option<CFRetained<CGContext>> {
    if shapes.is_empty() || !valid_bitmap(crop.width, crop.height) || !(24..=768).contains(&dpi) {
        return None;
    }
    let scale = dpi as f64 / 96.0;
    // SAFETY: CG 拥有像素存储；AlphaOnly 无色彩空间，8 位单通道，每行一个字节/像素。
    let ctx = unsafe {
        CGBitmapContextCreate(
            null_mut(),
            crop.width as usize,
            crop.height as usize,
            8,
            crop.width as usize,
            None,
            CGImageAlphaInfo::Only.0,
        )
    }?;
    CGContext::clear_rect(
        Some(&ctx),
        CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(crop.width as f64, crop.height as f64),
        ),
    );
    CGContext::translate_ctm(
        Some(&ctx),
        -(crop.x as f64),
        crop.height as f64 + crop.y as f64,
    );
    CGContext::scale_ctm(Some(&ctx), scale, -scale);
    CGContext::set_alpha(Some(&ctx), 1.0);
    for shape in shapes {
        match *shape {
            Shape::RoundedRect { rect, radius } => {
                if ![rect.left, rect.top, rect.right, rect.bottom, radius]
                    .into_iter()
                    .all(f32::is_finite)
                    || rect.width() <= 0.0
                    || rect.height() <= 0.0
                    || [rect.left, rect.top, rect.right, rect.bottom]
                        .into_iter()
                        .any(|v| v.abs() > 1.0e6)
                {
                    continue;
                }
                let r = CGRect::new(
                    CGPoint::new(rect.left as f64, rect.top as f64),
                    CGSize::new(rect.width() as f64, rect.height() as f64),
                );
                let radius = radius
                    .max(0.0)
                    .min(rect.width() * 0.5)
                    .min(rect.height() * 0.5) as f64;
                let path = unsafe { CGPath::with_rounded_rect(r, radius, radius, null()) };
                CGContext::add_path(Some(&ctx), Some(&path));
                CGContext::fill_path(Some(&ctx));
            }
            Shape::Circle { center, radius } => {
                if ![center.x, center.y, radius].into_iter().all(f32::is_finite)
                    || radius <= 0.0
                    || [center.x, center.y, radius]
                        .into_iter()
                        .any(|v| v.abs() > 1.0e6)
                {
                    continue;
                }
                CGContext::fill_ellipse_in_rect(
                    Some(&ctx),
                    CGRect::new(
                        CGPoint::new((center.x - radius) as f64, (center.y - radius) as f64),
                        CGSize::new(radius as f64 * 2.0, radius as f64 * 2.0),
                    ),
                );
            }
        }
    }
    Some(ctx)
}

#[derive(Default)]
pub(crate) struct GlassView {
    view: Option<Retained<NSVisualEffectView>>,
    mask_key: Option<(usize, CGSize)>,
    tone: Option<Tone>,
}

impl GlassView {
    pub fn apply(
        &mut self,
        panel: &NSWindow,
        root: &NSView,
        draw: &NSView,
        look: Look,
        mask: Option<&Image>,
        mtm: MainThreadMarker,
    ) {
        // SAFETY: 常量与 AppKit 对象只在主线程访问。
        if self.tone != Some(look.theme.tone) {
            unsafe {
                let name = match look.theme.tone {
                    Tone::Dark => NSAppearanceNameDarkAqua,
                    Tone::Light => NSAppearanceNameAqua,
                };
                panel.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
            }
            self.tone = Some(look.theme.tone);
        }
        let glass = look.glass.filter(|_| {
            mask.is_some()
                && !NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
        });
        let Some(glass) = glass else {
            if let Some(view) = self.view.take() {
                view.removeFromSuperview();
            }
            self.mask_key = None;
            return;
        };
        let frame = crate::screens::ns_rect(crate::screens::qrect(root.bounds()))
            .expect("validated root bounds");
        let view = self.view.get_or_insert_with(|| {
            let view = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
            // 在内容视图下方，避免玻璃层盖住位图或扩大绘制视图的命中矩形。
            root.addSubview_positioned_relativeTo(
                &view,
                objc2_app_kit::NSWindowOrderingMode::Below,
                Some(draw),
            );
            view
        });
        if view.frame() != frame {
            view.setFrame(frame);
        }
        view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        view.setState(NSVisualEffectState::Active);
        view.setMaterial(match glass.material {
            GlassMaterial::UnderWindow => NSVisualEffectMaterial::UnderWindowBackground,
            GlassMaterial::Content => NSVisualEffectMaterial::ContentBackground,
            GlassMaterial::Sidebar => NSVisualEffectMaterial::Sidebar,
        });
        if let Some(mask) = mask {
            let key = (CFRetained::as_ptr(&mask.0).as_ptr() as usize, frame.size);
            if self.mask_key != Some(key) {
                let image = NSImage::initWithCGImage_size(NSImage::alloc(), &mask.0, frame.size);
                view.setMaskImage(Some(&image));
                self.mask_key = Some(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayit_overlay_core::geom::Point;
    #[test]
    fn alpha_mask_is_single_channel_and_cached_by_geometry() {
        let _guard = crate::cg::cg_test_lock();
        let crop = PxRect::new(0, 0, 40, 40);
        let mut cache = MaskCache::default();
        let shape = Shape::Circle {
            center: Point::new(10.0, 10.0),
            radius: 10.0,
        };
        let first = cache.get(vec![shape], crop, 192).unwrap();
        let second = cache.get(vec![shape], crop, 192).unwrap();
        assert_eq!(CFRetained::as_ptr(&first.0), CFRetained::as_ptr(&second.0));
        assert_eq!(CGImage::width(Some(&first.0)), 40);
        assert_eq!(CGImage::height(Some(&first.0)), 40);
        assert_eq!(CGImage::bits_per_pixel(Some(&first.0)), 8);
        assert!(cache.get(Vec::new(), crop, 192).is_none());
    }

    #[test]
    fn mask_shape_and_content_offset_leave_outside_pixels_transparent() {
        let _guard = crate::cg::cg_test_lock();
        let shape = Shape::Circle {
            center: Point::new(20.0, 30.0),
            radius: 10.0,
        };
        let ctx = alpha_context(&[shape], PxRect::new(20, 40, 40, 40), 192).unwrap();
        let data = objc2_core_graphics::CGBitmapContextGetData(Some(&ctx)).cast::<u8>();
        let stride = objc2_core_graphics::CGBitmapContextGetBytesPerRow(Some(&ctx));
        assert!(!data.is_null());
        // SAFETY: 上下文拥有 stride × 40 字节存储，测试只读有效坐标。
        unsafe {
            assert_eq!(*data, 0);
            assert_eq!(*data.add(20 * stride + 20), 255);
            assert_eq!(*data.add(39 * stride + 39), 0);
        }
    }
}

//! 左上原点、DIP 坐标的预乘 BGRA 离屏画布。仅在单一渲染线程使用。
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::{null, null_mut};

use objc2_core_foundation::{CFRetained, CGAffineTransform, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGBitmapContextCreateImage,
    CGBitmapContextGetBytesPerRow, CGBitmapContextGetData, CGColor, CGColorSpace, CGContext,
    CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, CGLineCap, CGLineJoin, CGMutablePath, CGPath,
};
use sayit_overlay_core::blur::fast_blur_premultiplied;
use sayit_overlay_core::canvas::{Canvas, LayoutOf, SvgIcon, TextTint};
use sayit_overlay_core::geom::{Color, LineCap, LineJoin, Point, Rect, Stroke, Transform};
use sayit_overlay_core::svg::{arc_to_cubics, parse_svg_path, SvgSeg};

use crate::ct::{set_fill, CtText};

#[cfg(test)]
pub(crate) static CG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn cg_test_lock() -> std::sync::MutexGuard<'static, ()> {
    // 绘图上下文归每个测试所有；先前测试失败后仍需串行执行其余测试并报告各自的结果。
    CG_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

thread_local! {
    // 图标键和路径来自编译期常量；CGPath 不可变，缓存归渲染线程所有。
    static ICONS: RefCell<HashMap<(&'static str, bool), CFRetained<CGPath>>> = RefCell::new(HashMap::new());
}

pub struct CgCanvas {
    ctx: CFRetained<CGContext>,
    width: u32,
    height: u32,
    scale: f64,
    text: CtText,
}

fn rect(value: Rect) -> Option<CGRect> {
    [
        value.left,
        value.top,
        value.right,
        value.bottom,
        value.width(),
        value.height(),
    ]
    .iter()
    .all(|v| v.is_finite())
    .then_some(())?;
    if value.width() < 0.0 || value.height() < 0.0 {
        return None;
    }
    Some(CGRect::new(
        CGPoint::new(value.left as f64, value.top as f64),
        CGSize::new(value.width() as f64, value.height() as f64),
    ))
}

fn rounded(value: Rect, radius: f32) -> Option<CFRetained<CGPath>> {
    let rect = rect(value)?;
    if !radius.is_finite() {
        return None;
    }
    let radius = (radius as f64)
        .max(0.0)
        .min(rect.size.width / 2.0)
        .min(rect.size.height / 2.0);
    // SAFETY: 几何有限且半径不超过短边的一半；不附加变换。
    Some(unsafe { CGPath::with_rounded_rect(rect, radius, radius, null()) })
}

fn path(segments: &[SvgSeg], fill: bool) -> Option<CFRetained<CGPath>> {
    let path = CGMutablePath::new();
    let (mut current, mut start) = ((0.0, 0.0), (0.0, 0.0));
    let mut open = false;
    // SAFETY: 所有坐标在送入 CG 前验证有限，transform 为 null。
    unsafe {
        for segment in segments {
            match *segment {
                SvgSeg::Move(x, y) => {
                    if ![x, y].iter().all(|v| v.is_finite()) {
                        return None;
                    }
                    if fill && open {
                        CGMutablePath::close_subpath(Some(&path));
                    }
                    CGMutablePath::move_to_point(Some(&path), null(), x as f64, y as f64);
                    current = (x, y);
                    start = current;
                    open = true;
                }
                SvgSeg::Line(x, y) => {
                    if ![x, y].iter().all(|v| v.is_finite()) {
                        return None;
                    }
                    CGMutablePath::add_line_to_point(Some(&path), null(), x as f64, y as f64);
                    current = (x, y);
                }
                SvgSeg::Bezier(a, b, c, d, x, y) => {
                    if ![a, b, c, d, x, y].iter().all(|v| v.is_finite()) {
                        return None;
                    }
                    CGMutablePath::add_curve_to_point(
                        Some(&path),
                        null(),
                        a as f64,
                        b as f64,
                        c as f64,
                        d as f64,
                        x as f64,
                        y as f64,
                    );
                    current = (x, y);
                }
                SvgSeg::Arc {
                    x,
                    y,
                    rx,
                    ry,
                    rotation,
                    ..
                } => {
                    if ![x, y, rx, ry, rotation].iter().all(|v| v.is_finite()) {
                        return None;
                    }
                    for [a, b, end] in arc_to_cubics(current, segment) {
                        if ![a.0, a.1, b.0, b.1, end.0, end.1]
                            .iter()
                            .all(|v| v.is_finite())
                        {
                            return None;
                        }
                        CGMutablePath::add_curve_to_point(
                            Some(&path),
                            null(),
                            a.0 as f64,
                            a.1 as f64,
                            b.0 as f64,
                            b.1 as f64,
                            end.0 as f64,
                            end.1 as f64,
                        );
                    }
                    current = (x, y);
                }
                SvgSeg::Close => {
                    if fill {
                        CGMutablePath::close_subpath(Some(&path));
                    }
                    // 描边 Z 只结束子路径，但后续相对段的当前点仍是子路径起点。
                    current = start;
                    CGMutablePath::move_to_point(
                        Some(&path),
                        null(),
                        start.0 as f64,
                        start.1 as f64,
                    );
                    open = false;
                }
            }
        }
        if fill && open {
            CGMutablePath::close_subpath(Some(&path));
        }
    }
    CGPath::new_copy(Some(&path))
}

fn icon_path(icon: &SvgIcon, fill: bool) -> CFRetained<CGPath> {
    ICONS.with(|cache| {
        cache
            .borrow_mut()
            .entry((icon.key, fill))
            .or_insert_with(|| {
                let segments = parse_svg_path(icon.d).expect("invalid built-in SVG icon");
                path(&segments, fill).expect("non-finite built-in SVG icon")
            })
            .clone()
    })
}

impl CgCanvas {
    pub fn new(width_px: u32, height_px: u32, scale: f64) -> Option<Self> {
        if width_px == 0
            || height_px == 0
            || !scale.is_finite()
            || scale <= 0.0
            || scale * 96.0 > u32::MAX as f64
        {
            return None;
        }
        let row = (width_px as usize).checked_mul(4)?;
        let len = row.checked_mul(height_px as usize)?;
        if len > isize::MAX as usize {
            return None;
        }
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
        let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
        // SAFETY: CG 分配并管理存储，行长与尺寸已检查，像素格式为 8-bit sRGB BGRA 预乘。
        let ctx = unsafe {
            CGBitmapContextCreate(
                null_mut(),
                width_px as usize,
                height_px as usize,
                8,
                row,
                Some(&space),
                info,
            )
        }?;
        CGContext::clear_rect(
            Some(&ctx),
            CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(width_px as f64, height_px as f64),
            ),
        );
        CGContext::translate_ctm(Some(&ctx), 0.0, height_px as f64);
        CGContext::scale_ctm(Some(&ctx), scale, -scale);
        CGContext::set_text_matrix(
            Some(&ctx),
            CGAffineTransform {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                tx: 0.0,
                ty: 0.0,
            },
        );
        CGContext::set_should_antialias(Some(&ctx), true);
        CGContext::set_should_smooth_fonts(Some(&ctx), false);
        CGContext::set_allows_font_subpixel_positioning(Some(&ctx), true);
        CGContext::set_should_subpixel_position_fonts(Some(&ctx), true);
        Some(Self {
            ctx,
            width: width_px,
            height: height_px,
            scale,
            text: CtText::default(),
        })
    }

    pub fn finish(self) -> CFRetained<CGImage> {
        CGBitmapContextCreateImage(Some(&self.ctx)).expect("bitmap context image allocation failed")
    }

    /// 按顶行优先返回紧密排列的 BGRA 预乘像素，不暴露 CGContext 的存储。
    pub fn pixels(&self) -> Vec<u8> {
        let stride = CGBitmapContextGetBytesPerRow(Some(&self.ctx));
        let data = CGBitmapContextGetData(Some(&self.ctx)).cast::<u8>();
        assert!(!data.is_null());
        let mut pixels = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for y in 0..self.height as usize {
            // SAFETY: CGContext 的存储在 self 存活期间有效，单次只读取一行的有效像素。
            pixels.extend_from_slice(unsafe {
                std::slice::from_raw_parts(data.add(y * stride), self.width as usize * 4)
            });
        }
        pixels
    }

    fn stroke(&self, color: Color, stroke: Stroke) -> bool {
        if !stroke.width.is_finite() || stroke.width <= 0.0 {
            return false;
        }
        let ctx = Some(&*self.ctx);
        let color = CGColor::new_srgb(
            color.r as f64,
            color.g as f64,
            color.b as f64,
            color.a as f64,
        );
        CGContext::set_stroke_color_with_color(ctx, Some(&color));
        CGContext::set_line_width(ctx, stroke.width as f64);
        CGContext::set_line_cap(
            ctx,
            match stroke.cap {
                LineCap::Flat => CGLineCap::Butt,
                LineCap::Round => CGLineCap::Round,
            },
        );
        CGContext::set_line_join(
            ctx,
            match stroke.join {
                LineJoin::Miter => CGLineJoin::Miter,
                LineJoin::Round => CGLineJoin::Round,
            },
        );
        CGContext::set_miter_limit(ctx, 10.0);
        true
    }

    fn draw_path(&self, path: &CGPath, color: Color, stroke: Option<Stroke>) {
        if let Some(stroke) = stroke {
            if !self.stroke(color, stroke) {
                return;
            }
        } else {
            set_fill(&self.ctx, color);
        }
        CGContext::begin_path(Some(&self.ctx));
        CGContext::add_path(Some(&self.ctx), Some(path));
        if stroke.is_some() {
            CGContext::stroke_path(Some(&self.ctx));
        } else {
            CGContext::fill_path(Some(&self.ctx));
        }
    }

    fn image(&self, image: &CGImage, bounds: CGRect) {
        let ctx = Some(&*self.ctx);
        CGContext::save_g_state(ctx);
        CGContext::translate_ctm(ctx, bounds.origin.x, bounds.origin.y + bounds.size.height);
        CGContext::scale_ctm(ctx, 1.0, -1.0);
        CGContext::draw_image(
            ctx,
            CGRect::new(CGPoint::new(0.0, 0.0), bounds.size),
            Some(image),
        );
        CGContext::restore_g_state(ctx);
    }
}

impl Canvas for CgCanvas {
    type Text = CtText;
    fn text(&self) -> &CtText {
        &self.text
    }
    fn dpi(&self) -> u32 {
        (self.scale * 96.0).round() as u32
    }
    fn save(&mut self) {
        CGContext::save_g_state(Some(&self.ctx));
    }
    fn restore(&mut self) {
        CGContext::restore_g_state(Some(&self.ctx));
    }
    fn clip_rect(&mut self, value: Rect) {
        let Some(value) = rect(value) else {
            return;
        };
        let ctx = Some(&*self.ctx);
        let first = CGContext::convert_point_to_device_space(ctx, value.origin);
        let last = CGContext::convert_point_to_device_space(
            ctx,
            CGPoint::new(
                value.origin.x + value.size.width,
                value.origin.y + value.size.height,
            ),
        );
        // 翻转后的半开区间是 (min,max]；边界恰好落在像素中心时也须保留原来的包含关系。
        let edge = |value: f64, forward: bool| {
            if forward {
                (value - 0.5).ceil()
            } else {
                (value - 0.5).floor() + 1.0
            }
        };
        let x = edge(first.x.min(last.x), first.x < last.x);
        let y = edge(first.y.min(last.y), first.y < last.y);
        let right = edge(first.x.max(last.x), first.x < last.x);
        let bottom = edge(first.y.max(last.y), first.y < last.y);
        // CGRect 的坐标换算可能保留负尺寸，必须从两个端点重建正向矩形。
        let first = CGContext::convert_point_to_user_space(ctx, CGPoint::new(x, y));
        let last = CGContext::convert_point_to_user_space(ctx, CGPoint::new(right, bottom));
        let user = CGRect::new(
            CGPoint::new(first.x.min(last.x), first.y.min(last.y)),
            CGSize::new((last.x - first.x).abs(), (last.y - first.y).abs()),
        );
        CGContext::clip_to_rect(ctx, user);
    }
    fn concat_transform(&mut self, value: Transform) {
        if ![value.scale, value.dx, value.dy]
            .iter()
            .all(|v| v.is_finite())
            || value.scale == 0.0
        {
            return;
        }
        let transform = CGAffineTransform {
            a: value.scale as f64,
            b: 0.0,
            c: 0.0,
            d: value.scale as f64,
            tx: value.dx as f64,
            ty: value.dy as f64,
        };
        CGContext::concat_ctm(Some(&self.ctx), transform);
    }
    fn fill_rounded_rect(&mut self, value: Rect, radius: f32, color: Color) {
        if let Some(path) = rounded(value, radius) {
            self.draw_path(&path, color, None);
        }
    }
    fn stroke_rounded_rect(&mut self, value: Rect, radius: f32, color: Color, stroke: Stroke) {
        if let Some(path) = rounded(value, radius) {
            self.draw_path(&path, color, Some(stroke));
        }
    }
    fn fill_circle(&mut self, center: Point, radius: f32, color: Color) {
        if !radius.is_finite() || radius < 0.0 {
            return;
        }
        if let Some(bounds) = rect(Rect::new(
            center.x - radius,
            center.y - radius,
            center.x + radius,
            center.y + radius,
        )) {
            set_fill(&self.ctx, color);
            CGContext::fill_ellipse_in_rect(Some(&self.ctx), bounds);
        }
    }
    fn stroke_circle(&mut self, center: Point, radius: f32, color: Color, stroke: Stroke) {
        if !radius.is_finite() || radius < 0.0 || !self.stroke(color, stroke) {
            return;
        }
        if let Some(bounds) = rect(Rect::new(
            center.x - radius,
            center.y - radius,
            center.x + radius,
            center.y + radius,
        )) {
            CGContext::stroke_ellipse_in_rect(Some(&self.ctx), bounds);
        }
    }
    fn line(&mut self, from: Point, to: Point, color: Color, stroke: Stroke) {
        if ![from.x, from.y, to.x, to.y].iter().all(|v| v.is_finite())
            || !self.stroke(color, stroke)
        {
            return;
        }
        if from == to && stroke.cap == LineCap::Round {
            self.fill_circle(from, stroke.width / 2.0, color);
            return;
        }
        CGContext::begin_path(Some(&self.ctx));
        CGContext::move_to_point(Some(&self.ctx), from.x as f64, from.y as f64);
        CGContext::add_line_to_point(Some(&self.ctx), to.x as f64, to.y as f64);
        CGContext::stroke_path(Some(&self.ctx));
    }
    fn fill_icon(&mut self, icon: &SvgIcon, color: Color) {
        self.draw_path(&icon_path(icon, true), color, None);
    }
    fn stroke_icon(&mut self, icon: &SvgIcon, color: Color, stroke: Stroke) {
        self.draw_path(&icon_path(icon, false), color, Some(stroke));
    }
    fn stroke_path(&mut self, segments: &[SvgSeg], color: Color, stroke: Stroke) {
        if let Some(path) = path(segments, false) {
            self.draw_path(&path, color, Some(stroke));
        }
    }
    fn draw_text(
        &mut self,
        layout: &LayoutOf<Self>,
        origin: Point,
        color: Color,
        tint: Option<TextTint>,
    ) {
        layout.draw(&self.ctx, origin, color, tint);
    }
    fn draw_text_blurred(
        &mut self,
        layout: &LayoutOf<Self>,
        bounds: Rect,
        offset: Point,
        color: Color,
        sigma: f32,
    ) -> Result<(), String> {
        let bounds = rect(bounds).ok_or("invalid text blur bounds")?;
        if ![offset.x, offset.y, sigma].iter().all(|v| v.is_finite()) || sigma < 0.0 {
            return Err("invalid text blur parameters".into());
        }
        let width = ((bounds.size.width + 36.0) * self.scale).ceil();
        let height = ((bounds.size.height + 36.0) * self.scale).ceil();
        if width > u32::MAX as f64 || height > u32::MAX as f64 {
            return Err("text blur bitmap too large".into());
        }
        let mut bitmap = Self::new(width as u32, height as u32, self.scale)
            .ok_or("text blur bitmap allocation failed")?;
        bitmap.draw_text(
            layout,
            Point::new(18.0 + offset.x, 18.0 + offset.y),
            color,
            None,
        );
        let pixels = fast_blur_premultiplied(
            bitmap.pixels(),
            width as u32,
            height as u32,
            sigma * self.scale as f32,
        )
        .ok_or("invalid text blur buffer")?;
        let stride = CGBitmapContextGetBytesPerRow(Some(&bitmap.ctx));
        let data = CGBitmapContextGetData(Some(&bitmap.ctx)).cast::<u8>();
        if data.is_null() {
            return Err("text blur bitmap storage unavailable".into());
        }
        for (y, row) in pixels.chunks_exact(width as usize * 4).enumerate() {
            // SAFETY: 独占临时位图，只写入每行有效像素，不越过 CGContext 分配的存储。
            unsafe {
                std::ptr::copy_nonoverlapping(row.as_ptr(), data.add(y * stride), row.len());
            }
        }
        let image = bitmap.finish();
        self.image(
            &image,
            CGRect::new(
                CGPoint::new(bounds.origin.x - 18.0, bounds.origin.y - 18.0),
                CGSize::new(width / self.scale, height / self.scale),
            ),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayit_overlay_core::canvas::{
        FontFamily, ParagraphAlign, TextAlign, TextEngine, TextStyle,
    };

    const RED: Color = Color::new(1.0, 0.0, 0.0, 1.0);
    fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
        &pixels[(y * width + x) * 4..(y * width + x + 1) * 4]
    }
    fn style() -> TextStyle<'static> {
        TextStyle {
            family: FontFamily::Named("PingFang SC"),
            size: 24.0,
            weight: 400,
            align: TextAlign::Leading,
            paragraph: ParagraphAlign::Near,
            wrap: false,
            line_spacing: None,
        }
    }

    #[test]
    fn bitmap_format_orientation_and_premultiplication() {
        let _guard = cg_test_lock();
        let mut canvas = CgCanvas::new(40, 40, 2.0).unwrap();
        canvas.fill_rounded_rect(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, RED);
        canvas.fill_rounded_rect(
            Rect::new(10.0, 0.0, 20.0, 10.0),
            0.0,
            Color::new(1.0, 0.0, 0.0, 0.5),
        );
        let pixels = canvas.pixels();
        let pixels = pixels.as_slice();
        assert_eq!(pixel(&pixels, 40, 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&pixels, 40, 0, 39), [0, 0, 0, 0]);
        let p = pixel(&pixels, 40, 30, 5);
        assert!((p[2] as i32 - 128).abs() <= 1 && p[2] == p[3]);
        let _ = canvas.finish();
        assert!(CgCanvas::new(1, 1, f64::NAN).is_none());
        assert!(CgCanvas::new(0, 1, 1.0).is_none());
    }

    #[test]
    fn primitives_clip_transform_and_antialiasing() {
        let _guard = cg_test_lock();
        let mut canvas = CgCanvas::new(64, 64, 1.0).unwrap();
        canvas.save();
        canvas.concat_transform(Transform {
            scale: 2.0,
            dx: 3.0,
            dy: 5.0,
        });
        canvas.clip_rect(Rect::new(0.3, 0.3, 5.3, 5.3));
        canvas.fill_rounded_rect(Rect::new(-20.0, -20.0, 30.0, 30.0), 0.0, RED);
        canvas.restore();
        let pixels = canvas.pixels();
        for y in 0..64 {
            for x in 0..64 {
                assert_eq!(
                    pixel(&pixels, 64, x, y)[3],
                    if (4..14).contains(&x) && (6..16).contains(&y) {
                        255
                    } else {
                        0
                    }
                );
            }
        }
        canvas.fill_rounded_rect(Rect::new(20.0, 20.0, 30.0, 24.0), 1000.0, RED);
        canvas.stroke_rounded_rect(
            Rect::new(35.0, 20.0, 50.0, 30.0),
            1000.0,
            RED,
            Stroke::round(1.0),
        );
        canvas.fill_circle(Point::new(30.3, 45.2), 9.4, RED);
        canvas.stroke_circle(Point::new(50.0, 45.0), 5.0, RED, Stroke::plain(1.0));
        canvas.line(
            Point::new(10.0, 40.0),
            Point::new(10.0, 40.0),
            RED,
            Stroke::round(3.0),
        );
        assert!(canvas
            .pixels()
            .chunks_exact(4)
            .any(|p| p[3] > 0 && p[3] < 255));
    }

    #[test]
    fn svg_fill_closes_and_stroke_keeps_subpaths_open() {
        let _guard = cg_test_lock();
        let icon = SvgIcon {
            key: "cg-test-triangle",
            d: "M4 4L24 4L24 24Z",
        };
        let mut canvas = CgCanvas::new(32, 32, 1.0).unwrap();
        canvas.stroke_icon(&icon, RED, Stroke::plain(1.0));
        assert_eq!(pixel(&canvas.pixels(), 32, 12, 12)[3], 0);
        canvas.fill_icon(&icon, RED);
        assert!(pixel(&canvas.pixels(), 32, 18, 10)[3] > 0);
        canvas.stroke_path(
            &[
                SvgSeg::Move(10.0, 28.0),
                SvgSeg::Arc {
                    x: 25.0,
                    y: 28.0,
                    rx: 8.0,
                    ry: 8.0,
                    rotation: 0.0,
                    large: false,
                    sweep: true,
                },
            ],
            RED,
            Stroke::round(1.0),
        );
    }

    #[test]
    fn clip_pixel_centers_survive_axis_flips_and_nested_restore() {
        let _guard = cg_test_lock();
        for flipped in [false, true] {
            let mut canvas = CgCanvas::new(16, 16, 1.0).unwrap();
            canvas.save();
            if flipped {
                canvas.concat_transform(Transform {
                    scale: -1.0,
                    dx: 16.0,
                    dy: 16.0,
                });
            }
            canvas.clip_rect(Rect::new(2.5, 3.5, 7.5, 8.5));
            canvas.fill_rounded_rect(Rect::new(0.0, 0.0, 16.0, 16.0), 0.0, RED);
            canvas.restore();
            let pixels = canvas.pixels();
            for y in 0..16 {
                for x in 0..16 {
                    let (ux, uy) = if flipped { (15 - x, 15 - y) } else { (x, y) };
                    assert_eq!(
                        pixel(&pixels, 16, x, y)[3],
                        if (2..7).contains(&ux) && (3..8).contains(&uy) {
                            255
                        } else {
                            0
                        },
                        "flip={flipped}, pixel=({x},{y})"
                    );
                }
            }
            canvas.fill_rounded_rect(Rect::new(0.0, 0.0, 1.0, 1.0), 0.0, RED);
            assert_eq!(pixel(&canvas.pixels(), 16, 0, 0)[3], 255);
        }
    }

    #[test]
    fn utf16_tint_changes_only_requested_run_alpha() {
        let _guard = cg_test_lock();
        let mut canvas = CgCanvas::new(180, 60, 1.0).unwrap();
        let layout = canvas
            .text()
            .layout("中😀文中", &style(), 180.0, 60.0)
            .unwrap();
        let tint_left = canvas
            .text()
            .metrics(&canvas.text().layout("中😀", &style(), 180.0, 60.0).unwrap())
            .width;
        let tint_right = canvas
            .text()
            .metrics(
                &canvas
                    .text()
                    .layout("中😀文", &style(), 180.0, 60.0)
                    .unwrap(),
            )
            .width;
        canvas.draw_text(
            &layout,
            Point::new(0.0, 0.0),
            RED,
            Some(TextTint {
                utf16_start: 3,
                utf16_len: 1,
                color: Color::new(1.0, 0.0, 0.0, 0.2),
            }),
        );
        let pixels = canvas.pixels();
        let peak = |left, right| {
            let pixels = pixels.as_slice();
            (0..60)
                .flat_map(|y| (left..right).map(move |x| pixel(pixels, 180, x, y)[3]))
                .max()
                .unwrap()
        };
        assert!(peak(0, 24) > 200);
        let tinted_peak = peak(
            tint_left.ceil() as usize + 1,
            tint_right.floor() as usize - 1,
        );
        assert!(
            tinted_peak > 0 && tinted_peak <= 60,
            "tinted alpha: {tinted_peak}"
        );
        assert!(
            peak(
                tint_right.ceil() as usize + 1,
                tint_right.ceil() as usize + 20
            ) > 200
        );
    }

    #[test]
    fn blurred_text_preserves_alpha_mass_and_orientation() {
        let _guard = cg_test_lock();
        let mut sharp = CgCanvas::new(200, 100, 1.0).unwrap();
        let mut blurred = CgCanvas::new(200, 100, 1.0).unwrap();
        let layout = sharp.text().layout("字幕", &style(), 100.0, 50.0).unwrap();
        sharp.draw_text(&layout, Point::new(30.0, 25.0), RED, None);
        blurred
            .draw_text_blurred(
                &layout,
                Rect::new(30.0, 25.0, 130.0, 75.0),
                Point::new(0.0, 0.0),
                RED,
                2.0,
            )
            .unwrap();
        let mass =
            |pixels: &[u8]| pixels.chunks_exact(4).map(|p| u64::from(p[3])).sum::<u64>() as f64;
        let a = sharp.pixels();
        let b = blurred.pixels();
        assert!(mass(&a) > 0.0);
        assert!((mass(&a) - mass(&b)).abs() / mass(&a) < 0.12);
        let centroid = |pixels: &[u8]| {
            pixels
                .chunks_exact(4)
                .enumerate()
                .map(|(i, p)| (i / 200) as f64 * f64::from(p[3]))
                .sum::<f64>()
                / mass(pixels)
        };
        assert!((centroid(&a) - centroid(&b)).abs() < 1.5);
        assert!(blurred
            .draw_text_blurred(&layout, Rect::ZERO, Point::new(0.0, 0.0), RED, f32::NAN)
            .is_err());
    }
}

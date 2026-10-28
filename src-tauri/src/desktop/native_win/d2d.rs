//! Canvas / TextEngine 的 Direct2D 软件渲染实现；资源只属于当前窗口线程。
use super::{
    create_dc_render_target, create_dwrite_factory, create_text_format_weight, path_geometry,
    svg_path_geometry, svg_path_geometry_stroke, Dib,
};
use sayit_overlay_core::canvas::*;
use sayit_overlay_core::geom::{Color, LineCap, LineJoin, Point, Rect, Stroke, Transform};
use sayit_overlay_core::svg::SvgSeg;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::core::IUnknown;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

#[derive(Hash, PartialEq, Eq)]
struct FormatKey {
    family: String,
    size: u32,
    weight: u16,
    align: TextAlign,
    paragraph: ParagraphAlign,
}

pub(crate) struct DWriteText {
    factory: IDWriteFactory,
    formats: RefCell<HashMap<FormatKey, IDWriteTextFormat>>,
}
impl DWriteText {
    pub(crate) fn new() -> Result<Self, String> {
        Ok(Self {
            factory: create_dwrite_factory()?,
            formats: RefCell::new(HashMap::new()),
        })
    }
    fn format(&self, style: &TextStyle<'_>) -> Option<IDWriteTextFormat> {
        let family = match style.family {
            FontFamily::SystemUi => "Microsoft YaHei UI",
            FontFamily::SymbolGlyphs => "Segoe MDL2 Assets",
            FontFamily::Named(name) => name,
        };
        let key = FormatKey {
            family: family.to_owned(),
            size: style.size.to_bits(),
            weight: style.weight,
            align: style.align,
            paragraph: style.paragraph,
        };
        if let Some(format) = self.formats.borrow().get(&key) {
            return Some(format.clone());
        }
        let format = match create_text_format_weight(
            &self.factory,
            family,
            style.size,
            DWRITE_FONT_WEIGHT(style.weight as i32),
            style.align == TextAlign::Center,
            style.paragraph == ParagraphAlign::Center,
        ) {
            Ok(format) => format,
            Err(error) => {
                eprintln!("[native-overlay] {error}");
                return None;
            }
        };
        self.formats.borrow_mut().insert(key, format.clone());
        Some(format)
    }
}
impl TextEngine for DWriteText {
    type Layout = IDWriteTextLayout;
    fn layout(
        &self,
        text: &str,
        style: &TextStyle<'_>,
        max_width: f32,
        max_height: f32,
    ) -> Option<Self::Layout> {
        let format = self.format(style)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        let result = unsafe {
            (|| -> windows::core::Result<_> {
                let layout = self
                    .factory
                    .CreateTextLayout(&wide, &format, max_width, max_height)?;
                if !style.wrap {
                    layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                }
                if let Some(spacing) = style.line_spacing {
                    layout.SetLineSpacing(
                        DWRITE_LINE_SPACING_METHOD_UNIFORM,
                        spacing.height,
                        spacing.baseline,
                    )?;
                }
                Ok(layout)
            })()
        };
        match result {
            Ok(layout) => Some(layout),
            Err(error) => {
                eprintln!("[native-overlay] 创建文字布局失败：{error}");
                None
            }
        }
    }
    fn metrics(&self, layout: &Self::Layout) -> TextMetrics {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        if let Err(error) = unsafe { layout.GetMetrics(&mut metrics) } {
            eprintln!("[native-overlay] 测量文字失败：{error}");
        }
        TextMetrics {
            left: metrics.left,
            top: metrics.top,
            width: metrics.width,
            height: metrics.height,
            line_count: metrics.lineCount,
        }
    }
}

/// 设备无关几何跨帧缓存；画刷和渲染目标由 LayeredSurface 持有。
#[derive(Default)]
pub(crate) struct D2dResources {
    icons: HashMap<(&'static str, bool), ID2D1PathGeometry>,
    strokes: HashMap<(LineCap, LineJoin), ID2D1StrokeStyle>,
}
pub(crate) struct D2dCanvas<'a> {
    target: &'a ID2D1DCRenderTarget,
    brush: &'a ID2D1SolidColorBrush,
    text: &'a DWriteText,
    factory: &'a ID2D1Factory,
    resources: &'a mut D2dResources,
    dpi: u32,
    stack: Vec<(Matrix3x2, usize)>,
    clip_depth: usize,
}
impl<'a> D2dCanvas<'a> {
    pub(crate) fn new(
        target: &'a ID2D1DCRenderTarget,
        brush: &'a ID2D1SolidColorBrush,
        text: &'a DWriteText,
        factory: &'a ID2D1Factory,
        resources: &'a mut D2dResources,
        dpi: u32,
    ) -> Self {
        Self {
            target,
            brush,
            text,
            factory,
            resources,
            dpi,
            stack: Vec::new(),
            clip_depth: 0,
        }
    }
    fn color(&self, color: Color) {
        unsafe {
            self.brush.SetColor(&color_f(color));
        }
    }
    fn stroke(&mut self, stroke: Stroke) -> Option<ID2D1StrokeStyle> {
        if stroke.cap == LineCap::Flat && stroke.join == LineJoin::Miter {
            return None;
        }
        let key = (stroke.cap, stroke.join);
        if let Some(style) = self.resources.strokes.get(&key) {
            return Some(style.clone());
        }
        let cap = if stroke.cap == LineCap::Round {
            D2D1_CAP_STYLE_ROUND
        } else {
            D2D1_CAP_STYLE_FLAT
        };
        let result = unsafe {
            self.factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: cap,
                    endCap: cap,
                    dashCap: cap,
                    lineJoin: if stroke.join == LineJoin::Round {
                        D2D1_LINE_JOIN_ROUND
                    } else {
                        D2D1_LINE_JOIN_MITER
                    },
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )
        };
        match result {
            Ok(style) => {
                self.resources.strokes.insert(key, style.clone());
                Some(style)
            }
            Err(error) => {
                eprintln!("[native-overlay] 创建笔触失败：{error}");
                None
            }
        }
    }
    fn icon(&mut self, icon: &SvgIcon, filled: bool) -> Option<ID2D1PathGeometry> {
        if let Some(path) = self.resources.icons.get(&(icon.key, filled)) {
            return Some(path.clone());
        }
        let result = if filled {
            svg_path_geometry(self.factory, icon.d)
        } else {
            svg_path_geometry_stroke(self.factory, icon.d)
        };
        match result {
            Ok(path) => {
                self.resources
                    .icons
                    .insert((icon.key, filled), path.clone());
                Some(path)
            }
            Err(error) => {
                eprintln!("[native-overlay] {error}");
                None
            }
        }
    }
}
fn color_f(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}
fn rect_f(r: Rect) -> D2D_RECT_F {
    super::rect_f(r.left, r.top, r.right, r.bottom)
}
fn point_f(p: Point) -> D2D_POINT_2F {
    D2D_POINT_2F { x: p.x, y: p.y }
}
fn rounded(rect: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: rect_f(rect),
        radiusX: radius,
        radiusY: radius,
    }
}
fn ellipse(center: Point, radius: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: point_f(center),
        radiusX: radius,
        radiusY: radius,
    }
}
impl Canvas for D2dCanvas<'_> {
    type Text = DWriteText;
    fn text(&self) -> &DWriteText {
        self.text
    }
    fn dpi(&self) -> u32 {
        self.dpi
    }
    fn save(&mut self) {
        let mut transform = Matrix3x2::default();
        unsafe {
            self.target.GetTransform(&mut transform);
        }
        self.stack.push((transform, self.clip_depth));
    }
    fn restore(&mut self) {
        let (transform, depth) = self
            .stack
            .pop()
            .expect("Canvas save/restore must be nested");
        unsafe {
            while self.clip_depth > depth {
                self.target.PopAxisAlignedClip();
                self.clip_depth -= 1;
            }
            self.target.SetTransform(&transform);
        }
    }
    fn clip_rect(&mut self, rect: Rect) {
        unsafe {
            self.target
                .PushAxisAlignedClip(&rect_f(rect), D2D1_ANTIALIAS_MODE_ALIASED);
        }
        self.clip_depth += 1;
    }
    fn concat_transform(&mut self, t: Transform) {
        let mut old = Matrix3x2::default();
        unsafe {
            self.target.GetTransform(&mut old);
            self.target.SetTransform(&Matrix3x2 {
                M11: t.scale * old.M11,
                M12: t.scale * old.M12,
                M21: t.scale * old.M21,
                M22: t.scale * old.M22,
                M31: t.dx * old.M11 + t.dy * old.M21 + old.M31,
                M32: t.dx * old.M12 + t.dy * old.M22 + old.M32,
            });
        }
    }
    fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.color(color);
        unsafe {
            self.target
                .FillRoundedRectangle(&rounded(rect, radius), self.brush);
        }
    }
    fn stroke_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color, stroke: Stroke) {
        self.color(color);
        let style = self.stroke(stroke);
        unsafe {
            self.target.DrawRoundedRectangle(
                &rounded(rect, radius),
                self.brush,
                stroke.width,
                style.as_ref(),
            );
        }
    }
    fn fill_circle(&mut self, center: Point, radius: f32, color: Color) {
        self.color(color);
        unsafe {
            self.target
                .FillEllipse(&ellipse(center, radius), self.brush);
        }
    }
    fn stroke_circle(&mut self, center: Point, radius: f32, color: Color, stroke: Stroke) {
        self.color(color);
        let style = self.stroke(stroke);
        unsafe {
            self.target.DrawEllipse(
                &ellipse(center, radius),
                self.brush,
                stroke.width,
                style.as_ref(),
            );
        }
    }
    fn line(&mut self, from: Point, to: Point, color: Color, stroke: Stroke) {
        self.color(color);
        let style = self.stroke(stroke);
        unsafe {
            self.target.DrawLine(
                point_f(from),
                point_f(to),
                self.brush,
                stroke.width,
                style.as_ref(),
            );
        }
    }
    fn fill_icon(&mut self, icon: &SvgIcon, color: Color) {
        self.color(color);
        if let Some(path) = self.icon(icon, true) {
            unsafe {
                self.target.FillGeometry(&path, self.brush, None);
            }
        }
    }
    fn stroke_icon(&mut self, icon: &SvgIcon, color: Color, stroke: Stroke) {
        self.color(color);
        let style = self.stroke(stroke);
        if let Some(path) = self.icon(icon, false) {
            unsafe {
                self.target
                    .DrawGeometry(&path, self.brush, stroke.width, style.as_ref());
            }
        }
    }
    fn stroke_path(&mut self, path: &[SvgSeg], color: Color, stroke: Stroke) {
        self.color(color);
        let style = self.stroke(stroke);
        match path_geometry(self.factory, path, false) {
            Ok(path) => unsafe {
                self.target
                    .DrawGeometry(&path, self.brush, stroke.width, style.as_ref());
            },
            Err(error) => eprintln!("[native-overlay] {error}"),
        }
    }
    fn draw_text(
        &mut self,
        layout: &IDWriteTextLayout,
        origin: Point,
        color: Color,
        tint: Option<TextTint>,
    ) {
        self.color(color);
        unsafe {
            let range = tint.map(|t| DWRITE_TEXT_RANGE {
                startPosition: t.utf16_start,
                length: t.utf16_len,
            });
            if let (Some(tint), Some(range)) = (tint, range) {
                match self
                    .target
                    .CreateSolidColorBrush(&color_f(tint.color), None)
                {
                    Ok(brush) => {
                        if let Err(error) = layout.SetDrawingEffect(&brush, range) {
                            eprintln!("[native-overlay] 设置文字淡入失败：{error}");
                        }
                    }
                    Err(error) => eprintln!("[native-overlay] 创建文字淡入画刷失败：{error}"),
                }
            }
            self.target.DrawTextLayout(
                point_f(origin),
                layout,
                self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            if let Some(range) = range {
                if let Err(error) = layout.SetDrawingEffect(None::<&IUnknown>, range) {
                    eprintln!("[native-overlay] 清除文字淡入失败：{error}");
                }
            }
        }
    }
    fn draw_text_blurred(
        &mut self,
        layout: &IDWriteTextLayout,
        bounds: Rect,
        offset: Point,
        color: Color,
        sigma: f32,
    ) -> Result<(), String> {
        let (bitmap, destination) = blurred_text_bitmap(
            self.target,
            layout,
            &rect_f(bounds),
            (offset.x, offset.y),
            &color_f(color),
            sigma,
        )
        .map_err(|e| e.to_string())?;
        unsafe {
            self.target.DrawBitmap(
                &bitmap,
                Some(&destination),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        }
        Ok(())
    }
}

fn blurred_text_bitmap(
    target: &ID2D1DCRenderTarget,
    layout: &IDWriteTextLayout,
    flow: &D2D_RECT_F,
    offset: (f32, f32),
    color: &D2D1_COLOR_F,
    sigma: f32,
) -> windows::core::Result<(ID2D1Bitmap, D2D_RECT_F)> {
    const PAD: f32 = 18.0;
    unsafe {
        let (mut dpi_x, mut dpi_y) = (96.0, 96.0);
        target.GetDpi(&mut dpi_x, &mut dpi_y);
        let scale = dpi_x / 96.0;
        let width = ((flow.right - flow.left + 2.0 * PAD) * scale).ceil() as u32;
        let height = ((flow.bottom - flow.top + 2.0 * PAD) * scale).ceil() as u32;
        let dib = Dib::create(width as i32, height as i32)
            .ok_or_else(windows::core::Error::from_win32)?;
        let scratch = create_dc_render_target(&target.GetFactory()?, dpi_x)?;
        scratch.BindDC(
            dib.dc(),
            &RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            },
        )?;
        let brush = scratch.CreateSolidColorBrush(color, None)?;
        scratch.BeginDraw();
        scratch.Clear(Some(&color_f(Color::TRANSPARENT)));
        scratch.DrawTextLayout(
            D2D_POINT_2F {
                x: PAD + offset.0,
                y: PAD + offset.1,
            },
            layout,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        scratch.EndDraw(None, None)?;
        // BGRA 与 RGBA 的通道顺序不影响逐通道卷积；保留预乘 alpha 避免黑边。
        let blurred = sayit_overlay_core::blur::fast_blur_premultiplied(
            dib.pixels().to_vec(),
            width,
            height,
            sigma * scale,
        )
        .expect("DIB byte length matches its dimensions");
        let bitmap = target.CreateBitmap(
            D2D_SIZE_U { width, height },
            Some(blurred.as_ptr().cast()),
            width * 4,
            &D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: dpi_x,
                dpiY: dpi_y,
            },
        )?;
        Ok((
            bitmap,
            super::rect_f(
                flow.left - PAD,
                flow.top - PAD,
                flow.left - PAD + width as f32 / scale,
                flow.top - PAD + height as f32 / scale,
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::create_d2d_factory;
    use super::*;

    fn style() -> TextStyle<'static> {
        TextStyle {
            family: FontFamily::SystemUi,
            size: 14.0,
            weight: 400,
            align: TextAlign::Leading,
            paragraph: ParagraphAlign::Near,
            wrap: true,
            line_spacing: None,
        }
    }
    fn target(d2d: &ID2D1Factory, dib: &Dib) -> ID2D1DCRenderTarget {
        let target = create_dc_render_target(d2d, 96.0).unwrap();
        unsafe {
            target
                .BindDC(
                    dib.dc(),
                    &RECT {
                        left: 0,
                        top: 0,
                        right: dib.width,
                        bottom: dib.height,
                    },
                )
                .unwrap();
        }
        target
    }
    #[test]
    fn native_d2d_text_cache_keeps_wrap_spacing_and_alignment_independent() {
        let text = DWriteText::new().unwrap();
        let base = style();
        let line = "这是一段用来检查换行的文字";
        let wrapped = text.layout(line, &base, 40.0, 1000.0).unwrap();
        assert!(text.metrics(&wrapped).line_count > 1);
        let unwrapped = text
            .layout(
                line,
                &TextStyle {
                    wrap: false,
                    ..base
                },
                40.0,
                1000.0,
            )
            .unwrap();
        assert_eq!(text.metrics(&unwrapped).line_count, 1);
        let wrapped_again = text.layout(line, &base, 40.0, 1000.0).unwrap();
        assert_eq!(text.metrics(&wrapped), text.metrics(&wrapped_again));
        let uniform = text
            .layout(
                line,
                &TextStyle {
                    line_spacing: Some(UniformLineSpacing {
                        height: 25.0,
                        baseline: 20.0,
                    }),
                    ..base
                },
                40.0,
                1000.0,
            )
            .unwrap();
        let metrics = text.metrics(&uniform);
        assert!((metrics.height - metrics.line_count as f32 * 25.0).abs() < 0.01);
        assert_eq!(text.formats.borrow().len(), 1);
        for other in [
            TextStyle { size: 16.0, ..base },
            TextStyle {
                weight: 600,
                ..base
            },
            TextStyle {
                align: TextAlign::Center,
                ..base
            },
            TextStyle {
                paragraph: ParagraphAlign::Center,
                ..base
            },
            TextStyle {
                family: FontFamily::Named("Segoe UI"),
                ..base
            },
            TextStyle {
                family: FontFamily::SymbolGlyphs,
                ..base
            },
        ] {
            assert!(text.layout("x", &other, 100.0, 100.0).is_some());
        }
        assert_eq!(text.formats.borrow().len(), 7);
    }
    #[test]
    fn native_d2d_tint_is_removed_from_cached_layout_after_drawing() {
        let d2d = create_d2d_factory().unwrap();
        let text = DWriteText::new().unwrap();
        let dib = Dib::create(160, 64).unwrap();
        let target = target(&d2d, &dib);
        let brush = unsafe {
            target
                .CreateSolidColorBrush(&color_f(Color::TRANSPARENT), None)
                .unwrap()
        };
        let layout = text.layout("淡入文字", &style(), 150.0, 60.0).unwrap();
        let mut resources = D2dResources::default();
        let white = Color::new(1.0, 1.0, 1.0, 1.0);
        let mut frames = Vec::new();
        for tint in [
            Some(TextTint {
                utf16_start: 0,
                utf16_len: 4,
                color: Color::new(1.0, 1.0, 1.0, 0.2),
            }),
            None,
            None,
        ] {
            unsafe {
                target.BeginDraw();
                target.Clear(Some(&color_f(Color::TRANSPARENT)));
            }
            D2dCanvas::new(&target, &brush, &text, &d2d, &mut resources, 96).draw_text(
                &layout,
                Point::new(0.0, 0.0),
                white,
                tint,
            );
            unsafe {
                target.EndDraw(None, None).unwrap();
            }
            frames.push(dib.pixels().to_vec());
        }
        assert_ne!(frames[0], frames[1]);
        assert_eq!(frames[1], frames[2]);
        let alpha: u64 = frames[1].chunks_exact(4).map(|p| p[3] as u64).sum();
        let faint: u64 = frames[0].chunks_exact(4).map(|p| p[3] as u64).sum();
        assert!(alpha > faint * 3);
    }
    #[test]
    fn native_d2d_canvas_restores_transform_clip_and_draws_cached_and_dynamic_paths() {
        let d2d = create_d2d_factory().unwrap();
        let text = DWriteText::new().unwrap();
        let dib = Dib::create(100, 100).unwrap();
        let target = target(&d2d, &dib);
        let brush = unsafe {
            target
                .CreateSolidColorBrush(&color_f(Color::TRANSPARENT), None)
                .unwrap()
        };
        let mut resources = D2dResources::default();
        let white = Color::new(1.0, 1.0, 1.0, 1.0);
        unsafe {
            target.BeginDraw();
            target.Clear(Some(&color_f(Color::TRANSPARENT)));
        }
        let mut canvas = D2dCanvas::new(&target, &brush, &text, &d2d, &mut resources, 96);
        assert_eq!(canvas.dpi(), 96);
        canvas.save();
        canvas.clip_rect(Rect::new(0.0, 0.0, 40.0, 40.0));
        canvas.concat_transform(Transform {
            scale: 2.0,
            dx: 5.0,
            dy: 6.0,
        });
        canvas.save();
        canvas.concat_transform(Transform {
            scale: 0.5,
            dx: 3.0,
            dy: 4.0,
        });
        let mut matrix = Matrix3x2::default();
        unsafe {
            target.GetTransform(&mut matrix);
        }
        assert_eq!(
            (matrix.M11, matrix.M22, matrix.M31, matrix.M32),
            (1.0, 1.0, 11.0, 14.0)
        );
        canvas.restore();
        let icon = SvgIcon {
            key: "regression-triangle",
            d: "M1 1L8 1L4 8Z",
        };
        canvas.fill_icon(&icon, white);
        canvas.stroke_icon(&icon, white, Stroke::round(1.0));
        canvas.stroke_path(
            &[
                SvgSeg::Move(1.0, 1.0),
                SvgSeg::Arc {
                    x: 10.0,
                    y: 10.0,
                    rx: 9.0,
                    ry: 9.0,
                    rotation: 0.0,
                    large: false,
                    sweep: true,
                },
            ],
            white,
            Stroke::plain(1.0),
        );
        canvas.restore();
        unsafe {
            target.GetTransform(&mut matrix);
        }
        assert_eq!(matrix, Matrix3x2::identity());
        canvas.fill_circle(Point::new(80.0, 80.0), 4.0, white);
        let layout = text.layout("模糊", &style(), 40.0, 30.0).unwrap();
        canvas
            .draw_text_blurred(
                &layout,
                Rect::new(50.0, 45.0, 90.0, 75.0),
                Point::new(0.0, 0.0),
                white,
                2.0,
            )
            .unwrap();
        drop(canvas);
        unsafe {
            target.EndDraw(None, None).unwrap();
        }
        assert_eq!(resources.icons.len(), 2);
        assert_eq!(resources.strokes.len(), 1);
        assert!(dib.pixels().chunks_exact(4).any(|p| p[3] != 0));
        let center_alpha = dib.pixels()[(80 * 100 + 80) * 4 + 3];
        assert_eq!(center_alpha, 255, "restore 必须移除裁剪和缩放");
    }
}

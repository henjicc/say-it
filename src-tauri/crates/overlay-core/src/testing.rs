use crate::canvas::*;
use crate::geom::{Color, Point, Rect, Stroke, Transform};
use crate::svg::SvgSeg;
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct FakeText;
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FakeLayout {
    pub text: String,
    pub metrics: TextMetrics,
}
impl TextEngine for FakeText {
    type Layout = Rc<FakeLayout>;
    fn layout(
        &self,
        text: &str,
        style: &TextStyle<'_>,
        max_width: f32,
        max_height: f32,
    ) -> Option<Self::Layout> {
        let width = 0.6 * style.size * text.chars().count() as f32;
        let line_count = if style.wrap {
            (width / max_width).ceil().max(1.0) as u32
        } else {
            1
        };
        let width = if style.wrap {
            width.min(max_width)
        } else {
            width
        };
        let height = line_count as f32
            * style
                .line_spacing
                .map(|s| s.height)
                .unwrap_or(style.size * 1.5);
        Some(Rc::new(FakeLayout {
            text: text.into(),
            metrics: TextMetrics {
                left: if style.align == TextAlign::Center {
                    (max_width - width) / 2.0
                } else {
                    0.0
                },
                top: if style.paragraph == ParagraphAlign::Center {
                    (max_height - height) / 2.0
                } else {
                    0.0
                },
                width,
                height,
                line_count,
            },
        }))
    }
    fn metrics(&self, layout: &Self::Layout) -> TextMetrics {
        layout.metrics
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Draw {
    Save,
    Restore,
    Clip(Rect),
    Transform(Transform),
    FillRounded(Rect, f32, Color),
    StrokeRounded(Rect, f32, Color, Stroke),
    FillCircle(Point, f32, Color),
    StrokeCircle(Point, f32, Color, Stroke),
    Line(Point, Point, Color, Stroke),
    Icon(&'static str, Color, Option<Stroke>),
    Path(Vec<SvgSeg>, Color, Stroke),
    Text(String, Point, Color, Option<TextTint>),
    Blurred(String, Rect, Point, Color, f32),
}
pub(crate) struct RecordingCanvas {
    pub calls: Vec<Draw>,
    pub dpi: u32,
    text: FakeText,
}
impl RecordingCanvas {
    pub fn new(dpi: u32) -> Self {
        Self {
            calls: Vec::new(),
            dpi,
            text: FakeText,
        }
    }
}
impl Canvas for RecordingCanvas {
    type Text = FakeText;
    fn text(&self) -> &FakeText {
        &self.text
    }
    fn dpi(&self) -> u32 {
        self.dpi
    }
    fn save(&mut self) {
        self.calls.push(Draw::Save);
    }
    fn restore(&mut self) {
        self.calls.push(Draw::Restore);
    }
    fn clip_rect(&mut self, rect: Rect) {
        self.calls.push(Draw::Clip(rect));
    }
    fn concat_transform(&mut self, t: Transform) {
        self.calls.push(Draw::Transform(t));
    }
    fn fill_rounded_rect(&mut self, r: Rect, radius: f32, c: Color) {
        self.calls.push(Draw::FillRounded(r, radius, c));
    }
    fn stroke_rounded_rect(&mut self, r: Rect, radius: f32, c: Color, s: Stroke) {
        self.calls.push(Draw::StrokeRounded(r, radius, c, s));
    }
    fn fill_circle(&mut self, p: Point, r: f32, c: Color) {
        self.calls.push(Draw::FillCircle(p, r, c));
    }
    fn stroke_circle(&mut self, p: Point, r: f32, c: Color, s: Stroke) {
        self.calls.push(Draw::StrokeCircle(p, r, c, s));
    }
    fn line(&mut self, a: Point, b: Point, c: Color, s: Stroke) {
        self.calls.push(Draw::Line(a, b, c, s));
    }
    fn fill_icon(&mut self, icon: &SvgIcon, c: Color) {
        self.calls.push(Draw::Icon(icon.key, c, None));
    }
    fn stroke_icon(&mut self, icon: &SvgIcon, c: Color, s: Stroke) {
        self.calls.push(Draw::Icon(icon.key, c, Some(s)));
    }
    fn stroke_path(&mut self, path: &[SvgSeg], c: Color, s: Stroke) {
        self.calls.push(Draw::Path(path.into(), c, s));
    }
    fn draw_text(&mut self, l: &Rc<FakeLayout>, p: Point, c: Color, t: Option<TextTint>) {
        self.calls.push(Draw::Text(l.text.clone(), p, c, t));
    }
    fn draw_text_blurred(
        &mut self,
        l: &Rc<FakeLayout>,
        b: Rect,
        p: Point,
        c: Color,
        s: f32,
    ) -> Result<(), String> {
        self.calls.push(Draw::Blurred(l.text.clone(), b, p, c, s));
        Ok(())
    }
}

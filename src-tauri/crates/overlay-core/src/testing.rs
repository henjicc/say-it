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
    /// 每行的 UTF-16 区间 [start, end)；按固定字宽逐字折行，换行符结束一行。
    pub lines: Vec<(u32, u32)>,
    pub char_width: f32,
    pub line_height: f32,
}

/// 固定字宽折行：每字宽 0.6 × 字号，「\n」单独结束一行（不占宽度）。
fn fake_lines(text: &str, char_width: f32, max_width: f32, wrap: bool) -> Vec<(u32, u32)> {
    let per_line = if wrap {
        ((max_width / char_width).floor() as u32).max(1)
    } else {
        u32::MAX
    };
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut lines = Vec::new();
    let mut start = 0u32;
    let mut count = 0u32;
    for (i, unit) in units.iter().enumerate() {
        let i = i as u32;
        if *unit == u16::from(b'\n') {
            lines.push((start, i + 1));
            start = i + 1;
            count = 0;
            continue;
        }
        if count == per_line {
            lines.push((start, i));
            start = i;
            count = 0;
        }
        count += 1;
    }
    lines.push((start, units.len() as u32));
    lines
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
        let char_width = 0.6 * style.size;
        let line_height = style
            .line_spacing
            .map(|s| s.height)
            .unwrap_or(style.size * 1.5);
        Some(Rc::new(FakeLayout {
            lines: fake_lines(text, char_width, max_width, style.wrap),
            char_width,
            line_height,
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
    /// 富文本按逐字折行重新计算度量，使命中测试与行数一致；样式不影响字宽。
    fn layout_rich(
        &self,
        text: &str,
        style: &TextStyle<'_>,
        _runs: &[TextRun],
        max_width: f32,
        max_height: f32,
    ) -> Option<Self::Layout> {
        let base = self.layout(text, style, max_width, max_height)?;
        let mut layout = (*base).clone();
        let widest = layout
            .lines
            .iter()
            .map(|&(a, b)| {
                let visible = layout.text.encode_utf16().skip(a as usize).take((b - a) as usize);
                visible.filter(|u| *u != u16::from(b'\n')).count() as f32
            })
            .fold(0.0, f32::max);
        layout.metrics = TextMetrics {
            left: 0.0,
            top: 0.0,
            width: widest * layout.char_width,
            height: layout.lines.len() as f32 * layout.line_height,
            line_count: layout.lines.len() as u32,
        };
        Some(Rc::new(layout))
    }
    fn hit_test(&self, layout: &Self::Layout, point: Point) -> u32 {
        let row = ((point.y / layout.line_height).floor().max(0.0) as usize)
            .min(layout.lines.len() - 1);
        let (start, end) = layout.lines[row];
        let text_end = if end > start && layout.text.encode_utf16().nth(end as usize - 1)
            == Some(u16::from(b'\n'))
        {
            end - 1
        } else {
            end
        };
        let column = (point.x / layout.char_width).round().max(0.0) as u32;
        (start + column).min(text_end)
    }
    fn caret_rect(&self, layout: &Self::Layout, index: u32) -> Rect {
        let row = layout
            .lines
            .iter()
            .rposition(|&(start, _)| start <= index)
            .unwrap_or(0);
        let x = (index - layout.lines[row].0) as f32 * layout.char_width;
        let top = row as f32 * layout.line_height;
        Rect::new(x, top, x, top + layout.line_height)
    }
    fn range_rects(&self, layout: &Self::Layout, start: u32, len: u32) -> Vec<Rect> {
        let end = start + len;
        layout
            .lines
            .iter()
            .enumerate()
            .filter_map(|(row, &(a, b))| {
                let (from, to) = (start.max(a), end.min(b));
                (from < to).then(|| {
                    let top = row as f32 * layout.line_height;
                    Rect::new(
                        (from - a) as f32 * layout.char_width,
                        top,
                        (to - a) as f32 * layout.char_width,
                        top + layout.line_height,
                    )
                })
            })
            .collect()
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
    Tinted(String, Point, Color, Vec<TextTint>),
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
    fn draw_text_tinted(&mut self, l: &Rc<FakeLayout>, p: Point, c: Color, t: &[TextTint]) {
        self.calls.push(Draw::Tinted(l.text.clone(), p, c, t.to_vec()));
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

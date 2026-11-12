//! 共用的绘制组件：面板底、按钮底、文字标签、音量柱和计时文本。
use crate::canvas::{Canvas, FontFamily, ParagraphAlign, TextAlign, TextEngine, TextStyle};
use crate::geom::{Color, Point, Rect, Stroke};
use crate::look::Look;

/// 主前景色（与主窗口 --color-fg 一致）。
pub fn fg(look: &Look, alpha: f32) -> Color {
    look.color(236.0 / 255.0, 237.0 / 255.0, 239.0 / 255.0, alpha)
}

/// 面板底：深色底加一圈内侧浅色细线；深色桌面靠亮线、浅色桌面靠深底看清轮廓。
/// emphasis 取 0..1，越大描边越亮（悬停反馈）。
pub fn panel<C: Canvas>(
    canvas: &mut C,
    look: &Look,
    rect: Rect,
    radius: f32,
    alpha: f32,
    emphasis: f32,
) {
    canvas.fill_rounded_rect(
        rect,
        radius,
        look.color(23.0 / 255.0, 24.0 / 255.0, 27.0 / 255.0, 0.96 * alpha),
    );
    let inner = Rect::new(
        rect.left + 0.5,
        rect.top + 0.5,
        rect.right - 0.5,
        rect.bottom - 0.5,
    );
    canvas.stroke_rounded_rect(
        inner,
        radius - 0.5,
        look.color(1.0, 1.0, 1.0, (0.34 + 0.14 * emphasis) * alpha),
        Stroke::plain(1.0),
    );
}

/// 面板上的按钮底：平时不画，悬停和按下各加一层浅色。
pub fn button_backdrop<C: Canvas>(
    canvas: &mut C,
    look: &Look,
    rect: Rect,
    radius: f32,
    hovered: bool,
    pressed: bool,
) {
    let alpha = match (pressed, hovered) {
        (true, _) => 0.22,
        (false, true) => 0.12,
        (false, false) => return,
    };
    canvas.fill_rounded_rect(rect, radius, look.color(1.0, 1.0, 1.0, alpha));
}

/// 单行界面文字的样式：系统界面字体，在给定矩形内靠左、垂直居中。
pub fn label_style(size: f32, weight: u16) -> TextStyle<'static> {
    TextStyle {
        family: FontFamily::SystemUi,
        size,
        weight,
        align: TextAlign::Leading,
        paragraph: ParagraphAlign::Center,
        wrap: false,
        line_spacing: None,
    }
}

/// 在 rect 内画一行文字。
pub fn label<C: Canvas>(
    canvas: &mut C,
    text: &str,
    style: &TextStyle<'_>,
    rect: Rect,
    color: Color,
) {
    if let Some(layout) = canvas
        .text()
        .layout(text, style, rect.width(), rect.height())
    {
        canvas.draw_text(&layout, Point::new(rect.left, rect.top), color, None);
    }
}

/// 计时文本：不足一小时 `mm:ss`，否则 `h:mm:ss`。
pub fn format_elapsed(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// 一排居中的音量柱：柱宽、间距按设备像素取整，每根柱有自己的基础高度。
#[derive(Clone, Copy, Debug)]
pub struct LevelBars {
    /// 柱宽、间距占区域边长的比例。
    pub bar_ratio: f32,
    pub gap_ratio: f32,
    /// 每根柱满音量时的高度占区域边长的比例，长度即柱数。
    pub heights: &'static [f32],
    /// 音量再小也保留的高度比例。
    pub min_scale: f32,
}

impl LevelBars {
    /// origin 为正方形区域的左上角，area 为边长（DIP）；levels 与 heights 一一对应，取 0..1。
    pub fn draw<C: Canvas>(
        &self,
        canvas: &mut C,
        dpi: u32,
        origin: Point,
        area: f32,
        levels: &[f32],
        color: Color,
    ) {
        let dpr = dpi as f32 / 96.0;
        let dpr = if dpr > 0.0 { dpr } else { 1.0 };
        let bar = ((area * dpr * self.bar_ratio).round() as i32).max(1) as f32 / dpr;
        let gap = ((area * dpr * self.gap_ratio).round() as i32).max(1) as f32 / dpr;
        let count = self.heights.len();
        let total = count as f32 * bar + (count - 1) as f32 * gap;
        let start = origin.x + ((area * dpr - total * dpr) / 2.0).round() / dpr;
        for (index, level) in levels.iter().enumerate() {
            let scale = level.max(self.min_scale);
            let mut height = area * self.heights[index] * scale;
            height = height.max(bar);
            let left = start + index as f32 * (bar + gap);
            let top = origin.y + (area - height) / 2.0;
            canvas.fill_rounded_rect(
                Rect::new(left, top, left + bar, top + height),
                bar / 2.0,
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Draw, RecordingCanvas};

    #[test]
    fn elapsed_switches_to_hours_after_sixty_minutes() {
        assert_eq!(format_elapsed(0), "00:00");
        assert_eq!(format_elapsed(59 * 60 + 59), "59:59");
        assert_eq!(format_elapsed(3600), "1:00:00");
        assert_eq!(format_elapsed(10 * 3600 + 5 * 60 + 7), "10:05:07");
    }

    #[test]
    fn level_bars_snap_to_device_pixels_and_keep_a_minimum_height() {
        const BARS: LevelBars = LevelBars {
            bar_ratio: 0.1,
            gap_ratio: 0.08,
            heights: &[0.5, 1.0, 0.5],
            min_scale: 0.2,
        };
        let mut canvas = RecordingCanvas::new(144);
        let color = Color::new(1.0, 1.0, 1.0, 1.0);
        BARS.draw(&mut canvas, 144, Point::new(10.0, 20.0), 20.0, &[0.0, 1.0, 0.5], color);
        let rects: Vec<Rect> = canvas
            .calls
            .iter()
            .map(|call| match call {
                Draw::FillRounded(rect, _, _) => *rect,
                other => panic!("unexpected draw call: {other:?}"),
            })
            .collect();
        assert_eq!(rects.len(), 3);
        // 144 dpi 下柱宽 3 设备像素、间距 2 设备像素。
        for rect in &rects {
            assert!((rect.width() * 1.5 - 3.0).abs() < 1e-4);
        }
        assert!(((rects[1].left - rects[0].right) * 1.5 - 2.0).abs() < 1e-4);
        // 静音的柱保留最小比例，满音量的柱顶满区域，全部垂直居中。
        assert!((rects[0].height() - 20.0 * 0.5 * 0.2).abs() < 1e-4);
        assert!((rects[1].height() - 20.0).abs() < 1e-4);
        for rect in &rects {
            assert!(((rect.top + rect.bottom) / 2.0 - 30.0).abs() < 1e-4);
        }
    }
}

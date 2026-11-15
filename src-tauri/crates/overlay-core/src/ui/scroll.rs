//! 纵向滚动区：记录内容高度、可视高度与滚动位置，画一条细滚动条。
use crate::canvas::Canvas;
use crate::geom::{Color, Rect};

/// 滚动条宽度与离右边的距离（DIP）。
const BAR_WIDTH: f32 = 4.0;
const BAR_INSET: f32 = 3.0;
const MIN_THUMB: f32 = 24.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
    offset: f32,
    content: f32,
    viewport: f32,
}

impl Scroll {
    pub fn offset(&self) -> f32 {
        self.offset
    }
    pub fn max_offset(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }
    pub fn overflows(&self) -> bool {
        self.max_offset() > 0.5
    }
    pub fn at_bottom(&self) -> bool {
        self.offset >= self.max_offset() - 1.0
    }
    /// 内容或可视区变化；stick_to_bottom 时保持贴底（流式输出时跟着最新内容走）。
    pub fn set_extent(&mut self, content: f32, viewport: f32, stick_to_bottom: bool) {
        let was_bottom = self.at_bottom();
        self.content = content.max(0.0);
        self.viewport = viewport.max(0.0);
        if stick_to_bottom && was_bottom {
            self.offset = self.max_offset();
        }
        self.offset = self.offset.clamp(0.0, self.max_offset());
    }
    /// 滚动 delta（DIP，正数向下）；返回位置是否改变。
    pub fn scroll_by(&mut self, delta: f32) -> bool {
        let next = (self.offset + delta).clamp(0.0, self.max_offset());
        let changed = (next - self.offset).abs() > f32::EPSILON;
        self.offset = next;
        changed
    }
    /// 让内容区间 [top, bottom) 进入可视区；返回位置是否改变。
    pub fn reveal(&mut self, top: f32, bottom: f32) -> bool {
        if top < self.offset {
            self.scroll_by(top - self.offset)
        } else if bottom > self.offset + self.viewport {
            self.scroll_by(bottom - self.offset - self.viewport)
        } else {
            false
        }
    }
    /// 画在 viewport 矩形右侧的滚动条；内容放得下时不画。
    pub fn draw<C: Canvas>(&self, canvas: &mut C, viewport: Rect, color: Color) {
        if !self.overflows() {
            return;
        }
        let track = viewport.height();
        let thumb = (track * self.viewport / self.content)
            .max(MIN_THUMB)
            .min(track);
        let top = viewport.top + (track - thumb) * self.offset / self.max_offset();
        let right = viewport.right - BAR_INSET;
        canvas.fill_rounded_rect(
            Rect::new(right - BAR_WIDTH, top, right, top + thumb),
            BAR_WIDTH / 2.0,
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extent_clamps_and_sticks_to_bottom_only_when_already_there() {
        let mut scroll = Scroll::default();
        scroll.set_extent(300.0, 100.0, true);
        assert_eq!(scroll.offset(), 200.0);
        scroll.set_extent(400.0, 100.0, true);
        assert_eq!(scroll.offset(), 300.0);
        scroll.scroll_by(-50.0);
        scroll.set_extent(500.0, 100.0, true);
        assert_eq!(scroll.offset(), 250.0);
        scroll.set_extent(120.0, 100.0, true);
        assert_eq!(scroll.offset(), 20.0);
    }

    #[test]
    fn reveal_scrolls_the_minimum_needed() {
        let mut scroll = Scroll::default();
        scroll.set_extent(1000.0, 100.0, false);
        assert!(scroll.reveal(150.0, 170.0));
        assert_eq!(scroll.offset(), 70.0);
        assert!(!scroll.reveal(80.0, 120.0));
        assert!(scroll.reveal(10.0, 30.0));
        assert_eq!(scroll.offset(), 10.0);
    }
}

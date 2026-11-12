//! 行列排布：只做一条轴上的固定项、弹性项与间距，不做约束求解。
use crate::geom::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

/// 交叉轴上的对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// 主轴上的尺寸。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Main {
    Fixed(f32),
    /// 平分固定项和间距之外的剩余空间。
    Flex,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub main: Main,
    /// 交叉轴上的尺寸。
    pub cross: f32,
}

impl Item {
    pub const fn fixed(main: f32, cross: f32) -> Self {
        Self {
            main: Main::Fixed(main),
            cross,
        }
    }
    pub const fn flex(cross: f32) -> Self {
        Self {
            main: Main::Flex,
            cross,
        }
    }
}

/// 四边各向内收。
pub fn inset(rect: Rect, x: f32, y: f32) -> Rect {
    Rect::new(rect.left + x, rect.top + y, rect.right - x, rect.bottom - y)
}

/// 在 bounds 内沿主轴依次排开，返回与 items 一一对应的矩形。
pub fn stack(axis: Axis, bounds: Rect, gap: f32, align: Align, items: &[Item]) -> Vec<Rect> {
    let (main_start, main_len, cross_start, cross_len) = match axis {
        Axis::Row => (bounds.left, bounds.width(), bounds.top, bounds.height()),
        Axis::Column => (bounds.top, bounds.height(), bounds.left, bounds.width()),
    };
    let fixed: f32 = items
        .iter()
        .map(|item| match item.main {
            Main::Fixed(size) => size,
            Main::Flex => 0.0,
        })
        .sum();
    let flex_count = items.iter().filter(|item| item.main == Main::Flex).count();
    let gaps = gap * items.len().saturating_sub(1) as f32;
    let flex = if flex_count == 0 {
        0.0
    } else {
        ((main_len - fixed - gaps) / flex_count as f32).max(0.0)
    };
    let mut cursor = main_start;
    items
        .iter()
        .map(|item| {
            let size = match item.main {
                Main::Fixed(size) => size,
                Main::Flex => flex,
            };
            let cross = match align {
                Align::Start => cross_start,
                Align::Center => cross_start + (cross_len - item.cross) / 2.0,
                Align::End => cross_start + cross_len - item.cross,
            };
            let rect = match axis {
                Axis::Row => Rect::new(cursor, cross, cursor + size, cross + item.cross),
                Axis::Column => Rect::new(cross, cursor, cross + item.cross, cursor + size),
            };
            cursor += size + gap;
            rect
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_places_fixed_items_and_gives_flex_the_rest() {
        let rects = stack(
            Axis::Row,
            Rect::new(10.0, 0.0, 110.0, 40.0),
            5.0,
            Align::Center,
            &[Item::fixed(20.0, 10.0), Item::flex(40.0), Item::fixed(30.0, 20.0)],
        );
        assert_eq!(
            rects,
            vec![
                Rect::new(10.0, 15.0, 30.0, 25.0),
                Rect::new(35.0, 0.0, 75.0, 40.0),
                Rect::new(80.0, 10.0, 110.0, 30.0),
            ]
        );
    }

    #[test]
    fn column_swaps_axes_and_end_alignment_hugs_the_far_edge() {
        let rects = stack(
            Axis::Column,
            inset(Rect::new(0.0, 0.0, 60.0, 100.0), 10.0, 10.0),
            10.0,
            Align::End,
            &[Item::fixed(20.0, 30.0), Item::fixed(10.0, 40.0)],
        );
        assert_eq!(
            rects,
            vec![
                Rect::new(20.0, 10.0, 50.0, 30.0),
                Rect::new(10.0, 40.0, 50.0, 50.0),
            ]
        );
    }
}

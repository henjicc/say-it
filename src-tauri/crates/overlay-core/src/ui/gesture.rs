//! 指针手势的共用判定。
use crate::geom::Point;

/// 按下后移动超过这么多逻辑像素才算拖动，否则仍按点击处理。
pub const DRAG_THRESHOLD: f64 = 5.0;

/// 位移（逻辑像素）是否已构成拖动。
pub fn exceeds_drag_threshold(delta_x: f64, delta_y: f64) -> bool {
    delta_x.hypot(delta_y) >= DRAG_THRESHOLD
}

/// 指针的客户区物理像素换成布局坐标（DIP）；dpi 未知（0）时按 1:1。
pub fn client_to_dip(client: (i32, i32), dpi: u32) -> Point {
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    Point::new(client.0 as f32 / scale, client.1 as f32 / scale)
}

/// 连击计数：间隔与位移都在系统常见阈值内的按下算同一串连击（双击选词、三击选段）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ClickCounter {
    last: Option<(std::time::Instant, Point)>,
    count: u8,
}

impl ClickCounter {
    const INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
    const SLOP: f32 = 4.0;
    /// 记录一次按下，返回这是连击中的第几下（1 起，三击后从 1 重新开始）。
    pub fn press(&mut self, now: std::time::Instant, point: Point) -> u8 {
        let continues = self.last.is_some_and(|(at, from)| {
            now.saturating_duration_since(at) <= Self::INTERVAL
                && (point.x - from.x).abs() <= Self::SLOP
                && (point.y - from.y).abs() <= Self::SLOP
        });
        self.count = if continues && self.count < 3 { self.count + 1 } else { 1 };
        self.last = Some((now, point));
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_count_as_multi_clicks_only_when_close_in_time_and_place() {
        let start = std::time::Instant::now();
        let at = |ms| start + std::time::Duration::from_millis(ms);
        let mut clicks = ClickCounter::default();
        assert_eq!(clicks.press(at(0), Point::new(10.0, 10.0)), 1);
        assert_eq!(clicks.press(at(200), Point::new(12.0, 11.0)), 2);
        assert_eq!(clicks.press(at(400), Point::new(12.0, 11.0)), 3);
        assert_eq!(clicks.press(at(500), Point::new(12.0, 11.0)), 1);
        assert_eq!(clicks.press(at(1200), Point::new(12.0, 11.0)), 1);
        assert_eq!(clicks.press(at(1300), Point::new(30.0, 11.0)), 1);
    }

    #[test]
    fn drag_starts_at_five_logical_pixels_of_pointer_distance() {
        assert!(!exceeds_drag_threshold(4.9, 0.0));
        assert!(exceeds_drag_threshold(5.0, 0.0));
        assert!(exceeds_drag_threshold(3.0, 4.0));
        assert!(!exceeds_drag_threshold(-3.0, 3.9));
    }

    #[test]
    fn client_pixels_scale_to_dip_and_unknown_dpi_is_identity() {
        assert_eq!(client_to_dip((30, 15), 144), Point::new(20.0, 10.0));
        assert_eq!(client_to_dip((30, 15), 0), Point::new(30.0, 15.0));
    }
}

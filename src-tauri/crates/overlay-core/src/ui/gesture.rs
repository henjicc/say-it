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

#[cfg(test)]
mod tests {
    use super::*;

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

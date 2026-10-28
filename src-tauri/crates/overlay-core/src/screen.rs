use crate::geom::PxRect;

/// Quartz 全局点：主屏左上原点，y 向下。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QPoint {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl QRect {
    /// 半开区间。
    pub fn contains(&self, q: QPoint) -> bool {
        q.x >= self.x && q.x < self.x + self.width && q.y >= self.y && q.y < self.y + self.height
    }
    pub fn center(&self) -> QPoint {
        QPoint {
            x: self.x + self.width * 0.5,
            y: self.y + self.height * 0.5,
        }
    }
}

/// 一块屏的快照；id 为 CGDirectDisplayID。scale 必须为有限正数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenInfo {
    pub id: u32,
    pub frame: QRect,
    pub visible: QRect,
    pub scale: f64,
}

/// 屏的 DesktopPx 矩形 = frame × scale，与 tao 的屏坐标与尺寸一致。
pub fn screen_px_rect(screen: &ScreenInfo) -> PxRect {
    scaled_rect(screen.frame, screen.scale)
}

fn scaled_rect(r: QRect, scale: f64) -> PxRect {
    PxRect::new(
        (r.x * scale).round() as i32,
        (r.y * scale).round() as i32,
        (r.width * scale).round() as i32,
        (r.height * scale).round() as i32,
    )
}

fn distance_squared(rect: QRect, q: QPoint) -> f64 {
    let dx = (rect.x - q.x).max(0.0).max(q.x - (rect.x + rect.width));
    let dy = (rect.y - q.y).max(0.0).max(q.y - (rect.y + rect.height));
    dx * dx + dy * dy
}

fn nearest_screen(
    screens: &[ScreenInfo],
    point: impl Fn(&ScreenInfo) -> QPoint,
) -> Option<&ScreenInfo> {
    screens.iter().min_by(|a, b| {
        distance_squared(a.frame, point(a)).total_cmp(&distance_squared(b.frame, point(b)))
    })
}

/// 点所在屏（半开）；有多个候选时优先 hint，否则按屏序。无候选取 hint，再取最近屏。
pub fn screen_for_point<'a>(
    screens: &'a [ScreenInfo],
    q: QPoint,
    hint: Option<u32>,
) -> Option<&'a ScreenInfo> {
    screens
        .iter()
        .find(|s| Some(s.id) == hint && s.frame.contains(q))
        .or_else(|| screens.iter().find(|s| s.frame.contains(q)))
        .or_else(|| screens.iter().find(|s| Some(s.id) == hint))
        .or_else(|| nearest_screen(screens, |_| q))
}

/// 空屏幕快照时按 scale=1 恒等换算。
pub fn pts_to_px(screens: &[ScreenInfo], q: QPoint, hint: Option<u32>) -> (i32, i32) {
    let scale = screen_for_point(screens, q, hint).map_or(1.0, |s| s.scale);
    ((q.x * scale).round() as i32, (q.y * scale).round() as i32)
}

fn px_screen(screens: &[ScreenInfo], p: QPoint, hint: Option<u32>) -> Option<&ScreenInfo> {
    let point = |s: &ScreenInfo| QPoint {
        x: p.x / s.scale,
        y: p.y / s.scale,
    };
    screens
        .iter()
        .find(|s| Some(s.id) == hint && s.frame.contains(point(s)))
        .or_else(|| screens.iter().find(|s| s.frame.contains(point(s))))
        .or_else(|| nearest_screen(screens, point))
}

/// 候选为 p/scale 落在自身 frame 的屏；hint 在候选中优先，否则取首个候选。
/// 无候选取按自身 scale 换算后距 frame 最近的屏。空快照返回恒等点与 id=0。
pub fn px_to_pts(screens: &[ScreenInfo], p: (i32, i32), hint: Option<u32>) -> (QPoint, u32) {
    let p = QPoint {
        x: p.0 as f64,
        y: p.1 as f64,
    };
    let screen = px_screen(screens, p, hint);
    let scale = screen.map_or(1.0, |s| s.scale);
    (
        QPoint {
            x: p.x / scale,
            y: p.y / scale,
        },
        screen.map_or(0, |s| s.id),
    )
}

/// 按矩形中心选屏，原点与尺寸均用该屏 scale；空快照按 scale=1、id=0。
pub fn rect_px_to_pts(screens: &[ScreenInfo], r: PxRect, hint: Option<u32>) -> (QRect, u32) {
    let center = QPoint {
        x: r.x as f64 + r.width as f64 * 0.5,
        y: r.y as f64 + r.height as f64 * 0.5,
    };
    let screen = px_screen(screens, center, hint);
    let scale = screen.map_or(1.0, |s| s.scale);
    (
        QRect {
            x: r.x as f64 / scale,
            y: r.y as f64 / scale,
            width: r.width as f64 / scale,
            height: r.height as f64 / scale,
        },
        screen.map_or(0, |s| s.id),
    )
}

/// 按矩形中心选屏，整窗按同一屏缩放。
pub fn rect_pts_to_px(screens: &[ScreenInfo], r: QRect, hint: Option<u32>) -> PxRect {
    scaled_rect(
        r,
        screen_for_point(screens, r.center(), hint).map_or(1.0, |s| s.scale),
    )
}

pub fn dpi_for_scale(scale: f64) -> u32 {
    (96.0 * scale).round() as u32
}

/// primary_height 为 NSScreen.screens[0].frame.height；此公式为对合。
pub fn quartz_to_cocoa(primary_height: f64, r: QRect) -> QRect {
    QRect {
        y: primary_height - (r.y + r.height),
        ..r
    }
}
pub fn cocoa_to_quartz(primary_height: f64, r: QRect) -> QRect {
    quartz_to_cocoa(primary_height, r)
}

/// 翻转视图的本地点转为相对布局原点的物理像素，并加上内容偏移。
pub fn client_px(local: QPoint, scale: f64, content_offset: (i32, i32)) -> (i32, i32) {
    (
        (local.x * scale).round() as i32 + content_offset.0,
        (local.y * scale).round() as i32 + content_offset.1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    const fn q(x: f64, y: f64) -> QPoint {
        QPoint { x, y }
    }
    const fn r(x: f64, y: f64, width: f64, height: f64) -> QRect {
        QRect {
            x,
            y,
            width,
            height,
        }
    }
    const R: ScreenInfo = ScreenInfo {
        id: 1,
        frame: r(0.0, 0.0, 1512.0, 982.0),
        visible: r(0.0, 25.0, 1512.0, 890.0),
        scale: 2.0,
    };
    const E: ScreenInfo = ScreenInfo {
        id: 2,
        frame: r(1512.0, 0.0, 1920.0, 1080.0),
        visible: r(1512.0, 0.0, 1920.0, 1080.0),
        scale: 1.0,
    };
    const L: ScreenInfo = ScreenInfo {
        id: 3,
        frame: r(-1920.0, 0.0, 1920.0, 1080.0),
        visible: r(-1920.0, 0.0, 1920.0, 1080.0),
        scale: 1.0,
    };
    const U: ScreenInfo = ScreenInfo {
        id: 4,
        frame: r(0.0, -1080.0, 1920.0, 1080.0),
        visible: r(0.0, -1080.0, 1920.0, 1080.0),
        scale: 1.0,
    };
    #[test]
    fn single_retina_screen_roundtrip() {
        assert_eq!(pts_to_px(&[R], q(100.0, 200.0), None), (200, 400));
        assert_eq!(px_to_pts(&[R], (200, 400), None), (q(100.0, 200.0), 1));
    }
    #[test]
    fn monitor_rects_match_tao_even_when_overlapping() {
        assert_eq!(screen_px_rect(&E), PxRect::new(1512, 0, 1920, 1080));
        assert_eq!(screen_px_rect(&R), PxRect::new(0, 0, 3024, 1964));
    }
    #[test]
    fn external_screen_point_uses_own_scale() {
        assert_eq!(pts_to_px(&[R, E], q(2000.0, 100.0), None), (2000, 100));
    }
    #[test]
    fn overlap_hint_selects_external_screen() {
        assert_eq!(
            px_to_pts(&[R, E], (2000, 100), Some(2)),
            (q(2000.0, 100.0), 2)
        );
    }
    #[test]
    fn overlap_hint_selects_retina_screen() {
        assert_eq!(
            px_to_pts(&[R, E], (2000, 100), Some(1)),
            (q(1000.0, 50.0), 1)
        );
    }
    #[test]
    fn overlap_without_hint_uses_first_candidate() {
        assert_eq!(px_to_pts(&[R, E], (2000, 100), None), (q(1000.0, 50.0), 1));
    }
    #[test]
    fn outside_all_screens_uses_nearest_in_each_own_scale() {
        assert_eq!(px_to_pts(&[R, E], (3500, 500), None), (q(3500.0, 500.0), 2));
    }
    #[test]
    fn boundary_is_half_open() {
        assert_eq!(pts_to_px(&[R, E], q(1512.0, 0.0), None), (1512, 0));
    }
    #[test]
    fn left_screen_preserves_negative_coordinates() {
        assert_eq!(pts_to_px(&[R, L], q(-100.0, 50.0), None), (-100, 50));
        assert_eq!(px_to_pts(&[R, L], (-100, 50), None), (q(-100.0, 50.0), 3));
    }
    #[test]
    fn upper_screen_preserves_negative_y() {
        assert_eq!(pts_to_px(&[R, U], q(10.0, -10.0), None), (10, -10));
    }
    #[test]
    fn hinted_roundtrip_grid_stays_within_half_a_device_pixel() {
        let screens = [R, E, L, U];
        for screen in screens {
            for x in 0..5 {
                for y in 0..5 {
                    let point = q(
                        screen.frame.x + (x as f64 + 0.5) * screen.frame.width / 5.0,
                        screen.frame.y + (y as f64 + 0.5) * screen.frame.height / 5.0,
                    );
                    let px = pts_to_px(&screens, point, None);
                    let (actual, id) = px_to_pts(&screens, px, Some(screen.id));
                    assert_eq!(id, screen.id);
                    assert!((actual.x - point.x).abs() <= 0.5 / screen.scale);
                    assert!((actual.y - point.y).abs() <= 0.5 / screen.scale);
                }
            }
        }
    }
    #[test]
    fn physical_rect_keeps_fractional_point_size() {
        assert_eq!(
            rect_px_to_pts(&[R], PxRect::new(200, 400, 97, 97), None),
            (r(100.0, 200.0, 48.5, 48.5), 1)
        );
    }
    #[test]
    fn crossing_rect_uses_center_screen_for_all_components() {
        assert_eq!(
            rect_pts_to_px(&[R, E], r(1500.0, 100.0, 40.0, 40.0), None),
            PxRect::new(1500, 100, 40, 40)
        );
        assert_eq!(
            rect_px_to_pts(&[R, E], PxRect::new(1500, 100, 40, 40), Some(2)),
            (r(1500.0, 100.0, 40.0, 40.0), 2)
        );
    }
    #[test]
    fn cocoa_conversion_uses_primary_height_and_is_its_own_inverse() {
        let rect = r(100.0, 200.0, 50.0, 50.0);
        assert_eq!(quartz_to_cocoa(982.0, rect), r(100.0, 732.0, 50.0, 50.0));
        assert_eq!(
            quartz_to_cocoa(982.0, U.frame),
            r(0.0, 982.0, 1920.0, 1080.0)
        );
        assert_eq!(cocoa_to_quartz(982.0, quartz_to_cocoa(982.0, rect)), rect);
        assert_eq!(
            cocoa_to_quartz(982.0, quartz_to_cocoa(982.0, U.frame)),
            U.frame
        );
    }
    #[test]
    fn dpi_follows_scale() {
        assert_eq!(
            [dpi_for_scale(1.0), dpi_for_scale(2.0), dpi_for_scale(1.5)],
            [96, 192, 144]
        );
    }
    #[test]
    fn pointer_rounds_before_adding_content_offset() {
        assert_eq!(client_px(q(10.25, 3.5), 2.0, (0, 0)), (21, 7));
        assert_eq!(client_px(q(10.25, 3.5), 2.0, (5, 8)), (26, 15));
    }

    #[test]
    fn absent_screens_use_identity_and_unknown_id() {
        assert_eq!(screen_for_point(&[], q(1.0, 2.0), None), None);
        assert_eq!(pts_to_px(&[], q(1.0, 2.0), None), (1, 2));
        assert_eq!(px_to_pts(&[], (1, 2), None), (q(1.0, 2.0), 0));
        assert_eq!(
            rect_px_to_pts(&[], PxRect::new(1, 2, 3, 4), None),
            (r(1.0, 2.0, 3.0, 4.0), 0)
        );
        assert_eq!(
            rect_pts_to_px(&[], r(1.0, 2.0, 3.0, 4.0), None),
            PxRect::new(1, 2, 3, 4)
        );
    }
    #[test]
    fn outside_point_hint_is_used_but_invalid_pixel_hint_is_ignored() {
        assert_eq!(
            screen_for_point(&[R, E], q(5000.0, 0.0), Some(1)).map(|s| s.id),
            Some(1)
        );
        assert_eq!(
            screen_for_point(&[R, E], q(5000.0, 0.0), None).map(|s| s.id),
            Some(2)
        );
        assert_eq!(
            px_to_pts(&[R, E], (3500, 500), Some(1)),
            (q(3500.0, 500.0), 2)
        );
    }
}

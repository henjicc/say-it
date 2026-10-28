/// 绘制坐标：DIP（逻辑点），布局左上原点，y 向下。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}
impl Rect {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
    pub fn width(&self) -> f32 {
        self.right - self.left
    }
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
    /// 半开区间 [left,right)×[top,bottom)。
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.top && p.y < self.bottom
    }
    pub fn union(&self, other: &Rect) -> Rect {
        Rect::new(
            self.left.min(other.left),
            self.top.min(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
        )
    }
}

/// DesktopPx 矩形：Windows 虚拟桌面物理像素；macOS 为 Quartz 点乘所在屏缩放。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
impl PxRect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub const fn right(&self) -> i32 {
        self.x + self.width
    }
    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }
    /// 半开区间。
    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// DIP 矩形按 dpi 外扩到整设备像素，返回相对布局原点的物理像素矩形。
pub fn snap_out_px(rect: Rect, dpi: u32) -> PxRect {
    let scale = dpi as f32 / 96.0;
    let x = (rect.left * scale).floor() as i32;
    let y = (rect.top * scale).floor() as i32;
    PxRect::new(
        x,
        y,
        (rect.right * scale).ceil() as i32 - x,
        (rect.bottom * scale).ceil() as i32 - y,
    )
}

/// 画刷颜色，straight alpha；输出缓冲的预乘由后端负责。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
impl Color {
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
    /// WindowsLegacy 胶囊与悬浮球的画刷色兼容规则，仅由 Look::color 使用。
    pub const fn rgb_times_alpha(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::new(r * a, g * a, b * a, a)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineCap {
    Flat,
    Round,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineJoin {
    Miter,
    Round,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
}
impl Stroke {
    /// D2D 默认笔触：平头、斜接（miter limit 10）。
    pub const fn plain(width: f32) -> Self {
        Self {
            width,
            cap: LineCap::Flat,
            join: LineJoin::Miter,
        }
    }
    pub const fn round(width: f32) -> Self {
        Self {
            width,
            cap: LineCap::Round,
            join: LineJoin::Round,
        }
    }
}

/// 等比缩放 + 平移，线宽随之缩放。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub scale: f32,
    pub dx: f32,
    pub dy: f32,
}

/// 交互与玻璃形状（DIP，布局坐标）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    RoundedRect { rect: Rect, radius: f32 },
    Circle { center: Point, radius: f32 },
}
impl Shape {
    pub fn bounds(&self) -> Rect {
        match *self {
            Self::RoundedRect { rect, .. } => rect,
            Self::Circle { center, radius } => Rect::new(
                center.x - radius,
                center.y - radius,
                center.x + radius,
                center.y + radius,
            ),
        }
    }
    pub fn contains(&self, p: Point) -> bool {
        match *self {
            Self::Circle { center, radius } => {
                radius >= 0.0
                    && (p.x - center.x).powi(2) + (p.y - center.y).powi(2) <= radius * radius
            }
            Self::RoundedRect { rect, radius } => {
                if !rect.contains(p) {
                    return false;
                }
                let radius = radius
                    .max(0.0)
                    .min(rect.width() * 0.5)
                    .min(rect.height() * 0.5);
                let x = p.x.clamp(rect.left + radius, rect.right - radius);
                let y = p.y.clamp(rect.top + radius, rect.bottom - radius);
                (p.x - x).powi(2) + (p.y - y).powi(2) <= radius * radius
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shapes_respect_corners_and_half_open_rectangles() {
        let shape = Shape::RoundedRect {
            rect: Rect::new(0.0, 0.0, 20.0, 10.0),
            radius: 3.0,
        };
        assert!(!shape.contains(Point::new(0.0, 0.0)));
        assert!(shape.contains(Point::new(3.0, 0.0)));
        assert!(shape.contains(Point::new(10.0, 5.0)));
        assert!(!shape.contains(Point::new(20.0, 5.0)));
        assert!(Shape::RoundedRect {
            rect: shape.bounds(),
            radius: 100.0
        }
        .contains(Point::new(5.0, 0.0)));
        let circle = Shape::Circle {
            center: Point::new(5.0, 5.0),
            radius: 5.0,
        };
        assert_eq!(circle.bounds(), Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(circle.contains(Point::new(5.0, 0.0)));
        assert!(!circle.contains(Point::new(0.0, 0.0)));
    }
    #[test]
    fn snap_out_preserves_fractional_edges_at_device_scale() {
        assert_eq!(
            snap_out_px(Rect::new(-0.25, 0.25, 10.25, 20.75), 144),
            PxRect::new(-1, 0, 17, 32)
        );
        assert_eq!(
            snap_out_px(Rect::new(1.0, 2.0, 3.0, 4.0), 192),
            PxRect::new(2, 4, 4, 4)
        );
    }
}

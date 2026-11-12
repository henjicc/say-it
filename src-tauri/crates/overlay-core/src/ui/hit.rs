//! 命中表：模型发布给指针层的可交互区域快照。定长、Copy，可跨线程发布。
use super::gesture::client_to_dip;
use crate::geom::Rect;

/// 一个窗口最多登记的可交互区域数。
pub const MAX_REGIONS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    /// 悬停、按下后在同一区域抬起算一次点击。
    Button,
    /// 按住并移动超过阈值后拖动整个窗口。
    Drag,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub id: u16,
    /// 布局坐标（DIP）。
    pub rect: Rect,
    pub kind: HitKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitMap {
    dpi: u32,
    /// 模型当前认定的悬停按钮。指针层拿它和实际指针位置比较，不自己记悬停：
    /// 窗口在悬停中被隐藏或切成穿透时收不到离开事件，以模型为准才不会留下过期状态。
    hover: Option<u16>,
    len: usize,
    regions: [Region; MAX_REGIONS],
}

impl HitMap {
    pub fn new(dpi: u32) -> Self {
        Self {
            dpi,
            hover: None,
            len: 0,
            regions: [Region {
                id: 0,
                rect: Rect::ZERO,
                kind: HitKind::Drag,
            }; MAX_REGIONS],
        }
    }
    /// 后登记的区域叠在先登记的上面。
    pub fn push(&mut self, id: u16, rect: Rect, kind: HitKind) {
        assert!(self.len < MAX_REGIONS, "overlay hit map is full");
        self.regions[self.len] = Region { id, rect, kind };
        self.len += 1;
    }
    pub fn set_hover(&mut self, hover: Option<u16>) {
        self.hover = hover;
    }
    pub fn hover(&self) -> Option<u16> {
        self.hover
    }
    pub fn dpi(&self) -> u32 {
        self.dpi
    }
    /// 客户区物理像素处最上层的区域。
    pub fn at(&self, client: (i32, i32)) -> Option<Region> {
        let point = client_to_dip(client, self.dpi);
        self.regions[..self.len]
            .iter()
            .rev()
            .find(|region| region.rect.contains(point))
            .copied()
    }
}

impl Default for HitMap {
    fn default() -> Self {
        Self::new(96)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_regions_cover_earlier_ones_and_points_scale_with_dpi() {
        let mut map = HitMap::new(192);
        map.push(1, Rect::new(0.0, 0.0, 100.0, 40.0), HitKind::Drag);
        map.push(2, Rect::new(70.0, 8.0, 94.0, 32.0), HitKind::Button);
        assert_eq!(map.at((20, 20)).map(|r| r.id), Some(1));
        assert_eq!(map.at((160, 40)).map(|r| (r.id, r.kind)), Some((2, HitKind::Button)));
        assert_eq!(map.at((300, 20)), None);
    }
}

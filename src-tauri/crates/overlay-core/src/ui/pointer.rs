//! 通用指针层：按命中表处理悬停、按下、点击和拖动窗口。
use super::gesture::{client_to_dip, exceeds_drag_threshold};
use super::hit::{HitKind, HitMap, Region};
use crate::geom::Point;
use crate::window::{CursorShape, PointerEvent, PointerModel, PointerStep, WindowOp};
use std::marker::PhantomData;

/// 交给模型的输入；编号是模型登记区域时用的 id。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiInput {
    /// 指针下的按钮变化；None = 不在任何按钮上。
    Hover(Option<u16>),
    /// 按住的按钮变化；None = 已松开或取消。
    Press(Option<u16>),
    /// 在同一个按钮上按下并抬起。
    Click(u16),
    /// 在按钮上右键抬起。
    RightClick(u16),
    /// 在文字区域或滑块上按下；point 为布局坐标（DIP）。连击由模型按时间判断。
    PointPress { id: u16, point: Point },
    /// 按下后移动；指针可能已离开该区域（选中时拖到外面要继续滚动，滑块拖到外面停在端点）。
    PointDrag { id: u16, point: Point },
    /// 按下后抬起，或捕获丢失。
    PointRelease { id: u16, point: Point },
}

#[derive(Clone, Copy, Debug)]
struct Press {
    screen: (i32, i32),
    region: Option<Region>,
}

/// A 是所属模型的动作类型；指针层自己不产生动作，点击由模型换成动作。
pub struct UiPointer<A> {
    tracking: bool,
    /// 最近一次指针位置（客户区物理像素），决定光标形状。
    last: Option<(i32, i32)>,
    press: Option<Press>,
    dragging: bool,
    action: PhantomData<fn() -> A>,
}

impl<A> Default for UiPointer<A> {
    fn default() -> Self {
        Self {
            tracking: false,
            last: None,
            press: None,
            dragging: false,
            action: PhantomData,
        }
    }
}

impl<A> UiPointer<A> {
    fn pressed_button(&self) -> Option<u16> {
        let region = self.press?.region?;
        (region.kind == HitKind::Button).then_some(region.id)
    }
    fn pressed_point(&self) -> Option<u16> {
        let region = self.press?.region?;
        matches!(region.kind, HitKind::Text | HitKind::Slider).then_some(region.id)
    }
    fn cancel_press(&mut self, steps: &mut Vec<PointerStep<A, UiInput>>) {
        if self.pressed_button().is_some() {
            steps.push(PointerStep::Model(UiInput::Press(None)));
        }
        self.press = None;
        self.dragging = false;
    }
}

impl<A: Send + 'static> PointerModel for UiPointer<A> {
    type Hit = HitMap;
    type Input = UiInput;
    type Action = A;
    fn event(&mut self, event: PointerEvent, hit: &HitMap) -> Vec<PointerStep<A, UiInput>> {
        let mut steps = Vec::new();
        match event {
            PointerEvent::Move { client, screen } => {
                self.last = Some(client);
                if let Some(id) = self.pressed_point() {
                    let point = client_to_dip(client, hit.dpi());
                    steps.push(PointerStep::Model(UiInput::PointDrag { id, point }));
                }
                let region = hit.at(client);
                if !self.tracking {
                    self.tracking = true;
                    steps.push(PointerStep::Op(WindowOp::TrackMouseLeave));
                }
                let hover = region
                    .filter(|region| region.kind == HitKind::Button)
                    .map(|region| region.id);
                if hover != hit.hover() {
                    steps.push(PointerStep::Model(UiInput::Hover(hover)));
                }
                if let Some(press) = self.press {
                    let draggable = press
                        .region
                        .is_some_and(|region| region.kind == HitKind::Drag);
                    let dpr = f64::from(hit.dpi().max(96)) / 96.0;
                    if draggable
                        && !self.dragging
                        && exceeds_drag_threshold(
                            f64::from(screen.0 - press.screen.0) / dpr,
                            f64::from(screen.1 - press.screen.1) / dpr,
                        )
                    {
                        self.dragging = true;
                        steps.push(PointerStep::Op(WindowOp::BeginSystemDrag));
                    }
                }
            }
            PointerEvent::LeftDown { client, screen } => {
                self.press = Some(Press {
                    screen,
                    region: hit.at(client),
                });
                self.dragging = false;
                if let Some(id) = self.pressed_button() {
                    steps.push(PointerStep::Model(UiInput::Press(Some(id))));
                }
                if let Some(id) = self.pressed_point() {
                    let point = client_to_dip(client, hit.dpi());
                    steps.push(PointerStep::Model(UiInput::PointPress { id, point }));
                }
            }
            PointerEvent::LeftUp { client } => {
                if let Some(id) = self.pressed_point() {
                    let point = client_to_dip(client, hit.dpi());
                    steps.push(PointerStep::Model(UiInput::PointRelease { id, point }));
                }
                let pressed = self.pressed_button();
                self.cancel_press(&mut steps);
                if let Some(id) = pressed {
                    let released_on = hit.at(client).map(|region| (region.id, region.kind));
                    if released_on == Some((id, HitKind::Button)) {
                        steps.push(PointerStep::Model(UiInput::Click(id)));
                    }
                }
            }
            PointerEvent::RightUp { client } => {
                if let Some(region) = hit.at(client).filter(|region| region.kind == HitKind::Button) {
                    steps.push(PointerStep::Model(UiInput::RightClick(region.id)));
                }
            }
            PointerEvent::Leave => {
                self.tracking = false;
                if hit.hover().is_some() {
                    steps.push(PointerStep::Model(UiInput::Hover(None)));
                }
            }
            // 系统拖拽接管指针后不会再收到抬起；被其他窗口抢走捕获时同样放弃这次按下。
            PointerEvent::CaptureLost => {
                if let Some(id) = self.pressed_point() {
                    let point = client_to_dip(self.last.unwrap_or_default(), hit.dpi());
                    steps.push(PointerStep::Model(UiInput::PointRelease { id, point }));
                }
                self.cancel_press(&mut steps);
            }
        }
        steps
    }
    fn cursor(&self, hit: &HitMap) -> CursorShape {
        if hit.hover().is_some() {
            return CursorShape::Hand;
        }
        let over_text = self
            .last
            .and_then(|client| hit.at(client))
            .is_some_and(|region| region.kind == HitKind::Text);
        let pressing_text = self
            .press
            .and_then(|press| press.region)
            .is_some_and(|region| region.kind == HitKind::Text);
        if over_text || pressing_text {
            CursorShape::Text
        } else {
            CursorShape::Arrow
        }
    }
    fn ime_rect(&self, hit: &HitMap) -> Option<crate::geom::Rect> {
        hit.caret()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;

    type Pointer = UiPointer<()>;
    type Step = PointerStep<(), UiInput>;
    const BODY: u16 = 1;
    const STOP: u16 = 2;

    fn hit() -> HitMap {
        hit_hovering(None)
    }
    fn hit_hovering(hover: Option<u16>) -> HitMap {
        let mut map = HitMap::new(96);
        map.set_hover(hover);
        map.push(BODY, Rect::new(0.0, 0.0, 200.0, 36.0), HitKind::Drag);
        map.push(STOP, Rect::new(168.0, 6.0, 192.0, 30.0), HitKind::Button);
        map
    }
    fn moved(pointer: &mut Pointer, x: i32, y: i32) -> Vec<Step> {
        pointer.event(
            PointerEvent::Move {
                client: (x, y),
                screen: (1000 + x, 500 + y),
            },
            &hit(),
        )
    }
    fn down(pointer: &mut Pointer, x: i32, y: i32) -> Vec<Step> {
        pointer.event(
            PointerEvent::LeftDown {
                client: (x, y),
                screen: (1000 + x, 500 + y),
            },
            &hit(),
        )
    }
    fn up(pointer: &mut Pointer, x: i32, y: i32) -> Vec<Step> {
        pointer.event(PointerEvent::LeftUp { client: (x, y) }, &hit())
    }

    #[test]
    fn hover_is_compared_with_the_models_view_and_leave_rearms_tracking() {
        let mut pointer = Pointer::default();
        assert_eq!(
            moved(&mut pointer, 20, 18),
            vec![PointerStep::Op(WindowOp::TrackMouseLeave)]
        );
        assert_eq!(
            moved(&mut pointer, 180, 18),
            vec![PointerStep::Model(UiInput::Hover(Some(STOP)))]
        );
        // 模型已经记下悬停后不再重复上报；模型自己清掉悬停（如重新显示）后会再报一次。
        let over_stop = PointerEvent::Move {
            client: (181, 18),
            screen: (1181, 518),
        };
        assert!(pointer.event(over_stop, &hit_hovering(Some(STOP))).is_empty());
        assert_eq!(pointer.cursor(&hit_hovering(Some(STOP))), CursorShape::Hand);
        assert_eq!(
            pointer.event(over_stop, &hit()),
            vec![PointerStep::Model(UiInput::Hover(Some(STOP)))]
        );
        assert_eq!(
            pointer.event(PointerEvent::Leave, &hit_hovering(Some(STOP))),
            vec![PointerStep::Model(UiInput::Hover(None))]
        );
        assert_eq!(pointer.cursor(&hit()), CursorShape::Arrow);
        assert_eq!(
            moved(&mut pointer, 20, 18),
            vec![PointerStep::Op(WindowOp::TrackMouseLeave)]
        );
    }

    #[test]
    fn click_needs_press_and_release_on_the_same_button() {
        let mut pointer = Pointer::default();
        assert_eq!(
            down(&mut pointer, 180, 18),
            vec![PointerStep::Model(UiInput::Press(Some(STOP)))]
        );
        assert_eq!(
            up(&mut pointer, 180, 18),
            vec![
                PointerStep::Model(UiInput::Press(None)),
                PointerStep::Model(UiInput::Click(STOP))
            ]
        );
        down(&mut pointer, 180, 18);
        assert_eq!(
            up(&mut pointer, 20, 18),
            vec![PointerStep::Model(UiInput::Press(None))]
        );
        down(&mut pointer, 20, 18);
        assert!(up(&mut pointer, 180, 18).is_empty());
    }

    #[test]
    fn text_regions_forward_press_drag_and_release_in_dip_and_show_text_cursor() {
        const TEXT: u16 = 3;
        let mut map = HitMap::new(192);
        map.push(TEXT, Rect::new(0.0, 40.0, 200.0, 140.0), HitKind::Text);
        let mut pointer = Pointer::default();
        let at = |x, y| PointerEvent::Move {
            client: (x, y),
            screen: (x, y),
        };
        pointer.event(at(20, 100), &map);
        assert_eq!(pointer.cursor(&map), CursorShape::Text);
        assert_eq!(
            pointer.event(
                PointerEvent::LeftDown {
                    client: (20, 100),
                    screen: (20, 100)
                },
                &map
            ),
            vec![PointerStep::Model(UiInput::PointPress {
                id: TEXT,
                point: Point::new(10.0, 50.0)
            })]
        );
        // 拖出区域仍算选中拖动，不会进入窗口拖拽。
        assert_eq!(
            pointer.event(at(20, 400), &map),
            vec![PointerStep::Model(UiInput::PointDrag {
                id: TEXT,
                point: Point::new(10.0, 200.0)
            })]
        );
        assert_eq!(pointer.cursor(&map), CursorShape::Text);
        assert_eq!(
            pointer.event(PointerEvent::LeftUp { client: (20, 400) }, &map),
            vec![PointerStep::Model(UiInput::PointRelease {
                id: TEXT,
                point: Point::new(10.0, 200.0)
            })]
        );
        assert_eq!(pointer.cursor(&map), CursorShape::Arrow);
    }

    #[test]
    fn drag_starts_once_from_a_drag_region_and_never_from_a_button() {
        let mut pointer = Pointer::default();
        down(&mut pointer, 20, 18);
        moved(&mut pointer, 23, 18);
        assert_eq!(
            moved(&mut pointer, 26, 18),
            vec![PointerStep::Op(WindowOp::BeginSystemDrag)]
        );
        assert!(moved(&mut pointer, 60, 18).is_empty());
        // 系统拖拽开始后宿主补发捕获丢失；之后的移动不会再次进入拖拽。
        assert!(pointer.event(PointerEvent::CaptureLost, &hit()).is_empty());
        assert!(moved(&mut pointer, 90, 18).is_empty());

        down(&mut pointer, 180, 18);
        let steps = moved(&mut pointer, 100, 18);
        assert!(!steps.contains(&PointerStep::Op(WindowOp::BeginSystemDrag)));
        assert_eq!(
            pointer.event(PointerEvent::CaptureLost, &hit()),
            vec![PointerStep::Model(UiInput::Press(None))]
        );
    }
}

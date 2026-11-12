//! 通用指针层：按命中表处理悬停、按下、点击和拖动窗口。
use super::gesture::exceeds_drag_threshold;
use super::hit::{HitKind, HitMap, Region};
use crate::window::{CursorShape, PointerEvent, PointerModel, PointerStep, WindowOp};
use std::marker::PhantomData;

/// 交给模型的输入；编号是模型登记区域时用的 id。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiInput {
    /// 指针下的按钮变化；None = 不在任何按钮上。
    Hover(Option<u16>),
    /// 按住的按钮变化；None = 已松开或取消。
    Press(Option<u16>),
    /// 在同一个按钮上按下并抬起。
    Click(u16),
    /// 在按钮上右键抬起。
    RightClick(u16),
}

#[derive(Clone, Copy, Debug)]
struct Press {
    screen: (i32, i32),
    region: Option<Region>,
}

/// A 是所属模型的动作类型；指针层自己不产生动作，点击由模型换成动作。
pub struct UiPointer<A> {
    tracking: bool,
    press: Option<Press>,
    dragging: bool,
    action: PhantomData<fn() -> A>,
}

impl<A> Default for UiPointer<A> {
    fn default() -> Self {
        Self {
            tracking: false,
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
            }
            PointerEvent::LeftUp { client } => {
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
            PointerEvent::CaptureLost => self.cancel_press(&mut steps),
        }
        steps
    }
    fn cursor(&self, hit: &HitMap) -> CursorShape {
        if hit.hover().is_some() {
            CursorShape::Hand
        } else {
            CursorShape::Arrow
        }
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

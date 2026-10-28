use crate::geom::{Point, Rect};
use crate::orb::should_start_orb_drag;
use crate::subtitle::SubtitleAction;
use crate::window::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleButton {
    Lock,
    Reset,
    Close,
}
impl SubtitleButton {
    pub fn index(self) -> usize {
        match self {
            Self::Lock => 0,
            Self::Reset => 1,
            Self::Close => 2,
        }
    }
    pub(super) fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Lock,
            1 => Self::Reset,
            _ => Self::Close,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleInput {
    Hover {
        over_block: bool,
        button: Option<SubtitleButton>,
    },
    Leave,
    Button(SubtitleButton),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubtitleHit {
    pub dpi: u32,
    pub block: Rect,
    pub buttons: Option<[Rect; 3]>,
    pub locked: bool,
    pub hovering_block: bool,
    pub hover_button: Option<SubtitleButton>,
    /// 宿主结束系统拖拽后递增，指针层据此清除拖拽会话；捕获丢失不改变它。
    pub drag_epoch: u64,
}
impl SubtitleHit {
    fn point(&self, client: (i32, i32)) -> Point {
        let scale = if self.dpi == 0 {
            1.0
        } else {
            self.dpi as f32 / 96.0
        };
        Point::new(client.0 as f32 / scale, client.1 as f32 / scale)
    }
    fn button_at(&self, point: Point) -> Option<SubtitleButton> {
        self.buttons?
            .iter()
            .position(|rect| rect.contains(point))
            .map(SubtitleButton::from_index)
    }
}
#[derive(Clone, Copy, Debug)]
struct Press {
    screen: (i32, i32),
    on_button: Option<SubtitleButton>,
    draggable: bool,
}
#[derive(Debug, Default)]
pub struct SubtitlePointer {
    press: Option<Press>,
    dragging: bool,
    hover_tracking: bool,
    drag_epoch: u64,
}
impl PointerModel for SubtitlePointer {
    type Hit = SubtitleHit;
    type Input = SubtitleInput;
    type Action = SubtitleAction;
    fn event(
        &mut self,
        event: PointerEvent,
        hit: &SubtitleHit,
    ) -> Vec<PointerStep<SubtitleAction, SubtitleInput>> {
        if self.drag_epoch != hit.drag_epoch {
            self.press = None;
            self.dragging = false;
            self.drag_epoch = hit.drag_epoch;
        }
        let mut steps = Vec::new();
        match event {
            PointerEvent::Move { client, screen } => {
                let point = hit.point(client);
                let over_block = hit.block.contains(point);
                if over_block && !self.hover_tracking {
                    self.hover_tracking = true;
                    steps.push(PointerStep::Op(WindowOp::TrackMouseLeave));
                }
                let button = if over_block {
                    hit.button_at(point)
                } else {
                    None
                };
                if over_block != hit.hovering_block || button != hit.hover_button {
                    steps.push(PointerStep::Model(SubtitleInput::Hover {
                        over_block,
                        button,
                    }));
                }
                if let Some(press) = self.press {
                    let dpr = f64::from(hit.dpi.max(96)) / 96.0;
                    if !self.dragging
                        && press.draggable
                        && should_start_orb_drag(
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
                let point = hit.point(client);
                let on_button = hit.button_at(point);
                self.press = Some(Press {
                    screen,
                    on_button,
                    draggable: hit.block.contains(point) && on_button.is_none() && !hit.locked,
                });
                self.dragging = false;
            }
            PointerEvent::LeftUp { client } => {
                let press = self.press.take();
                self.dragging = false;
                if let Some(button) = press.and_then(|press| {
                    press
                        .on_button
                        .filter(|button| hit.button_at(hit.point(client)) == Some(*button))
                }) {
                    steps.push(PointerStep::Model(SubtitleInput::Button(button)));
                }
            }
            PointerEvent::Leave => {
                self.hover_tracking = false;
                steps.push(PointerStep::Model(SubtitleInput::Leave));
            }
            // Windows 字幕保留捕获丢失后的按下状态，直到抬起或系统拖拽完成。
            PointerEvent::CaptureLost | PointerEvent::RightUp { .. } => {}
        }
        steps
    }
    fn cursor(&self, hit: &SubtitleHit) -> CursorShape {
        if (self.dragging && self.drag_epoch == hit.drag_epoch)
            || (hit.hovering_block && hit.hover_button.is_none() && !hit.locked)
        {
            CursorShape::Move
        } else {
            CursorShape::Arrow
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hit(dpi: u32) -> SubtitleHit {
        SubtitleHit {
            dpi,
            block: Rect::new(0.0, 0.0, 100.0, 100.0),
            buttons: Some([
                Rect::new(10.0, 10.0, 20.0, 20.0),
                Rect::new(30.0, 10.0, 40.0, 20.0),
                Rect::new(50.0, 10.0, 60.0, 20.0),
            ]),
            locked: false,
            hovering_block: true,
            hover_button: None,
            drag_epoch: 0,
        }
    }
    fn down(pointer: &mut SubtitlePointer, point: (i32, i32), hit: &SubtitleHit) {
        pointer.event(
            PointerEvent::LeftDown {
                client: point,
                screen: (100, 100),
            },
            hit,
        );
    }
    #[test]
    fn clicks_require_the_same_button_on_release() {
        let hit = hit(96);
        let mut pointer = SubtitlePointer::default();
        for (x, button) in [
            (15, SubtitleButton::Lock),
            (35, SubtitleButton::Reset),
            (55, SubtitleButton::Close),
        ] {
            down(&mut pointer, (x, 15), &hit);
            assert_eq!(
                pointer.event(PointerEvent::LeftUp { client: (x, 15) }, &hit),
                [PointerStep::Model(SubtitleInput::Button(button))]
            );
        }
        down(&mut pointer, (15, 15), &hit);
        assert!(pointer
            .event(PointerEvent::LeftUp { client: (35, 15) }, &hit)
            .is_empty());
        down(&mut pointer, (70, 70), &hit);
        assert!(pointer
            .event(PointerEvent::LeftUp { client: (15, 15) }, &hit)
            .is_empty());
    }
    #[test]
    fn drag_threshold_keeps_low_dpi_clamp_capture_loss_and_completion() {
        for (dpi, below, threshold) in [(48, 4, 5), (96, 4, 5), (144, 7, 8)] {
            let mut hit = hit(dpi);
            let mut pointer = SubtitlePointer::default();
            let body = (70 * dpi as i32 / 96, 70 * dpi as i32 / 96);
            down(&mut pointer, body, &hit);
            assert!(pointer.event(PointerEvent::CaptureLost, &hit).is_empty());
            let movement = |dx| PointerEvent::Move {
                client: (70, 70),
                screen: (100 + dx, 100),
            };
            assert!(!pointer
                .event(movement(below), &hit)
                .contains(&PointerStep::Op(WindowOp::BeginSystemDrag)));
            assert!(pointer
                .event(movement(threshold), &hit)
                .contains(&PointerStep::Op(WindowOp::BeginSystemDrag)));
            assert_eq!(pointer.cursor(&hit), CursorShape::Move);
            assert!(!pointer
                .event(movement(threshold + 10), &hit)
                .contains(&PointerStep::Op(WindowOp::BeginSystemDrag)));
            hit.drag_epoch += 1;
            hit.locked = true;
            assert_eq!(pointer.cursor(&hit), CursorShape::Arrow);
            assert!(!pointer
                .event(movement(threshold + 20), &hit)
                .contains(&PointerStep::Op(WindowOp::BeginSystemDrag)));
            assert!(pointer
                .event(PointerEvent::LeftUp { client: (15, 15) }, &hit)
                .is_empty());
        }
    }
    #[test]
    fn hover_tracking_cursor_and_locked_press_match_legacy() {
        let mut hit = hit(96);
        hit.hovering_block = false;
        let mut pointer = SubtitlePointer::default();
        assert_eq!(
            pointer.event(
                PointerEvent::Move {
                    client: (70, 70),
                    screen: (0, 0)
                },
                &hit
            ),
            [
                PointerStep::Op(WindowOp::TrackMouseLeave),
                PointerStep::Model(SubtitleInput::Hover {
                    over_block: true,
                    button: None
                })
            ]
        );
        hit.hovering_block = true;
        assert_eq!(pointer.cursor(&hit), CursorShape::Move);
        assert!(pointer
            .event(
                PointerEvent::Move {
                    client: (70, 70),
                    screen: (0, 0)
                },
                &hit
            )
            .is_empty());
        hit.hover_button = Some(SubtitleButton::Lock);
        assert_eq!(pointer.cursor(&hit), CursorShape::Arrow);
        hit.hover_button = None;
        hit.locked = true;
        assert_eq!(pointer.cursor(&hit), CursorShape::Arrow);
        down(&mut pointer, (70, 70), &hit);
        assert!(!pointer
            .event(
                PointerEvent::Move {
                    client: (70, 70),
                    screen: (200, 200)
                },
                &hit
            )
            .contains(&PointerStep::Op(WindowOp::BeginSystemDrag)));
        assert_eq!(
            pointer.event(PointerEvent::Leave, &hit),
            [PointerStep::Model(SubtitleInput::Leave)]
        );
        assert_eq!(
            pointer
                .event(
                    PointerEvent::Move {
                        client: (70, 70),
                        screen: (200, 200)
                    },
                    &hit
                )
                .first(),
            Some(&PointerStep::Op(WindowOp::TrackMouseLeave))
        );
    }
}

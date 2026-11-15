//! 主线程指针层仅读 Copy 命中快照；模型输入通过单向通道投递。
use crate::{
    ct::CtText,
    panel::OverlayView,
    render::{self, Host, WindowId},
};
use sayit_overlay_core::geom::{Point, Rect};
use sayit_overlay_core::window::{
    CursorShape, KeyInput, OverlayModel, PointerEvent, PointerModel, PointerStep, WindowOp,
};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

pub(crate) struct MainCx<'a> {
    pub view: &'a OverlayView,
    tasks: RefCell<Vec<MainTask>>,
}
impl<'a> MainCx<'a> {
    pub fn new(view: &'a OverlayView) -> Self {
        Self {
            view,
            tasks: RefCell::new(Vec::new()),
        }
    }
    pub fn run(&self) {
        let tasks = std::mem::take(&mut *self.tasks.borrow_mut());
        for task in tasks {
            task(self);
        }
    }
}

/// 输出在结束指针状态借用后执行，AppKit 同步通知可安全重入视图。
pub(crate) type MainTask = Box<dyn FnOnce(&MainCx<'_>)>;
pub(crate) trait ViewInput {
    fn event(&mut self, event: PointerEvent, cx: &MainCx<'_>);
    fn cursor(&self) -> CursorShape;
    /// 键盘与输入法输入，转给渲染线程的模型。
    fn key(&self, input: KeyInput);
    /// 滚轮：point 为布局坐标（DIP），lines 向上为正。
    fn wheel(&self, point: Point, lines: f32);
    /// 输入框光标（DIP，布局坐标），供输入法候选窗定位。
    fn ime_rect(&self) -> Option<Rect>;
}

pub(crate) struct PointerBridge<P: PointerModel> {
    pointer: P,
    hit: Arc<Mutex<P::Hit>>,
    id: WindowId,
    input: fn(P::Input) -> render::HostJob,
    key: fn(KeyInput) -> render::HostJob,
    wheel: fn(Point, f32) -> render::HostJob,
    action: fn(P::Action),
}

impl<P: PointerModel + 'static> PointerBridge<P> {
    pub fn factory<M: OverlayModel<CtText, Pointer = P> + 'static>(
        host: &Host<M>,
        action: fn(P::Action),
    ) -> impl FnOnce() -> Option<Box<dyn ViewInput>> + Send {
        let hit = host.hit.clone();
        let id = host.slot.id;
        move || {
            Some(Box::new(Self {
                pointer: P::default(),
                hit,
                id,
                action,
                input: |input| {
                    Box::new(move |host, cx| {
                        host.as_any_mut()
                            .downcast_mut::<Host<M>>()
                            .expect("overlay host type")
                            .input(input, cx);
                    })
                },
                key: |input| {
                    Box::new(move |host, cx| {
                        host.as_any_mut()
                            .downcast_mut::<Host<M>>()
                            .expect("overlay host type")
                            .key(input, cx);
                    })
                },
                wheel: |point, lines| {
                    Box::new(move |host, cx| {
                        host.as_any_mut()
                            .downcast_mut::<Host<M>>()
                            .expect("overlay host type")
                            .wheel(point, lines, cx);
                    })
                },
            }))
        }
    }
}

impl<P: PointerModel + 'static> ViewInput for PointerBridge<P> {
    fn event(&mut self, event: PointerEvent, cx: &MainCx<'_>) {
        let hit = *self.hit.lock().expect("overlay hit poisoned");
        let tasks = self
            .pointer
            .event(event, &hit)
            .into_iter()
            .map(|step| {
                let id = self.id;
                let input = self.input;
                let action = self.action;
                Box::new(move |cx: &MainCx<'_>| match step {
                    PointerStep::Model(value) => render::run_on_host(id, input(value)),
                    PointerStep::Op(WindowOp::SetInteractive(value)) => {
                        if let Some(window) = cx.view.window() {
                            window.setIgnoresMouseEvents(!value);
                        }
                    }
                    PointerStep::Op(WindowOp::BeginSystemDrag) => cx.view.begin_system_drag(),
                    PointerStep::Op(WindowOp::Action(value)) => action(value),
                    PointerStep::Op(WindowOp::TrackMouseLeave) => {}
                    PointerStep::Op(op) => {
                        // 指针副作用在此事件当场执行，模型输入才跨线程。
                        match op {
                            WindowOp::Hide => crate::commit::apply_immediate(
                                id,
                                sayit_overlay_core::present::MainOp::Hide,
                            ),
                            WindowOp::RaiseTopmost => crate::commit::apply_immediate(
                                id,
                                sayit_overlay_core::present::MainOp::RaiseTopmost,
                            ),
                            WindowOp::SetBounds(r) => crate::commit::apply_immediate(
                                id,
                                sayit_overlay_core::present::MainOp::SetBounds(r),
                            ),
                            WindowOp::MoveTo { x, y } => crate::commit::apply_immediate(
                                id,
                                sayit_overlay_core::present::MainOp::MoveTo { x, y },
                            ),
                            WindowOp::Redraw => {
                                render::run_on_host(id, Box::new(|host, cx| host.redraw(cx)))
                            }
                            _ => unreachable!("pointer op handled above"),
                        }
                    }
                }) as MainTask
            })
            .collect::<Vec<_>>();
        cx.tasks.borrow_mut().extend(tasks);
    }
    fn cursor(&self) -> CursorShape {
        let hit = *self.hit.lock().expect("overlay hit poisoned");
        self.pointer.cursor(&hit)
    }
    fn key(&self, input: KeyInput) {
        render::run_on_host(self.id, (self.key)(input));
    }
    fn wheel(&self, point: Point, lines: f32) {
        render::run_on_host(self.id, (self.wheel)(point, lines));
    }
    fn ime_rect(&self) -> Option<Rect> {
        let hit = *self.hit.lock().expect("overlay hit poisoned");
        self.pointer.ime_rect(&hit)
    }
}

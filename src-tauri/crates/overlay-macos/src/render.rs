//! 三窗共用一个渲染线程；模型、CtText、CGContext 从不离开这个线程。
use objc2::rc::autoreleasepool;
use sayit_overlay_core::{
    canvas::Canvas,
    geom::{snap_out_px, PxRect, Rect, Transform},
    look::Look,
    present::MainOp,
    window::{Cx, FrameClock, Ops, OverlayModel, PointerModel, WindowEvent, WindowOp},
};
use std::any::Any;
use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

use crate::{
    cg::CgCanvas,
    commit::{CommitSlot, Frame},
    ct::CtText,
    glass::{Image, MaskCache},
    screens,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowId {
    Indicator,
    Orb,
    Subtitle,
}
impl WindowId {
    pub fn index(self) -> usize {
        self as usize
    }
}

pub(crate) struct RenderCx {
    pub now: Instant,
    pub text: CtText,
    pub look: Look,
    pub screens: Arc<screens::Screens>,
}
impl RenderCx {
    fn model(&self, id: WindowId, dpi: u32, fallback: Option<u32>) -> Cx<'_, CtText> {
        let screen = (id == WindowId::Indicator)
            .then(|| self.screens.indicator_screen(fallback))
            .flatten();
        Cx {
            now: self.now,
            dpi: screen.map_or(dpi, |s| sayit_overlay_core::screen::dpi_for_scale(s.scale)),
            area: screen.map_or_else(
                || self.screens.area(),
                |s| sayit_overlay_core::screen::rect_pts_to_px(&[*s], s.visible, Some(s.id)),
            ),
            text: &self.text,
        }
    }
}

pub(crate) trait AnyHost {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn deadline(&self) -> Option<Instant>;
    fn frame(&mut self, cx: &RenderCx);
    fn redraw(&mut self, cx: &RenderCx);
    fn look(&mut self, cx: &RenderCx);
    fn window_event(&mut self, event: WindowEvent, cx: &RenderCx);
}

pub(crate) type HostJob = Box<dyn FnOnce(&mut dyn AnyHost, &RenderCx) + Send>;
type Factory = Box<dyn FnOnce(&RenderCx) -> Box<dyn AnyHost> + Send>;
enum Message {
    Host(WindowId, HostJob),
    Install(WindowId, Factory),
    Look(Look),
    AreaChanged,
}
struct Mailbox {
    tx: Sender<Message>,
    rx: Mutex<Option<Receiver<Message>>>,
}
impl Mailbox {
    fn update_look(&self, look: Look) {
        // 启动前由 LOOK 保存最新值；默认关闭的 Mac 窗口不能积压外观消息。
        let running = self.rx.lock().expect("overlay mailbox poisoned").is_none();
        if running {
            self.tx
                .send(Message::Look(look))
                .expect("overlay render thread stopped");
        }
    }
}
fn mailbox() -> &'static Mailbox {
    static MAILBOX: OnceLock<Mailbox> = OnceLock::new();
    MAILBOX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        Mailbox {
            tx,
            rx: Mutex::new(Some(rx)),
        }
    })
}

fn send(message: Message) {
    mailbox()
        .tx
        .send(message)
        .expect("overlay render thread stopped");
}
pub(crate) fn run_on_host(id: WindowId, f: HostJob) {
    send(Message::Host(id, f));
}
pub(crate) fn install_host(id: WindowId, factory: Factory) {
    send(Message::Install(id, factory));
}
pub(crate) fn set_look(look: Look) {
    mailbox().update_look(look);
}
pub(crate) fn area_changed() {
    send(Message::AreaChanged);
}
pub(crate) fn window_event(id: WindowId, event: WindowEvent) {
    run_on_host(id, Box::new(move |host, cx| host.window_event(event, cx)));
}

pub(crate) fn start() {
    // 同 set_look 的存储/发送串行，避免初始外观覆盖更晚的更新。
    let look = crate::LOOK.lock().expect("overlay look poisoned");
    let rx = mailbox()
        .rx
        .lock()
        .expect("overlay mailbox poisoned")
        .take()
        .expect("overlay thread starts once");
    let initial = *look;
    std::thread::Builder::new()
        .name("overlay-render".into())
        .spawn(move || render_loop(rx, initial))
        .expect("start overlay render thread");
    dispatch2::DispatchQueue::main().exec_async(|| {
        screens::observe(objc2::MainThreadMarker::new().expect("main queue"));
        area_changed();
    });
}

fn wait_duration(deadlines: impl Iterator<Item = Instant>, now: Instant) -> Option<Duration> {
    deadlines
        .min()
        .map(|deadline| deadline.saturating_duration_since(now))
}

fn render_loop(rx: Receiver<Message>, look: Look) {
    let mut hosts: [Option<Box<dyn AnyHost>>; 3] = std::array::from_fn(|_| None);
    let mut pending: [Vec<HostJob>; 3] = std::array::from_fn(|_| Vec::new());
    let mut cx = RenderCx {
        now: Instant::now(),
        text: CtText::default(),
        look,
        screens: screens::snapshot(),
    };
    crate::hosts::register();
    loop {
        let wait = wait_duration(
            hosts.iter().flatten().filter_map(|h| h.deadline()),
            Instant::now(),
        );
        let message = match wait {
            Some(wait) => match rx.recv_timeout(wait) {
                Ok(m) => Some(m),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            },
        };
        autoreleasepool(|_| {
            cx.now = Instant::now();
            cx.screens = screens::snapshot();
            match message {
                Some(Message::Host(id, f)) => match &mut hosts[id.index()] {
                    Some(host) => f(host.as_mut(), &cx),
                    None => pending[id.index()].push(f),
                },
                Some(Message::Install(id, factory)) => {
                    assert!(hosts[id.index()].is_none(), "overlay host registered twice");
                    let mut host = factory(&cx);
                    host.look(&cx);
                    for f in pending[id.index()].drain(..) {
                        f(host.as_mut(), &cx);
                    }
                    hosts[id.index()] = Some(host);
                }
                Some(Message::Look(look)) => {
                    cx.look = look;
                    for host in hosts.iter_mut().flatten() {
                        host.look(&cx);
                    }
                }
                Some(Message::AreaChanged) => {
                    for host in hosts.iter_mut().flatten() {
                        host.window_event(WindowEvent::AreaChanged, &cx);
                    }
                }
                None => {}
            }
            // 即使消息一直到达，已到期的帧仍须执行，不能等通道空闲。
            for host in hosts.iter_mut().flatten() {
                if host.deadline().is_some_and(|d| d <= cx.now) {
                    host.frame(&cx);
                }
            }
        });
    }
}

pub(crate) struct Host<M: OverlayModel<CtText>> {
    pub model: M,
    pub hit: Arc<Mutex<<M::Pointer as PointerModel>::Hit>>,
    pub slot: Arc<CommitSlot>,
    clock: FrameClock,
    dpi: u32,
    mask: MaskCache,
    action: fn(M::Action) -> crate::OverlayEvent,
}

impl<M: OverlayModel<CtText> + 'static> Host<M> {
    pub fn new(
        id: WindowId,
        model: M,
        dpi: u32,
        action: fn(M::Action) -> crate::OverlayEvent,
    ) -> Self {
        Self {
            hit: Arc::new(Mutex::new(model.hit())),
            model,
            slot: Arc::new(CommitSlot::new(id)),
            clock: FrameClock::new(),
            dpi: dpi.clamp(24, 768),
            mask: MaskCache::default(),
            action,
        }
    }

    pub fn apply(&mut self, command: M::Command, cx: &RenderCx) {
        let ops = self.model.apply(
            command,
            &cx.model(self.slot.id, self.dpi, self.slot.snapshot().screen),
        );
        self.execute(ops, cx);
    }
    /// AppKit 与 PointerModel 在主线程创建，模型与命中快照先在渲染线程创建。
    pub fn attach(&self) {
        crate::commit::attach(
            self.slot.clone(),
            crate::input::PointerBridge::<M::Pointer>::factory(self, self.action),
        );
    }
    pub fn input(&mut self, input: <M::Pointer as PointerModel>::Input, cx: &RenderCx) {
        let ops = self.model.input(
            input,
            &cx.model(self.slot.id, self.dpi, self.slot.snapshot().screen),
        );
        self.execute(ops, cx);
    }
    fn execute(&mut self, ops: Ops<M::Action>, cx: &RenderCx) {
        for op in ops {
            match op {
                WindowOp::Redraw => self.redraw(cx),
                WindowOp::Hide => self.slot.push(MainOp::Hide),
                WindowOp::RaiseTopmost => self.slot.push(MainOp::RaiseTopmost),
                WindowOp::SetBounds(r) => self.slot.push(MainOp::SetBounds(r)),
                WindowOp::MoveTo { x, y } => self.slot.push(MainOp::MoveTo { x, y }),
                WindowOp::SetInteractive(value) => self.slot.push(MainOp::SetInteractive(value)),
                WindowOp::Action(action) => crate::emit((self.action)(action)),
                WindowOp::TrackMouseLeave => {}
                WindowOp::BeginSystemDrag => {
                    panic!("system drag belongs to the main-thread pointer bridge")
                }
            }
        }
        *self.hit.lock().expect("overlay hit poisoned") = self.model.hit();
        self.clock.sync(self.model.wants_frames(cx.now));
    }
    fn redraw(&mut self, cx: &RenderCx) {
        let fallback = self.slot.snapshot().screen;
        let Some(mut spec) = self
            .model
            .prepare_frame(&cx.model(self.slot.id, self.dpi, fallback))
        else {
            return;
        };
        let screen = (self.slot.id == WindowId::Indicator)
            .then(|| cx.screens.indicator_screen(fallback).copied())
            .flatten();
        if self.slot.id == WindowId::Indicator {
            let Some(screen) = screen else { return };
            spec.bounds = screens::indicator_placement(&screen, spec.bounds);
        }
        let Some((crop, bounds)) =
            content_frame(spec.bounds, self.model.content_bounds(), spec.dpi)
        else {
            eprintln!("overlay: invalid frame geometry");
            return;
        };
        let scale = spec.dpi as f64 / 96.0;
        let Some(mut canvas) = CgCanvas::new(crop.width as u32, crop.height as u32, scale) else {
            eprintln!("overlay: bitmap allocation failed");
            return;
        };
        canvas.concat_transform(Transform {
            scale: 1.0,
            dx: -(crop.x as f32) / scale as f32,
            dy: -(crop.y as f32) / scale as f32,
        });
        self.model.draw(&mut canvas, cx.now);
        let mask = if cx.look.glass.is_some() {
            self.mask.get(self.model.glass(), crop, spec.dpi)
        } else {
            None
        };
        self.slot.push(MainOp::Present(Frame {
            image: Image(canvas.finish()),
            mask,
            bounds,
            layout: spec.bounds,
            offset: (crop.x, crop.y),
            dpi: spec.dpi,
            alpha: if spec.alpha.is_finite() {
                spec.alpha.clamp(0.0, 1.0)
            } else {
                0.0
            },
            look: cx.look,
            screen,
        }));
    }
}

fn content_frame(layout: PxRect, content: Option<Rect>, dpi: u32) -> Option<(PxRect, PxRect)> {
    if !(24..=768).contains(&dpi) || layout.width <= 0 || layout.height <= 0 {
        return None;
    }
    let crop = if let Some(r) = content {
        if ![r.left, r.top, r.right, r.bottom]
            .into_iter()
            .all(f32::is_finite)
            || r.width() <= 0.0
            || r.height() <= 0.0
            || [r.left, r.top, r.right, r.bottom]
                .into_iter()
                .any(|v| v.abs() > 1.0e6)
        {
            return None;
        }
        snap_out_px(r, dpi)
    } else {
        PxRect::new(0, 0, layout.width, layout.height)
    };
    if !crate::glass::valid_bitmap(crop.width, crop.height) {
        return None;
    }
    Some((
        crop,
        PxRect::new(
            layout.x.checked_add(crop.x)?,
            layout.y.checked_add(crop.y)?,
            crop.width,
            crop.height,
        ),
    ))
}

impl<M: OverlayModel<CtText> + 'static> AnyHost for Host<M> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn deadline(&self) -> Option<Instant> {
        self.clock.next_deadline()
    }
    fn frame(&mut self, cx: &RenderCx) {
        let dt = self.clock.delta(cx.now);
        let ops = self.model.on_frame(
            dt,
            &cx.model(self.slot.id, self.dpi, self.slot.snapshot().screen),
        );
        self.execute(ops, cx);
    }
    fn redraw(&mut self, cx: &RenderCx) {
        self.execute(vec![WindowOp::Redraw], cx);
    }
    fn look(&mut self, cx: &RenderCx) {
        let ops = self.model.set_look(&cx.look);
        self.execute(ops, cx);
    }
    fn window_event(&mut self, event: WindowEvent, cx: &RenderCx) {
        if let WindowEvent::DpiChanged { dpi, .. } = event {
            self.dpi = dpi.clamp(24, 768);
        }
        let ops = self.model.window_event(
            event,
            &cx.model(self.slot.id, self.dpi, self.slot.snapshot().screen),
        );
        self.execute(ops, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayit_overlay_core::{
        geom::{Color, Point},
        window::{FrameSpec, PointerEvent, PointerStep},
    };
    use std::{cell::RefCell, convert::Infallible, rc::Rc};

    #[derive(Default)]
    struct TestPointer;
    impl PointerModel for TestPointer {
        type Hit = u32;
        type Input = ();
        type Action = Infallible;
        fn event(&mut self, _: PointerEvent, _: &u32) -> Vec<PointerStep<Infallible, ()>> {
            Vec::new()
        }
    }
    struct TestModel {
        hit: u32,
        trace: Rc<RefCell<Vec<&'static str>>>,
    }
    impl OverlayModel<CtText> for TestModel {
        type Command = Ops<Infallible>;
        type Action = Infallible;
        type Pointer = TestPointer;
        fn apply(&mut self, ops: Self::Command, _: &Cx<'_, CtText>) -> Ops<Infallible> {
            self.hit += 1;
            self.trace.borrow_mut().push("apply");
            ops
        }
        fn input(&mut self, _: (), _: &Cx<'_, CtText>) -> Ops<Infallible> {
            Vec::new()
        }
        fn window_event(&mut self, _: WindowEvent, _: &Cx<'_, CtText>) -> Ops<Infallible> {
            Vec::new()
        }
        fn set_look(&mut self, _: &Look) -> Ops<Infallible> {
            Vec::new()
        }
        fn on_frame(&mut self, _: f32, _: &Cx<'_, CtText>) -> Ops<Infallible> {
            Vec::new()
        }
        fn wants_frames(&self, _: Instant) -> bool {
            self.hit > 0
        }
        fn prepare_frame(&mut self, _: &Cx<'_, CtText>) -> Option<FrameSpec> {
            self.trace.borrow_mut().push("prepare");
            Some(FrameSpec {
                bounds: PxRect::new(100, 200, 100, 100),
                dpi: 192,
                alpha: 0.5,
            })
        }
        fn draw<C: Canvas<Text = CtText>>(&self, canvas: &mut C, _: Instant) {
            self.trace.borrow_mut().push("draw");
            canvas.fill_circle(Point::new(10.0, 10.0), 4.0, Color::new(1.0, 0.0, 0.0, 1.0));
        }
        fn hit(&self) -> u32 {
            self.hit
        }
        fn content_bounds(&self) -> Option<Rect> {
            Some(Rect::new(5.0, 5.0, 15.0, 15.0))
        }
    }
    #[test]
    fn host_executes_core_model_ops_in_order_and_publishes_hit_without_a_main_thread() {
        let _guard = crate::cg::cg_test_lock();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut host = Host::new(
            WindowId::Orb,
            TestModel {
                hit: 0,
                trace: trace.clone(),
            },
            192,
            |never| match never {},
        );
        let cx = RenderCx {
            now: Instant::now(),
            text: CtText::default(),
            look: Look::MACOS_DEFAULT,
            screens: Arc::new(screens::Screens::default()),
        };
        host.apply(
            vec![
                WindowOp::Redraw,
                WindowOp::Hide,
                WindowOp::Redraw,
                WindowOp::SetInteractive(false),
                WindowOp::MoveTo { x: 3, y: 4 },
                WindowOp::RaiseTopmost,
            ],
            &cx,
        );
        assert_eq!(
            &*trace.borrow(),
            &["apply", "prepare", "draw", "prepare", "draw"]
        );
        assert_eq!(*host.hit.lock().unwrap(), 1);
        assert!(host.deadline().is_some());
        // 假 Presenter 只消费提交，测试线程从不创建 AppKit 窗口。
        let ops = host.slot.take_for_test();
        assert!(matches!(
            ops.as_slice(),
            [
                MainOp::Hide,
                MainOp::Present(_),
                MainOp::SetInteractive(false),
                MainOp::MoveTo { x: 3, y: 4 },
                MainOp::RaiseTopmost
            ]
        ));
        let MainOp::Present(frame) = &ops[1] else {
            unreachable!()
        };
        assert_eq!(frame.bounds, PxRect::new(110, 210, 20, 20));
        assert_eq!(frame.offset, (10, 10));
        assert_eq!(
            objc2_core_graphics::CGImage::width(Some(&frame.image.0)),
            20
        );
        // 最后一次隐藏不能被更早的 Present 越过。
        host.apply(vec![WindowOp::Redraw, WindowOp::Hide], &cx);
        assert!(matches!(
            host.slot.take_for_test().as_slice(),
            [MainOp::Present(_), MainOp::Hide]
        ));
    }
    #[test]
    fn scheduler_uses_earliest_deadline_without_delaying_overdue_frames() {
        let now = Instant::now();
        assert_eq!(wait_duration([].into_iter(), now), None);
        assert_eq!(
            wait_duration(
                [
                    now + Duration::from_millis(32),
                    now + Duration::from_millis(16)
                ]
                .into_iter(),
                now
            ),
            Some(Duration::from_millis(16))
        );
        assert_eq!(
            wait_duration([now - Duration::from_millis(1)].into_iter(), now),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn look_updates_do_not_accumulate_before_start() {
        let (tx, rx) = mpsc::channel();
        let mailbox = Mailbox {
            tx,
            rx: Mutex::new(Some(rx)),
        };
        mailbox.update_look(Look::MACOS_DEFAULT);
        mailbox.update_look(Look::MACOS_DEFAULT);
        let rx = mailbox.rx.lock().unwrap().take().unwrap();
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        mailbox.update_look(Look::MACOS_DEFAULT);
        assert!(matches!(
            rx.try_recv(),
            Ok(Message::Look(Look::MACOS_DEFAULT))
        ));
    }

    #[test]
    fn renderer_replays_early_commands_and_services_due_frames_before_the_next_message() {
        struct ScheduledHost {
            trace: Arc<Mutex<Vec<&'static str>>>,
            next: Option<Instant>,
        }
        impl AnyHost for ScheduledHost {
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
            fn deadline(&self) -> Option<Instant> {
                self.next
            }
            fn frame(&mut self, _: &RenderCx) {
                self.trace.lock().unwrap().push("frame");
                self.next = None;
            }
            fn redraw(&mut self, _: &RenderCx) {}
            fn look(&mut self, _: &RenderCx) {
                self.trace.lock().unwrap().push("look");
            }
            fn window_event(&mut self, _: WindowEvent, _: &RenderCx) {}
        }
        let (tx, rx) = mpsc::channel();
        let trace = Arc::new(Mutex::new(Vec::new()));
        for label in ["first", "second"] {
            tx.send(Message::Host(
                WindowId::Orb,
                Box::new(move |host, _| {
                    let host = host.as_any_mut().downcast_mut::<ScheduledHost>().unwrap();
                    host.trace.lock().unwrap().push(label);
                }),
            ))
            .ok()
            .unwrap();
        }
        let host_trace = trace.clone();
        tx.send(Message::Install(
            WindowId::Orb,
            Box::new(move |cx| {
                host_trace.lock().unwrap().push("install");
                Box::new(ScheduledHost {
                    trace: host_trace,
                    next: Some(cx.now - Duration::from_millis(1)),
                })
            }),
        ))
        .ok()
        .unwrap();
        tx.send(Message::Look(Look::MACOS_DEFAULT)).ok().unwrap();
        drop(tx);
        std::thread::spawn(move || render_loop(rx, Look::MACOS_DEFAULT))
            .join()
            .unwrap();
        assert_eq!(
            &*trace.lock().unwrap(),
            &["install", "look", "first", "second", "frame", "look"]
        );
    }
    #[test]
    fn content_fit_preserves_layout_origin_and_pixel_alignment() {
        let (crop, bounds) = content_frame(
            PxRect::new(100, 200, 400, 600),
            Some(Rect::new(1.25, 10.25, 80.5, 60.75)),
            192,
        )
        .unwrap();
        assert_eq!(crop, PxRect::new(2, 20, 159, 102));
        assert_eq!(bounds, PxRect::new(102, 220, 159, 102));
        assert!(content_frame(
            PxRect::new(i32::MAX, 0, 400, 600),
            Some(Rect::new(10.0, 0.0, 30.0, 20.0)),
            192
        )
        .is_none());
        assert!(content_frame(
            PxRect::new(0, 0, 400, 600),
            Some(Rect::new(f32::NAN, 0.0, 1.0, 1.0)),
            192
        )
        .is_none());
    }
}

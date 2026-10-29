//! AppKit 面板与输入视图；所有实例都只存在于主线程。
use crate::{
    commit::{self, CommitSlot},
    input::{MainCx, ViewInput},
    render, screens,
};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::ProtocolObject, AnyThread, DefinedClass,
    MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSEventModifierFlags, NSFloatingWindowLevel, NSPanel,
    NSResponder, NSTrackingArea, NSTrackingAreaOptions, NSView, NSViewLayerContentsRedrawPolicy,
    NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObjectProtocol};
use sayit_overlay_core::{
    geom::PxRect,
    screen::{self, QPoint, QRect},
    window::{PointerEvent, WindowEvent},
};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::Duration;

const COLLECTION: NSWindowCollectionBehavior = NSWindowCollectionBehavior(
    NSWindowCollectionBehavior::CanJoinAllSpaces.0
        | NSWindowCollectionBehavior::FullScreenAuxiliary.0
        | NSWindowCollectionBehavior::IgnoresCycle.0,
);

define_class!(
    // SAFETY: NSPanel 无额外子类化要求，实例只在主线程创建和访问。
    #[unsafe(super(NSPanel, NSWindow, NSResponder, objc2::runtime::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SayItOverlayPanel"]
    pub(crate) struct OverlayPanel;
    impl OverlayPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { false }
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool { false }
    }
);

pub(crate) struct ViewIvars {
    input: RefCell<Option<Box<dyn ViewInput>>>,
    down: RefCell<Option<Retained<NSEvent>>>,
    control_down: Cell<bool>,
    dragging: Cell<bool>,
    slot: Arc<CommitSlot>,
}

define_class!(
    // SAFETY: NSView 与 NSWindowDelegate 的覆盖签名与 AppKit 契约一致；无跨线程实例访问。
    #[unsafe(super(NSView, NSResponder, objc2::runtime::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SayItOverlayView"]
    #[ivars = ViewIvars]
    pub(crate) struct OverlayView;
    unsafe impl NSObjectProtocol for OverlayView {}
    unsafe impl NSWindowDelegate for OverlayView {
        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            if self.ivars().dragging.get() {
                if let Some(bounds) = self.publish_geometry() {
                    render::window_event(self.ivars().slot.id, WindowEvent::Moved { x: bounds.x, y: bounds.y });
                }
            }
        }
    }
    impl OverlayView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool { true }
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> bool { false }
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool { true }
        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {} // 位图归提交队列所有，AppKit 的显示循环不能重画它。
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let control = event.modifierFlags().contains(NSEventModifierFlags::Control);
            self.ivars().control_down.set(control);
            if !control {
                *self.ivars().down.borrow_mut() = Some(event.retain());
                if let Some((client, screen)) = self.coordinates(event) {
                    self.dispatch(PointerEvent::LeftDown { client, screen });
                }
            }
        }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if let Some((client, _)) = self.coordinates(event) {
                let control = self.ivars().control_down.replace(false)
                    || event.modifierFlags().contains(NSEventModifierFlags::Control);
                if control {
                    self.dispatch(PointerEvent::CaptureLost);
                    self.dispatch(PointerEvent::RightUp { client });
                } else { self.dispatch(PointerEvent::LeftUp { client }); }
            }
            self.ivars().down.borrow_mut().take();
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if !self.ivars().control_down.get() { self.moved(event); }
        }
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) { self.moved(event); }
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &NSEvent) { self.moved(event); }
        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) { self.dispatch(PointerEvent::Leave); }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {}
        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            if let Some((client, _)) = self.coordinates(event) { self.dispatch(PointerEvent::RightUp { client }); }
        }
        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, _event: &NSEvent) {}
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, _event: &NSEvent) {}
        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, _event: &NSEvent) {}
        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, _event: &NSEvent) {}
        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            self.update_tracking();
        }
        #[unsafe(method(viewDidChangeBackingProperties))]
        fn backing_changed(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeBackingProperties] };
            if let Some(window) = self.window() {
                let scale = window.backingScaleFactor();
                if !scale.is_finite() || !(0.25..=8.0).contains(&scale) { return; }
                if let Some(bounds) = self.publish_geometry() {
                    render::window_event(self.ivars().slot.id, WindowEvent::DpiChanged { dpi: screen::dpi_for_scale(scale), suggested: bounds });
                }
            }
        }
    }
);

impl OverlayView {
    fn new(
        mtm: MainThreadMarker,
        slot: Arc<CommitSlot>,
        input: Option<Box<dyn ViewInput>>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            input: RefCell::new(input),
            down: RefCell::new(None),
            control_down: Cell::new(false),
            dragging: Cell::new(false),
            slot,
        });
        let frame = screens::ns_rect(QRect {
            width: 1.0,
            height: 1.0,
            ..QRect::default()
        })
        .unwrap();
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
    pub fn dispatch(&self, event: PointerEvent) {
        let cx = MainCx::new(self);
        if let Some(input) = self.ivars().input.borrow_mut().as_mut() {
            input.event(event, &cx);
        }
        cx.run();
    }
    fn coordinates(&self, event: &NSEvent) -> Option<((i32, i32), (i32, i32))> {
        let window = self.window()?;
        let scale = window.backingScaleFactor();
        let point = event.locationInWindow();
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        let local = self.convertPoint_fromView(point, None);
        let global = window.convertPointToScreen(point);
        if ![local.x, local.y, global.x, global.y, scale]
            .into_iter()
            .all(f64::is_finite)
            || !(0.25..=8.0).contains(&scale)
        {
            return None;
        }
        let snapshot = screens::snapshot();
        let hint = window.screen().map(|s| screens::screen_id(&s));
        let offset = self.ivars().slot.snapshot().offset;
        // 防止不可表示的坐标在 core 的加偏移处发生整数溢出。
        let local = QPoint {
            x: local.x.clamp(-1.0e6, 1.0e6),
            y: local.y.clamp(-1.0e6, 1.0e6),
        };
        Some((
            screen::client_px(local, scale, offset),
            screen::pts_to_px(
                &snapshot.list,
                QPoint {
                    x: global.x,
                    y: snapshot.primary_height - global.y,
                },
                hint,
            ),
        ))
    }
    fn moved(&self, event: &NSEvent) {
        if let Some((client, screen)) = self.coordinates(event) {
            self.dispatch(PointerEvent::Move { client, screen });
        }
    }
    pub fn update_tracking(&self) {
        for area in self.trackingAreas().iter() {
            self.removeTrackingArea(&area);
        }
        let Some(rect) = screens::ns_rect(screens::qrect(self.bounds())) else {
            return;
        };
        if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
            return;
        }
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::EnabledDuringMouseDrag;
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                rect,
                options,
                Some(self),
                None,
            )
        };
        self.addTrackingArea(&area);
    }
    pub fn begin_system_drag(&self) {
        if self.ivars().dragging.get() {
            return;
        }
        let down = self.ivars().down.borrow_mut().take();
        if let (Some(window), Some(down)) = (self.window(), down) {
            self.ivars().dragging.set(true);
            window.performWindowDragWithEvent(&down);
            self.dispatch(PointerEvent::CaptureLost);
            self.poll_drag();
        }
    }
    fn poll_drag(&self) {
        if !self.ivars().dragging.get() {
            return;
        }
        if NSEvent::pressedMouseButtons() == 0 {
            self.ivars().dragging.set(false);
            if let Some(bounds) = self.publish_geometry() {
                render::window_event(
                    self.ivars().slot.id,
                    WindowEvent::SystemDragEnded { bounds },
                );
            }
        } else {
            let id = self.ivars().slot.id;
            DispatchQueue::main()
                .after(
                    DispatchTime::try_from(Duration::from_millis(50)).expect("valid drag interval"),
                    move || {
                        commit::with_view(id, |view| view.poll_drag());
                    },
                )
                .expect("schedule overlay drag completion");
        }
    }
    fn publish_geometry(&self) -> Option<PxRect> {
        let window = self.window()?;
        let snapshot = screens::snapshot();
        let quartz =
            screen::cocoa_to_quartz(snapshot.primary_height, screens::qrect(window.frame()));
        screens::ns_rect(quartz)?;
        let scale = window.backingScaleFactor();
        if !scale.is_finite() || !(0.25..=8.0).contains(&scale) {
            return None;
        }
        let hint = window.screen().map(|s| screens::screen_id(&s));
        let frame = screen::rect_pts_to_px(&snapshot.list, quartz, hint);
        let mut mirror = self
            .ivars()
            .slot
            .mirror
            .lock()
            .expect("overlay mirror poisoned");
        if mirror.bounds.width <= 0 || mirror.bounds.height <= 0 {
            return None;
        }
        let dpi = screen::dpi_for_scale(scale);
        let ratio = dpi as f64 / mirror.dpi.max(24) as f64;
        let scaled = |value: i32| (value as f64 * ratio).round() as i32;
        let offset = (scaled(mirror.offset.0), scaled(mirror.offset.1));
        let bounds = PxRect::new(
            frame.x.checked_sub(offset.0)?,
            frame.y.checked_sub(offset.1)?,
            scaled(mirror.bounds.width),
            scaled(mirror.bounds.height),
        );
        mirror.bounds = bounds;
        mirror.offset = offset;
        mirror.content_size = mirror.content_size.map(|(w, h)| (scaled(w), scaled(h)));
        mirror.quartz = quartz;
        mirror.screen = hint;
        mirror.dpi = dpi;
        Some(bounds)
    }
}

pub(crate) fn create(
    mtm: MainThreadMarker,
    slot: Arc<CommitSlot>,
    input: Option<Box<dyn ViewInput>>,
) -> (
    Retained<OverlayPanel>,
    Retained<NSView>,
    Retained<OverlayView>,
) {
    let frame = screens::ns_rect(QRect {
        width: 1.0,
        height: 1.0,
        ..QRect::default()
    })
    .unwrap();
    let panel: Retained<OverlayPanel> = unsafe {
        msg_send![OverlayPanel::alloc(mtm),
        initWithContentRect: frame, styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        backing: NSBackingStoreType::Buffered, defer: false]
    };
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setHidesOnDeactivate(false);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(false);
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
    panel.setCanHide(false);
    panel.setExcludedFromWindowsMenu(true);
    panel.setMovableByWindowBackground(false);
    panel.setIgnoresMouseEvents(true);
    panel.setAcceptsMouseMovedEvents(true);
    panel.setLevel(NSFloatingWindowLevel);
    panel.setCollectionBehavior(COLLECTION);
    let root = NSView::initWithFrame(NSView::alloc(mtm), frame);
    let view = OverlayView::new(mtm, slot, input);
    view.setWantsLayer(true);
    view.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
    root.addSubview(&view);
    panel.setContentView(Some(&root));
    panel.setDelegate(Some(ProtocolObject::from_ref(&*view)));
    view.update_tracking();
    (panel, root, view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::ClassType;
    #[test]
    fn overlay_classes_register_with_valid_appkit_signatures() {
        let _ = OverlayPanel::class();
        let _ = OverlayView::class();
    }
}

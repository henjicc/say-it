//! AppKit 面板与输入视图；所有实例都只存在于主线程。
use crate::{
    commit::{self, CommitSlot},
    input::{MainCx, ViewInput},
    render, screens,
};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSEventModifierFlags, NSFloatingWindowLevel, NSPanel,
    NSResponder, NSTextInputClient, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSViewLayerContentsRedrawPolicy, NSWindow, NSWindowAnimationBehavior,
    NSWindowCollectionBehavior, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound, NSNotification,
    NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSSize, NSString, NSUInteger,
};
use sayit_overlay_core::{
    geom::{Point, PxRect},
    screen::{self, QPoint, QRect},
    window::{Key, KeyInput, Modifiers, PointerEvent, WindowEvent},
};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::Duration;

const COLLECTION: NSWindowCollectionBehavior = NSWindowCollectionBehavior(
    NSWindowCollectionBehavior::CanJoinAllSpaces.0
        | NSWindowCollectionBehavior::FullScreenAuxiliary.0
        | NSWindowCollectionBehavior::IgnoresCycle.0,
);

pub(crate) struct PanelIvars {
    /// 对应 `OverlaySpec::FOCUSABLE`；不可取焦点的面板永远不成为 key window。
    focusable: bool,
}

define_class!(
    // SAFETY: NSPanel 无额外子类化要求，实例只在主线程创建和访问。
    #[unsafe(super(NSPanel, NSWindow, NSResponder, objc2::runtime::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SayItOverlayPanel"]
    #[ivars = PanelIvars]
    pub(crate) struct OverlayPanel;
    impl OverlayPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool { self.ivars().focusable }
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool { false }
    }
);

pub(crate) struct ViewIvars {
    input: RefCell<Option<Box<dyn ViewInput>>>,
    down: RefCell<Option<Retained<NSEvent>>>,
    control_down: Cell<bool>,
    dragging: Cell<bool>,
    /// 输入法组字中（setMarkedText 之后、上屏或取消之前）。
    marked: Cell<bool>,
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
        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            if self.ivars().slot.focusable {
                render::window_event(self.ivars().slot.id, WindowEvent::Focus(true));
            }
        }
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            if self.ivars().slot.focusable {
                render::window_event(self.ivars().slot.id, WindowEvent::Focus(false));
            }
        }
        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            if self.ivars().dragging.get() {
                if let Some(bounds) = self.publish_geometry() {
                    render::window_event(self.ivars().slot.id, WindowEvent::Moved { x: bounds.x, y: bounds.y });
                }
            }
        }
    }
    // SAFETY: 方法签名与 NSTextInputClient 协议一致；只在主线程由 AppKit 调用。
    unsafe impl NSTextInputClient for OverlayView {
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _replacement: NSRange) {
            self.ivars().marked.set(false);
            self.key(KeyInput::Text(plain_text(string)));
        }
        #[unsafe(method(doCommandBySelector:))]
        fn do_command(&self, _selector: Sel) {
            // 功能键已在 keyDown: 里直接处理；这里吞掉其余命令，避免系统提示音。
        }
        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(&self, string: &AnyObject, selected: NSRange, _replacement: NSRange) {
            let text = plain_text(string);
            self.ivars().marked.set(!text.is_empty());
            let caret = (selected.location as u64).min(u32::MAX as u64) as u32;
            self.key(KeyInput::Compose { text, caret });
        }
        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            self.ivars().marked.set(false);
            self.key(KeyInput::Compose { text: String::new(), caret: 0 });
        }
        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            NSRange { location: 0, length: 0 }
        }
        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            if self.ivars().marked.get() {
                NSRange { location: 0, length: 0 }
            } else {
                NSRange { location: NSNotFound as NSUInteger, length: 0 }
            }
        }
        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            self.ivars().marked.get()
        }
        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(&self, _range: NSRange, _actual: NSRangePointer) -> Option<Retained<NSAttributedString>> {
            None
        }
        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect(&self, _range: NSRange, _actual: NSRangePointer) -> NSRect {
            self.ime_screen_rect().unwrap_or(NSRect::ZERO)
        }
        #[unsafe(method(characterIndexForPoint:))]
        fn character_index(&self, _point: NSPoint) -> NSUInteger {
            NSNotFound as NSUInteger
        }
    }
    impl OverlayView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { self.ivars().slot.focusable }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !self.ivars().slot.focusable {
                return;
            }
            let flags = event.modifierFlags();
            let modifiers = Modifiers {
                command: flags.contains(NSEventModifierFlags::Command),
                shift: flags.contains(NSEventModifierFlags::Shift),
                alt: flags.contains(NSEventModifierFlags::Option),
            };
            // 组字期间所有键都交给输入法；否则功能键和 Command 组合键直接交给模型。
            if !self.ivars().marked.get() {
                if let Some(key) = function_key(event.keyCode()) {
                    self.key(KeyInput::Down { key, modifiers });
                    return;
                }
                if modifiers.command {
                    let letter = event
                        .charactersIgnoringModifiers()
                        .and_then(|chars| chars.to_string().chars().next())
                        .filter(char::is_ascii_alphabetic)
                        .map(|c| c.to_ascii_uppercase());
                    if let Some(letter) = letter {
                        self.key(KeyInput::Down { key: Key::Letter(letter), modifiers });
                    }
                    return;
                }
            }
            self.interpretKeyEvents(&NSArray::from_slice(&[event]));
        }
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let Some((client, _)) = self.coordinates(event) else { return };
            let Some(window) = self.window() else { return };
            let scale = window.backingScaleFactor() as f32;
            let delta = event.scrollingDeltaY() as f32;
            // 触控板给的是点，换算成「行」：一行约 20 点。
            let lines = if event.hasPreciseScrollingDeltas() { delta / 20.0 } else { delta };
            if lines != 0.0 && lines.is_finite() {
                let point = Point::new(client.0 as f32 / scale, client.1 as f32 / scale);
                if let Some(input) = self.ivars().input.borrow().as_ref() {
                    input.wheel(point, lines);
                }
            }
        }
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
            marked: Cell::new(false),
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
    fn key(&self, input: KeyInput) {
        if let Some(view_input) = self.ivars().input.borrow().as_ref() {
            view_input.key(input);
        }
    }
    /// 输入框光标在屏幕上的矩形（Cocoa 屏幕坐标），给输入法候选窗定位。
    fn ime_screen_rect(&self) -> Option<NSRect> {
        let caret = self.ivars().input.borrow().as_ref()?.ime_rect()?;
        let window = self.window()?;
        let scale = window.backingScaleFactor();
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        // 布局坐标（DIP）就是视图的点；视图按内容裁剪时减去裁掉的物理像素偏移。
        let offset = self.ivars().slot.snapshot().offset;
        let local = NSRect::new(
            NSPoint::new(
                caret.left as f64 - offset.0 as f64 / scale,
                caret.top as f64 - offset.1 as f64 / scale,
            ),
            NSSize::new(
                (caret.right - caret.left).max(1.0) as f64,
                (caret.bottom - caret.top) as f64,
            ),
        );
        let in_window = self.convertRect_toView(local, None);
        Some(window.convertRectToScreen(in_window))
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
    /// 系统拖拽进行中：窗口位置由系统跟着指针走。
    pub fn is_dragging(&self) -> bool {
        self.ivars().dragging.get()
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
    let focusable = slot.focusable;
    let panel = OverlayPanel::alloc(mtm).set_ivars(PanelIvars { focusable });
    let panel: Retained<OverlayPanel> = unsafe {
        msg_send![super(panel),
        initWithContentRect: frame, styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        backing: NSBackingStoreType::Buffered, defer: false]
    };
    panel.setFloatingPanel(true);
    // 可取焦点的面板（回答窗）点一下就成为 key window，不激活应用本身。
    panel.setBecomesKeyOnlyIfNeeded(!focusable);
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
    if focusable {
        panel.makeFirstResponder(Some(&view));
    }
    view.update_tracking();
    (panel, root, view)
}

/// 输入法传来的是 NSString 或 NSAttributedString。
fn plain_text(string: &AnyObject) -> String {
    if let Some(attributed) = string.downcast_ref::<NSAttributedString>() {
        attributed.string().to_string()
    } else if let Some(text) = string.downcast_ref::<NSString>() {
        text.to_string()
    } else {
        String::new()
    }
}

/// macOS 虚拟键码（kVK_*）里模型关心的功能键。
fn function_key(code: u16) -> Option<Key> {
    Some(match code {
        36 | 76 => Key::Enter,
        48 => Key::Tab,
        51 => Key::Backspace,
        117 => Key::Delete,
        53 => Key::Escape,
        123 => Key::Left,
        124 => Key::Right,
        125 => Key::Down,
        126 => Key::Up,
        115 => Key::Home,
        119 => Key::End,
        116 => Key::PageUp,
        121 => Key::PageDown,
        _ => return None,
    })
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

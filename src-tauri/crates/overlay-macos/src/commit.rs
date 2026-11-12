//! 每窗一个异步提交槽；从不持锁调用 AppKit，防止 setFrame 的同步通知重入。
use crate::{
    glass::{GlassView, Image},
    input::ViewInput,
    panel::{OverlayPanel, OverlayView},
    render::WindowId,
    screens,
};
use dispatch2::DispatchQueue;
use objc2::{rc::Retained, runtime::AnyObject, MainThreadMarker};
use objc2_app_kit::NSView;
use objc2_core_foundation::CFRetained;
use objc2_quartz_core::CATransaction;
use sayit_overlay_core::{
    geom::PxRect,
    look::Look,
    present::{CommitQueue, MainOp},
    spec::Placement,
    screen::{self, QRect, ScreenInfo},
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub(crate) struct Frame {
    pub image: Image,
    pub mask: Option<Image>,
    pub bounds: PxRect,
    pub layout: PxRect,
    pub offset: (i32, i32),
    pub dpi: u32,
    pub alpha: f32,
    pub look: Look,
    pub screen: Option<ScreenInfo>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Mirror {
    pub bounds: PxRect,
    pub quartz: QRect,
    pub offset: (i32, i32),
    pub dpi: u32,
    pub screen: Option<u32>,
    pub visible: bool,
    pub content_size: Option<(i32, i32)>,
}

struct Pending {
    queue: CommitQueue<Frame>,
    scheduled: bool,
    ready: bool,
}
pub(crate) struct CommitSlot {
    pub id: WindowId,
    pub placement: Placement,
    pending: Mutex<Pending>,
    pub mirror: Mutex<Mirror>,
}
impl CommitSlot {
    pub fn new(id: WindowId, placement: Placement) -> Self {
        Self {
            id,
            placement,
            pending: Mutex::new(Pending {
                queue: CommitQueue::new(),
                scheduled: false,
                ready: false,
            }),
            mirror: Mutex::new(Mirror::default()),
        }
    }
    pub fn push(self: &Arc<Self>, op: MainOp<Frame>) {
        let schedule = {
            let mut pending = self.pending.lock().expect("overlay commit poisoned");
            pending.queue.push(op);
            Self::schedule(&mut pending)
        };
        if schedule {
            self.dispatch();
        }
    }
    fn schedule(pending: &mut Pending) -> bool {
        if !pending.ready || pending.scheduled || pending.queue.is_empty() {
            return false;
        }
        pending.scheduled = true;
        true
    }
    fn dispatch(self: &Arc<Self>) {
        let slot = self.clone();
        DispatchQueue::main().exec_async(move || {
            let ops = {
                let mut pending = slot.pending.lock().expect("overlay commit poisoned");
                pending.scheduled = false;
                pending.queue.take()
            };
            let host = HOSTS.with(|hosts| hosts.borrow().get(&slot.id).cloned());
            if let Some(host) = host {
                // 取出主线程状态，AppKit 通知重入时不持 TLS RefCell 借用。
                for op in ops {
                    host.apply(op);
                }
            }
        });
    }
    fn ready(self: &Arc<Self>) {
        let schedule = {
            let mut pending = self.pending.lock().expect("overlay commit poisoned");
            pending.ready = true;
            Self::schedule(&mut pending)
        };
        if schedule {
            self.dispatch();
        }
    }
    pub fn snapshot(&self) -> Mirror {
        *self.mirror.lock().expect("overlay mirror poisoned")
    }
    #[cfg(test)]
    pub fn take_for_test(&self) -> Vec<MainOp<Frame>> {
        self.pending
            .lock()
            .expect("overlay commit poisoned")
            .queue
            .take()
    }
}

// Rc 和 Retained<AppKit> 只保存在主线程 TLS 内，跨线程仅传 CommitSlot。
thread_local! {
    static HOSTS: RefCell<BTreeMap<WindowId, std::rc::Rc<MainHost>>> = const { RefCell::new(BTreeMap::new()) };
}

struct MainHost {
    panel: Retained<OverlayPanel>,
    root: Retained<NSView>,
    view: Retained<OverlayView>,
    slot: Arc<CommitSlot>,
    glass: RefCell<GlassView>,
    /// 已经为哪个实际缩放请求过改正；帧与实际一致后清零。模型不采用 `cx.dpi` 时不会反复请求。
    corrected_dpi: std::cell::Cell<u32>,
}

pub(crate) fn attach(
    slot: Arc<CommitSlot>,
    input: impl FnOnce() -> Option<Box<dyn ViewInput>> + Send + 'static,
) {
    DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().expect("main queue");
        let (panel, root, view) = crate::panel::create(mtm, slot.clone(), input());
        let host = std::rc::Rc::new(MainHost {
            panel,
            root,
            view,
            slot: slot.clone(),
            glass: RefCell::new(GlassView::default()),
            corrected_dpi: std::cell::Cell::new(0),
        });
        HOSTS.with(|hosts| {
            let previous = hosts.borrow_mut().insert(slot.id, host);
            assert!(previous.is_none(), "overlay panel registered twice");
        });
        slot.ready();
    });
}

pub(crate) fn with_view(id: WindowId, f: impl FnOnce(&OverlayView)) {
    let view = HOSTS.with(|hosts| hosts.borrow().get(&id).map(|h| h.view.clone()));
    if let Some(view) = view {
        f(&view);
    }
}

/// 系统辅助显示选项变化也要唤醒静止宿主；隐藏窗口留到下一次 Present 读取选项。
pub(crate) fn refresh_accessibility() {
    let shown = HOSTS.with(|hosts| {
        hosts
            .borrow()
            .values()
            .filter(|host| host.slot.snapshot().visible)
            .map(|host| host.slot.id)
            .collect::<Vec<_>>()
    });
    for id in shown {
        crate::render::run_on_host(id, Box::new(|host, cx| host.look(cx)));
    }
}

pub(crate) fn apply_immediate(id: WindowId, op: MainOp<Frame>) {
    assert!(
        MainThreadMarker::new().is_some(),
        "pointer op must run on main thread"
    );
    let host = HOSTS.with(|hosts| hosts.borrow().get(&id).cloned());
    if let Some(host) = host {
        host.apply(op);
    }
}

impl MainHost {
    fn set_bounds(&self, bounds: PxRect, screen_hint: Option<u32>) {
        if bounds.width <= 0 || bounds.height <= 0 {
            return;
        }
        let snapshot = screens::snapshot();
        let mirror = self.slot.snapshot();
        let hint = screen_hint.or_else(|| {
            self.panel
                .screen()
                .map(|s| screens::screen_id(&s))
                .or(mirror.screen)
        });
        let (quartz, id) = screen::rect_px_to_pts(&snapshot.list, bounds, hint);
        let Some(mut frame) =
            screens::ns_rect(screen::quartz_to_cocoa(snapshot.primary_height, quartz))
        else {
            return;
        };
        // 拖拽中位置归系统：渲染线程的帧和定位命令带的都是稍早的位置，照它摆会把窗口
        // 从指针下拽开。只跟随尺寸，左上角保持在系统拖到的地方。
        if self.view.is_dragging() {
            let current = self.panel.frame();
            frame.origin.x = current.origin.x;
            frame.origin.y = current.origin.y + current.size.height - frame.size.height;
        }
        if self.panel.frame() != frame {
            self.panel.setFrame_display(frame, false);
        }
        let local = screens::ns_rect(QRect {
            width: frame.size.width,
            height: frame.size.height,
            ..QRect::default()
        })
        .expect("validated panel size");
        if self.root.frame() != local {
            self.root.setFrame(local);
        }
        if self.view.frame() != local {
            self.view.setFrame(local);
            self.view.update_tracking();
        }
        let mut mirror = self.slot.mirror.lock().expect("overlay mirror poisoned");
        mirror.quartz = screen::cocoa_to_quartz(snapshot.primary_height, screens::qrect(frame));
        mirror.screen = Some(id);
    }
    fn apply(&self, op: MainOp<Frame>) {
        let mtm = MainThreadMarker::new().expect("main queue");
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        match op {
            MainOp::Present(frame) => {
                if !frame.alpha.is_finite() || !(24..=768).contains(&frame.dpi) {
                    CATransaction::commit();
                    return;
                }
                if self.slot.placement == Placement::FollowFocus && !self.panel.isVisible() {
                    // 全局快捷键可能先于激活/屏幕通知；显示前只在主线程复核，不等待渲染线程。
                    screens::refresh(mtm);
                    let snapshot = screens::snapshot();
                    let fallback = self.panel.screen().map(|s| screens::screen_id(&s));
                    if snapshot.indicator_screen(fallback).copied() != frame.screen {
                        crate::render::run_on_host(
                            self.slot.id,
                            Box::new(|host, cx| host.redraw(cx)),
                        );
                        CATransaction::commit();
                        return;
                    }
                }
                {
                    let mut mirror = self.slot.mirror.lock().expect("overlay mirror poisoned");
                    mirror.bounds = frame.layout;
                    mirror.offset = frame.offset;
                    mirror.dpi = frame.dpi;
                    mirror.content_size = (frame.bounds.width != frame.layout.width
                        || frame.bounds.height != frame.layout.height
                        || frame.offset != (0, 0))
                        .then_some((frame.bounds.width, frame.bounds.height));
                }
                self.set_bounds(frame.bounds, frame.screen.map(|s| s.id));
                // 宿主建立时取的是焦点屏的缩放，窗口可能落在缩放不同的另一块屏上，
                // 而且不一定触发缩放变化通知：让宿主改用面板的实际缩放重画。
                let actual = screen::dpi_for_scale(self.panel.backingScaleFactor());
                if self.slot.placement == Placement::FollowFocus || actual == frame.dpi {
                    self.corrected_dpi.set(0);
                } else if self.corrected_dpi.replace(actual) != actual {
                    crate::render::run_on_host(
                        self.slot.id,
                        Box::new(move |host, cx| host.adopt_dpi(actual, cx)),
                    );
                }
                if let Some(layer) = self.view.layer() {
                    // SAFETY: CGImage 的 CALayer contents ABI 接受 CoreFoundation 对象，图像保留到 setContents 返回，layer 会 retain。
                    let image = unsafe {
                        CFRetained::as_ptr(&frame.image.0)
                            .cast::<AnyObject>()
                            .as_ref()
                    };
                    unsafe {
                        layer.setContents(Some(image));
                        layer.setContentsScale(frame.dpi as f64 / 96.0);
                    }
                }
                self.glass.borrow_mut().apply(
                    &self.panel,
                    &self.root,
                    &self.view,
                    frame.look,
                    frame.mask.as_ref(),
                    mtm,
                );
                self.panel
                    .setAlphaValue((frame.alpha as f64).clamp(0.0, 1.0));
                if !self.panel.isVisible() {
                    self.panel.orderFrontRegardless();
                }
                self.slot
                    .mirror
                    .lock()
                    .expect("overlay mirror poisoned")
                    .visible = true;
            }
            MainOp::Hide => {
                self.panel.orderOut(None);
                self.slot
                    .mirror
                    .lock()
                    .expect("overlay mirror poisoned")
                    .visible = false;
            }
            MainOp::RaiseTopmost => {
                if self.panel.isVisible() {
                    self.panel.orderFrontRegardless();
                }
            }
            MainOp::SetInteractive(value) => self.panel.setIgnoresMouseEvents(!value),
            MainOp::SetBounds(bounds) => {
                let mirror = self.slot.snapshot();
                if let (Some(x), Some(y)) = (
                    bounds.x.checked_add(mirror.offset.0),
                    bounds.y.checked_add(mirror.offset.1),
                ) {
                    let (width, height) =
                        mirror.content_size.unwrap_or((bounds.width, bounds.height));
                    self.set_bounds(PxRect::new(x, y, width, height), None);
                    self.slot
                        .mirror
                        .lock()
                        .expect("overlay mirror poisoned")
                        .bounds = bounds;
                }
            }
            MainOp::MoveTo { x, y } => {
                let mirror = self.slot.snapshot();
                if let (Some(x_frame), Some(y_frame)) = (
                    x.checked_add(mirror.offset.0),
                    y.checked_add(mirror.offset.1),
                ) {
                    let (width, height) = mirror
                        .content_size
                        .unwrap_or((mirror.bounds.width, mirror.bounds.height));
                    self.set_bounds(PxRect::new(x_frame, y_frame, width, height), None);
                    let mut mirror = self.slot.mirror.lock().expect("overlay mirror poisoned");
                    mirror.bounds.x = x;
                    mirror.bounds.y = y;
                }
            }
        }
        CATransaction::commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_waits_for_panel_and_schedules_once_per_batch() {
        let mut pending = Pending {
            queue: CommitQueue::new(),
            scheduled: false,
            ready: false,
        };
        pending.queue.push(MainOp::Hide);
        assert!(!CommitSlot::schedule(&mut pending));
        pending.ready = true;
        assert!(CommitSlot::schedule(&mut pending));
        pending.queue.push(MainOp::SetInteractive(false));
        assert!(!CommitSlot::schedule(&mut pending));
        let ops = pending.queue.take();
        pending.scheduled = false;
        assert!(matches!(
            ops.as_slice(),
            [MainOp::Hide, MainOp::SetInteractive(false)]
        ));
        assert!(!CommitSlot::schedule(&mut pending));
        pending.queue.push(MainOp::RaiseTopmost);
        assert!(CommitSlot::schedule(&mut pending));
    }
}

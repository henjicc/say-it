//! AppKit 屏幕信息只在主线程采集，其余线程读取不可变快照。
use std::cell::RefCell;
use std::sync::{Arc, OnceLock, RwLock};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::{msg_send, rc::Retained, runtime::ProtocolObject, MainThreadMarker};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSScreen, NSWorkspace,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{
    ns_string, NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol, NSRect,
};
use sayit_overlay_core::geom::PxRect;
use sayit_overlay_core::screen::{self, QRect, ScreenInfo};

#[derive(Clone, Default)]
pub(crate) struct Screens {
    pub list: Vec<ScreenInfo>,
    pub primary_height: f64,
    pub main: Option<u32>,
}

impl Screens {
    pub fn area(&self) -> PxRect {
        self.list
            .iter()
            .find(|s| Some(s.id) == self.main)
            .or_else(|| self.list.first())
            .map(|s| screen::rect_pts_to_px(&self.list, s.visible, Some(s.id)))
            .unwrap_or_default()
    }
}

fn store() -> &'static RwLock<Arc<Screens>> {
    static SCREENS: OnceLock<RwLock<Arc<Screens>>> = OnceLock::new();
    SCREENS.get_or_init(|| RwLock::new(Arc::new(Screens::default())))
}

pub(crate) fn snapshot() -> Arc<Screens> {
    store().read().expect("overlay screens poisoned").clone()
}

/// 进入 AppKit/CA 的几何边界；拒绝非有限值，尺寸与坐标限制到可表示范围。
pub(crate) fn ns_rect(r: QRect) -> Option<NSRect> {
    if ![r.x, r.y, r.width, r.height]
        .into_iter()
        .all(f64::is_finite)
    {
        return None;
    }
    Some(NSRect::new(
        objc2_foundation::NSPoint::new(r.x.clamp(-1.0e7, 1.0e7), r.y.clamp(-1.0e7, 1.0e7)),
        objc2_foundation::NSSize::new(r.width.clamp(0.0, 65536.0), r.height.clamp(0.0, 65536.0)),
    ))
}

pub(crate) fn qrect(r: NSRect) -> QRect {
    QRect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}

pub(crate) fn screen_id(screen: &NSScreen) -> u32 {
    // SAFETY: NSScreenNumber 是 AppKit 定义的 NSNumber 设备描述项。
    unsafe {
        let number: Option<Retained<NSNumber>> =
            msg_send![&*screen.deviceDescription(), objectForKey: ns_string!("NSScreenNumber")];
        number.map_or(0, |n| n.unsignedIntValue())
    }
}

pub(crate) fn refresh(mtm: MainThreadMarker) {
    let screens = NSScreen::screens(mtm);
    let primary_height = screens.firstObject().map_or(0.0, |s| s.frame().size.height);
    if !primary_height.is_finite() || primary_height < 0.0 {
        return;
    }
    let main = NSScreen::mainScreen(mtm).map(|s| screen_id(&s));
    let list = screens
        .iter()
        .filter_map(|s| {
            let scale = s.backingScaleFactor();
            let frame = screen::cocoa_to_quartz(primary_height, qrect(s.frame()));
            let visible = screen::cocoa_to_quartz(primary_height, qrect(s.visibleFrame()));
            if !scale.is_finite()
                || !(0.25..=8.0).contains(&scale)
                || ns_rect(frame).is_none()
                || ns_rect(visible).is_none()
                || frame.width <= 0.0
                || frame.height <= 0.0
            {
                return None;
            }
            Some(ScreenInfo {
                id: screen_id(&s),
                frame,
                visible,
                scale,
            })
        })
        .collect();
    *store().write().expect("overlay screens poisoned") = Arc::new(Screens {
        list,
        primary_height,
        main,
    });
}

thread_local! {
    // 观察者与主线程生命周期相同；保留 token，避免注册块提前释放。
    static OBSERVERS: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn observe(mtm: MainThreadMarker) {
    refresh(mtm);
    let callback = RcBlock::new(|_: std::ptr::NonNull<NSNotification>| {
        DispatchQueue::main().exec_async(|| {
            let mtm = MainThreadMarker::new().expect("main queue");
            refresh(mtm);
            crate::render::area_changed();
        });
    });
    // SAFETY: 通知块只异步投递主队列，不跨线程访问 AppKit，token 由主线程保留。
    unsafe {
        let app = NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidChangeScreenParametersNotification),
            None,
            None,
            &callback,
        );
        let workspace = NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceDidActivateApplicationNotification),
                None,
                None,
                &callback,
            );
        OBSERVERS.with(|tokens| tokens.borrow_mut().extend([app, workspace]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appkit_geometry_rejects_nan_and_clamps_sizes() {
        assert!(ns_rect(QRect {
            x: f64::NAN,
            ..QRect::default()
        })
        .is_none());
        assert!(ns_rect(QRect {
            height: f64::INFINITY,
            ..QRect::default()
        })
        .is_none());
        let r = ns_rect(QRect {
            x: 1.0e20,
            y: -1.0e20,
            width: -2.0,
            height: 1.0e20,
        })
        .unwrap();
        assert_eq!(r.origin.x, 1.0e7);
        assert_eq!(r.origin.y, -1.0e7);
        assert_eq!(r.size.width, 0.0);
        assert_eq!(r.size.height, 65536.0);
    }
}

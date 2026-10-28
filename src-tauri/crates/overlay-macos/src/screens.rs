//! AppKit 屏幕信息只在主线程采集，其余线程读取不可变快照。
use std::cell::RefCell;
use std::sync::{Arc, OnceLock, RwLock};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::{msg_send, rc::Retained, runtime::ProtocolObject, MainThreadMarker};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSScreen, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
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
    pub fn indicator_screen(&self, fallback: Option<u32>) -> Option<&ScreenInfo> {
        self.list
            .iter()
            .find(|s| Some(s.id) == self.main)
            .or_else(|| self.list.iter().find(|s| Some(s.id) == fallback))
            .or_else(|| self.list.first())
    }

    pub fn area(&self) -> PxRect {
        self.list
            .iter()
            .find(|s| Some(s.id) == self.main)
            .or_else(|| self.list.first())
            .map(|s| screen::rect_pts_to_px(&self.list, s.visible, Some(s.id)))
            .unwrap_or_default()
    }
}

/// 输入是模型相对 visibleFrame 排好的帧（含入退场位移），只修正底部 Dock 锚点。
pub(crate) fn indicator_placement(screen: &ScreenInfo, requested: PxRect) -> PxRect {
    let visible = screen::rect_pts_to_px(&[*screen], screen.visible, Some(screen.id));
    let full = screen::screen_px_rect(screen);
    let full_bottom = screen.frame.y + screen.frame.height;
    let visible_bottom = screen.visible.y + screen.visible.height;
    let bottom = if full_bottom - visible_bottom > 0.5 {
        visible.bottom()
    } else {
        full.bottom()
    };
    PxRect::new(
        full.x + (full.width - requested.width) / 2,
        requested.y + bottom - visible.bottom(),
        requested.width,
        requested.height,
    )
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
    let accessibility = RcBlock::new(|_: std::ptr::NonNull<NSNotification>| {
        DispatchQueue::main().exec_async(crate::commit::refresh_accessibility);
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
        let display_options = NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
                None,
                &accessibility,
            );
        OBSERVERS.with(|tokens| {
            tokens
                .borrow_mut()
                .extend([app, workspace, display_options])
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: u32, x: f64, y: f64, scale: f64, visible: QRect) -> ScreenInfo {
        ScreenInfo {
            id,
            frame: QRect {
                x,
                y,
                width: 1512.0,
                height: 982.0,
            },
            visible,
            scale,
        }
    }

    #[test]
    fn indicator_bottom_anchor_handles_bottom_left_right_and_hidden_dock() {
        for scale in [1.0, 2.0] {
            for (visible, expected_y) in [
                (
                    QRect {
                        x: 0.0,
                        y: 25.0,
                        width: 1512.0,
                        height: 890.0,
                    },
                    739,
                ),
                (
                    QRect {
                        x: 70.0,
                        y: 25.0,
                        width: 1442.0,
                        height: 957.0,
                    },
                    806,
                ),
                (
                    QRect {
                        x: 0.0,
                        y: 25.0,
                        width: 1442.0,
                        height: 957.0,
                    },
                    806,
                ),
                (
                    QRect {
                        x: 0.0,
                        y: 25.0,
                        width: 1512.0,
                        height: 957.0,
                    },
                    806,
                ),
            ] {
                let screen = screen(1, 0.0, 0.0, scale, visible);
                let area = screen::rect_pts_to_px(&[screen], visible, Some(1));
                let width = (460.0 * scale) as i32;
                let height = (188.0 * scale) as i32;
                let requested = PxRect::new(
                    area.x + (area.width - width) / 2,
                    area.bottom() - height + (12.0 * scale) as i32,
                    width,
                    height,
                );
                assert_eq!(
                    indicator_placement(&screen, requested),
                    PxRect::new(
                        (526.0 * scale) as i32,
                        (expected_y as f64 * scale) as i32,
                        width,
                        height
                    )
                );
                let sliding = PxRect {
                    y: requested.y + (10.0 * scale) as i32,
                    ..requested
                };
                assert_eq!(
                    indicator_placement(&screen, sliding).y,
                    ((expected_y + 10) as f64 * scale) as i32
                );
            }
        }
    }

    #[test]
    fn indicator_follows_main_screen_with_window_and_primary_fallbacks() {
        let primary = screen(
            1,
            0.0,
            0.0,
            2.0,
            QRect {
                x: 0.0,
                y: 25.0,
                width: 1512.0,
                height: 890.0,
            },
        );
        for (x, y) in [(1512.0, 0.0), (-1512.0, 0.0), (0.0, -982.0)] {
            let external = screen(
                2,
                x,
                y,
                1.0,
                QRect {
                    x,
                    y: y + 25.0,
                    width: 1512.0,
                    height: 957.0,
                },
            );
            let mut screens = Screens {
                list: vec![primary, external],
                main: Some(2),
                primary_height: 982.0,
            };
            assert_eq!(screens.indicator_screen(Some(1)), Some(&external));
            let requested = PxRect::new(x as i32 + 526, y as i32 + 806, 460, 188);
            assert_eq!(indicator_placement(&external, requested), requested);
            screens.main = None;
            assert_eq!(screens.indicator_screen(Some(2)), Some(&external));
            assert_eq!(screens.indicator_screen(Some(99)), Some(&primary));
            screens.list.clear();
            assert!(screens.indicator_screen(Some(2)).is_none());
        }
    }

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

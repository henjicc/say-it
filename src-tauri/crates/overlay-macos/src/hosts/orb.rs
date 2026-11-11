//! 悬浮球宿主入口。
use crate::render::{self, Host, WindowId};
use sayit_overlay_core::orb::{Command, Orb, OrbMirrors};
use sayit_overlay_core::screen::{self, QPoint};
use std::sync::Once;

/// 首次调用注册宿主并绑定请求镜像；之后的调用无副作用。
pub fn start(mirrors: &'static OrbMirrors) {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        render::install_host(
            WindowId::Orb,
            Box::new(move |cx| {
                let dpi = cx
                    .screens
                    .area_screen()
                    .map_or(96, |s| screen::dpi_for_scale(s.scale));
                let host = Host::new(
                    WindowId::Orb,
                    Orb::new(dpi, &cx.look).with_mirrors(mirrors),
                    dpi,
                    crate::OverlayEvent::Orb,
                );
                host.attach();
                Box::new(host)
            }),
        );
    });
}

/// 可在任意线程调用；须先 start。
pub fn post(command: Command) {
    render::run_on_host(
        WindowId::Orb,
        Box::new(move |host, cx| {
            host.as_any_mut()
                .downcast_mut::<Host<Orb>>()
                .expect("orb host type")
                .apply(command, cx);
        }),
    );
}

/// 指针的 DesktopPx 坐标；屏幕快照尚未采集时为 None。
pub fn cursor_px() -> Option<(i32, i32)> {
    let snapshot = crate::screens::snapshot();
    if snapshot.list.is_empty() {
        return None;
    }
    // Quartz 全局坐标（左上原点、点为单位）；CGEvent 读取可在任意线程进行。
    let event = objc2_core_graphics::CGEvent::new(None)?;
    let point = objc2_core_graphics::CGEvent::location(Some(&event));
    if !point.x.is_finite() || !point.y.is_finite() {
        return None;
    }
    Some(screen::pts_to_px(
        &snapshot.list,
        QPoint {
            x: point.x,
            y: point.y,
        },
        None,
    ))
}

//! macOS 悬浮球薄桥：首次配置时注册宿主，此后的命令只在宿主存在时投递。
pub(crate) use sayit_overlay_core::orb::Command;
use sayit_overlay_core::orb::OrbAction;
use std::sync::atomic::{AtomicBool, Ordering};

static STARTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn started() -> bool {
    STARTED.load(Ordering::Acquire)
}

pub(crate) fn post(command: Command) {
    sayit_overlay_macos::orb::start(&crate::desktop::native_orb::MIRRORS);
    STARTED.store(true, Ordering::Release);
    sayit_overlay_macos::orb::post(command);
}

pub(crate) fn post_if_started(command: Command) {
    if started() {
        sayit_overlay_macos::orb::post(command);
    }
}

pub(crate) fn cursor_px() -> Option<(i32, i32)> {
    sayit_overlay_macos::orb::cursor_px()
}

pub(super) fn on_action(action: OrbAction) {
    crate::desktop::native_orb::run_orb_action(action);
}

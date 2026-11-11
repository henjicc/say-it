//! macOS 字幕条薄桥：配置、布局或显示时注册宿主，文本与隐藏只在宿主存在时投递。
pub(crate) use sayit_overlay_core::subtitle::Command;
use sayit_overlay_core::subtitle::SubtitleAction;
use std::sync::atomic::{AtomicBool, Ordering};

static STARTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn post(command: Command) {
    sayit_overlay_macos::subtitle::post(command);
    STARTED.store(true, Ordering::Release);
}

pub(crate) fn post_if_started(command: Command) {
    if STARTED.load(Ordering::Acquire) {
        sayit_overlay_macos::subtitle::post(command);
    }
}

pub(crate) fn reference_size() -> Option<(f64, f64)> {
    sayit_overlay_macos::subtitle::reference_size()
}

pub(super) fn on_action(action: SubtitleAction) {
    crate::desktop::native_subtitle::run_subtitle_action(action);
}

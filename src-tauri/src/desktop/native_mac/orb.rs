//! macOS 悬浮球桥接契约；宿主接入前保持无操作桩。

use sayit_overlay_core::orb::OrbAction;

pub(crate) fn cursor_over() -> bool {
    false
}

pub(crate) fn is_visible() -> bool {
    false
}

pub(crate) fn rect() -> Option<(i32, i32, i32)> {
    None
}

pub(crate) fn set_waveform(_level: f32, _peaks: Vec<f32>) {}

pub(crate) fn set_state(_phase: &str, _transient: bool, _can_submit: bool) {}

pub(crate) fn set_interactive(_interactive: bool) {}

pub(crate) fn hide() {}

pub(crate) fn show() {}

pub(crate) fn set_opacity(_percent: u8) {}

pub(crate) fn move_to(_x: i32, _y: i32) {}

pub(crate) fn set_rect(_x: i32, _y: i32, _size: i32) {}

pub(crate) fn configure(_x: i32, _y: i32, _size: i32, _opacity_percent: u8) {}

pub(crate) fn started() -> bool {
    false
}

pub(crate) fn attach(_app: &tauri::AppHandle) {}

pub(super) fn on_action(_app: &tauri::AppHandle, _action: OrbAction) {}

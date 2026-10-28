//! macOS 字幕条桥接契约；宿主接入前保持无操作桩。

use sayit_overlay_core::subtitle::{SubtitleAction, SubtitleConfig};

pub(crate) fn set_translation(_text: String) {}

pub(crate) fn set_text(_text: String, _fade: bool) {}

pub(crate) fn hide() {}

pub(crate) fn show() {}

pub(crate) fn set_layout(_width: f64, _height: f64, _anchor: &str, _offset_y: f64) {}

pub(crate) fn set_config(_config: SubtitleConfig) {}

pub(crate) fn attach(_app: &tauri::AppHandle) {}

pub(crate) fn reference_size() -> Option<(f64, f64)> {
    None
}

pub(super) fn on_action(_app: &tauri::AppHandle, _action: SubtitleAction) {}

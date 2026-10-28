//! macOS 原生悬浮窗薄桥；宿主接入前保持无操作桩。

pub(crate) mod indicator;
pub(crate) mod orb;
pub(crate) mod subtitle;

pub(crate) fn ensure_started(_app: &tauri::AppHandle) {}
pub(crate) fn set_look(_look: sayit_overlay_core::look::Look) {}

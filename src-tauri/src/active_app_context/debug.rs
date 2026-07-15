use std::sync::atomic::{AtomicU64, Ordering};

use crate::prelude::*;
use crate::state::RuntimeState;
use tauri::AppHandle;

use super::{activation_target, CaptureStatus, CapturedActiveAppContext};

pub(crate) const DEBUG_RESULT_EVENT: &str = "active-app-context-debug-result";
pub(crate) const DEBUG_STATE_EVENT: &str = "active-app-context-debug-state";

static DEBUG_CAPTURE_EPOCH: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveAppContextDebugPayload {
    #[serde(flatten)]
    context: CapturedActiveAppContext,
    formatted_context: String,
    message: Option<String>,
}

pub(crate) fn reset_debug_capture() {
    DEBUG_CAPTURE_EPOCH.fetch_add(1, Ordering::AcqRel);
}

pub(crate) fn request_debug_capture(app: AppHandle) {
    let epoch = DEBUG_CAPTURE_EPOCH.fetch_add(1, Ordering::AcqRel) + 1;
    let Some(window) = app.get_webview_window(crate::desktop::CONTEXT_DEBUG_WINDOW_LABEL) else {
        crate::hotkey::set_context_debug_active(false);
        return;
    };
    let _ = window.emit(DEBUG_STATE_EVENT, json!({ "state": "capturing" }));

    let Some(target) = activation_target() else {
        let context = CapturedActiveAppContext::with_status(CaptureStatus::Failed);
        let _ = window.emit(
            DEBUG_RESULT_EVENT,
            ActiveAppContextDebugPayload {
                context,
                formatted_context: String::new(),
                message: Some("未找到可捕获的前台窗口；请先点击其他应用，再按调试快捷键。".into()),
            },
        );
        return;
    };
    let debug_window_handle = window.hwnd().ok().map(|hwnd| hwnd.0 as isize);

    tauri::async_runtime::spawn(async move {
        let state = app.state::<RuntimeState>();
        let blocked_apps = state
            .app_settings
            .lock()
            .ok()
            .and_then(|settings| {
                settings
                    .dictation_prefs
                    .get("activeAppContextBlockedApps")
                    .and_then(Value::as_array)
                    .cloned()
            })
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect();
        let handle =
            state
                .active_app_context
                .begin_debug_capture(target, blocked_apps, debug_window_handle);
        let context = state.active_app_context.resolve(handle).await;
        if DEBUG_CAPTURE_EPOCH.load(Ordering::Acquire) != epoch {
            return;
        }
        let formatted_context = context.format_for_prompt();
        if let Some(window) = app.get_webview_window(crate::desktop::CONTEXT_DEBUG_WINDOW_LABEL) {
            let _ = window.emit(
                DEBUG_RESULT_EVENT,
                ActiveAppContextDebugPayload {
                    context,
                    formatted_context,
                    message: None,
                },
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_payload_flattens_captured_fields_for_the_window_contract() {
        let value = serde_json::to_value(ActiveAppContextDebugPayload {
            context: CapturedActiveAppContext {
                status: CaptureStatus::Captured,
                app_name: "Notepad".into(),
                ..Default::default()
            },
            formatted_context: "应用：Notepad".into(),
            message: None,
        })
        .unwrap();
        assert_eq!(value["status"], "captured");
        assert_eq!(value["appName"], "Notepad");
        assert_eq!(value["formattedContext"], "应用：Notepad");
    }
}

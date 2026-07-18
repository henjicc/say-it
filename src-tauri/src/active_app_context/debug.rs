use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::prelude::*;
use crate::state::RuntimeState;
use tauri::AppHandle;

use super::{activation_target, CaptureStatus, CapturedActiveAppContext, OcrEngineKind};

pub(crate) const DEBUG_RESULT_EVENT: &str = "active-app-context-debug-result";
pub(crate) const DEBUG_STATE_EVENT: &str = "active-app-context-debug-state";

static DEBUG_CAPTURE_EPOCH: AtomicU64 = AtomicU64::new(0);

/// 调试窗口设置的临时参数覆盖，仅影响调试捕获，不写入应用设置。
#[derive(Clone, Copy, Default)]
struct DebugCaptureOverrides {
    ocr_engine: Option<OcrEngineKind>,
    max_capture_side: Option<u32>,
}

static DEBUG_OVERRIDES: Mutex<DebugCaptureOverrides> = Mutex::new(DebugCaptureOverrides {
    ocr_engine: None,
    max_capture_side: None,
});

pub(crate) fn set_debug_capture_overrides(
    ocr_engine: Option<OcrEngineKind>,
    max_capture_side: Option<u32>,
) {
    if let Ok(mut guard) = DEBUG_OVERRIDES.lock() {
        *guard = DebugCaptureOverrides {
            ocr_engine,
            max_capture_side,
        };
    }
}

fn debug_capture_overrides() -> DebugCaptureOverrides {
    DEBUG_OVERRIDES.lock().map(|guard| *guard).unwrap_or_default()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveAppContextDebugPayload {
    #[serde(flatten)]
    context: CapturedActiveAppContext,
    formatted_context: String,
    message: Option<String>,
    /// 本次调试捕获实际使用的 OCR 引擎与截图长边上限；仅 `captureMethod` 为 `ocr` 时有意义。
    ocr_engine: Option<&'static str>,
    max_capture_side: Option<u32>,
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
                ocr_engine: None,
                max_capture_side: None,
            },
        );
        return;
    };
    let debug_window_handle = window.hwnd().ok().map(|hwnd| hwnd.0 as isize);

    tauri::async_runtime::spawn(async move {
        let state = app.state::<RuntimeState>();
        let (blocked_apps, method, ocr_engine) = state
            .app_settings
            .lock()
            .ok()
            .map(|settings| {
                let blocked_apps = settings
                    .dictation_prefs
                    .get("activeAppContextBlockedApps")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect();
                let method =
                    crate::application::dictation::active_app_context_extraction_method_from_value(
                        &settings.dictation_prefs,
                    );
                let ocr_engine =
                    crate::application::dictation::active_app_context_ocr_engine_from_value(
                        &settings.dictation_prefs,
                    );
                (blocked_apps, method, ocr_engine)
            })
            .unwrap_or_default();
        let overrides = debug_capture_overrides();
        let ocr_engine = overrides.ocr_engine.unwrap_or(ocr_engine);
        let handle = state.active_app_context.begin_debug_capture(
            target,
            blocked_apps,
            debug_window_handle,
            method,
            ocr_engine,
            overrides.max_capture_side,
        );
        let context = state.active_app_context.resolve(handle).await;
        if DEBUG_CAPTURE_EPOCH.load(Ordering::Acquire) != epoch {
            return;
        }
        let formatted_context = context.format_for_prompt();
        let is_ocr = method == super::ActiveAppContextExtractionMethod::Ocr;
        if let Some(window) = app.get_webview_window(crate::desktop::CONTEXT_DEBUG_WINDOW_LABEL) {
            let _ = window.emit(
                DEBUG_RESULT_EVENT,
                ActiveAppContextDebugPayload {
                    context,
                    formatted_context,
                    message: None,
                    ocr_engine: is_ocr.then_some(ocr_engine.as_str()),
                    max_capture_side: is_ocr.then_some(
                        overrides
                            .max_capture_side
                            .unwrap_or(super::model::DEFAULT_MAX_CAPTURE_SIDE),
                    ),
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
            ocr_engine: Some("system"),
            max_capture_side: Some(1_600),
        })
        .unwrap();
        assert_eq!(value["status"], "captured");
        assert_eq!(value["appName"], "Notepad");
        assert_eq!(value["formattedContext"], "应用：Notepad");
        assert_eq!(value["ocrEngine"], "system");
        assert_eq!(value["maxCaptureSide"], 1_600);
    }
}

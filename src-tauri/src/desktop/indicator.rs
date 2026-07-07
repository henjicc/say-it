use crate::prelude::*;

#[cfg(windows)]
use std::ffi::c_void;

#[cfg(windows)]
const HWND_TOPMOST_RAW: *mut c_void = -1isize as *mut c_void;
#[cfg(windows)]
const SWP_NOSIZE_RAW: u32 = 0x0001;
#[cfg(windows)]
const SWP_NOMOVE_RAW: u32 = 0x0002;
#[cfg(windows)]
const SWP_NOACTIVATE_RAW: u32 = 0x0010;
#[cfg(windows)]
const SWP_SHOWWINDOW_RAW: u32 = 0x0040;

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn SetWindowPos(
        hwnd: *mut c_void,
        hwnd_insert_after: *mut c_void,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
}

const DICTATION_INDICATOR_LABEL: &str = "dictation-indicator";
const DEFAULT_INDICATOR_WIDTH: f64 = 460.0;
const DEFAULT_INDICATOR_HEIGHT: f64 = 188.0;

fn place_indicator_window(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
    anchor: &str,
    offset_y: f64,
) {
    let _ = window.set_size(tauri::LogicalSize::new(width, height));
    if let Ok(Some(monitor)) = window.current_monitor() {
        let size = monitor.size();
        let scale = window.scale_factor().unwrap_or(1.0);
        let win_w = (width * scale) as i32;
        let win_h = (height * scale) as i32;
        let x = (size.width as i32 - win_w) / 2;
        let margin = (offset_y * scale) as i32;
        let y = match anchor {
            "top" => margin,
            "center" => ((size.height as i32 - win_h) / 2) + margin,
            _ => size.height as i32 - win_h - margin,
        };
        let _ = window.set_position(tauri::PhysicalPosition::new(x.max(0), y.max(0)));
    }
}

pub(crate) fn ensure_indicator_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(win) = app.get_webview_window(DICTATION_INDICATOR_LABEL) {
        return Ok(win);
    }
    let window = WebviewWindowBuilder::new(
        app,
        DICTATION_INDICATOR_LABEL,
        WebviewUrl::App("indicator.html".into()),
    )
    .title("语音输入")
    .inner_size(DEFAULT_INDICATOR_WIDTH, DEFAULT_INDICATOR_HEIGHT)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .shadow(false)
    .transparent(true)
    .build()
    .map_err(|e| format!("创建指示器窗口失败: {e}"))?;

    // 点击穿透：空闲时整块透明、不拦截鼠标。
    let _ = window.set_ignore_cursor_events(true);

    place_indicator_window(&window, DEFAULT_INDICATOR_WIDTH, DEFAULT_INDICATOR_HEIGHT, "bottom", 36.0);
    Ok(window)
}

pub(crate) fn raise_indicator_window(window: &tauri::WebviewWindow) {
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    #[cfg(windows)]
    {
        if let Ok(hwnd) = window.hwnd() {
            let _ = unsafe {
                SetWindowPos(
                    hwnd.0,
                    HWND_TOPMOST_RAW,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE_RAW | SWP_NOSIZE_RAW | SWP_NOACTIVATE_RAW | SWP_SHOWWINDOW_RAW,
                )
            };
        }
    }
}

/// 切换指示器内容。state: "recording" | "processing" | "hidden"。
/// 显示态会重新提升到 topmost，但不激活窗口，避免抢走目标程序焦点。
#[tauri::command]
pub(crate) fn set_indicator_state(app: tauri::AppHandle, state: String) -> Result<(), String> {
    hotkey::set_dictation_active(state == "recording" || state == "processing");
    let window = ensure_indicator_window(&app)?;
    if state != "hidden" {
        raise_indicator_window(&window);
    }
    let _ = window.emit("dictation-indicator-state", json!({ "state": state }));
    Ok(())
}


#[tauri::command]
pub(crate) fn set_indicator_text(app: tauri::AppHandle, text: String, fade: Option<bool>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(DICTATION_INDICATOR_LABEL) {
        let _ = window.emit(
            "dictation-indicator-text",
            json!({ "text": text, "fade": fade.unwrap_or(false) }),
        );
    }
    Ok(())
}

/// 字幕翻译的第二行文本通道，与 `set_indicator_text`（原文）相互独立，
/// 便于双语字幕分别控制各自内容而不互相打断动画。
#[tauri::command]
pub(crate) fn set_indicator_translation(app: tauri::AppHandle, text: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(DICTATION_INDICATOR_LABEL) {
        let _ = window.emit("dictation-indicator-translation", json!({ "text": text }));
    }
    Ok(())
}

/// 返回指示器窗口所在显示器的逻辑尺寸，供前端把百分比换算成像素。
#[tauri::command]
pub(crate) fn get_indicator_monitor_metrics(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let window = ensure_indicator_window(&app)?;
    let scale = window.scale_factor().unwrap_or(1.0);
    if let Ok(Some(monitor)) = window.current_monitor() {
        let size = monitor.size();
        return Ok(json!({
            "width": size.width as f64 / scale,
            "height": size.height as f64 / scale,
        }));
    }
    Ok(json!({ "width": 1920.0, "height": 1080.0 }))
}

/// 调整字幕/指示器窗口尺寸与屏幕位置。anchor: "top" | "center" | "bottom"。
#[tauri::command]
pub(crate) fn set_indicator_layout(
    app: tauri::AppHandle,
    width: Option<f64>,
    height: Option<f64>,
    anchor: Option<String>,
    offset_y: Option<f64>,
) -> Result<(), String> {
    let window = ensure_indicator_window(&app)?;
    let width = width.unwrap_or(DEFAULT_INDICATOR_WIDTH).clamp(320.0, 2400.0);
    let height = height.unwrap_or(DEFAULT_INDICATOR_HEIGHT).clamp(120.0, 720.0);
    let anchor = anchor.unwrap_or_else(|| "bottom".to_string());
    let offset_y = offset_y.unwrap_or(36.0).clamp(-240.0, 240.0);
    place_indicator_window(&window, width, height, &anchor, offset_y);
    Ok(())
}



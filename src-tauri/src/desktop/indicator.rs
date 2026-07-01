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
    .inner_size(520.0, 220.0)
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

    // 放到屏幕底部居中附近。
    if let Ok(Some(monitor)) = window.current_monitor() {
        let size = monitor.size();
        let scale = window.scale_factor().unwrap_or(1.0);
        let win_w = (520.0 * scale) as i32;
        let x = (size.width as i32 - win_w) / 2;
        let y = size.height as i32 - (256.0 * scale) as i32;
        let _ = window.set_position(tauri::PhysicalPosition::new(x.max(0), y.max(0)));
    }
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
    hotkey::set_dictation_active(state != "hidden");
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



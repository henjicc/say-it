use crate::prelude::*;

pub(crate) const CONTEXT_DEBUG_WINDOW_LABEL: &str = "active-app-context-debug";
const DEBUG_WINDOW_WIDTH: f64 = 720.0;
const DEBUG_WINDOW_HEIGHT: f64 = 700.0;

fn place_window(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = monitor.size();
    let position = monitor.position();
    let margin = (24.0 * scale) as i32;
    let width = (DEBUG_WINDOW_WIDTH * scale) as i32;
    let x = position.x + size.width as i32 - width - margin;
    let y = position.y + margin;
    let _ = window.set_position(tauri::PhysicalPosition::new(x.max(position.x), y));
}

fn ensure_context_debug_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(CONTEXT_DEBUG_WINDOW_LABEL) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(
        app,
        CONTEXT_DEBUG_WINDOW_LABEL,
        WebviewUrl::App("context-debug.html".into()),
    )
    .title("当前软件上下文调试")
    .inner_size(DEBUG_WINDOW_WIDTH, DEBUG_WINDOW_HEIGHT)
    .min_inner_size(520.0, 480.0)
    .resizable(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(false)
    .focused(true)
    .visible(false)
    .shadow(true)
    .build()
    .map_err(|error| format!("创建上下文调试窗口失败：{error}"))?;
    place_window(&window);
    Ok(window)
}

#[tauri::command]
pub(crate) fn open_active_app_context_debug(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = app;
        return Err("当前软件上下文调试首版仅支持 Windows".into());
    }
    #[cfg(windows)]
    {
        let window = ensure_context_debug_window(&app)?;
        crate::active_app_context::reset_debug_capture();
        crate::hotkey::set_context_debug_active(true);
        let _ = window.emit(
            crate::active_app_context::DEBUG_STATE_EVENT,
            json!({ "state": "waiting" }),
        );
        window
            .show()
            .map_err(|error| format!("显示上下文调试窗口失败：{error}"))?;
        let _ = window.set_focus();
        Ok(())
    }
}

#[tauri::command]
pub(crate) fn close_active_app_context_debug(app: tauri::AppHandle) -> Result<(), String> {
    crate::hotkey::set_context_debug_active(false);
    crate::active_app_context::reset_debug_capture();
    if let Some(window) = app.get_webview_window(CONTEXT_DEBUG_WINDOW_LABEL) {
        window
            .close()
            .map_err(|error| format!("关闭上下文调试窗口失败：{error}"))?;
    }
    Ok(())
}

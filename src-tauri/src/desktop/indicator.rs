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
// macOS 听写内容距窗口底边还有 24px 透明内边距；-12px 让可见内容最终
// 保持在 Dock 或屏幕底边上方约 12px，而不是重复叠加两份间距。
#[cfg(target_os = "macos")]
pub(crate) const DICTATION_INDICATOR_OFFSET_Y: f64 = -12.0;
#[cfg(not(target_os = "macos"))]
pub(crate) const DICTATION_INDICATOR_OFFSET_Y: f64 = 36.0;

fn fallback_indicator_position(
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: i32,
    monitor_height: i32,
    window_width: i32,
    window_height: i32,
    anchor: &str,
    margin: i32,
) -> (i32, i32) {
    let x = monitor_x + (monitor_width - window_width) / 2;
    let y = match anchor {
        "top" => monitor_y + margin,
        "center" => monitor_y + (monitor_height - window_height) / 2 + margin,
        _ => monitor_y + monitor_height - window_height - margin,
    };
    (x, y)
}

fn place_indicator_window(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
    anchor: &str,
    offset_y: f64,
) {
    let _ = window.set_size(tauri::LogicalSize::new(width, height));

    #[cfg(target_os = "macos")]
    if let Ok(ns_window) = window.ns_window() {
        if crate::macos_native::place_indicator_window(ns_window, width, height, anchor, offset_y)
            .is_ok()
        {
            return;
        }
    }

    if let Ok(Some(monitor)) = window.current_monitor() {
        let size = monitor.size();
        let position = monitor.position();
        let scale = window.scale_factor().unwrap_or(1.0);
        let win_w = (width * scale) as i32;
        let win_h = (height * scale) as i32;
        let margin = (offset_y * scale) as i32;
        let (x, y) = fallback_indicator_position(
            position.x,
            position.y,
            size.width as i32,
            size.height as i32,
            win_w,
            win_h,
            anchor,
            margin,
        );
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

pub(crate) fn ensure_indicator_window(
    app: &tauri::AppHandle,
) -> Result<tauri::WebviewWindow, String> {
    if let Some(win) = app.get_webview_window(DICTATION_INDICATOR_LABEL) {
        return Ok(win);
    }
    let builder = WebviewWindowBuilder::new(
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
    .visible(false)
    .shadow(false)
    .transparent(true);
    let window = builder
        .build()
        .map_err(|e| format!("创建指示器窗口失败: {e}"))?;
    crate::desktop::floating_orb::sync_system_glass_window(&window);

    // 点击穿透：空闲时整块透明、不拦截鼠标。
    let _ = window.set_ignore_cursor_events(true);

    place_indicator_window(
        &window,
        DEFAULT_INDICATOR_WIDTH,
        DEFAULT_INDICATOR_HEIGHT,
        "bottom",
        DICTATION_INDICATOR_OFFSET_Y,
    );
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

/// 听写和实时字幕共用同一个悬浮窗口。每次听写启动都必须显式清掉字幕配置，
/// 否则上一次字幕会话留下的样式会被下一次听写复用。
pub(crate) fn prepare_dictation_indicator(app: &tauri::AppHandle) -> Result<(), String> {
    let window = ensure_indicator_window(app)?;
    let _ = window.emit("dictation-indicator-config", json!({ "mode": "dictation" }));
    let _ = window.emit(
        "dictation-indicator-error",
        json!({ "message": "", "canUseRawText": false }),
    );
    let _ = window.emit(
        "dictation-indicator-text",
        json!({ "text": "", "fade": false }),
    );
    let _ = window.emit("dictation-indicator-translation", json!({ "text": "" }));
    let _ = window.emit(
        "dictation-indicator-waveform",
        json!({ "active": false, "level": 0, "peaks": [] }),
    );
    Ok(())
}

/// 切换指示器内容。state: "recording" | "processing" | "smartProcessing" | "subtitle" | "error" | "hidden"。
/// 显示态会重新提升到 topmost，但不激活窗口，避免抢走目标程序焦点。
#[tauri::command]
pub(crate) fn set_indicator_state(app: tauri::AppHandle, state: String) -> Result<(), String> {
    hotkey::set_dictation_active(
        state == "recording" || state == "processing" || state == "smartProcessing",
    );
    if state == "hidden" {
        if let Some(window) = app.get_webview_window(DICTATION_INDICATOR_LABEL) {
            let _ = window.emit("dictation-indicator-state", json!({ "state": state }));
            let _ = window.set_ignore_cursor_events(true);
            window
                .hide()
                .map_err(|error| format!("隐藏指示器窗口失败: {error}"))?;
        }
        return Ok(());
    }
    let window = ensure_indicator_window(&app)?;
    let _ = window.set_ignore_cursor_events(state != "subtitle" && state != "error");
    raise_indicator_window(&window);
    let _ = window.emit("dictation-indicator-state", json!({ "state": state }));
    Ok(())
}

/// 在听写悬浮窗中展示可操作错误。`can_use_raw_text` 仅用于智能处理失败：
/// 待恢复的原文仍由 Rust 会话持有，WebView 只发送恢复命令。
pub(crate) fn show_dictation_indicator_error(
    app: &tauri::AppHandle,
    message: String,
    can_use_raw_text: bool,
) -> Result<(), String> {
    let window = ensure_indicator_window(app)?;
    place_indicator_window(
        &window,
        DEFAULT_INDICATOR_WIDTH,
        DEFAULT_INDICATOR_HEIGHT,
        "bottom",
        DICTATION_INDICATOR_OFFSET_Y,
    );
    let _ = window.emit("dictation-indicator-config", json!({ "mode": "dictation" }));
    let _ = window.emit(
        "dictation-indicator-text",
        json!({ "text": "", "fade": false }),
    );
    let _ = window.emit("dictation-indicator-translation", json!({ "text": "" }));
    let _ = window.emit(
        "dictation-indicator-waveform",
        json!({ "active": false, "level": 0, "peaks": [] }),
    );
    let _ = window.set_ignore_cursor_events(false);
    raise_indicator_window(&window);
    let _ = window.emit(
        "dictation-indicator-error",
        json!({ "message": message, "canUseRawText": can_use_raw_text }),
    );
    let _ = window.emit("dictation-indicator-state", json!({ "state": "error" }));
    hotkey::set_dictation_active(false);
    Ok(())
}

#[tauri::command]
pub(crate) fn set_indicator_text(
    app: tauri::AppHandle,
    text: String,
    fade: Option<bool>,
) -> Result<(), String> {
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
pub(crate) fn get_indicator_monitor_metrics(
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    let window = ensure_indicator_window(&app)?;
    #[cfg(target_os = "macos")]
    if let Ok(ns_window) = window.ns_window() {
        if let Ok((width, height)) = crate::macos_native::indicator_visible_screen_size(ns_window) {
            return Ok(json!({ "width": width, "height": height }));
        }
    }
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
    let width = width
        .unwrap_or(DEFAULT_INDICATOR_WIDTH)
        .clamp(160.0, 2400.0);
    let height = height
        .unwrap_or(DEFAULT_INDICATOR_HEIGHT)
        .clamp(56.0, 720.0);
    let anchor = anchor.unwrap_or_else(|| "bottom".to_string());
    let offset_y = offset_y.unwrap_or(36.0).clamp(-240.0, 240.0);
    place_indicator_window(&window, width, height, &anchor, offset_y);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::fallback_indicator_position;

    #[test]
    fn fallback_position_preserves_negative_secondary_monitor_origin() {
        assert_eq!(
            fallback_indicator_position(-1_920, 0, 1_920, 1_080, 460, 188, "bottom", 36),
            (-1_190, 856)
        );
    }

    #[test]
    fn fallback_position_applies_anchor_margin_in_monitor_coordinates() {
        assert_eq!(
            fallback_indicator_position(200, -900, 1_600, 900, 400, 180, "top", 24),
            (800, -876)
        );
        assert_eq!(
            fallback_indicator_position(200, -900, 1_600, 900, 400, 180, "center", 24),
            (800, -516)
        );
    }
}

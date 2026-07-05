use crate::prelude::*;
use crate::state::*;

pub(crate) fn is_normal_window_position(position: tauri::PhysicalPosition<i32>) -> bool {
    position.x > -10000 && position.y > -10000
}

/// `size` 必须传 `inner_size`:恢复时用的 `set_size` 设置的是内容区尺寸,
/// 而 Windows 无边框窗口的 `outer_size` 含不可见调整边框,混用会导致窗口每次恢复都变大。
pub(crate) fn remember_main_window_placement(
    app: &tauri::AppHandle,
    minimized: bool,
    position: Result<tauri::PhysicalPosition<i32>, tauri::Error>,
    size: Result<tauri::PhysicalSize<u32>, tauri::Error>,
) {
    if minimized {
        return;
    }
    let Ok(position) = position else {
        return;
    };
    if !is_normal_window_position(position) {
        return;
    }
    let Ok(size) = size else {
        return;
    };
    let state = app.state::<RuntimeState>();
    if let Ok(mut placement) = state.main_window_placement.lock() {
        *placement = Some(MainWindowPlacement { position, size });
    };
}

pub(crate) fn park_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        remember_main_window_placement(
            app,
            window.is_minimized().unwrap_or(false),
            window.outer_position(),
            window.inner_size(),
        );

        let _ = window.unminimize();
        let _ = window.set_skip_taskbar(true);
        let _ = window.set_position(tauri::PhysicalPosition::new(-32000, -32000));
        let _ = window.show();
    }
}

pub(crate) fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let state = app.state::<RuntimeState>();
        let placement = state
            .main_window_placement
            .lock()
            .ok()
            .and_then(|mut value| value.take());

        let _ = window.set_skip_taskbar(false);
        let _ = window.show();
        let _ = window.unminimize();
        if let Some(placement) = placement {
            let _ = window.set_size(placement.size);
            let _ = window.set_position(placement.position);
        }
        let _ = window.set_focus();
    }
}


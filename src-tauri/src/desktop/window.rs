use crate::application::window_lifecycle::EnsureMainWindowAction;
use crate::prelude::*;
use crate::state::*;

const MAIN_WINDOW_LABEL: &str = "main";
const MIN_VISIBLE_EDGE: i32 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MonitorBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub(crate) fn is_normal_window_position(position: tauri::PhysicalPosition<i32>) -> bool {
    position.x > -10000 && position.y > -10000
}

fn placement_is_visible(
    position: tauri::PhysicalPosition<i32>,
    monitors: &[MonitorBounds],
) -> bool {
    monitors.iter().any(|monitor| {
        let right = monitor.x.saturating_add_unsigned(monitor.width);
        let bottom = monitor.y.saturating_add_unsigned(monitor.height);
        position.x < right
            && position.y < bottom
            && position.x.saturating_add(MIN_VISIBLE_EDGE) > monitor.x
            && position.y.saturating_add(MIN_VISIBLE_EDGE) > monitor.y
    })
}

/// 只记录非最小化时的正常内容区尺寸；最大化时保留最近一次正常尺寸，单独记录最大化标记。
pub(crate) fn remember_main_window_placement(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let maximized = window.is_maximized().unwrap_or(false);
    let state = app.state::<RuntimeState>();
    let Ok(mut placement) = state.main_window_placement.lock() else {
        return;
    };
    if maximized {
        if let Some(value) = placement.as_mut() {
            value.maximized = true;
        }
        return;
    }

    let (Ok(position), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.inner_size(),
        window.scale_factor(),
    ) else {
        return;
    };
    if !is_normal_window_position(position) || scale <= 0.0 {
        return;
    }
    *placement = Some(MainWindowPlacement {
        position,
        size: tauri::LogicalSize::new(size.width as f64 / scale, size.height as f64 / scale),
        maximized: false,
    });
}

fn saved_placement(app: &tauri::AppHandle) -> Option<MainWindowPlacement> {
    app.state::<RuntimeState>()
        .main_window_placement
        .lock()
        .ok()
        .and_then(|value| *value)
}

fn restore_main_window_placement(window: &tauri::WebviewWindow) -> bool {
    let Some(placement) = saved_placement(window.app_handle()) else {
        return false;
    };
    let monitors = window
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|monitor| MonitorBounds {
            x: monitor.position().x,
            y: monitor.position().y,
            width: monitor.size().width,
            height: monitor.size().height,
        })
        .collect::<Vec<_>>();

    let _ = window.set_size(placement.size);
    if placement_is_visible(placement.position, &monitors) {
        let _ = window.set_position(placement.position);
    } else {
        let _ = window.center();
    }
    placement.maximized
}

fn reveal_main_window(window: &tauri::WebviewWindow) {
    let maximized = restore_main_window_placement(window);
    let _ = window.set_skip_taskbar(false);
    let _ = window.unminimize();
    let _ = window.show();
    if maximized {
        let _ = window.maximize();
    }
    let _ = window.set_focus();
}

pub(crate) fn register_initial_main_window(app: &tauri::AppHandle, should_open: bool) {
    if let Ok(mut lifecycle) = app.state::<RuntimeState>().main_window_lifecycle.lock() {
        lifecycle.register_initial_window(should_open);
    }
}

/// 托盘、单实例和其他显式打开路径共用的幂等入口。
pub(crate) fn ensure_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let existing = app.get_webview_window(MAIN_WINDOW_LABEL);
    let action = {
        let state = app.state::<RuntimeState>();
        let mut lifecycle = state
            .main_window_lifecycle
            .lock()
            .map_err(|_| "主窗口生命周期锁已损坏".to_string())?;
        lifecycle.request_open(existing.is_some())
    };

    match action {
        EnsureMainWindowAction::ShowExisting => {
            if let Some(window) = existing {
                reveal_main_window(&window);
                Ok(())
            } else {
                // 窗口在状态检查后消失；下一次点击可以重新创建。
                let state = app.state::<RuntimeState>();
                if let Ok(mut lifecycle) = state.main_window_lifecycle.lock() {
                    lifecycle.close_completed();
                }
                Err("主窗口已在打开过程中被销毁，请重试".into())
            }
        }
        EnsureMainWindowAction::AwaitReady => Ok(()),
        EnsureMainWindowAction::Create { generation } => {
            let result = (|| {
                let config = app
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|config| config.label == MAIN_WINDOW_LABEL)
                    .cloned()
                    .ok_or_else(|| "Tauri 配置中缺少 main 窗口".to_string())?;
                WebviewWindowBuilder::from_config(app, &config)
                    .map_err(|error| format!("读取主窗口配置失败: {error}"))?
                    .visible(false)
                    .build()
                    .map_err(|error| format!("创建主窗口失败: {error}"))
            })();
            match result {
                Ok(window) => {
                    let _ = restore_main_window_placement(&window);
                    Ok(())
                }
                Err(error) => {
                    if let Ok(mut lifecycle) =
                        app.state::<RuntimeState>().main_window_lifecycle.lock()
                    {
                        lifecycle.creation_failed(generation);
                    }
                    Err(error)
                }
            }
        }
    }
}

#[tauri::command]
pub(crate) fn main_window_ready(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != MAIN_WINDOW_LABEL {
        return Err("只有主窗口可以完成主窗口 ready 握手".into());
    }
    let should_show = {
        let state = window.state::<RuntimeState>();
        let mut lifecycle = state
            .main_window_lifecycle
            .lock()
            .map_err(|_| "主窗口生命周期锁已损坏".to_string())?;
        lifecycle.mark_ready()
    };
    if should_show {
        reveal_main_window(&window);
    }
    Ok(())
}

/// 保存窗口位置后真正销毁 WebView。该函数不触碰任何后台业务服务。
pub(crate) fn destroy_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        if let Ok(mut lifecycle) = app.state::<RuntimeState>().main_window_lifecycle.lock() {
            lifecycle.close_completed();
        }
        return Ok(());
    };
    remember_main_window_placement(app);
    if let Ok(mut lifecycle) = app.state::<RuntimeState>().main_window_lifecycle.lock() {
        lifecycle.begin_close();
    }
    match window.destroy() {
        Ok(()) => {
            if let Ok(mut lifecycle) = app.state::<RuntimeState>().main_window_lifecycle.lock() {
                lifecycle.close_completed();
            }
            Ok(())
        }
        Err(error) => {
            // 销毁失败时回退到旧 hide 语义，确保用户仍能从托盘恢复。
            let _ = window.set_skip_taskbar(true);
            let _ = window.hide();
            if let Ok(mut lifecycle) = app.state::<RuntimeState>().main_window_lifecycle.lock() {
                lifecycle.close_failed_hidden();
            }
            Err(format!("销毁主窗口失败，已回退为隐藏: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_accepts_negative_coordinates_on_secondary_monitor() {
        let monitors = [
            MonitorBounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            MonitorBounds {
                x: -2560,
                y: 0,
                width: 2560,
                height: 1440,
            },
        ];
        assert!(placement_is_visible(
            tauri::PhysicalPosition::new(-2200, 120),
            &monitors
        ));
    }

    #[test]
    fn placement_rejects_disconnected_monitor_but_keeps_partially_visible_window() {
        let monitors = [MonitorBounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert!(!placement_is_visible(
            tauri::PhysicalPosition::new(2600, 80),
            &monitors
        ));
        assert!(placement_is_visible(
            tauri::PhysicalPosition::new(-50, 80),
            &monitors
        ));
    }
}

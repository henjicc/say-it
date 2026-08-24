use crate::prelude::*;
use crate::state::{
    FloatingOrbPosition, FloatingOrbSettings, RuntimeState, DEFAULT_FLOATING_ORB_OPACITY,
    DEFAULT_FLOATING_ORB_SIZE,
};
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings as EnigoSettings};
use std::sync::atomic::Ordering;

pub(crate) const FLOATING_ORB_LABEL: &str = "floating-orb";
const ORB_SHADOW_PADDING: f64 = 4.0;
const DEFAULT_MARGIN: f64 = 24.0;
const MIN_VISIBLE_EDGE: i32 = 16;
const ORB_SIZES: [u16; 3] = [48, 56, 64];
const ORB_OPACITIES: [u8; 3] = [70, 85, 100];

fn normalized_orb_size(size: u16) -> u16 {
    ORB_SIZES
        .contains(&size)
        .then_some(size)
        .unwrap_or(DEFAULT_FLOATING_ORB_SIZE)
}

fn normalized_orb_opacity(opacity: u8) -> u8 {
    ORB_OPACITIES
        .contains(&opacity)
        .then_some(opacity)
        .unwrap_or(DEFAULT_FLOATING_ORB_OPACITY)
}

fn orb_window_extent(size: u16) -> f64 {
    normalized_orb_size(size) as f64 + ORB_SHADOW_PADDING * 2.0
}

fn current_settings(app: &tauri::AppHandle) -> FloatingOrbSettings {
    app.state::<RuntimeState>()
        .floating_orb
        .lock()
        .map(|settings| {
            let mut settings = settings.clone();
            settings.size = normalized_orb_size(settings.size);
            settings.opacity = normalized_orb_opacity(settings.opacity);
            settings
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl MonitorRect {
    fn from_monitor(monitor: &tauri::Monitor) -> Self {
        let area = monitor.work_area();
        Self {
            x: area.position.x,
            y: area.position.y,
            width: area.size.width,
            height: area.size.height,
        }
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add_unsigned(self.width)
            && y < self.y.saturating_add_unsigned(self.height)
    }

    fn distance_squared(&self, x: i32, y: i32) -> i64 {
        let center_x = self.x as i64 + self.width as i64 / 2;
        let center_y = self.y as i64 + self.height as i64 / 2;
        let dx = center_x - x as i64;
        let dy = center_y - y as i64;
        dx * dx + dy * dy
    }
}

fn monitor_for_window(
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    monitors: &[MonitorRect],
) -> Option<MonitorRect> {
    let center_x = position.x.saturating_add((size.width / 2) as i32);
    let center_y = position.y.saturating_add((size.height / 2) as i32);
    monitors
        .iter()
        .copied()
        .find(|monitor| monitor.contains(center_x, center_y))
        .or_else(|| {
            monitors
                .iter()
                .copied()
                .min_by_key(|monitor| monitor.distance_squared(center_x, center_y))
        })
}

fn clamp_orb_position(
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    monitors: &[MonitorRect],
) -> tauri::PhysicalPosition<i32> {
    let Some(monitor) = monitor_for_window(position, size, monitors) else {
        return position;
    };
    let min_x = monitor
        .x
        .saturating_sub(size.width as i32)
        .saturating_add(MIN_VISIBLE_EDGE);
    let max_x = monitor
        .x
        .saturating_add_unsigned(monitor.width)
        .saturating_sub(MIN_VISIBLE_EDGE);
    let min_y = monitor
        .y
        .saturating_sub(size.height as i32)
        .saturating_add(MIN_VISIBLE_EDGE);
    let max_y = monitor
        .y
        .saturating_add_unsigned(monitor.height)
        .saturating_sub(MIN_VISIBLE_EDGE);
    tauri::PhysicalPosition::new(
        position.x.clamp(min_x, max_x),
        position.y.clamp(min_y, max_y),
    )
}

fn position_is_on_a_monitor(
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    monitors: &[MonitorRect],
) -> bool {
    let right = position.x.saturating_add_unsigned(size.width);
    let bottom = position.y.saturating_add_unsigned(size.height);
    monitors.iter().any(|monitor| {
        let monitor_right = monitor.x.saturating_add_unsigned(monitor.width);
        let monitor_bottom = monitor.y.saturating_add_unsigned(monitor.height);
        let visible_width = right
            .min(monitor_right)
            .saturating_sub(position.x.max(monitor.x));
        let visible_height = bottom
            .min(monitor_bottom)
            .saturating_sub(position.y.max(monitor.y));
        visible_width >= MIN_VISIBLE_EDGE && visible_height >= MIN_VISIBLE_EDGE
    })
}

fn resolve_idle_position(
    saved: Option<tauri::PhysicalPosition<i32>>,
    fallback: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    monitors: &[MonitorRect],
) -> tauri::PhysicalPosition<i32> {
    let position = saved
        .filter(|position| position_is_on_a_monitor(*position, size, monitors))
        .unwrap_or(fallback);
    clamp_orb_position(position, size, monitors)
}

fn monitor_rects(window: &tauri::WebviewWindow) -> Vec<MonitorRect> {
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(MonitorRect::from_monitor)
        .collect()
}

fn default_position(window: &tauri::WebviewWindow) -> tauri::PhysicalPosition<i32> {
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return tauri::PhysicalPosition::new(24, 120);
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let size =
        (orb_window_extent(current_settings(window.app_handle()).size) * scale).round() as i32;
    let margin = (DEFAULT_MARGIN * scale).round() as i32;
    tauri::PhysicalPosition::new(
        area.position
            .x
            .saturating_add_unsigned(area.size.width)
            .saturating_sub(size)
            .saturating_sub(margin),
        area.position
            .y
            .saturating_add_unsigned(area.size.height / 2)
            .saturating_sub(size / 2),
    )
}

fn saved_position(app: &tauri::AppHandle) -> Option<tauri::PhysicalPosition<i32>> {
    app.state::<RuntimeState>()
        .floating_orb
        .lock()
        .ok()
        .and_then(|settings| settings.position)
        .map(|position| tauri::PhysicalPosition::new(position.x, position.y))
}

fn resolved_idle_position(window: &tauri::WebviewWindow) -> tauri::PhysicalPosition<i32> {
    let scale = window.scale_factor().unwrap_or(1.0);
    let extent = orb_window_extent(current_settings(window.app_handle()).size);
    let size = tauri::PhysicalSize::new(
        (extent * scale).round() as u32,
        (extent * scale).round() as u32,
    );
    resolve_idle_position(
        saved_position(window.app_handle()),
        default_position(window),
        size,
        &monitor_rects(window),
    )
}

pub(crate) fn ensure_floating_orb_window(
    app: &tauri::AppHandle,
) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        return Ok(window);
    }
    let settings = current_settings(app);
    let extent = orb_window_extent(settings.size);
    let window = WebviewWindowBuilder::new(
        app,
        FLOATING_ORB_LABEL,
        WebviewUrl::App("floating-orb.html".into()),
    )
    .title("语音输入悬浮球")
    .inner_size(extent, extent)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focusable(false)
    .focused(false)
    .visible(false)
    .shadow(false)
    .transparent(true)
    .visible_on_all_workspaces(true)
    .on_menu_event(|window, event| {
        if let Err(error) = handle_floating_orb_menu_event(window.app_handle(), event.id().as_ref())
        {
            eprintln!("[floating-orb] 处理右键菜单失败: {error}");
        }
    })
    .build()
    .map_err(|error| format!("创建悬浮球窗口失败：{error}"))?;
    #[cfg(target_os = "macos")]
    if let Err(error) = window
        .ns_window()
        .map_err(|error| format!("读取 macOS 悬浮球窗口失败：{error}"))
        .and_then(crate::macos_native::configure_floating_orb_window)
    {
        let _ = window.destroy();
        return Err(error);
    }
    let position = resolved_idle_position(&window);
    window
        .set_position(position)
        .map_err(|error| format!("定位悬浮球失败：{error}"))?;
    window
        .show()
        .map_err(|error| format!("显示悬浮球失败：{error}"))?;
    emit_config(app, &settings);
    Ok(window)
}

fn resize_floating_orb_window(window: &tauri::WebviewWindow, size: u16) -> Result<(), String> {
    let extent = orb_window_extent(size);
    let scale = window.scale_factor().unwrap_or(1.0);
    let target_size = tauri::PhysicalSize::new(
        (extent * scale).round() as u32,
        (extent * scale).round() as u32,
    );
    let old_size = window
        .outer_size()
        .map_err(|error| format!("读取悬浮球尺寸失败：{error}"))?;
    if old_size == target_size {
        return Ok(());
    }
    let old_position = window
        .outer_position()
        .map_err(|error| format!("读取悬浮球位置失败：{error}"))?;
    let centered_position = tauri::PhysicalPosition::new(
        old_position.x + (old_size.width as i32 - target_size.width as i32) / 2,
        old_position.y + (old_size.height as i32 - target_size.height as i32) / 2,
    );
    window
        .set_size(tauri::LogicalSize::new(extent, extent))
        .map_err(|error| format!("调整悬浮球尺寸失败：{error}"))?;
    let position = clamp_orb_position(centered_position, target_size, &monitor_rects(window));
    window
        .set_position(position)
        .map_err(|error| format!("调整悬浮球位置失败：{error}"))
}

fn emit_config(app: &tauri::AppHandle, settings: &FloatingOrbSettings) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let _ = window.emit(
            "floating-orb-config",
            json!({
                "size": normalized_orb_size(settings.size),
                "opacity": normalized_orb_opacity(settings.opacity),
            }),
        );
    }
}

fn apply_floating_orb_config(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let settings = current_settings(app);
    resize_floating_orb_window(window, settings.size)?;
    emit_config(app, &settings);
    Ok(())
}

pub(crate) fn sync_floating_orb_window(app: &tauri::AppHandle) -> Result<(), String> {
    let enabled = app
        .state::<RuntimeState>()
        .floating_orb
        .lock()
        .map_err(|_| "悬浮球配置锁失败".to_string())?
        .enabled;
    if enabled {
        let window = ensure_floating_orb_window(app)?;
        let _ = window.set_always_on_top(true);
        apply_floating_orb_config(app, &window)?;
        window
            .show()
            .map_err(|error| format!("显示悬浮球失败：{error}"))
    } else if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        window
            .destroy()
            .map_err(|error| format!("关闭悬浮球失败：{error}"))
    } else {
        Ok(())
    }
}

#[tauri::command]
pub(crate) fn set_floating_orb_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<FloatingOrbSettings, String> {
    if !enabled && crate::application::dictation::is_floating_orb_active(&app) {
        return Err("请先结束当前悬浮球听写".into());
    }
    let state = app.state::<RuntimeState>();
    let previous = {
        let mut settings = state
            .floating_orb
            .lock()
            .map_err(|_| "悬浮球配置锁失败".to_string())?;
        let previous = settings.clone();
        settings.enabled = enabled;
        previous
    };
    if let Err(error) = crate::persistence::save_persisted_state(&app, &state)
        .and_then(|_| sync_floating_orb_window(&app))
    {
        if let Ok(mut settings) = state.floating_orb.lock() {
            *settings = previous;
        }
        let _ = crate::persistence::save_persisted_state(&app, &state);
        let _ = sync_floating_orb_window(&app);
        return Err(error);
    }
    crate::application::contract::next_revision(&state.snapshot_revision);
    state
        .floating_orb
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| "悬浮球配置锁失败".to_string())
}

#[tauri::command]
pub(crate) fn get_floating_orb_settings(
    app: tauri::AppHandle,
) -> Result<FloatingOrbSettings, String> {
    Ok(current_settings(&app))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloatingOrbMenuAction {
    Size(u16),
    Opacity(u8),
    Disable,
}

fn parse_floating_orb_menu_action(id: &str) -> Option<FloatingOrbMenuAction> {
    id.strip_prefix("floating-orb-size-")
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| ORB_SIZES.contains(value))
        .map(FloatingOrbMenuAction::Size)
        .or_else(|| {
            id.strip_prefix("floating-orb-opacity-")
                .and_then(|value| value.parse::<u8>().ok())
                .filter(|value| ORB_OPACITIES.contains(value))
                .map(FloatingOrbMenuAction::Opacity)
        })
        .or_else(|| (id == "floating-orb-disable").then_some(FloatingOrbMenuAction::Disable))
}

fn set_floating_orb_appearance(
    app: &tauri::AppHandle,
    size: Option<u16>,
    opacity: Option<u8>,
) -> Result<FloatingOrbSettings, String> {
    let state = app.state::<RuntimeState>();
    let previous = {
        let mut settings = state
            .floating_orb
            .lock()
            .map_err(|_| "悬浮球配置锁失败".to_string())?;
        let previous = settings.clone();
        if let Some(size) = size {
            settings.size = normalized_orb_size(size);
        }
        if let Some(opacity) = opacity {
            settings.opacity = normalized_orb_opacity(opacity);
        }
        previous
    };
    let result = if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        apply_floating_orb_config(app, &window).and_then(|_| persist_current_position(app))
    } else {
        crate::persistence::save_persisted_state(app, &state)
    };
    if let Err(error) = result {
        if let Ok(mut settings) = state.floating_orb.lock() {
            *settings = previous.clone();
        }
        if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
            let _ = apply_floating_orb_config(app, &window);
        }
        let _ = crate::persistence::save_persisted_state(app, &state);
        return Err(error);
    }
    crate::application::contract::next_revision(&state.snapshot_revision);
    state
        .floating_orb
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| "悬浮球配置锁失败".to_string())
}

fn handle_floating_orb_menu_event(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    match parse_floating_orb_menu_action(id) {
        Some(FloatingOrbMenuAction::Size(size)) => {
            set_floating_orb_appearance(app, Some(size), None)?;
        }
        Some(FloatingOrbMenuAction::Opacity(opacity)) => {
            set_floating_orb_appearance(app, None, Some(opacity))?;
        }
        Some(FloatingOrbMenuAction::Disable) => {
            set_floating_orb_enabled(app.clone(), false)?;
        }
        None => {}
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn show_floating_orb_menu(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(FLOATING_ORB_LABEL)
        .ok_or_else(|| "悬浮球窗口不存在".to_string())?;
    let settings = current_settings(&app);
    let size = normalized_orb_size(settings.size);
    let opacity = normalized_orb_opacity(settings.opacity);
    let size_small = tauri::menu::CheckMenuItem::with_id(
        &app,
        "floating-orb-size-48",
        "小",
        true,
        size == 48,
        None::<&str>,
    )
    .map_err(|error| format!("创建悬浮球尺寸菜单失败：{error}"))?;
    let size_normal = tauri::menu::CheckMenuItem::with_id(
        &app,
        "floating-orb-size-56",
        "标准",
        true,
        size == 56,
        None::<&str>,
    )
    .map_err(|error| format!("创建悬浮球尺寸菜单失败：{error}"))?;
    let size_large = tauri::menu::CheckMenuItem::with_id(
        &app,
        "floating-orb-size-64",
        "大",
        true,
        size == 64,
        None::<&str>,
    )
    .map_err(|error| format!("创建悬浮球尺寸菜单失败：{error}"))?;
    let size_menu = tauri::menu::Submenu::with_items(
        &app,
        "大小",
        true,
        &[&size_small, &size_normal, &size_large],
    )
    .map_err(|error| format!("创建悬浮球尺寸菜单失败：{error}"))?;
    let opacity_low = tauri::menu::CheckMenuItem::with_id(
        &app,
        "floating-orb-opacity-70",
        "70%",
        true,
        opacity == 70,
        None::<&str>,
    )
    .map_err(|error| format!("创建悬浮球透明度菜单失败：{error}"))?;
    let opacity_normal = tauri::menu::CheckMenuItem::with_id(
        &app,
        "floating-orb-opacity-85",
        "85%",
        true,
        opacity == 85,
        None::<&str>,
    )
    .map_err(|error| format!("创建悬浮球透明度菜单失败：{error}"))?;
    let opacity_solid = tauri::menu::CheckMenuItem::with_id(
        &app,
        "floating-orb-opacity-100",
        "100%",
        true,
        opacity == 100,
        None::<&str>,
    )
    .map_err(|error| format!("创建悬浮球透明度菜单失败：{error}"))?;
    let opacity_menu = tauri::menu::Submenu::with_items(
        &app,
        "不透明度",
        true,
        &[&opacity_low, &opacity_normal, &opacity_solid],
    )
    .map_err(|error| format!("创建悬浮球透明度菜单失败：{error}"))?;
    let disable = tauri::menu::MenuItem::with_id(
        &app,
        "floating-orb-disable",
        "关闭悬浮球",
        !crate::application::dictation::is_floating_orb_active(&app),
        None::<&str>,
    )
    .map_err(|error| format!("创建悬浮球关闭菜单失败：{error}"))?;
    let menu = MenuBuilder::new(&app)
        .item(&size_menu)
        .item(&opacity_menu)
        .separator()
        .item(&disable)
        .build()
        .map_err(|error| format!("创建悬浮球右键菜单失败：{error}"))?;
    window
        .popup_menu(&menu)
        .map_err(|error| format!("显示悬浮球右键菜单失败：{error}"))
}

fn persist_current_position(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) else {
        return Ok(());
    };
    let size = window
        .outer_size()
        .map_err(|error| format!("读取悬浮球尺寸失败：{error}"))?;
    let position = clamp_orb_position(
        window
            .outer_position()
            .map_err(|error| format!("读取悬浮球位置失败：{error}"))?,
        size,
        &monitor_rects(&window),
    );
    if window.outer_position().ok() != Some(position) {
        let _ = window.set_position(position);
    }
    let state = app.state::<RuntimeState>();
    {
        let mut settings = state
            .floating_orb
            .lock()
            .map_err(|_| "悬浮球配置锁失败".to_string())?;
        settings.position = Some(FloatingOrbPosition {
            x: position.x,
            y: position.y,
        });
    }
    crate::persistence::save_persisted_state(app, &state)
}

pub(crate) fn schedule_remember_floating_orb_position(app: tauri::AppHandle) {
    let state = app.state::<RuntimeState>();
    let generation = state
        .floating_orb_runtime
        .placement_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(250)).await;
        let state = app.state::<RuntimeState>();
        if state
            .floating_orb_runtime
            .placement_generation
            .load(Ordering::Acquire)
            == generation
        {
            let _ = persist_current_position(&app);
        }
    });
}

fn emit_state(app: &tauri::AppHandle, phase: &str, message: Option<&str>) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let _ = window.emit(
            "floating-orb-state",
            json!({ "phase": phase, "message": message }),
        );
    }
}

async fn cursor_location() -> Option<(i32, i32)> {
    tauri::async_runtime::spawn_blocking(|| {
        Enigo::new(&EnigoSettings::default())
            .ok()
            .and_then(|enigo| enigo.location().ok())
    })
    .await
    .ok()
    .flatten()
}

async fn forward_click(position: (i32, i32)) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut enigo = Enigo::new(&EnigoSettings::default())
            .map_err(|error| format!("初始化鼠标模拟失败：{error}"))?;
        enigo
            .move_mouse(position.0, position.1, Coordinate::Abs)
            .map_err(|error| format!("恢复点击坐标失败：{error}"))?;
        enigo
            .button(Button::Left, Direction::Click)
            .map_err(|error| format!("模拟点击失败：{error}"))
    })
    .await
    .map_err(|error| format!("模拟点击任务失败：{error}"))?
}

async fn return_to_idle(
    app: tauri::AppHandle,
    delay_ms: u64,
    message_phase: &'static str,
    message: String,
) {
    let generation = app
        .state::<RuntimeState>()
        .floating_orb_runtime
        .transition_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    emit_state(&app, message_phase, Some(&message));
    sleep(Duration::from_millis(delay_ms)).await;
    let state = app.state::<RuntimeState>();
    if state
        .floating_orb_runtime
        .transition_generation
        .load(Ordering::Acquire)
        != generation
    {
        return;
    }
    let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) else {
        return;
    };
    let _ = window.set_ignore_cursor_events(false);
    emit_state(&app, "idle", None);
}

pub(crate) fn complete_floating_orb(
    app: tauri::AppHandle,
    phase: &'static str,
    message: String,
    delay_ms: u64,
) {
    tauri::async_runtime::spawn(return_to_idle(app, delay_ms, phase, message));
}

pub(crate) fn set_floating_orb_phase(app: &tauri::AppHandle, phase: &str, message: &str) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let interactive = phase == "recording";
        let _ = window.set_ignore_cursor_events(!interactive);
        emit_state(app, phase, Some(message));
    }
}

pub(crate) fn emit_floating_orb_cue(app: &tauri::AppHandle, which: &str, kind: &str) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let _ = window.emit(
            "dictation-indicator-play-cue",
            json!({ "which": which, "kind": kind }),
        );
    }
}

pub(crate) fn emit_floating_orb_waveform(app: &tauri::AppHandle, level: f32, peaks: Vec<f32>) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let _ = window.emit(
            "dictation-indicator-waveform",
            json!({ "active": true, "level": level, "peaks": peaks }),
        );
    }
}

#[tauri::command]
pub(crate) async fn floating_orb_activate(app: tauri::AppHandle) -> Result<(), String> {
    let enabled = app
        .state::<RuntimeState>()
        .floating_orb
        .lock()
        .map_err(|_| "悬浮球配置锁失败".to_string())?
        .enabled;
    if !enabled {
        return Err("悬浮球未启用".into());
    }
    if app.state::<RuntimeState>().audio_session.is_busy()
        || crate::application::dictation::is_active(&app)
    {
        emit_state(&app, "busy", Some("当前有其他音频任务"));
        let busy_app = app.clone();
        tauri::async_runtime::spawn(async move {
            sleep(Duration::from_millis(1500)).await;
            emit_state(&busy_app, "idle", None);
        });
        return Ok(());
    }
    persist_current_position(&app)?;
    let window = ensure_floating_orb_window(&app)?;
    let click_position = cursor_location().await;
    app.state::<RuntimeState>()
        .floating_orb_runtime
        .transition_generation
        .fetch_add(1, Ordering::AcqRel);
    let _ = window.set_ignore_cursor_events(true);
    emit_state(&app, "moving", None);
    sleep(Duration::from_millis(16)).await;
    let forwarded = match click_position {
        Some(position) => forward_click(position).await.is_ok(),
        None => false,
    };
    sleep(Duration::from_millis(80)).await;
    let target = forwarded
        .then(crate::active_app_context::activation_target)
        .flatten();
    emit_state(&app, "recording", Some("聆听中…"));
    let _ = window.set_ignore_cursor_events(false);
    if let Err(error) = crate::application::dictation::start_from_floating_orb(
        app.clone(),
        target,
        forwarded && target.is_some(),
    )
    .await
    {
        complete_floating_orb(app, "error", error.clone(), 3000);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn floating_orb_stop(app: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) else {
        return Ok(());
    };
    let _ = window.set_ignore_cursor_events(true);
    emit_state(&app, "processing", Some("识别中…"));
    crate::application::dictation::stop_from_floating_orb(app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_values_use_supported_presets() {
        assert_eq!(normalized_orb_size(48), 48);
        assert_eq!(normalized_orb_size(60), DEFAULT_FLOATING_ORB_SIZE);
        assert_eq!(normalized_orb_opacity(85), 85);
        assert_eq!(normalized_orb_opacity(90), DEFAULT_FLOATING_ORB_OPACITY);
        assert_eq!(orb_window_extent(56), 64.0);
    }

    #[test]
    fn menu_ids_only_accept_known_appearance_values() {
        assert_eq!(
            parse_floating_orb_menu_action("floating-orb-size-64"),
            Some(FloatingOrbMenuAction::Size(64))
        );
        assert_eq!(
            parse_floating_orb_menu_action("floating-orb-opacity-70"),
            Some(FloatingOrbMenuAction::Opacity(70))
        );
        assert_eq!(
            parse_floating_orb_menu_action("floating-orb-disable"),
            Some(FloatingOrbMenuAction::Disable)
        );
        assert_eq!(
            parse_floating_orb_menu_action("floating-orb-opacity-90"),
            None
        );
    }

    #[test]
    fn clamp_preserves_negative_secondary_monitor_coordinates() {
        let monitors = [MonitorRect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert_eq!(
            clamp_orb_position(
                tauri::PhysicalPosition::new(-1800, 100),
                tauri::PhysicalSize::new(56, 56),
                &monitors,
            ),
            tauri::PhysicalPosition::new(-1800, 100)
        );
    }

    #[test]
    fn clamp_keeps_an_edge_visible() {
        let monitors = [MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert_eq!(
            clamp_orb_position(
                tauri::PhysicalPosition::new(3000, 2000),
                tauri::PhysicalSize::new(56, 56),
                &monitors,
            ),
            tauri::PhysicalPosition::new(1904, 1064)
        );
    }

    #[test]
    fn disconnected_monitor_position_falls_back_to_primary_default() {
        let monitors = [MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert_eq!(
            resolve_idle_position(
                Some(tauri::PhysicalPosition::new(-1700, 240)),
                tauri::PhysicalPosition::new(1840, 512),
                tauri::PhysicalSize::new(56, 56),
                &monitors,
            ),
            tauri::PhysicalPosition::new(1840, 512)
        );
    }
}

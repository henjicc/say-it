use crate::prelude::*;
use crate::state::{
    FloatingOrbGlassMaterial, FloatingOrbPosition, FloatingOrbPostInjectionAction,
    FloatingOrbSettings, MouseGestureMode, RuntimeState,
};
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings as EnigoSettings};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const FLOATING_ORB_LABEL: &str = "floating-orb";
pub(crate) const FLOATING_ORB_MENU_LABEL: &str = "floating-orb-menu";
const DEFAULT_MARGIN: f64 = 24.0;
const MIN_VISIBLE_EDGE: i32 = 16;
const ORB_SIZE_MIN: u16 = 44;
const ORB_SIZE_MAX: u16 = 72;
const ORB_OPACITY_MIN: u8 = 40;
const ORB_OPACITY_MAX: u8 = 100;
const ORB_GLASS_TINT_MAX: u8 = 40;
const ORB_GLASS_BORDER_MAX: u8 = 30;
const ORB_MENU_WIDTH: f64 = 280.0;
const ORB_MENU_HEIGHT: f64 = 318.0;
const ORB_MENU_GAP: f64 = 8.0;
const ORB_MAIN_REOPEN_SUPPRESSION_MS: u64 = 1500;

fn normalized_orb_size(size: u16) -> u16 {
    size.clamp(ORB_SIZE_MIN, ORB_SIZE_MAX)
}

fn normalized_orb_opacity(opacity: u8) -> u8 {
    opacity.clamp(ORB_OPACITY_MIN, ORB_OPACITY_MAX)
}

#[cfg(any(windows, test))]
fn parse_hex_rgb(value: Option<&str>) -> Option<(u8, u8, u8)> {
    let value = value?.trim();
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&value[0..2], 16).ok()?,
        u8::from_str_radix(&value[2..4], 16).ok()?,
        u8::from_str_radix(&value[4..6], 16).ok()?,
    ))
}

#[cfg(any(windows, test))]
fn mix_rgb(from: (u8, u8, u8), to: (u8, u8, u8), amount: f32) -> (u8, u8, u8) {
    let mix = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * amount).round() as u8;
    (mix(from.0, to.0), mix(from.1, to.1), mix(from.2, to.2))
}

#[cfg(any(windows, test))]
fn theme_background_rgb(value: &serde_json::Value) -> (u8, u8, u8) {
    let custom = value.get("backgroundMode").and_then(Value::as_str) == Some("custom");
    if custom {
        return parse_hex_rgb(value.get("background").and_then(Value::as_str))
            .unwrap_or((10, 14, 22));
    }
    let accent =
        parse_hex_rgb(value.get("accent").and_then(Value::as_str)).unwrap_or((81, 153, 255));
    let target = if value.get("tone").and_then(Value::as_str) == Some("light") {
        (244, 247, 251)
    } else {
        (7, 10, 16)
    };
    mix_rgb(accent, target, 0.98)
}

#[cfg(any(windows, test))]
fn theme_glass_tint_rgb(value: &serde_json::Value) -> (u8, u8, u8) {
    let accent =
        parse_hex_rgb(value.get("accent").and_then(Value::as_str)).unwrap_or((81, 153, 255));
    let light = value.get("tone").and_then(Value::as_str) == Some("light");
    let base = if value.get("backgroundMode").and_then(Value::as_str) == Some("custom") {
        theme_background_rgb(value)
    } else if light {
        (248, 250, 252)
    } else {
        (5, 7, 11)
    };
    mix_rgb(accent, base, if light { 0.98 } else { 0.97 })
}

#[cfg(windows)]
fn current_theme_glass_tint_rgb(app: &tauri::AppHandle) -> (u8, u8, u8) {
    app.state::<RuntimeState>()
        .app_settings
        .lock()
        .map(|settings| theme_glass_tint_rgb(&settings.theme))
        .unwrap_or((10, 14, 22))
}

fn orb_window_extent(size: u16) -> f64 {
    normalized_orb_size(size) as f64
}

fn current_settings(app: &tauri::AppHandle) -> FloatingOrbSettings {
    app.state::<RuntimeState>()
        .floating_orb
        .lock()
        .map(|settings| {
            let mut settings = settings.clone();
            settings.size = normalized_orb_size(settings.size);
            settings.opacity = normalized_orb_opacity(settings.opacity);
            settings.glass_tint = settings.glass_tint.min(ORB_GLASS_TINT_MAX);
            settings.glass_border = settings.glass_border.min(ORB_GLASS_BORDER_MAX);
            settings
        })
        .unwrap_or_default()
}

fn apply_native_glass(
    window: &tauri::WebviewWindow,
    enabled: bool,
    radius: f64,
    material: FloatingOrbGlassMaterial,
    tint: u8,
) {
    #[cfg(target_os = "macos")]
    {
        let _ = tint;
        let _ = window_vibrancy::clear_vibrancy(window);
        if enabled {
            let material = if window.label() == "main" {
                // 主窗口需要的是整个窗口背后的系统模糊，侧栏材质用于整窗时会产生
                // 过强的乳白/灰色覆盖，看起来更像普通半透明层。
                window_vibrancy::NSVisualEffectMaterial::UnderWindowBackground
            } else {
                match material {
                    FloatingOrbGlassMaterial::UnderWindow => {
                        window_vibrancy::NSVisualEffectMaterial::UnderWindowBackground
                    }
                    FloatingOrbGlassMaterial::Content => {
                        window_vibrancy::NSVisualEffectMaterial::ContentBackground
                    }
                    FloatingOrbGlassMaterial::Sidebar => {
                        window_vibrancy::NSVisualEffectMaterial::Sidebar
                    }
                }
            };
            if let Err(error) = window_vibrancy::apply_vibrancy(
                window,
                material,
                Some(window_vibrancy::NSVisualEffectState::Active),
                Some(radius),
            ) {
                eprintln!("[floating-orb] macOS 毛玻璃不可用: {error}");
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = material;
        let _ = window_vibrancy::clear_blur(window);
        let _ = window_vibrancy::clear_acrylic(window);
        let _ = window_vibrancy::clear_mica(window);
        if enabled {
            let tint_alpha = ((tint.min(ORB_GLASS_TINT_MAX) as u16 * 255) / 100) as u8;
            let (red, green, blue) = current_theme_glass_tint_rgb(window.app_handle());
            if let Err(error) =
                window_vibrancy::apply_acrylic(window, Some((red, green, blue, tint_alpha)))
            {
                eprintln!("[floating-orb] Windows Acrylic 不可用: {error}");
            }
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (window, enabled, radius, material, tint);
}

fn native_glass_radius(window: &tauri::WebviewWindow, settings: &FloatingOrbSettings) -> f64 {
    match window.label() {
        FLOATING_ORB_LABEL => orb_window_extent(settings.size) / 2.0,
        FLOATING_ORB_MENU_LABEL => 14.0,
        "dictation-indicator" | "assistant-answer" => 16.0,
        _ => 0.0,
    }
}

pub(crate) fn sync_system_glass_window(window: &tauri::WebviewWindow) {
    let settings = current_settings(window.app_handle());
    apply_native_glass(
        window,
        settings.glass_enabled,
        native_glass_radius(window, &settings),
        settings.glass_material,
        settings.glass_tint,
    );
}

pub(crate) fn sync_system_glass_windows(app: &tauri::AppHandle) {
    for label in [
        "main",
        FLOATING_ORB_LABEL,
        FLOATING_ORB_MENU_LABEL,
        "dictation-indicator",
        "assistant-answer",
    ] {
        if let Some(window) = app.get_webview_window(label) {
            sync_system_glass_window(&window);
        }
    }
}

fn native_glass_appearance_changed(
    previous: &FloatingOrbSettings,
    current: &FloatingOrbSettings,
) -> bool {
    previous.glass_enabled != current.glass_enabled
        || (current.glass_enabled
            && ((cfg!(target_os = "macos") && previous.glass_material != current.glass_material)
                || (cfg!(windows) && previous.glass_tint != current.glass_tint)))
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

fn resolve_menu_position(
    orb_position: tauri::PhysicalPosition<i32>,
    orb_size: tauri::PhysicalSize<u32>,
    menu_size: tauri::PhysicalSize<u32>,
    gap: i32,
    monitors: &[MonitorRect],
) -> tauri::PhysicalPosition<i32> {
    let Some(monitor) = monitor_for_window(orb_position, orb_size, monitors) else {
        return tauri::PhysicalPosition::new(
            orb_position
                .x
                .saturating_add_unsigned(orb_size.width)
                .saturating_add(gap),
            orb_position.y,
        );
    };
    let monitor_right = monitor.x.saturating_add_unsigned(monitor.width);
    let monitor_bottom = monitor.y.saturating_add_unsigned(monitor.height);
    let right_x = orb_position
        .x
        .saturating_add_unsigned(orb_size.width)
        .saturating_add(gap);
    let left_x = orb_position
        .x
        .saturating_sub(menu_size.width as i32)
        .saturating_sub(gap);
    let preferred_x = if right_x.saturating_add_unsigned(menu_size.width) <= monitor_right {
        right_x
    } else {
        left_x
    };
    let max_x = monitor_right
        .saturating_sub(menu_size.width as i32)
        .max(monitor.x);
    let centered_y = orb_position
        .y
        .saturating_add((orb_size.height / 2) as i32)
        .saturating_sub((menu_size.height / 2) as i32);
    let max_y = monitor_bottom
        .saturating_sub(menu_size.height as i32)
        .max(monitor.y);
    tauri::PhysicalPosition::new(
        preferred_x.clamp(monitor.x, max_x),
        centered_y.clamp(monitor.y, max_y),
    )
}

fn point_is_inside_window(
    point: tauri::PhysicalPosition<f64>,
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
) -> bool {
    point.x >= position.x as f64
        && point.y >= position.y as f64
        && point.x < position.x as f64 + size.width as f64
        && point.y < position.y as f64 + size.height as f64
}

fn is_cursor_over_floating_orb(app: &tauri::AppHandle) -> bool {
    let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) else {
        return false;
    };
    #[cfg(target_os = "macos")]
    if window
        .ns_window()
        .ok()
        .is_some_and(crate::macos_native::floating_orb_owns_pointer_event)
    {
        return true;
    }
    let (Ok(cursor), Ok(position), Ok(size)) = (
        app.cursor_position(),
        window.outer_position(),
        window.outer_size(),
    ) else {
        return false;
    };
    point_is_inside_window(cursor, position, size)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn reopen_suppression_is_active(deadline_ms: u64, current_ms: u64) -> bool {
    deadline_ms > current_ms
}

pub(crate) fn mark_floating_orb_interaction(app: &tauri::AppHandle) {
    app.state::<RuntimeState>()
        .floating_orb_runtime
        .suppress_main_reopen_until_ms
        .store(
            now_millis().saturating_add(ORB_MAIN_REOPEN_SUPPRESSION_MS),
            Ordering::Release,
        );
}

pub(crate) fn should_suppress_main_reopen(app: &tauri::AppHandle) -> bool {
    if is_cursor_over_floating_orb(app) {
        return true;
    }
    let deadline_ms = app
        .state::<RuntimeState>()
        .floating_orb_runtime
        .suppress_main_reopen_until_ms
        .load(Ordering::Acquire);
    reopen_suppression_is_active(deadline_ms, now_millis())
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
    .theme(Some(current_window_theme(app)))
    .focusable(false)
    .focused(false)
    .visible(false)
    .shadow(false)
    .transparent(true)
    .visible_on_all_workspaces(true)
    .build()
    .map_err(|error| format!("创建悬浮球窗口失败：{error}"))?;
    #[cfg(target_os = "macos")]
    if let Err(error) = window
        .ns_window()
        .map_err(|error| format!("读取 macOS 悬浮球窗口失败：{error}"))
        .and_then(|window| crate::macos_native::configure_floating_orb_window(window, true))
    {
        let _ = window.destroy();
        return Err(error);
    }
    apply_native_glass(
        &window,
        settings.glass_enabled,
        extent / 2.0,
        settings.glass_material,
        settings.glass_tint,
    );
    let position = resolved_idle_position(&window);
    window
        .set_position(position)
        .map_err(|error| format!("定位悬浮球失败：{error}"))?;
    if !app
        .state::<RuntimeState>()
        .floating_orb_runtime
        .transient
        .load(Ordering::Acquire)
    {
        window
            .show()
            .map_err(|error| format!("显示悬浮球失败：{error}"))?;
    }
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
    let payload = json!({
        "size": normalized_orb_size(settings.size),
        "opacity": normalized_orb_opacity(settings.opacity),
        "glassEnabled": settings.glass_enabled,
        "glassMaterial": settings.glass_material,
        "glassTint": settings.glass_tint.min(ORB_GLASS_TINT_MAX),
        "glassBorder": settings.glass_border.min(ORB_GLASS_BORDER_MAX),
    });
    for label in [FLOATING_ORB_LABEL, FLOATING_ORB_MENU_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.emit("floating-orb-config", payload.clone());
        }
    }
}

fn apply_floating_orb_config(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let settings = current_settings(app);
    resize_floating_orb_window(window, settings.size)?;
    apply_native_glass(
        window,
        settings.glass_enabled,
        orb_window_extent(settings.size) / 2.0,
        settings.glass_material,
        settings.glass_tint,
    );
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
    } else {
        if let Some(menu) = app.get_webview_window(FLOATING_ORB_MENU_LABEL) {
            let _ = menu.destroy();
        }
        if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
            window
                .destroy()
                .map_err(|error| format!("关闭悬浮球失败：{error}"))?;
        }
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

#[tauri::command]
pub(crate) fn set_floating_orb_appearance(
    app: tauri::AppHandle,
    size: u16,
    opacity: u8,
    glass_enabled: bool,
    glass_material: FloatingOrbGlassMaterial,
    glass_tint: u8,
    glass_border: u8,
) -> Result<FloatingOrbSettings, String> {
    let state = app.state::<RuntimeState>();
    let previous = {
        let mut settings = state
            .floating_orb
            .lock()
            .map_err(|_| "悬浮球配置锁失败".to_string())?;
        let previous = settings.clone();
        settings.size = normalized_orb_size(size);
        settings.opacity = normalized_orb_opacity(opacity);
        settings.glass_enabled = glass_enabled;
        settings.glass_material = glass_material;
        settings.glass_tint = glass_tint.min(ORB_GLASS_TINT_MAX);
        settings.glass_border = glass_border.min(ORB_GLASS_BORDER_MAX);
        previous
    };
    let current = current_settings(&app);
    let result = if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        resize_floating_orb_window(&window, current.size).map(|_| {
            if native_glass_appearance_changed(&previous, &current)
                || (current.glass_enabled && previous.size != current.size)
            {
                apply_native_glass(
                    &window,
                    current.glass_enabled,
                    orb_window_extent(current.size) / 2.0,
                    current.glass_material,
                    current.glass_tint,
                );
            }
        })
    } else {
        Ok(())
    };
    if let Err(error) = result {
        if let Ok(mut settings) = state.floating_orb.lock() {
            *settings = previous.clone();
        }
        if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
            let _ = apply_floating_orb_config(&app, &window);
        }
        return Err(error);
    }
    if native_glass_appearance_changed(&previous, &current) {
        sync_system_glass_windows(&app);
    }
    emit_config(&app, &current);
    crate::application::contract::next_revision(&state.snapshot_revision);
    schedule_persist_floating_orb_appearance(app);
    Ok(current)
}

fn ensure_floating_orb_menu_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_MENU_LABEL) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(
        app,
        FLOATING_ORB_MENU_LABEL,
        WebviewUrl::App("floating-orb-menu.html".into()),
    )
    .title("悬浮球设置")
    .inner_size(ORB_MENU_WIDTH, ORB_MENU_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .theme(Some(current_window_theme(app)))
    .focusable(true)
    .focused(false)
    .visible(false)
    .shadow(true)
    .transparent(true)
    .visible_on_all_workspaces(true)
    .build()
    .map_err(|error| format!("创建悬浮球设置面板失败：{error}"))?;
    #[cfg(target_os = "macos")]
    if let Err(error) = window
        .ns_window()
        .map_err(|error| format!("读取 macOS 悬浮球设置窗口失败：{error}"))
        .and_then(|window| crate::macos_native::configure_floating_orb_window(window, false))
    {
        let _ = window.destroy();
        return Err(error);
    }
    let settings = current_settings(app);
    apply_native_glass(
        &window,
        settings.glass_enabled,
        14.0,
        settings.glass_material,
        settings.glass_tint,
    );
    Ok(window)
}

fn window_theme(value: &serde_json::Value) -> tauri::Theme {
    if value.get("tone").and_then(serde_json::Value::as_str) == Some("light") {
        tauri::Theme::Light
    } else {
        tauri::Theme::Dark
    }
}

fn current_window_theme(app: &tauri::AppHandle) -> tauri::Theme {
    app.state::<RuntimeState>()
        .app_settings
        .lock()
        .map(|settings| window_theme(&settings.theme))
        .unwrap_or(tauri::Theme::Dark)
}

pub(crate) fn sync_floating_orb_theme(app: &tauri::AppHandle, value: &serde_json::Value) {
    let theme = window_theme(value);
    for label in [
        "main",
        FLOATING_ORB_LABEL,
        FLOATING_ORB_MENU_LABEL,
        "dictation-indicator",
        "assistant-answer",
    ] {
        if let Some(window) = app.get_webview_window(label) {
            if let Err(error) = window.set_theme(Some(theme)) {
                eprintln!("[theme] 同步窗口主题失败: {error}");
            }
        }
    }
    sync_system_glass_windows(app);
}

fn resize_and_position_floating_orb_menu(
    app: &tauri::AppHandle,
    menu: &tauri::WebviewWindow,
) -> Result<(), String> {
    let orb = app
        .get_webview_window(FLOATING_ORB_LABEL)
        .ok_or_else(|| "悬浮球窗口不存在".to_string())?;
    menu.set_size(tauri::LogicalSize::new(ORB_MENU_WIDTH, ORB_MENU_HEIGHT))
        .map_err(|error| format!("调整悬浮球设置面板尺寸失败：{error}"))?;
    let scale = orb.scale_factor().unwrap_or(1.0);
    let menu_size = tauri::PhysicalSize::new(
        (ORB_MENU_WIDTH * scale).round() as u32,
        (ORB_MENU_HEIGHT * scale).round() as u32,
    );
    let position = resolve_menu_position(
        orb.outer_position()
            .map_err(|error| format!("读取悬浮球位置失败：{error}"))?,
        orb.outer_size()
            .map_err(|error| format!("读取悬浮球尺寸失败：{error}"))?,
        menu_size,
        (ORB_MENU_GAP * scale).round() as i32,
        &monitor_rects(&orb),
    );
    menu.set_position(position)
        .map_err(|error| format!("定位悬浮球设置面板失败：{error}"))
}

#[tauri::command]
pub(crate) fn show_floating_orb_menu(app: tauri::AppHandle) -> Result<(), String> {
    let menu = ensure_floating_orb_menu_window(&app)?;
    let settings = current_settings(&app);
    apply_native_glass(
        &menu,
        settings.glass_enabled,
        14.0,
        settings.glass_material,
        settings.glass_tint,
    );
    resize_and_position_floating_orb_menu(&app, &menu)?;
    menu.show()
        .map_err(|error| format!("显示悬浮球设置面板失败：{error}"))?;
    menu.set_focus()
        .map_err(|error| format!("激活悬浮球设置面板失败：{error}"))?;
    emit_config(&app, &settings);
    Ok(())
}

#[tauri::command]
pub(crate) fn hide_floating_orb_menu(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(menu) = app.get_webview_window(FLOATING_ORB_MENU_LABEL) {
        menu.hide()
            .map_err(|error| format!("隐藏悬浮球设置面板失败：{error}"))?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn floating_orb_open_main_window(app: tauri::AppHandle) -> Result<(), String> {
    hide_floating_orb_menu(app.clone())?;
    crate::desktop::ensure_main_window(&app)
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
    mark_floating_orb_interaction(&app);
    let state = app.state::<RuntimeState>();
    if state
        .floating_orb_runtime
        .transient
        .load(Ordering::Acquire)
    {
        return;
    }
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

#[tauri::command]
pub(crate) fn floating_orb_start_dragging(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(FLOATING_ORB_LABEL)
        .ok_or_else(|| "悬浮球窗口不存在".to_string())?;
    mark_floating_orb_interaction(&app);
    let result = window
        .start_dragging()
        .map_err(|error| format!("拖动悬浮球失败：{error}"));
    // macOS 的原生拖拽可能阻塞到鼠标松开；返回后重新计时，覆盖随后才派发的 Reopen。
    mark_floating_orb_interaction(&app);
    result
}

fn schedule_persist_floating_orb_appearance(app: tauri::AppHandle) {
    let state = app.state::<RuntimeState>();
    let generation = state
        .floating_orb_runtime
        .appearance_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(250)).await;
        let state = app.state::<RuntimeState>();
        if state
            .floating_orb_runtime
            .appearance_generation
            .load(Ordering::Acquire)
            == generation
        {
            if let Err(error) = crate::persistence::save_persisted_state(&app, &state) {
                eprintln!("[floating-orb] 保存悬浮球外观失败: {error}");
            }
        }
    });
}

fn emit_state(app: &tauri::AppHandle, phase: &str, message: Option<&str>) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let runtime = &app.state::<RuntimeState>().floating_orb_runtime;
        let can_submit = runtime
            .post_injection_action
            .lock()
            .ok()
            .and_then(|action| *action)
            .is_some_and(|action| submit_enter_is_available(action.expires_at, Instant::now()));
        let _ = window.emit(
            "floating-orb-state",
            json!({
                "phase": phase,
                "message": message,
                "transient": runtime.transient.load(Ordering::Acquire),
                "canSubmit": can_submit,
            }),
        );
    }
}

fn submit_enter_is_available(expires_at: Instant, now: Instant) -> bool {
    expires_at > now
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

async fn focused_editable_target() -> Option<crate::active_app_context::ActivationTarget> {
    let target = crate::active_app_context::activation_target()?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        crate::active_app_context::focused_target_is_editable(target)
    });
    match tokio::time::timeout(Duration::from_millis(180), task).await {
        Ok(Ok(Ok(true))) => Some(target),
        _ => None,
    }
}

fn should_forward_orb_click(
    focused_editable_target: Option<crate::active_app_context::ActivationTarget>,
) -> bool {
    focused_editable_target.is_none()
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
    if let Ok(mut action) = state.floating_orb_runtime.post_injection_action.lock() {
        *action = None;
    }
    state
        .floating_orb_runtime
        .armed
        .store(false, Ordering::Release);
    if let Ok(mut target) = state.floating_orb_runtime.armed_target.lock() {
        *target = None;
    }
    if state
        .floating_orb_runtime
        .transient
        .swap(false, Ordering::AcqRel)
    {
        let enabled = state
            .floating_orb
            .lock()
            .map(|settings| settings.enabled)
            .unwrap_or(false);
        if enabled {
            let _ = window.set_position(resolved_idle_position(&window));
            let _ = window.set_ignore_cursor_events(false);
            let _ = window.show();
            emit_state(&app, "idle", None);
        } else {
            let _ = window.hide();
        }
    } else {
        let _ = window.set_ignore_cursor_events(false);
        emit_state(&app, "idle", None);
    }
}

pub(crate) fn complete_floating_orb(
    app: tauri::AppHandle,
    phase: &'static str,
    message: String,
    delay_ms: u64,
) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let can_submit = phase == "success"
            && app
                .state::<RuntimeState>()
                .floating_orb_runtime
                .post_injection_action
                .lock()
                .ok()
                .and_then(|action| *action)
                .is_some_and(|action| submit_enter_is_available(action.expires_at, Instant::now()));
        let _ = window.set_ignore_cursor_events(!can_submit);
    }
    tauri::async_runtime::spawn(return_to_idle(app, delay_ms, phase, message));
}

pub(crate) fn set_floating_orb_phase(app: &tauri::AppHandle, phase: &str, message: &str) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let interactive = phase == "recording" || phase == "armed";
        let _ = window.set_ignore_cursor_events(!interactive);
        emit_state(app, phase, Some(message));
    }
}

pub(crate) fn arm_floating_orb_submit_enter(
    app: &tauri::AppHandle,
    target: crate::active_app_context::ActivationTarget,
) {
    let expires_at = Instant::now() + Duration::from_millis(1000);
    if let Ok(mut action) = app
        .state::<RuntimeState>()
        .floating_orb_runtime
        .post_injection_action
        .lock()
    {
        *action = Some(FloatingOrbPostInjectionAction {
            target,
            expires_at,
        });
    }
    let timeout_app = app.clone();
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(1000)).await;
        let expired = timeout_app
            .state::<RuntimeState>()
            .floating_orb_runtime
            .post_injection_action
            .lock()
            .map(|mut action| {
                if action.is_some_and(|value| value.expires_at == expires_at) {
                    *action = None;
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if expired {
            if let Some(window) = timeout_app.get_webview_window(FLOATING_ORB_LABEL) {
                let _ = window.set_ignore_cursor_events(true);
            }
            emit_state(&timeout_app, "success", Some("已完成并复制"));
        }
    });
}

fn transient_orb_position(
    window: &tauri::WebviewWindow,
    fallback: (i32, i32),
) -> tauri::PhysicalPosition<i32> {
    let cursor = window
        .app_handle()
        .cursor_position()
        .ok()
        .map(|value| (value.x.round() as i32, value.y.round() as i32))
        .unwrap_or(fallback);
    let size = window.outer_size().unwrap_or_else(|_| {
        let extent = current_settings(window.app_handle()).size as u32;
        tauri::PhysicalSize::new(extent, extent)
    });
    clamp_orb_position(
        tauri::PhysicalPosition::new(
            cursor.0.saturating_sub((size.width / 2) as i32),
            cursor.1.saturating_sub((size.height / 2) as i32),
        ),
        size,
        &monitor_rects(window),
    )
}

async fn prepare_transient_orb(
    app: &tauri::AppHandle,
    position: (i32, i32),
) -> Result<tauri::WebviewWindow, String> {
    let state = app.state::<RuntimeState>();
    state
        .floating_orb_runtime
        .transient
        .store(true, Ordering::Release);
    state
        .floating_orb_runtime
        .transition_generation
        .fetch_add(1, Ordering::AcqRel);
    if let Ok(mut action) = state.floating_orb_runtime.post_injection_action.lock() {
        *action = None;
    }
    let window = ensure_floating_orb_window(app)?;
    let _ = window.hide();
    window
        .set_position(transient_orb_position(&window, position))
        .map_err(|error| format!("定位手势悬浮球失败：{error}"))?;
    window
        .show()
        .map_err(|error| format!("显示手势悬浮球失败：{error}"))?;
    Ok(window)
}

async fn start_mouse_gesture_dictation(
    app: tauri::AppHandle,
    target: Option<crate::active_app_context::ActivationTarget>,
) -> Result<(), String> {
    let target_confirmed = target.is_some();
    app.state::<RuntimeState>()
        .floating_orb_runtime
        .armed
        .store(false, Ordering::Release);
    set_floating_orb_phase(&app, "recording", "聆听中…");
    crate::application::dictation::start_from_mouse_gesture(app, target, target_confirmed).await
}

async fn show_mouse_gesture_armed(
    app: tauri::AppHandle,
    position: (i32, i32),
) -> Result<(), String> {
    if app.state::<RuntimeState>().audio_session.is_busy()
        || crate::application::dictation::is_active(&app)
    {
        prepare_transient_orb(&app, position).await?;
        complete_floating_orb(app, "busy", "当前有其他音频任务".into(), 1500);
        return Ok(());
    }
    let target = focused_editable_target().await;
    let window = prepare_transient_orb(&app, position).await?;
    let state = app.state::<RuntimeState>();
    state
        .floating_orb_runtime
        .armed
        .store(true, Ordering::Release);
    if let Ok(mut current) = state.floating_orb_runtime.armed_target.lock() {
        *current = target;
    }
    let generation = state
        .floating_orb_runtime
        .armed_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    let _ = window.set_ignore_cursor_events(false);
    emit_state(&app, "armed", Some("点击开始语音输入"));
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(3000)).await;
        let state = app.state::<RuntimeState>();
        if state
            .floating_orb_runtime
            .armed_generation
            .load(Ordering::Acquire)
            == generation
            && state
                .floating_orb_runtime
                .armed
                .swap(false, Ordering::AcqRel)
        {
            complete_floating_orb(app, "idle", String::new(), 0);
        }
    });
    Ok(())
}

async fn handle_mouse_gesture(
    app: tauri::AppHandle,
    position: (i32, i32),
    mode: MouseGestureMode,
) -> Result<(), String> {
    if app
        .state::<RuntimeState>()
        .floating_orb_runtime
        .post_injection_action
        .lock()
        .ok()
        .and_then(|action| *action)
        .is_some_and(|action| submit_enter_is_available(action.expires_at, Instant::now()))
    {
        return Ok(());
    }
    if app
        .state::<RuntimeState>()
        .floating_orb_runtime
        .armed
        .load(Ordering::Acquire)
    {
        return Ok(());
    }
    if mode == MouseGestureMode::Direct
        && crate::application::dictation::is_mouse_gesture_recording(&app)
    {
        return floating_orb_stop(app).await;
    }
    // 录音中的确认模式以及任意处理阶段都忽略手势，避免临时球覆盖当前状态。
    if crate::application::dictation::is_active(&app) {
        return Ok(());
    }
    if app.state::<RuntimeState>().audio_session.is_busy() {
        prepare_transient_orb(&app, position).await?;
        complete_floating_orb(app, "busy", "当前有其他音频任务".into(), 1500);
        return Ok(());
    }
    if mode == MouseGestureMode::Confirm {
        return show_mouse_gesture_armed(app, position).await;
    }
    let target = focused_editable_target().await;
    prepare_transient_orb(&app, position).await?;
    start_mouse_gesture_dictation(app, target).await
}

pub(crate) fn request_mouse_gesture(
    app: tauri::AppHandle,
    position: (i32, i32),
    mode: MouseGestureMode,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = handle_mouse_gesture(app.clone(), position, mode).await {
            complete_floating_orb(app, "error", error, 3000);
        }
    });
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
    let runtime = &app.state::<RuntimeState>().floating_orb_runtime;
    if runtime.armed.swap(false, Ordering::AcqRel) {
        runtime.armed_generation.fetch_add(1, Ordering::AcqRel);
        let armed_target = runtime
            .armed_target
            .lock()
            .map_err(|_| "手势目标状态锁失败".to_string())?
            .take();
        let current = focused_editable_target().await;
        let target = match (armed_target, current) {
            (Some(armed), Some(current))
                if crate::active_app_context::same_activation_target(armed, current) =>
            {
                Some(armed)
            }
            _ => None,
        };
        return start_mouse_gesture_dictation(app, target).await;
    }
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
    app.state::<RuntimeState>()
        .floating_orb_runtime
        .transition_generation
        .fetch_add(1, Ordering::AcqRel);
    let _ = window.set_ignore_cursor_events(true);
    emit_state(&app, "moving", None);
    let already_focused = focused_editable_target().await;
    let (target, target_confirmed) = if should_forward_orb_click(already_focused) {
        let click_position = cursor_location().await;
        sleep(Duration::from_millis(16)).await;
        let forwarded = match click_position {
            Some(position) => forward_click(position).await.is_ok(),
            None => false,
        };
        sleep(Duration::from_millis(80)).await;
        let target = forwarded
            .then(crate::active_app_context::activation_target)
            .flatten();
        (target, forwarded && target.is_some())
    } else {
        (already_focused, true)
    };
    emit_state(&app, "recording", Some("聆听中…"));
    let _ = window.set_ignore_cursor_events(false);
    if let Err(error) = crate::application::dictation::start_from_floating_orb(
        app.clone(),
        target,
        target_confirmed,
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

async fn send_return_key(target: crate::active_app_context::ActivationTarget) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            crate::macos_native::press_return(target.process_id)
        }
        #[cfg(not(target_os = "macos"))]
        {
            use enigo::{Direction, Enigo, Key, Keyboard, Settings};
            let _ = target;
            let mut enigo = Enigo::new(&Settings::default())
                .map_err(|error| format!("初始化键盘模拟失败：{error}"))?;
            enigo
                .key(Key::Return, Direction::Click)
                .map_err(|error| format!("模拟回车失败：{error}"))
        }
    })
    .await
    .map_err(|error| format!("模拟回车任务失败：{error}"))?
}

fn validate_submit_enter_target(
    expected: crate::active_app_context::ActivationTarget,
    current: crate::active_app_context::ActivationTarget,
    sensitive: bool,
) -> Result<(), String> {
    if !crate::active_app_context::same_activation_target(current, expected) {
        return Err("当前输入目标已变化".into());
    }
    if sensitive {
        return Err("安全输入框不发送回车".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn floating_orb_submit_enter(app: tauri::AppHandle) -> Result<(), String> {
    let action = app
        .state::<RuntimeState>()
        .floating_orb_runtime
        .post_injection_action
        .lock()
        .map_err(|_| "快捷回车状态锁失败".to_string())?
        .take()
        .ok_or_else(|| "快捷回车窗口已结束".to_string())?;
    if !submit_enter_is_available(action.expires_at, Instant::now()) {
        return Err("快捷回车窗口已结束".into());
    }
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let _ = window.set_ignore_cursor_events(true);
    }
    emit_state(&app, "submitting", Some("正在发送回车…"));
    // 等待悬浮球的鼠标事件完全结束，再向仍持有焦点的目标发送按键。
    sleep(Duration::from_millis(40)).await;
    let result = async {
        let current = crate::active_app_context::activation_target()
            .ok_or_else(|| "当前输入目标已变化".to_string())?;
        // 文本刚刚已成功注入；这里不再依赖可编辑性探针。浏览器 contenteditable
        // 等控件会对辅助功能/UIA 报告不可编辑，继续检查会误拦截本可送达的回车。
        let sensitive = crate::active_app_context::target_is_sensitive(current)?;
        validate_submit_enter_target(action.target, current, sensitive)?;
        send_return_key(current).await
    }
    .await;
    match result {
        Ok(()) => {
            complete_floating_orb(app, "submitted", "已发送回车".into(), 800);
            Ok(())
        }
        Err(error) => {
            eprintln!("[floating-orb] 快捷回车未发送: {error}");
            complete_floating_orb(app, "error", "未发送回车".into(), 1500);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_values_are_clamped_to_slider_ranges() {
        assert_eq!(normalized_orb_size(48), 48);
        assert_eq!(normalized_orb_size(20), ORB_SIZE_MIN);
        assert_eq!(normalized_orb_size(90), ORB_SIZE_MAX);
        assert_eq!(normalized_orb_opacity(85), 85);
        assert_eq!(normalized_orb_opacity(20), ORB_OPACITY_MIN);
        assert_eq!(normalized_orb_opacity(120), ORB_OPACITY_MAX);
        assert_eq!(orb_window_extent(56), 56.0);
    }

    #[test]
    fn window_theme_follows_the_persisted_tone() {
        assert!(matches!(
            window_theme(&serde_json::json!({"tone": "light"})),
            tauri::Theme::Light
        ));
        assert!(matches!(
            window_theme(&serde_json::json!({"tone": "dark"})),
            tauri::Theme::Dark
        ));
    }

    #[test]
    fn background_tint_follows_accent_or_explicit_background() {
        assert_eq!(
            theme_background_rgb(&serde_json::json!({
                "tone": "dark",
                "accent": "#FF4013",
                "backgroundMode": "followAccent"
            })),
            (12, 11, 16)
        );
        assert_eq!(
            theme_background_rgb(&serde_json::json!({
                "backgroundMode": "custom",
                "background": "#221A18"
            })),
            (34, 26, 24)
        );
        assert_eq!(
            theme_glass_tint_rgb(&serde_json::json!({
                "tone": "dark",
                "accent": "#FF4013",
                "backgroundMode": "followAccent"
            })),
            (13, 9, 11)
        );
    }

    #[test]
    fn focused_editable_target_skips_forwarded_pointer_click() {
        let target = crate::active_app_context::ActivationTarget {
            window_handle: 42,
            process_id: 7,
            cursor_position: None,
        };
        assert!(!should_forward_orb_click(Some(target)));
        assert!(should_forward_orb_click(None));
    }

    #[test]
    fn menu_prefers_the_right_side_and_flips_at_the_screen_edge() {
        let monitors = [MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert_eq!(
            resolve_menu_position(
                tauri::PhysicalPosition::new(200, 400),
                tauri::PhysicalSize::new(64, 64),
                tauri::PhysicalSize::new(280, 286),
                8,
                &monitors,
            ),
            tauri::PhysicalPosition::new(272, 289)
        );
        assert_eq!(
            resolve_menu_position(
                tauri::PhysicalPosition::new(1840, 900),
                tauri::PhysicalSize::new(64, 64),
                tauri::PhysicalSize::new(280, 444),
                8,
                &monitors,
            ),
            tauri::PhysicalPosition::new(1552, 636)
        );
    }

    #[test]
    fn cursor_hit_test_uses_the_orb_window_bounds() {
        let position = tauri::PhysicalPosition::new(-120, 80);
        let size = tauri::PhysicalSize::new(48, 48);
        assert!(point_is_inside_window(
            tauri::PhysicalPosition::new(-100.0, 100.0),
            position,
            size,
        ));
        assert!(!point_is_inside_window(
            tauri::PhysicalPosition::new(-72.0, 100.0),
            position,
            size,
        ));
    }

    #[test]
    fn reopen_suppression_expires_after_the_drag_window() {
        assert!(reopen_suppression_is_active(2_500, 1_000));
        assert!(!reopen_suppression_is_active(2_500, 2_500));
        assert!(!reopen_suppression_is_active(2_500, 3_000));
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

    #[test]
    fn submit_enter_window_expires_at_the_deadline() {
        let now = Instant::now();
        assert!(submit_enter_is_available(
            now + Duration::from_millis(1000),
            now
        ));
        assert!(!submit_enter_is_available(now, now));
        assert!(!submit_enter_is_available(
            now,
            now + Duration::from_millis(1)
        ));
    }

    #[test]
    fn submit_enter_reuses_a_matching_non_sensitive_target() {
        let target = crate::active_app_context::ActivationTarget {
            window_handle: 42,
            process_id: 7,
            cursor_position: None,
        };
        assert!(validate_submit_enter_target(target, target, false).is_ok());
        assert!(validate_submit_enter_target(target, target, true).is_err());
        assert!(validate_submit_enter_target(
            target,
            crate::active_app_context::ActivationTarget {
                window_handle: 99,
                ..target
            },
            false,
        )
        .is_err());
    }
}

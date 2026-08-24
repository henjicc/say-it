use crate::prelude::*;
use crate::state::{FloatingOrbPosition, FloatingOrbSettings, RuntimeState};
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings as EnigoSettings};
use std::sync::atomic::Ordering;

pub(crate) const FLOATING_ORB_LABEL: &str = "floating-orb";
const ORB_LOGICAL_SIZE: f64 = 56.0;
const BUBBLE_LOGICAL_WIDTH: f64 = 172.0;
const BUBBLE_LOGICAL_HEIGHT: f64 = 56.0;
const DEFAULT_MARGIN: f64 = 24.0;
const BUBBLE_BOTTOM_MARGIN: f64 = 28.0;
const MIN_VISIBLE_EDGE: i32 = 16;
const ANIMATION_STEPS: u32 = 12;
const ANIMATION_FRAME_MS: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale_milli: u32,
}

impl MonitorRect {
    fn from_monitor(monitor: &tauri::Monitor) -> Self {
        let area = monitor.work_area();
        Self {
            x: area.position.x,
            y: area.position.y,
            width: area.size.width,
            height: area.size.height,
            scale_milli: (monitor.scale_factor() * 1000.0).round().max(1.0) as u32,
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

    fn scale(&self) -> f64 {
        self.scale_milli as f64 / 1000.0
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
        let visible_width = right.min(monitor_right).saturating_sub(position.x.max(monitor.x));
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
    let size = (ORB_LOGICAL_SIZE * scale).round() as i32;
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
    let size = tauri::PhysicalSize::new(
        (ORB_LOGICAL_SIZE * scale).round() as u32,
        (ORB_LOGICAL_SIZE * scale).round() as u32,
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
    let window = WebviewWindowBuilder::new(
        app,
        FLOATING_ORB_LABEL,
        WebviewUrl::App("floating-orb.html".into()),
    )
    .title("语音输入悬浮球")
    .inner_size(ORB_LOGICAL_SIZE, ORB_LOGICAL_SIZE)
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
    Ok(window)
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

fn persist_current_position(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) else {
        return Ok(());
    };
    if app
        .state::<RuntimeState>()
        .floating_orb_runtime
        .transient_position
        .load(Ordering::Acquire)
    {
        return Ok(());
    }
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
    if state
        .floating_orb_runtime
        .transient_position
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

fn emit_state(app: &tauri::AppHandle, phase: &str, message: Option<&str>) {
    if let Some(window) = app.get_webview_window(FLOATING_ORB_LABEL) {
        let _ = window.emit(
            "floating-orb-state",
            json!({ "phase": phase, "message": message }),
        );
    }
}

fn bottom_geometry(
    window: &tauri::WebviewWindow,
) -> (tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>) {
    let start = window
        .outer_position()
        .unwrap_or_else(|_| resolved_idle_position(window));
    let start_size = window.outer_size().unwrap_or(tauri::PhysicalSize::new(56, 56));
    let monitor = monitor_for_window(start, start_size, &monitor_rects(window));
    let Some(monitor) = monitor else {
        return (start, start_size);
    };
    let scale = monitor.scale();
    let width = (BUBBLE_LOGICAL_WIDTH * scale).round() as u32;
    let height = (BUBBLE_LOGICAL_HEIGHT * scale).round() as u32;
    let margin = (BUBBLE_BOTTOM_MARGIN * scale).round() as i32;
    (
        tauri::PhysicalPosition::new(
            monitor
                .x
                .saturating_add_unsigned(monitor.width / 2)
                .saturating_sub((width / 2) as i32),
            monitor
                .y
                .saturating_add_unsigned(monitor.height)
                .saturating_sub(height as i32)
                .saturating_sub(margin),
        ),
        tauri::PhysicalSize::new(width, height),
    )
}

async fn animate_geometry(
    window: tauri::WebviewWindow,
    target_position: tauri::PhysicalPosition<i32>,
    target_size: tauri::PhysicalSize<u32>,
    reduced_motion: bool,
) {
    let start_position = window.outer_position().unwrap_or(target_position);
    let start_size = window.outer_size().unwrap_or(target_size);
    let steps = if reduced_motion { 1 } else { ANIMATION_STEPS };
    for step in 1..=steps {
        let progress = step as f64 / steps as f64;
        let eased = 1.0 - (1.0 - progress).powi(3);
        let x = start_position.x as f64
            + (target_position.x - start_position.x) as f64 * eased;
        let y = start_position.y as f64
            + (target_position.y - start_position.y) as f64 * eased;
        let width = start_size.width as f64
            + (target_size.width as f64 - start_size.width as f64) * eased;
        let height = start_size.height as f64
            + (target_size.height as f64 - start_size.height as f64) * eased;
        let _ = window.set_position(tauri::PhysicalPosition::new(
            x.round() as i32,
            y.round() as i32,
        ));
        let _ = window.set_size(tauri::PhysicalSize::new(
            width.round() as u32,
            height.round() as u32,
        ));
        if step < steps {
            sleep(Duration::from_millis(ANIMATION_FRAME_MS)).await;
        }
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
    let idle_position = resolved_idle_position(&window);
    let scale = window.scale_factor().unwrap_or(1.0);
    let idle_size = tauri::PhysicalSize::new(
        (ORB_LOGICAL_SIZE * scale).round() as u32,
        (ORB_LOGICAL_SIZE * scale).round() as u32,
    );
    let reduced_motion = state
        .floating_orb_runtime
        .reduced_motion
        .load(Ordering::Acquire);
    animate_geometry(window.clone(), idle_position, idle_size, reduced_motion).await;
    let _ = window.set_ignore_cursor_events(false);
    state
        .floating_orb_runtime
        .transient_position
        .store(false, Ordering::Release);
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

#[tauri::command]
pub(crate) async fn floating_orb_activate(
    app: tauri::AppHandle,
    reduced_motion: Option<bool>,
) -> Result<(), String> {
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
    let state = app.state::<RuntimeState>();
    state
        .floating_orb_runtime
        .transient_position
        .store(true, Ordering::Release);
    state
        .floating_orb_runtime
        .reduced_motion
        .store(reduced_motion.unwrap_or(false), Ordering::Release);
    state
        .floating_orb_runtime
        .transition_generation
        .fetch_add(1, Ordering::AcqRel);
    let _ = window.set_ignore_cursor_events(true);
    emit_state(&app, "moving", None);
    let (target_position, target_size) = bottom_geometry(&window);
    let animation = animate_geometry(
        window.clone(),
        target_position,
        target_size,
        reduced_motion.unwrap_or(false),
    );
    let click = async {
        sleep(Duration::from_millis(16)).await;
        let forwarded = match click_position {
            Some(position) => forward_click(position).await.is_ok(),
            None => false,
        };
        sleep(Duration::from_millis(80)).await;
        let target = forwarded
            .then(crate::active_app_context::activation_target)
            .flatten();
        (forwarded, target)
    };
    let (_, (forwarded, target)) = tokio::join!(animation, click);
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
    fn clamp_preserves_negative_secondary_monitor_coordinates() {
        let monitors = [MonitorRect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale_milli: 1000,
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
            scale_milli: 1000,
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
            scale_milli: 1000,
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

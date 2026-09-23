use crate::prelude::*;
use crate::state::RuntimeState;

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
    if crate::desktop::native_dictation_indicator_enabled() {
        // 原生指示器接管听写展示；残留的错误态 WebView 窗口要一并收掉，
        // 否则上一轮的错误面板会盖在新一轮原生指示器旁边。
        hide_webview_indicator_if_present(app);
        crate::desktop::native_indicator_prepare();
        return Ok(());
    }
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

/// 原生指示器启用时，error/subtitle 仍走 WebView；结束后 WebView 窗口
/// 需要主动收掉，避免与原生窗口并存。
fn hide_webview_indicator_if_present(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(DICTATION_INDICATOR_LABEL) {
        let _ = window.emit("dictation-indicator-state", json!({ "state": "hidden" }));
        let _ = window.set_ignore_cursor_events(true);
        let _ = window.hide();
    }
}

/// 指示器共享通道（文本/翻译/状态）的当前拥有者：听写胶囊或实时字幕条。
/// 听写与字幕是两个独立原生窗口，但业务侧共用一个指示窗语义，文本/状态
/// 通道按 owner 路由到对应的原生窗口。WebView 路径下两者本就共享同一窗口，
/// 不需要 owner。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IndicatorOwner {
    Dictation,
    Subtitle,
}

static INDICATOR_OWNER: Mutex<Option<IndicatorOwner>> = Mutex::new(None);

fn indicator_owner() -> Option<IndicatorOwner> {
    INDICATOR_OWNER.lock().ok().and_then(|owner| *owner)
}

fn set_indicator_owner(owner: Option<IndicatorOwner>) {
    if let Ok(mut current) = INDICATOR_OWNER.lock() {
        *current = owner;
    }
}

/// set_indicator_state 的 owner 转移（纯函数，便于单测）。
/// error 不改变归属（错误面板是 WebView，字幕/听写会话仍在底层继续）。
fn owner_after_state(
    current: Option<IndicatorOwner>,
    state: &str,
    native_subtitle: bool,
) -> Option<IndicatorOwner> {
    match state {
        "subtitle" => native_subtitle.then_some(IndicatorOwner::Subtitle),
        "recording" | "processing" | "smartProcessing" | "fallback" => {
            Some(IndicatorOwner::Dictation)
        }
        "hidden" => None,
        _ => current,
    }
}

/// 新建 WebView 的就绪回调只补发当前回退字幕，不能抢占原生字幕或听写通道。
pub(crate) fn can_rehydrate_subtitle_webview() -> bool {
    !crate::desktop::native_subtitle::native_subtitle_enabled()
        && indicator_owner() != Some(IndicatorOwner::Dictation)
}

/// 共享文本通道是否路由给原生字幕窗。owner 为 None 且字幕会话仍在运行
/// （未被 OBS 接管）时，字幕的后续更新让字幕条重新出现——与 WebView 共享窗
/// 「听写临时接管、字幕文本恢复后回到字幕」的语义一致。
fn route_text_to_subtitle(app: &tauri::AppHandle) -> bool {
    if !crate::desktop::native_subtitle::native_subtitle_enabled() {
        return false;
    }
    match indicator_owner() {
        Some(IndicatorOwner::Dictation) => false,
        Some(IndicatorOwner::Subtitle) => true,
        None => {
            if crate::application::subtitles::wants_indicator_visible(
                &app.state::<RuntimeState>(),
            ) {
                set_indicator_owner(Some(IndicatorOwner::Subtitle));
                crate::desktop::native_subtitle::native_subtitle_show();
                true
            } else {
                false
            }
        }
    }
}

/// 切换指示器内容。state: "recording" | "processing" | "smartProcessing" | "fallback" | "subtitle" | "error" | "hidden"。
/// 显示态会重新提升到 topmost，但不激活窗口，避免抢走目标程序焦点。
#[tauri::command]
pub(crate) fn set_indicator_state(app: tauri::AppHandle, state: String) -> Result<(), String> {
    hotkey::set_dictation_active(
        state == "recording" || state == "processing" || state == "smartProcessing",
    );
    let native_subtitle = crate::desktop::native_subtitle::native_subtitle_enabled();
    set_indicator_owner(owner_after_state(indicator_owner(), &state, native_subtitle));
    // 字幕条由原生窗口接管：不再创建/触碰 WebView 指示窗。
    if state == "subtitle" && native_subtitle {
        crate::desktop::native_subtitle::native_subtitle_attach(&app);
        if crate::desktop::native_dictation_indicator_enabled() {
            crate::desktop::native_indicator_hide();
        }
        hide_webview_indicator_if_present(&app);
        crate::desktop::native_subtitle::native_subtitle_show();
        return Ok(());
    }
    if crate::desktop::native_dictation_indicator_enabled() {
        match state.as_str() {
            // 原生接管的听写状态，不再触碰 WebView 窗口。听写临时接管共享通道时
            // 字幕窗先藏起来；字幕会话未结束时，后续文本更新会重新显示它。
            "recording" | "processing" | "smartProcessing" | "fallback" => {
                crate::desktop::native_subtitle::native_subtitle_hide();
                crate::desktop::native_indicator_set_state(&state);
            }
            // 字幕显式回退及 error 仍由 WebView 展示，不能落入隐藏分支。
            "subtitle" | "error" => {
                crate::desktop::native_subtitle::native_subtitle_hide();
                crate::desktop::native_indicator_hide();
                return set_indicator_state_webview(&app, &state);
            }
            _ => {
                crate::desktop::native_subtitle::native_subtitle_hide();
                crate::desktop::native_indicator_hide();
                if let Some(window) = app.get_webview_window(DICTATION_INDICATOR_LABEL) {
                    let _ = window.emit("dictation-indicator-state", json!({ "state": state }));
                    let _ = window.set_ignore_cursor_events(true);
                    window
                        .hide()
                        .map_err(|error| format!("隐藏指示器窗口失败: {error}"))?;
                }
            }
        }
        return Ok(());
    }
    // 原生字幕开、原生胶囊关的组合：非字幕状态也要收掉字幕窗。
    if native_subtitle {
        crate::desktop::native_subtitle::native_subtitle_hide();
    }
    set_indicator_state_webview(&app, &state)
}

fn set_indicator_state_webview(app: &tauri::AppHandle, state: &str) -> Result<(), String> {
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
    let window = ensure_indicator_window(app)?;
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
    if crate::desktop::native_dictation_indicator_enabled() {
        // error 有操作按钮，原生指示器让位给 WebView。
        crate::desktop::native_indicator_hide();
    }
    // 原生模式下启动时不再预创建指示器 WebView，这里很可能是首次创建：
    // 前端脚本加载、事件监听注册需要时间，立即 emit 的状态会全部丢进虚空，
    // 错误面板挂载着却永远停在 hidden。新建时延迟重发一次（emit 幂等，
    // 已就绪的窗口重复应用同一状态无副作用）。
    let fresh = app.get_webview_window(DICTATION_INDICATOR_LABEL).is_none();
    let window = ensure_indicator_window(app)?;
    if fresh {
        let app = app.clone();
        let message = message.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let Some(window) = app.get_webview_window(DICTATION_INDICATOR_LABEL) else {
                return;
            };
            let _ = window.emit("dictation-indicator-config", json!({ "mode": "dictation" }));
            let _ = window.emit(
                "dictation-indicator-error",
                json!({ "message": message, "canUseRawText": can_use_raw_text }),
            );
            let _ = window.emit("dictation-indicator-state", json!({ "state": "error" }));
        });
    }
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

/// 原生模式下的错误/结果通知：原生面板显示短文案（哪步失败、已如何兜底），
/// 几秒后自动消失；不为可交互错误面板激活 WebView。
pub(crate) fn show_dictation_indicator_notice(
    app: &tauri::AppHandle,
    message: String,
) -> Result<(), String> {
    if crate::desktop::native_dictation_indicator_enabled() {
        hide_webview_indicator_if_present(app);
        crate::desktop::native_indicator_prepare();
        crate::desktop::native_indicator_notice(message);
        hotkey::set_dictation_active(false);
        return Ok(());
    }
    // macOS 等仍走 WebView 错误面板。
    show_dictation_indicator_error(app, message, false)
}

/// 开发构建专用的通知面板自检入口：直接从主窗口触发一条原生通知，
/// 用于验证渲染与自动消失，release 中不存在此命令。
#[cfg(debug_assertions)]
#[tauri::command]
pub(crate) fn dev_show_indicator_notice(app: tauri::AppHandle, text: String) -> Result<(), String> {
    show_dictation_indicator_notice(&app, text)
}

/// 开发构建专用的波形驱动入口：注入合成响度，验证波形/缩放动效。
#[cfg(debug_assertions)]
#[tauri::command]
pub(crate) fn dev_indicator_waveform(level: f32, fade: Option<bool>) -> Result<(), String> {
    let _ = fade;
    crate::desktop::native_indicator_set_waveform(level, vec![level; 6]);
    Ok(())
}

/// 文本已经生成、只是原输入窗口不可再注入时，改为明确的剪贴板交付提示。
/// 这不是识别或智能处理失败，因此不展示错误操作区，也不允许窗口抢焦点。
pub(crate) fn show_dictation_indicator_clipboard_fallback(
    app: &tauri::AppHandle,
) -> Result<(), String> {
    if crate::desktop::native_dictation_indicator_enabled() {
        hide_webview_indicator_if_present(app);
        crate::desktop::native_indicator_prepare();
        crate::desktop::native_indicator_set_state("fallback");
        hotkey::set_dictation_active(false);
        return Ok(());
    }
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
    let _ = window.set_ignore_cursor_events(true);
    raise_indicator_window(&window);
    let _ = window.emit("dictation-indicator-state", json!({ "state": "fallback" }));
    hotkey::set_dictation_active(false);
    Ok(())
}

#[tauri::command]
pub(crate) fn set_indicator_text(
    app: tauri::AppHandle,
    text: String,
    fade: Option<bool>,
) -> Result<(), String> {
    if route_text_to_subtitle(&app) {
        crate::desktop::native_subtitle::native_subtitle_set_text(
            text.clone(),
            fade.unwrap_or(false),
        );
    } else if crate::desktop::native_dictation_indicator_enabled() {
        crate::desktop::native_indicator_set_text(text.clone(), fade.unwrap_or(false));
    }
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
    // 译文只属于字幕条；听写态到达的译文（正常不会发生）丢弃。
    if route_text_to_subtitle(&app) {
        crate::desktop::native_subtitle::native_subtitle_set_translation(text.clone());
    }
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
    let width = width
        .unwrap_or(DEFAULT_INDICATOR_WIDTH)
        .clamp(160.0, 2400.0);
    let height = height
        .unwrap_or(DEFAULT_INDICATOR_HEIGHT)
        .clamp(56.0, 720.0);
    let anchor = anchor.unwrap_or_else(|| "bottom".to_string());
    let offset_y = offset_y.unwrap_or(36.0).clamp(-240.0, 240.0);
    if crate::desktop::native_subtitle::native_subtitle_enabled() {
        // 原生字幕窗自己管理几何；原生胶囊几何固定，本来就忽略它。
        // 这里不再连带创建 WebView 指示窗。
        crate::desktop::native_subtitle::native_subtitle_set_layout(
            width, height, &anchor, offset_y,
        );
        return Ok(());
    }
    let window = ensure_indicator_window(&app)?;
    place_indicator_window(&window, width, height, &anchor, offset_y);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{fallback_indicator_position, owner_after_state, IndicatorOwner};

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

    #[test]
    fn owner_follows_state_transitions() {
        // 字幕状态只在原生字幕启用时接管 owner。
        assert_eq!(
            owner_after_state(None, "subtitle", true),
            Some(IndicatorOwner::Subtitle)
        );
        assert_eq!(owner_after_state(None, "subtitle", false), None);
        assert_eq!(
            owner_after_state(Some(IndicatorOwner::Dictation), "subtitle", false),
            None
        );
        // 听写临时接管共享通道。
        assert_eq!(
            owner_after_state(Some(IndicatorOwner::Subtitle), "recording", true),
            Some(IndicatorOwner::Dictation)
        );
        assert_eq!(
            owner_after_state(Some(IndicatorOwner::Subtitle), "smartProcessing", true),
            Some(IndicatorOwner::Dictation)
        );
        // hidden 清空归属。
        assert_eq!(
            owner_after_state(Some(IndicatorOwner::Dictation), "hidden", true),
            None
        );
        // error 不改变归属：错误面板是 WebView，底层会话不受影响。
        assert_eq!(
            owner_after_state(Some(IndicatorOwner::Subtitle), "error", true),
            Some(IndicatorOwner::Subtitle)
        );
    }
}

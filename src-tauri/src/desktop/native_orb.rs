//! Windows 原生悬浮球原型。
//!
//! 目标与 native_indicator 相同：悬浮球不再占用常驻 WebView 进程。圆形窗口
//! 画在 UpdateLayeredWindow 分层窗口的透明背景上，按像素 alpha 命中测试天然
//! 获得圆形可点区域；「非交互态点击穿透」用动态切换 WS_EX_TRANSPARENT 实现，
//! 等价于 WebView 路径的 set_ignore_cursor_events。
//!
//! 视觉规格以 ui/src/floating-orb.css 为准；点击/右键动作映射与拖拽阈值
//! 移植自 ui/src/floating-orb/interaction.ts；波形柱布局来自 OrbWaveform.tsx。
//! 模型与指针层在 overlay-core，Windows 宿主与绘图后端在 native_win。

#[cfg(windows)]
use super::native_win::orb as imp;
use sayit_overlay_core::orb::{OrbAction, OrbClickAction, OrbContextAction};
use std::sync::OnceLock;
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
#[cfg(windows)]
static VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(windows)]
static LAST_RECT: std::sync::Mutex<Option<(i32, i32, i32)>> = std::sync::Mutex::new(None);

/// Windows 上默认启用原生悬浮球；`SAYIT_NATIVE_ORB=0` 回退 WebView。
pub(crate) fn native_orb_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| super::native_common::native_window_enabled("SAYIT_NATIVE_ORB"))
}

pub(crate) fn native_orb_attach(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
    #[cfg(target_os = "macos")]
    super::native_mac::orb::attach(app);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = app;
}

/// 原生悬浮球的 UI 线程是否已经启动（窗口是否已创建）。
pub(crate) fn native_orb_started() -> bool {
    #[cfg(windows)]
    {
        imp::started()
    }
    #[cfg(target_os = "macos")]
    {
        super::native_mac::orb::started()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

/// 首次创建原生悬浮球窗口并设置矩形与整体透明度（物理像素 / 百分比）。
pub(crate) fn native_orb_configure(x: i32, y: i32, size: i32, opacity_percent: u8) {
    #[cfg(windows)]
    {
        set_last_rect(x, y, size);
        imp::post(imp::Command::SetRect { x, y, size });
        imp::post(imp::Command::SetOpacity(f32::from(opacity_percent) / 100.0));
    }
    #[cfg(target_os = "macos")]
    super::native_mac::orb::configure(x, y, size, opacity_percent);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (x, y, size, opacity_percent);
}

pub(crate) fn native_orb_set_rect(x: i32, y: i32, size: i32) {
    #[cfg(windows)]
    {
        set_last_rect(x, y, size);
        imp::post_if_started(imp::Command::SetRect { x, y, size });
    }
    #[cfg(target_os = "macos")]
    super::native_mac::orb::set_rect(x, y, size);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (x, y, size);
}

pub(crate) fn native_orb_move_to(x: i32, y: i32) {
    #[cfg(windows)]
    {
        move_last_rect(x, y);
        imp::post_if_started(imp::Command::MoveTo { x, y });
    }
    #[cfg(target_os = "macos")]
    super::native_mac::orb::move_to(x, y);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (x, y);
}

pub(crate) fn native_orb_set_opacity(percent: u8) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetOpacity(f32::from(percent) / 100.0));
    #[cfg(target_os = "macos")]
    super::native_mac::orb::set_opacity(percent);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = percent;
}

pub(crate) fn native_orb_show() {
    #[cfg(windows)]
    {
        VISIBLE.store(true, std::sync::atomic::Ordering::Release);
        imp::post_if_started(imp::Command::Show);
    }
    #[cfg(target_os = "macos")]
    super::native_mac::orb::show();
}

pub(crate) fn native_orb_hide() {
    #[cfg(windows)]
    {
        VISIBLE.store(false, std::sync::atomic::Ordering::Release);
        imp::post_if_started(imp::Command::Hide);
    }
    #[cfg(target_os = "macos")]
    super::native_mac::orb::hide();
}

/// 等价于 WebView 的 `set_ignore_cursor_events(!interactive)`。
pub(crate) fn native_orb_set_interactive(interactive: bool) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetInteractive(interactive));
    #[cfg(target_os = "macos")]
    super::native_mac::orb::set_interactive(interactive);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = interactive;
}

/// 未知 phase 静默丢弃，与 native_indicator 的状态过滤一致。
pub(crate) fn native_orb_set_state(phase: &str, transient: bool, can_submit: bool) {
    #[cfg(windows)]
    if let Some(phase) = imp::OrbPhase::from_str(phase) {
        imp::post_if_started(imp::Command::SetState {
            phase,
            transient,
            can_submit,
        });
    }
    #[cfg(target_os = "macos")]
    super::native_mac::orb::set_state(phase, transient, can_submit);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (phase, transient, can_submit);
}

pub(crate) fn native_orb_set_waveform(level: f32, peaks: Vec<f32>) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetWaveform { level, peaks });
    #[cfg(target_os = "macos")]
    super::native_mac::orb::set_waveform(level, peaks);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (level, peaks);
}

/// 最近一次已知的窗口矩形（物理像素 x/y/边长），由命令投递方与拖拽
/// 结束后的窗口过程共同维护，供任意线程同步读取。
pub(crate) fn native_orb_rect() -> Option<(i32, i32, i32)> {
    #[cfg(windows)]
    {
        last_rect()
    }
    #[cfg(target_os = "macos")]
    {
        super::native_mac::orb::rect()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

pub(crate) fn native_orb_is_visible() -> bool {
    #[cfg(windows)]
    {
        VISIBLE.load(std::sync::atomic::Ordering::Acquire)
    }
    #[cfg(target_os = "macos")]
    {
        super::native_mac::orb::is_visible()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

/// 与 WebView 路径的 is_cursor_over_floating_orb 同语义：真实指针位置
/// 是否落在当前可见的悬浮球矩形内。
pub(crate) fn native_orb_cursor_over() -> bool {
    #[cfg(windows)]
    {
        imp::cursor_over()
    }
    #[cfg(target_os = "macos")]
    {
        super::native_mac::orb::cursor_over()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

#[cfg(windows)]
fn set_last_rect(x: i32, y: i32, size: i32) {
    *LAST_RECT.lock().expect("orb rect mirror poisoned") = Some((x, y, size));
}
#[cfg(windows)]
fn move_last_rect(x: i32, y: i32) {
    if let Some(rect) = LAST_RECT.lock().expect("orb rect mirror poisoned").as_mut() {
        rect.0 = x;
        rect.1 = y;
    }
}
#[cfg(windows)]
fn last_rect() -> Option<(i32, i32, i32)> {
    *LAST_RECT.lock().expect("orb rect mirror poisoned")
}
#[cfg(windows)]
pub(super) fn update_mirrors(model: &sayit_overlay_core::orb::Orb) {
    let rect = model.rect();
    set_last_rect(rect.x, rect.y, rect.width);
    VISIBLE.store(model.is_visible(), std::sync::atomic::Ordering::Release);
}

pub(crate) fn run_orb_action(action: OrbAction) {
    match action {
        OrbAction::Click(action) => run_click_action(action),
        OrbAction::Context(action) => run_context_action(action),
        OrbAction::DragStarted => {
            if let Some(app) = APP.get() {
                super::floating_orb::native_orb_drag_started(app);
            }
        }
        OrbAction::DragFinished => {
            if let Some(app) = APP.get() {
                super::floating_orb::native_orb_drag_finished(app);
            }
        }
    }
}

fn run_click_action(action: OrbClickAction) {
    use crate::desktop::floating_orb as orb;
    let Some(app) = APP.get().cloned() else {
        return;
    };
    match action {
        OrbClickAction::Activate => {
            tauri::async_runtime::spawn(async move {
                if orb::floating_orb_activate(app).await.is_err() {
                    // 激活在早期失败（尚未进入任何相位切换）时恢复可交互，
                    // 否则悬浮球会永远停在穿透态，再也点不中。
                    crate::desktop::native_orb::native_orb_set_interactive(true);
                }
            });
        }
        OrbClickAction::Stop => {
            tauri::async_runtime::spawn(async move {
                let _ = orb::floating_orb_stop(app).await;
            });
        }
        OrbClickAction::Submit => {
            tauri::async_runtime::spawn(async move {
                let _ = orb::floating_orb_submit_enter(app).await;
            });
        }
        // 原型期差异：WebView 里左键 error 会打开错误详情对话框；
        // 原生改为打开主窗口。
        OrbClickAction::ShowError => {
            tauri::async_runtime::spawn(async move {
                let _ = orb::floating_orb_open_main_window(app).await;
            });
        }
    }
}

fn run_context_action(action: OrbContextAction) {
    use crate::desktop::floating_orb as orb;
    let Some(app) = APP.get().cloned() else {
        return;
    };
    match action {
        OrbContextAction::DismissError => {
            tauri::async_runtime::spawn(async move {
                let _ = orb::floating_orb_dismiss_error(app).await;
            });
        }
        OrbContextAction::Cancel => {
            tauri::async_runtime::spawn(async move {
                let _ = orb::floating_orb_cancel(app).await;
            });
        }
        OrbContextAction::DismissSubmit => {
            let _ = orb::floating_orb_dismiss_submit_enter(app);
        }
        // show_floating_orb_menu 是 async 命令（Windows 上窗口操作不能在
        // WebView2 同步 IPC 回调里）；这里从原生 UI 线程调用，放到 blocking 线程池。
        OrbContextAction::Menu => {
            tauri::async_runtime::spawn_blocking(move || {
                let _ = orb::show_floating_orb_menu(app);
            });
        }
    }
}

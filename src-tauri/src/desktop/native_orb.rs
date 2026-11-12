//! 原生悬浮球的平台分发。
//!
//! 目标与 native_indicator 相同：悬浮球不再占用常驻 WebView 进程。Windows 用
//! UpdateLayeredWindow 分层窗口，按像素 alpha 命中测试获得圆形可点区域；macOS 用
//! 不激活的 NSPanel。「非交互态点击穿透」等价于 WebView 路径的 set_ignore_cursor_events。
//!
//! 视觉规格以 ui/src/floating-orb.css 为准；点击/右键动作映射与拖拽阈值
//! 移植自 ui/src/floating-orb/interaction.ts；波形柱布局来自 OrbWaveform.tsx。
//! 模型与指针层在 overlay-core，Windows 宿主在 native_win，macOS 宿主在 overlay-macos。

use super::native_common::{post, post_if_started, run_action, started};
use sayit_overlay_core::canvas::TextEngine;
use sayit_overlay_core::look::Look;
use sayit_overlay_core::orb::{
    Command, Orb, OrbAction, OrbClickAction, OrbContextAction, OrbMirrors, OrbPhase,
};
use sayit_overlay_core::spec::{OverlaySpec, Placement};
use std::sync::OnceLock;
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
/// 投递侧写入请求，窗口的移动、DPI 与拖拽结束由模型回写；任意线程同步读取。
static MIRRORS: OrbMirrors = OrbMirrors::new();

pub(crate) struct OrbWindow;
impl OverlaySpec for OrbWindow {
    const NAME: &'static str = "sayit-native-orb";
    const TITLE: &'static str = "说吧！语音输入悬浮球";
    const PLACEMENT: Placement = Placement::Free;
    const CLICK_THROUGH: bool = false;
    type Command = Command;
    type Action = OrbAction;
    type Model<T: TextEngine + 'static> = Orb;
    fn model<T: TextEngine + 'static>(dpi: u32, look: &Look) -> Orb {
        Orb::new(dpi, look).with_mirrors(&MIRRORS)
    }
    fn dispatch(action: OrbAction) {
        run_action(move || run_orb_action(action));
    }
}

/// 未设置 `SAYIT_NATIVE_ORB` 时取平台默认；`0` 回退 WebView。
pub(crate) fn native_orb_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| super::native_common::native_window_enabled("SAYIT_NATIVE_ORB"))
}

pub(crate) fn native_orb_attach(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
    super::native_common::native_overlay_start();
}

/// 原生悬浮球窗口是否已经创建。
pub(crate) fn native_orb_started() -> bool {
    started::<OrbWindow>()
}

/// 首次创建原生悬浮球窗口并设置矩形与整体透明度（物理像素 / 百分比）。
pub(crate) fn native_orb_configure(x: i32, y: i32, size: i32, opacity_percent: u8) {
    MIRRORS.set_rect(x, y, size);
    post::<OrbWindow>(Command::SetRect { x, y, size });
    post::<OrbWindow>(Command::SetOpacity(f32::from(opacity_percent) / 100.0));
}

pub(crate) fn native_orb_set_rect(x: i32, y: i32, size: i32) {
    MIRRORS.set_rect(x, y, size);
    post_if_started::<OrbWindow>(Command::SetRect { x, y, size });
}

pub(crate) fn native_orb_move_to(x: i32, y: i32) {
    MIRRORS.move_to(x, y);
    post_if_started::<OrbWindow>(Command::MoveTo { x, y });
}

pub(crate) fn native_orb_set_opacity(percent: u8) {
    post_if_started::<OrbWindow>(Command::SetOpacity(f32::from(percent) / 100.0));
}

pub(crate) fn native_orb_show() {
    MIRRORS.set_visible(true);
    post_if_started::<OrbWindow>(Command::Show);
}

pub(crate) fn native_orb_hide() {
    MIRRORS.set_visible(false);
    post_if_started::<OrbWindow>(Command::Hide);
}

/// 等价于 WebView 的 `set_ignore_cursor_events(!interactive)`。
pub(crate) fn native_orb_set_interactive(interactive: bool) {
    post_if_started::<OrbWindow>(Command::SetInteractive(interactive));
}

/// 未知 phase 静默丢弃，与 native_indicator 的状态过滤一致。
pub(crate) fn native_orb_set_state(phase: &str, transient: bool, can_submit: bool) {
    if let Some(phase) = OrbPhase::from_str(phase) {
        post_if_started::<OrbWindow>(Command::SetState {
            phase,
            transient,
            can_submit,
        });
    }
}

pub(crate) fn native_orb_set_waveform(level: f32, peaks: Vec<f32>) {
    post_if_started::<OrbWindow>(Command::SetWaveform { level, peaks });
}

/// 最近一次已知的窗口矩形（物理像素 x/y/边长）。
pub(crate) fn native_orb_rect() -> Option<(i32, i32, i32)> {
    MIRRORS.rect()
}

pub(crate) fn native_orb_is_visible() -> bool {
    MIRRORS.is_visible()
}

/// 与 WebView 路径的 is_cursor_over_floating_orb 同语义：真实指针位置
/// 是否落在当前可见的悬浮球矩形内。
pub(crate) fn native_orb_cursor_over() -> bool {
    let (Some((x, y, size)), true, Some((cx, cy))) =
        (MIRRORS.rect(), MIRRORS.is_visible(), cursor_px())
    else {
        return false;
    };
    cx >= x && cx < x + size && cy >= y && cy < y + size
}

/// 指针的 DesktopPx 坐标。
fn cursor_px() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor) }.ok()?;
        Some((cursor.x, cursor.y))
    }
    #[cfg(target_os = "macos")]
    {
        sayit_overlay_macos::cursor_px()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

fn run_orb_action(action: OrbAction) {
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

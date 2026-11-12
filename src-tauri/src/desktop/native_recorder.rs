//! 会议录制条：录制期间停在屏幕上的原生小窗口，显示时长与两路音量，可拖动，点停止结束录制。
//!
//! 模型在 overlay-core 的 `recorder`；这里是窗口声明、位置记忆和动作派发。

use super::native_common::{post, post_if_started, run_action};
use crate::state::{FloatingOrbPosition, RuntimeState};
use sayit_overlay_core::canvas::TextEngine;
use sayit_overlay_core::look::Look;
use sayit_overlay_core::recorder::{Command, Recorder, RecorderAction};
use sayit_overlay_core::spec::{OverlaySpec, Placement};
use std::sync::OnceLock;
use tauri::Manager;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub(crate) struct RecorderWindow;
impl OverlaySpec for RecorderWindow {
    const NAME: &'static str = "sayit-native-recorder";
    const TITLE: &'static str = "说吧！会议录制";
    const PLACEMENT: Placement = Placement::Screen;
    const CLICK_THROUGH: bool = false;
    type Command = Command;
    type Action = RecorderAction;
    type Model<T: TextEngine + 'static> = Recorder;
    fn model<T: TextEngine + 'static>(dpi: u32, look: &Look) -> Recorder {
        Recorder::new(dpi, look)
    }
    fn dispatch(action: RecorderAction) {
        run_action(move || run_recorder_action(action));
    }
}

/// 显示录制条并从零计时。记住的位置已不在任何屏幕上时回到默认位置。
pub(crate) fn native_recorder_show(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
    super::native_common::native_overlay_start();
    super::native_common::native_overlay_set_look(super::floating_orb::native_look(app));
    let position = app
        .state::<RuntimeState>()
        .meeting_bar_position
        .lock()
        .ok()
        .and_then(|position| *position)
        .filter(|position| super::floating_orb::origin_is_on_a_monitor(app, position.x, position.y))
        .map(|position| (position.x, position.y));
    post::<RecorderWindow>(Command::Show { position });
}

pub(crate) fn native_recorder_levels(mic: f32, system: f32) {
    post_if_started::<RecorderWindow>(Command::Levels { mic, system });
}

pub(crate) fn native_recorder_set_lost(mic: bool, system: bool) {
    post_if_started::<RecorderWindow>(Command::SetLost { mic, system });
}

pub(crate) fn native_recorder_hide() {
    post_if_started::<RecorderWindow>(Command::Hide);
}

fn run_recorder_action(action: RecorderAction) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    match action {
        RecorderAction::Stop => {
            tauri::async_runtime::spawn(async move {
                let state = app.state::<RuntimeState>();
                // 主窗口可能已经关闭，结果用胶囊通知告诉用户。
                let notice = match crate::application::meeting::stop(&app, &state).await {
                    Ok(file) => file.notice(),
                    Err(error) => error,
                };
                if crate::desktop::native_dictation_indicator_enabled() {
                    crate::desktop::native_indicator_notice(notice);
                }
            });
        }
        RecorderAction::Moved { x, y } => {
            // 写盘放到阻塞线程池：这里可能是窗口线程或异步运行时的工作线程。
            tauri::async_runtime::spawn_blocking(move || {
                let state = app.state::<RuntimeState>();
                if let Ok(mut position) = state.meeting_bar_position.lock() {
                    *position = Some(FloatingOrbPosition { x, y });
                }
                if let Err(error) = crate::persistence::save_persisted_state(&app, &state) {
                    crate::application::diagnostics::event(
                        "error",
                        "meeting.barPositionSaveFailed",
                        serde_json::json!({ "error": error }),
                    );
                }
            });
        }
    }
}

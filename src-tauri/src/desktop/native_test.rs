//! 开发者模式的原生窗口测试：用假数据把各个原生窗口摆到屏幕上，看外观、动画和手感。
//!
//! 正在测试的窗口，其操作只给出视觉反馈，不触发真实动作（见各窗口声明的 `dispatch`）；
//! 命令直接投给原生窗口，不经过听写与字幕的状态路由，所以不会影响快捷键和真实会话。
//! 真实的语音输入、字幕或会议在显示或改动自己的窗口之前先调用 `end`，把测试窗口全部收起。

use crate::desktop::native_common::{post, post_if_started, started};
use crate::desktop::native_subtitle::SubtitleWindow;
use crate::desktop::{native_orb, native_recorder};
use crate::state::RuntimeState;
use sayit_overlay_core::subtitle::{Command as SubtitleCommand, SubtitleConfig};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Manager;

const FEED_INTERVAL: Duration = Duration::from_millis(100);
const DICTATION_TEXT: &str = "今天下午三点开会，讨论下个季度的产品计划，记得把上周的数据带上。";
const SUBTITLE_TEXT: &str = "欢迎来到今天的分享。我们先看一下过去一年的整体情况，然后再讨论接下来的计划，最后留十分钟回答大家的问题。";
const SUBTITLE_TRANSLATION: &str = "Welcome to today's session. We'll first look at the past year, then discuss what comes next, and leave ten minutes for questions.";
const NOTICE_TEXT: &str = "这是一条测试通知，几秒后自动消失";

/// 被测试的窗口。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestWindow {
    Indicator,
    Orb,
    Subtitle,
    Recorder,
}

/// 各窗口是否正在测试，下标同 `TestWindow`。窗口线程在 `dispatch` 里读取。
static TESTING: [AtomicBool; 4] = [const { AtomicBool::new(false) }; 4];
/// 每开始或结束一轮测试加一；推送任务只在自己那一轮里运行。
static ROUND: AtomicU32 = AtomicU32::new(0);
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();
static SCENES: Mutex<Scenes> = Mutex::new(Scenes::IDLE);

struct Scenes {
    indicator_recording: bool,
    orb_recording: bool,
    /// 本轮测试动过悬浮球，结束时要把它恢复成真实状态。
    orb_touched: bool,
    /// Some(是否带译文)。
    subtitle: Option<bool>,
    recorder: bool,
    tick: u32,
    /// 胶囊、字幕条各自从哪一拍开始出字，互不打断。
    indicator_since: u32,
    subtitle_since: u32,
}

impl Scenes {
    const IDLE: Self = Self {
        indicator_recording: false,
        orb_recording: false,
        orb_touched: false,
        subtitle: None,
        recorder: false,
        tick: 0,
        indicator_since: 0,
        subtitle_since: 0,
    };
}

/// 这个窗口正在测试：它的声明据此把动作换成只有视觉反馈的处理。
pub(crate) fn testing(window: TestWindow) -> bool {
    TESTING[window as usize].load(Ordering::Acquire)
}

fn set_testing(window: TestWindow, value: bool) {
    TESTING[window as usize].store(value, Ordering::Release);
}

fn any_testing() -> bool {
    TESTING.iter().any(|flag| flag.load(Ordering::Acquire))
}

fn scenes() -> Result<std::sync::MutexGuard<'static, Scenes>, String> {
    SCENES.lock().map_err(|_| "原生窗口测试状态锁失败".to_string())
}

/// window: indicator | orb | subtitle | recorder；scene 是该窗口的一种状态。
/// 一次只摆出一个窗口：显示这一个之前先收起其他正在测试的。
// 悬浮球的窗口操作不能在 WebView2 同步 IPC 回调里做，Windows 上换到异步线程派发。
#[cfg_attr(windows, tauri::command(async))]
#[cfg_attr(not(windows), tauri::command)]
pub(crate) fn dev_native_window_test(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    window: String,
    scene: String,
) -> Result<(), String> {
    crate::application::diagnostics::require_developer_mode()?;
    if state.audio_session.is_busy() {
        return Err("麦克风正在使用，结束后再测试".into());
    }
    let _ = APP.set(app.clone());
    let mut scenes = scenes()?;
    let was_testing = any_testing();
    let target = match window.as_str() {
        "indicator" => TestWindow::Indicator,
        "orb" => TestWindow::Orb,
        "subtitle" => TestWindow::Subtitle,
        "recorder" => TestWindow::Recorder,
        _ => return Err(format!("未知的原生窗口：{window}")),
    };
    for other in [
        TestWindow::Indicator,
        TestWindow::Orb,
        TestWindow::Subtitle,
        TestWindow::Recorder,
    ] {
        if other != target {
            hide(&app, &mut scenes, other);
        }
    }
    match target {
        TestWindow::Indicator => show_indicator(&mut scenes, &scene),
        TestWindow::Orb => show_orb(&app, &mut scenes, &scene),
        TestWindow::Subtitle => show_subtitle(&mut scenes, &scene),
        TestWindow::Recorder => show_recorder(&app, &mut scenes, &scene),
    }?;
    if !was_testing && any_testing() {
        spawn_feed(app, ROUND.fetch_add(1, Ordering::AcqRel) + 1);
    }
    Ok(())
}

/// 收起全部测试窗口并恢复真实状态。
#[cfg_attr(windows, tauri::command(async))]
#[cfg_attr(not(windows), tauri::command)]
pub(crate) fn dev_native_window_test_stop() -> Result<(), String> {
    crate::application::diagnostics::require_developer_mode()?;
    end();
    Ok(())
}

/// 测试中的窗口被它自己的按钮收起（字幕条的关闭、录制条的停止）。
pub(crate) fn closed_by_button(window: TestWindow) {
    if let Ok(mut scenes) = scenes() {
        closed_by_button_locked(&mut scenes, window);
    }
}

fn prepare_look(app: &tauri::AppHandle) {
    crate::desktop::native_common::native_overlay_start();
    crate::desktop::native_common::native_overlay_set_look(
        crate::desktop::floating_orb::native_look(app),
    );
}

fn show_indicator(scenes: &mut Scenes, scene: &str) -> Result<(), String> {
    if !crate::desktop::native_dictation_indicator_enabled() {
        return Err("原生听写胶囊已用环境变量关闭".into());
    }
    match scene {
        "notice" => crate::desktop::native_indicator_notice(NOTICE_TEXT.into()),
        "recording" | "processing" | "smartProcessing" | "fallback" => {
            crate::desktop::native_indicator_prepare();
            crate::desktop::native_indicator_set_text(String::new(), false, None);
            crate::desktop::native_indicator_set_state(scene);
        }
        _ => return Err(format!("未知的胶囊状态：{scene}")),
    }
    scenes.indicator_recording = scene == "recording";
    scenes.indicator_since = scenes.tick;
    set_testing(TestWindow::Indicator, true);
    Ok(())
}

fn show_orb(app: &tauri::AppHandle, scenes: &mut Scenes, scene: &str) -> Result<(), String> {
    if !native_orb::native_orb_enabled() {
        return Err("原生悬浮球已用环境变量关闭".into());
    }
    if sayit_overlay_core::orb::OrbPhase::from_str(scene).is_none() {
        return Err(format!("未知的悬浮球状态：{scene}"));
    }
    prepare_look(app);
    crate::desktop::floating_orb::present_floating_orb(app)?;
    scenes.orb_touched = true;
    scenes.orb_recording = scene == "recording";
    native_orb::native_orb_set_state(scene, false, false);
    set_testing(TestWindow::Orb, true);
    Ok(())
}

/// 悬浮球交还给真实状态：回到空闲相位和设置里的位置，没启用就收起。
fn restore_orb(app: &tauri::AppHandle, scenes: &mut Scenes) {
    set_testing(TestWindow::Orb, false);
    scenes.orb_recording = false;
    if !std::mem::take(&mut scenes.orb_touched) {
        return;
    }
    native_orb::native_orb_set_state("idle", false, false);
    crate::desktop::floating_orb::restore_native_orb_rect(app);
    if let Err(error) = crate::desktop::floating_orb::sync_floating_orb_window(app) {
        crate::application::diagnostics::event(
            "error",
            "devNativeTest.orbRestoreFailed",
            serde_json::json!({ "error": error }),
        );
    }
}

fn show_subtitle(scenes: &mut Scenes, scene: &str) -> Result<(), String> {
    if !crate::desktop::native_subtitle::native_subtitle_enabled() {
        return Err("原生字幕条已用环境变量关闭".into());
    }
    let translated = match scene {
        "original" => false,
        "translated" => true,
        _ => return Err(format!("未知的字幕状态：{scene}")),
    };
    // 字幕条用过之后保留用户自己的样式；没用过就给一份默认样式和位置。
    if !started::<SubtitleWindow>() {
        let config = SubtitleConfig {
            translation_enabled: true,
            // 两行原文加译文的高度，「重置位置」也回到这个尺寸。
            window_height: 214.0,
            ..SubtitleConfig::default()
        };
        post::<SubtitleWindow>(SubtitleCommand::SetLayout {
            width: config.window_width as f32,
            height: config.window_height as f32,
            anchor: config.anchor.clone(),
            offset_y: config.offset_y as f32,
        });
        post::<SubtitleWindow>(SubtitleCommand::SetConfig(Box::new(config)));
    }
    scenes.subtitle = Some(translated);
    scenes.subtitle_since = scenes.tick;
    post::<SubtitleWindow>(SubtitleCommand::SetText {
        text: String::new(),
        fade: false,
    });
    post::<SubtitleWindow>(SubtitleCommand::SetTranslation(String::new()));
    post::<SubtitleWindow>(SubtitleCommand::Show);
    set_testing(TestWindow::Subtitle, true);
    Ok(())
}

fn show_recorder(app: &tauri::AppHandle, scenes: &mut Scenes, scene: &str) -> Result<(), String> {
    let lost = match scene {
        "recording" => (false, false),
        "micLost" => (true, false),
        "systemLost" => (false, true),
        _ => return Err(format!("未知的录制条状态：{scene}")),
    };
    // 已经在屏幕上就只切状态，不重新计时。
    if !scenes.recorder {
        scenes.recorder = true;
        prepare_look(app);
        native_recorder::native_recorder_show(app);
    }
    set_testing(TestWindow::Recorder, true);
    native_recorder::native_recorder_set_lost(lost.0, lost.1);
    Ok(())
}

/// 前 `count` 个字符。
fn head(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

/// 定时送假音量和逐字出现的文字；主窗口关闭时自动结束测试。
fn spawn_feed(app: tauri::AppHandle, round: u32) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(FEED_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if ROUND.load(Ordering::Acquire) != round {
                break;
            }
            if app.get_webview_window("main").is_none() {
                end();
                break;
            }
            let Ok(mut scenes) = scenes() else { break };
            // 拿到锁之后再看：切换窗口的命令在持锁期间会先收起旧的再显示新的。
            if !any_testing() {
                break;
            }
            scenes.tick += 1;
            let time = scenes.tick as f32 * 0.1;
            let wave = |phase: f32| {
                let slow = 0.5 + 0.5 * (time * 0.37 + phase).sin();
                let fast = 0.5 + 0.5 * (time * 1.7 + phase * 2.0).sin();
                0.02 + 0.14 * slow * fast
            };
            let peaks: Vec<f32> = (0..6).map(|index| wave(index as f32 * 0.6)).collect();
            // 文字和真实识别一样逐字追加（不整段换新）：说完停几秒再从头来。
            if scenes.indicator_recording {
                crate::desktop::native_indicator_set_waveform(wave(0.0), peaks.clone());
                let tick = (scenes.tick - scenes.indicator_since) as usize;
                let total = DICTATION_TEXT.chars().count();
                let shown = (tick / 3) % (total + 20);
                if tick % 3 == 0 && shown <= total {
                    if shown == 0 {
                        // 和真实流程一样，每一遍开始前清掉「已经显示过的字」，新字才会淡入。
                        crate::desktop::native_indicator_prepare();
                    }
                    let text = head(DICTATION_TEXT, shown);
                    // 逗号前的分句当作已定稿，正在「说」的这一句用预览的浅色画。
                    let preview_from = text.rfind('，').map(|at| text[..at].chars().count() + 1).unwrap_or(0);
                    let preview_from = (shown < total).then_some(preview_from);
                    crate::desktop::native_indicator_set_text(text, false, preview_from);
                }
            }
            if scenes.orb_recording {
                native_orb::native_orb_set_waveform(wave(0.0), peaks);
            }
            if let Some(translated) = scenes.subtitle {
                let tick = (scenes.tick - scenes.subtitle_since) as usize;
                let total = SUBTITLE_TEXT.chars().count();
                let shown = (tick / 2) % (total + 30);
                if tick % 2 == 0 && shown <= total {
                    post_if_started::<SubtitleWindow>(SubtitleCommand::SetText {
                        text: head(SUBTITLE_TEXT, shown),
                        fade: false,
                    });
                    if translated {
                        let ratio = SUBTITLE_TRANSLATION.chars().count() * shown / total;
                        post_if_started::<SubtitleWindow>(SubtitleCommand::SetTranslation(head(
                            SUBTITLE_TRANSLATION,
                            ratio,
                        )));
                    }
                }
            }
            if scenes.recorder {
                native_recorder::native_recorder_levels(wave(0.0), wave(2.0));
            }
        }
    });
}

/// 收起一个正在测试的窗口；没在测试的不动，免得碰到真实状态。
fn hide(app: &tauri::AppHandle, scenes: &mut Scenes, window: TestWindow) {
    match window {
        TestWindow::Indicator => {
            if testing(window) {
                crate::desktop::native_indicator_hide();
            }
            scenes.indicator_recording = false;
            set_testing(window, false);
        }
        TestWindow::Orb => restore_orb(app, scenes),
        TestWindow::Subtitle | TestWindow::Recorder => closed_by_button_locked(scenes, window),
    }
}

fn closed_by_button_locked(scenes: &mut Scenes, window: TestWindow) {
    match window {
        TestWindow::Subtitle => {
            if scenes.subtitle.take().is_some() {
                post_if_started::<SubtitleWindow>(SubtitleCommand::Hide);
            }
        }
        TestWindow::Recorder => {
            if std::mem::take(&mut scenes.recorder) {
                native_recorder::native_recorder_hide();
            }
        }
        TestWindow::Indicator | TestWindow::Orb => {}
    }
    set_testing(window, false);
}

/// 结束测试：收起全部测试窗口，悬浮球恢复成真实状态。没有在测试时什么也不做。
/// 真实的语音输入、字幕和会议在显示或改动自己的窗口之前调用。
pub(crate) fn end() {
    if !any_testing() {
        return;
    }
    ROUND.fetch_add(1, Ordering::AcqRel);
    let (Ok(mut scenes), Some(app)) = (scenes(), APP.get()) else {
        return;
    };
    for window in [
        TestWindow::Indicator,
        TestWindow::Orb,
        TestWindow::Subtitle,
        TestWindow::Recorder,
    ] {
        hide(app, &mut scenes, window);
    }
    *scenes = Scenes::IDLE;
}

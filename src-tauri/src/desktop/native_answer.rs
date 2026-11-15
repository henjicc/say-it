//! 智能助手回答窗的原生实现：窗口声明、对外命令与动作派发。
//!
//! 模型在 overlay-core 的 `answer`；回答内容、置顶和会话都由 `application::assistant` 保存，
//! 这里只把快照投给窗口，把窗口里的操作转回那边的命令。

use super::native_common::{post, post_if_started, run_action};
use sayit_overlay_core::answer::{Answer, AnswerAction, AnswerView, Command};
use sayit_overlay_core::canvas::TextEngine;
use sayit_overlay_core::geom::PxRect;
use sayit_overlay_core::look::Look;
use sayit_overlay_core::spec::{OverlaySpec, Placement};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();
static VISIBLE: AtomicBool = AtomicBool::new(false);
static FOCUSED: AtomicBool = AtomicBool::new(false);
/// 当前回答能否继续追问（问答类回答才有对话上下文）。
static CAN_FOLLOW_UP: AtomicBool = AtomicBool::new(false);
/// 当前语音追问是不是按住听写快捷键开始的（决定语音条提示「松开发送」还是「点这里发送」）。
static HOTKEY_VOICE: AtomicBool = AtomicBool::new(false);

pub(crate) struct AnswerWindow;
impl OverlaySpec for AnswerWindow {
    const NAME: &'static str = "sayit-native-answer";
    const TITLE: &'static str = "说吧！智能助手";
    const PLACEMENT: Placement = Placement::Free;
    const CLICK_THROUGH: bool = false;
    const FOCUSABLE: bool = true;
    type Command = Command;
    type Action = AnswerAction;
    type Model<T: TextEngine + 'static> = Answer<T::Layout>;
    fn model<T: TextEngine + 'static>(dpi: u32, look: &Look) -> Answer<T::Layout> {
        Answer::new(dpi, look)
    }
    fn dispatch(action: AnswerAction) {
        if super::native_test::testing(super::native_test::TestWindow::Answer) {
            // 测试中只响应关闭，其余操作都不触发真实请求。
            if action == AnswerAction::Close {
                super::native_test::closed_by_button(super::native_test::TestWindow::Answer);
            }
            return;
        }
        run_action(move || run(action));
    }
}

/// SAYIT_NATIVE_ASSISTANT=0 时回退到 WebView 回答窗；进程内只读一次。
pub(crate) fn native_answer_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| super::native_common::native_window_enabled("SAYIT_NATIVE_ASSISTANT"))
}

pub(crate) fn native_answer_visible() -> bool {
    VISIBLE.load(Ordering::Acquire)
}

/// 回答窗拿着键盘焦点时，听写快捷键用于语音追问。
pub(crate) fn hotkey_follows_up() -> bool {
    native_answer_enabled()
        && native_answer_visible()
        && FOCUSED.load(Ordering::Acquire)
        && CAN_FOLLOW_UP.load(Ordering::Acquire)
}

/// 显示新回答：左上角对齐此刻的鼠标位置，放不下时由模型在鼠标所在屏的工作区里挪开。
pub(crate) fn native_answer_show(app: &AppHandle, view: AnswerView, focus: bool) {
    // 指针位置与工作区都是物理像素（DesktopPx）。
    let pointer = app
        .cursor_position()
        .map(|point| (point.x.round() as i32, point.y.round() as i32))
        .unwrap_or_default();
    let work = app
        .monitor_from_point(f64::from(pointer.0), f64::from(pointer.1))
        .ok()
        .flatten()
        .map(|monitor| {
            let area = monitor.work_area();
            PxRect::new(
                area.position.x,
                area.position.y,
                area.size.width as i32,
                area.size.height as i32,
            )
        })
        .unwrap_or_default();
    let _ = APP.set(app.clone());
    super::native_common::native_overlay_start();
    super::native_common::native_overlay_set_look(super::floating_orb::native_look(app));
    VISIBLE.store(true, Ordering::Release);
    CAN_FOLLOW_UP.store(view.can_follow_up, Ordering::Release);
    post::<AnswerWindow>(Command::Show {
        view,
        pointer,
        work,
        focus,
    });
}

pub(crate) fn native_answer_update(view: AnswerView) {
    CAN_FOLLOW_UP.store(view.can_follow_up, Ordering::Release);
    post_if_started::<AnswerWindow>(Command::Update(view));
}

pub(crate) fn native_answer_voice(active: bool, text: &str, hotkey: Option<bool>) {
    if let Some(hotkey) = hotkey {
        HOTKEY_VOICE.store(hotkey, Ordering::Release);
    }
    post_if_started::<AnswerWindow>(Command::Voice {
        active,
        text: text.to_owned(),
        hotkey: HOTKEY_VOICE.load(Ordering::Acquire),
    });
}

pub(crate) fn native_answer_level(level: f32) {
    post_if_started::<AnswerWindow>(Command::Level(level));
}

pub(crate) fn native_answer_hide() {
    VISIBLE.store(false, Ordering::Release);
    FOCUSED.store(false, Ordering::Release);
    post_if_started::<AnswerWindow>(Command::Hide);
}

/// 追问快捷键的显示名，与设置页的写法一致。
pub(crate) fn hotkey_label(
    key_code: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
) -> Option<String> {
    let key_code = key_code.trim();
    if key_code.is_empty() {
        return None;
    }
    let mac = cfg!(target_os = "macos");
    let mut parts: Vec<String> = Vec::new();
    for (held, windows, macos) in [
        (ctrl, "Ctrl", "⌃"),
        (alt, "Alt", "⌥"),
        (shift, "Shift", "⇧"),
        (meta, "Win", "⌘"),
    ] {
        if held {
            parts.push(if mac { macos } else { windows }.into());
        }
    }
    let side = |code: &str| {
        let (name, side) = match code {
            "ControlLeft" => ("Ctrl", "左"),
            "ControlRight" => ("Ctrl", "右"),
            "AltLeft" => (if mac { "Option" } else { "Alt" }, "左"),
            "AltRight" => (if mac { "Option" } else { "Alt" }, "右"),
            "ShiftLeft" => ("Shift", "左"),
            "ShiftRight" => ("Shift", "右"),
            "MetaLeft" => (if mac { "Command" } else { "Win" }, "左"),
            "MetaRight" => (if mac { "Command" } else { "Win" }, "右"),
            _ => return None,
        };
        Some(format!("{side} {name}"))
    };
    let key = side(key_code).unwrap_or_else(|| match key_code {
        "CapsLock" => "Caps Lock".into(),
        "Space" => "Space".into(),
        "ArrowLeft" => "←".into(),
        "ArrowUp" => "↑".into(),
        "ArrowRight" => "→".into(),
        "ArrowDown" => "↓".into(),
        "Backquote" => "`".into(),
        code => code
            .strip_prefix("Key")
            .or_else(|| code.strip_prefix("Digit"))
            .unwrap_or(code)
            .into(),
    });
    parts.push(key);
    Some(parts.join(" + "))
}

fn run(action: AnswerAction) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    use crate::application::assistant as assistant;
    match action {
        AnswerAction::Focused(focused) => {
            FOCUSED.store(focused, Ordering::Release);
            if !focused {
                assistant::answer_window_blurred(&app);
            }
        }
        AnswerAction::Close => spawn(app, assistant::close_assistant_answer),
        AnswerAction::SetPinned(pinned) => {
            if let Err(error) = assistant::set_assistant_answer_pinned(app, pinned) {
                eprintln!("[native-answer] {error}");
            }
        }
        AnswerAction::Copy(text) => {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = crate::commands::dictation::write_clipboard_text_inner(text).await {
                    eprintln!("[native-answer] {error}");
                }
            });
        }
        AnswerAction::Paste => {
            tauri::async_runtime::spawn_blocking(|| {
                match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
                    Ok(text) => post_if_started::<AnswerWindow>(Command::Paste(text)),
                    // 剪贴板里不是文字时没有可粘贴的内容。
                    Err(arboard::Error::ContentNotAvailable) => {}
                    Err(error) => eprintln!("[native-answer] 读取剪贴板失败：{error}"),
                }
            });
        }
        AnswerAction::Regenerate => spawn(app, assistant::regenerate_assistant_answer),
        AnswerAction::Insert => spawn(app, assistant::insert_assistant_answer),
        AnswerAction::Send(prompt) => {
            tauri::async_runtime::spawn(async move {
                if let Err(error) =
                    assistant::continue_assistant_answer(app, prompt.clone()).await
                {
                    // 已取消（关窗、重新提问）时草稿不再需要；其余失败把草稿还给用户。
                    if !error.contains("已取消") {
                        post_if_started::<AnswerWindow>(Command::RestoreDraft(prompt));
                    }
                }
            });
        }
        AnswerAction::StartVoice => {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = assistant::start_follow_up_voice(app, false).await {
                    eprintln!("[native-answer] {error}");
                }
            });
        }
        AnswerAction::StopVoice => spawn(app, assistant::stop_assistant_follow_up_voice),
        AnswerAction::CancelVoice => spawn(app, assistant::cancel_follow_up_voice),
        AnswerAction::OpenLink(url) => {
            use tauri_plugin_opener::OpenerExt;
            // 只打开网页链接：回答来自大模型，其它协议可能启动本机程序。
            if url.starts_with("https://") || url.starts_with("http://") {
                if let Err(error) = app.opener().open_url(url, None::<&str>) {
                    eprintln!("[native-answer] 打开链接失败：{error}");
                }
            }
        }
    }
}

fn spawn<F, Fut>(app: AppHandle, f: F)
where
    F: FnOnce(AppHandle) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        if let Err(error) = f(app).await {
            eprintln!("[native-answer] {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_label_reads_like_the_settings_page() {
        assert_eq!(hotkey_label("", false, false, false, false), None);
        assert_eq!(
            hotkey_label("KeyD", true, false, true, false).as_deref(),
            Some(if cfg!(target_os = "macos") { "⌃ + ⇧ + D" } else { "Ctrl + Shift + D" })
        );
        assert_eq!(
            hotkey_label("AltRight", false, false, false, false).as_deref(),
            Some(if cfg!(target_os = "macos") { "右 Option" } else { "右 Alt" })
        );
    }
}

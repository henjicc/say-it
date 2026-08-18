use crate::active_app_context::{ActivationTarget, AppIdentity, CaptureStatus};
use crate::state::RuntimeState;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub(crate) const ANSWER_WINDOW_LABEL: &str = "assistant-answer";
pub(crate) const ANSWER_EVENT: &str = "assistant-answer-changed";
static REGISTERED_SHORTCUTS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantShortcut {
    #[serde(default)]
    pub(crate) key_code: String,
    #[serde(default)]
    pub(crate) ctrl: bool,
    #[serde(default)]
    pub(crate) shift: bool,
    #[serde(default)]
    pub(crate) alt: bool,
    #[serde(default)]
    pub(crate) meta: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantShortcutSettings {
    #[serde(default)]
    pub(crate) translate_speech: AssistantShortcut,
    #[serde(default)]
    pub(crate) edit_selection: AssistantShortcut,
    #[serde(default)]
    pub(crate) ask: AssistantShortcut,
}

impl AssistantShortcutSettings {
    pub(crate) fn get(&self, action: AssistantAction) -> &AssistantShortcut {
        match action {
            AssistantAction::TranslateSpeech => &self.translate_speech,
            AssistantAction::EditSelection => &self.edit_selection,
            AssistantAction::Ask => &self.ask,
        }
    }
    pub(crate) fn get_mut(&mut self, action: AssistantAction) -> &mut AssistantShortcut {
        match action {
            AssistantAction::TranslateSpeech => &mut self.translate_speech,
            AssistantAction::EditSelection => &mut self.edit_selection,
            AssistantAction::Ask => &mut self.ask,
        }
    }
}

fn accelerator(shortcut: &AssistantShortcut) -> Result<Option<String>, String> {
    let code = shortcut.key_code.trim();
    if code.is_empty() {
        return Ok(None);
    }
    let key = if let Some(value) = code.strip_prefix("Key").filter(|value| value.len() == 1) {
        value.to_string()
    } else if let Some(value) = code.strip_prefix("Digit").filter(|value| value.len() == 1) {
        value.to_string()
    } else if code.starts_with('F') && code[1..].parse::<u8>().is_ok() {
        code.to_string()
    } else {
        match code {
            "Space" => "Space".into(),
            "Enter" => "Enter".into(),
            "Tab" => "Tab".into(),
            _ => return Err(format!("语音助手不支持按键 {code}")),
        }
    };
    let mut parts = Vec::new();
    if shortcut.ctrl {
        parts.push("Control");
    }
    if shortcut.shift {
        parts.push("Shift");
    }
    if shortcut.alt {
        parts.push("Alt");
    }
    if shortcut.meta {
        parts.push("Super");
    }
    parts.push(&key);
    Ok(Some(parts.join("+")))
}

pub(crate) fn set_shortcuts(
    app: &AppHandle,
    settings: &AssistantShortcutSettings,
) -> Result<(), String> {
    let storage = REGISTERED_SHORTCUTS.get_or_init(|| Mutex::new(Vec::new()));
    let mut registered = storage.lock().map_err(|_| "语音助手快捷键状态锁失败")?;
    for shortcut in registered.drain(..) {
        let _ = app.global_shortcut().unregister(shortcut.as_str());
    }
    let mut next: Vec<String> = Vec::new();
    for action in [
        AssistantAction::TranslateSpeech,
        AssistantAction::EditSelection,
        AssistantAction::Ask,
    ] {
        let Some(shortcut) = accelerator(settings.get(action))? else {
            continue;
        };
        let callback_action = action;
        if let Err(error) =
            app.global_shortcut()
                .on_shortcut(shortcut.as_str(), move |app, _, event| {
                    if event.state == ShortcutState::Pressed {
                        request_shortcut(app.clone(), callback_action);
                    }
                })
        {
            for value in &next {
                let _ = app.global_shortcut().unregister(value.as_str());
            }
            return Err(format!("注册语音助手快捷键 {shortcut} 失败：{error}"));
        }
        next.push(shortcut);
    }
    *registered = next;
    Ok(())
}

fn request_shortcut(app: AppHandle, action: AssistantAction) {
    tauri::async_runtime::spawn(async move {
        let result = if crate::application::dictation::is_active(&app) {
            crate::application::dictation::dictation_stop(app.clone()).await
        } else {
            assistant_start(app.clone(), action).await
        };
        if let Err(error) = result {
            publish_answer(
                &app,
                &AssistantRequest {
                    action,
                    selection: None,
                    target: None,
                    identity: None,
                    started_at: Instant::now(),
                },
                String::new(),
                Some(error),
                false,
            );
        }
    });
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AssistantAction {
    TranslateSpeech,
    EditSelection,
    Ask,
}

impl AssistantAction {
    pub(crate) fn task_kind(self) -> &'static str {
        match self {
            Self::TranslateSpeech => "translateSpeech",
            Self::EditSelection => "editSelection",
            Self::Ask => "ask",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionBounds {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionSnapshot {
    pub(crate) text: String,
    pub(crate) app_name: String,
    pub(crate) process_name: String,
    pub(crate) process_id: u32,
    pub(crate) editable: bool,
    pub(crate) secure: bool,
    pub(crate) method: String,
    pub(crate) bounds: Option<SelectionBounds>,
    pub(crate) elapsed_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct AssistantRequest {
    pub(crate) action: AssistantAction,
    pub(crate) selection: Option<SelectionSnapshot>,
    pub(crate) target: Option<ActivationTarget>,
    pub(crate) identity: Option<AppIdentity>,
    pub(crate) started_at: Instant,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantAnswer {
    pub(crate) action: Option<AssistantAction>,
    pub(crate) text: String,
    pub(crate) source_text: String,
    pub(crate) error: Option<String>,
    pub(crate) can_insert: bool,
}

#[derive(Default)]
pub(crate) struct AssistantRuntime {
    answer: Mutex<AssistantAnswer>,
    answer_target: Mutex<Option<ActivationTarget>>,
    regeneration: Mutex<Option<RegenerationContext>>,
}

#[derive(Clone, Debug)]
struct RegenerationContext {
    request: AssistantRequest,
    spoken_text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ProcessedAssistant {
    pub(crate) output: String,
    pub(crate) should_inject: bool,
    pub(crate) show_answer: bool,
}

pub(crate) async fn capture_selection_internal(
    app: &AppHandle,
) -> Result<(SelectionSnapshot, ActivationTarget), String> {
    let started = Instant::now();
    let target = crate::active_app_context::activation_target()
        .ok_or_else(|| "无法定位当前前台窗口".to_string())?;
    if target.process_id == std::process::id() {
        return Err("请先选中其他应用中的文本，再按语音助手快捷键".into());
    }
    let state = app.state::<RuntimeState>();
    let handle = state.active_app_context.begin_selection_capture(target);
    let captured = state.active_app_context.resolve_for_dictation(handle).await;
    crate::application::performance::record(
        "selection.capture",
        started.elapsed().as_millis() as u64,
    );
    let secure = captured.status == CaptureStatus::Sensitive;
    let mut text = captured
        .selected_text
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut method = captured
        .source
        .map(|source| format!("{source:?}"))
        .unwrap_or_else(|| "unavailable".into());
    #[cfg(target_os = "macos")]
    if text.is_empty() && !secure {
        if let Ok(value) = crate::macos_native::copy_selection_text(target.process_id) {
            text = value.trim().to_string();
            method = "clipboardSelection".into();
        }
    }
    Ok((
        SelectionSnapshot {
            text,
            app_name: captured.app_name,
            process_name: captured.process_name,
            process_id: target.process_id,
            editable: !secure,
            secure,
            method,
            bounds: None,
            elapsed_ms: started.elapsed().as_millis() as u64,
        },
        target,
    ))
}

#[tauri::command]
pub(crate) async fn capture_current_selection(app: AppHandle) -> Result<SelectionSnapshot, String> {
    capture_selection_internal(&app).await.map(|value| value.0)
}

#[tauri::command]
pub(crate) async fn assistant_start(app: AppHandle, action: AssistantAction) -> Result<(), String> {
    {
        let state = app.state::<RuntimeState>();
        match action {
            AssistantAction::TranslateSpeech => {
                let prefs = state
                    .app_settings
                    .lock()
                    .map_err(|_| "应用配置锁失败")?
                    .assistant_prefs
                    .clone();
                let model = prefs
                    .get("translationModel")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("none");
                crate::application::translation::validate_available(&state, model)?;
            }
            AssistantAction::EditSelection | AssistantAction::Ask => {
                crate::application::smart_text::validate_available(&state)?;
            }
        }
    }
    let (selection, target) = match action {
        AssistantAction::TranslateSpeech => {
            let target = crate::active_app_context::activation_target()
                .ok_or_else(|| "无法定位当前输入窗口".to_string())?;
            (None, Some(target))
        }
        AssistantAction::EditSelection | AssistantAction::Ask => {
            match capture_selection_internal(&app).await {
                Ok((snapshot, _))
                    if action == AssistantAction::EditSelection && snapshot.text.is_empty() =>
                {
                    return Err("请先选中需要修改的文本".into())
                }
                Ok((snapshot, target)) => (Some(snapshot), Some(target)),
                Err(error) if action == AssistantAction::Ask => {
                    let target = crate::active_app_context::activation_target();
                    if target.is_none() {
                        return Err(error);
                    }
                    (None, target)
                }
                Err(error) => return Err(error),
            }
        }
    };
    let identity = target.and_then(crate::active_app_context::app_identity);
    crate::application::dictation::start_assistant(
        app,
        AssistantRequest {
            action,
            selection,
            target,
            identity,
            started_at: Instant::now(),
        },
    )
    .await
}

#[tauri::command]
pub(crate) async fn assistant_stop(app: AppHandle) -> Result<(), String> {
    crate::application::dictation::dictation_stop(app).await
}

#[tauri::command]
pub(crate) async fn assistant_cancel(app: AppHandle) -> Result<(), String> {
    crate::application::dictation::dictation_cancel(app).await
}

pub(crate) async fn process(
    app: &AppHandle,
    state: &RuntimeState,
    request: &AssistantRequest,
    spoken_text: &str,
) -> Result<ProcessedAssistant, String> {
    if let Ok(mut current) = state.assistant_runtime.regeneration.lock() {
        *current = Some(RegenerationContext {
            request: request.clone(),
            spoken_text: spoken_text.to_string(),
        });
    }
    let prefs = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败")?
        .assistant_prefs
        .clone();
    match request.action {
        AssistantAction::TranslateSpeech => {
            let model = prefs
                .get("translationModel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none");
            let source = prefs
                .get("sourceLanguage")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("auto");
            let target = prefs
                .get("targetLanguage")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("zh");
            let output = crate::application::translation::translate_text(
                state,
                model,
                spoken_text,
                source,
                target,
            )
            .await?;
            Ok(ProcessedAssistant {
                output,
                should_inject: true,
                show_answer: false,
            })
        }
        AssistantAction::EditSelection => {
            let selection = request
                .selection
                .as_ref()
                .ok_or_else(|| "选区快照不存在".to_string())?;
            let prompt = format!("根据语音指令修改下面的选中文本。保持原意和事实，只返回修改后的完整文本，不要解释。\n选中文本：\n{}\n\n语音指令：{{{{text}}}}", selection.text);
            let output =
                crate::application::smart_text::process_smart_text(state, spoken_text, &prompt, "")
                    .await?;
            let verified = verify_selection(app, request).await.unwrap_or(false);
            Ok(ProcessedAssistant {
                output,
                should_inject: verified,
                show_answer: !verified,
            })
        }
        AssistantAction::Ask => {
            let context = request
                .selection
                .as_ref()
                .map(|selection| selection.text.as_str())
                .unwrap_or("");
            let prompt = if context.is_empty() {
                "直接回答用户的语音问题。答案清晰、简洁；只返回答案正文。\n问题：{{text}}"
                    .to_string()
            } else {
                format!("根据选中文本回答用户问题。只返回答案正文。\n选中文本：\n{context}\n\n问题：{{{{text}}}}")
            };
            let output =
                crate::application::smart_text::process_smart_text(state, spoken_text, &prompt, "")
                    .await?;
            Ok(ProcessedAssistant {
                output,
                should_inject: false,
                show_answer: true,
            })
        }
    }
}

async fn verify_selection(app: &AppHandle, request: &AssistantRequest) -> Result<bool, String> {
    let Some(expected) = &request.selection else {
        return Ok(false);
    };
    let Some(target) = request.target else {
        return Ok(false);
    };
    crate::active_app_context::activate_target(target)?;
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    let (current, current_target) = capture_selection_internal(app).await?;
    Ok(
        crate::active_app_context::same_activation_target(current_target, target)
            && current.text == expected.text
            && !current.secure,
    )
}

pub(crate) fn publish_answer(
    app: &AppHandle,
    request: &AssistantRequest,
    output: String,
    error: Option<String>,
    can_insert: bool,
) {
    let answer = AssistantAnswer {
        action: Some(request.action),
        text: output,
        source_text: request
            .selection
            .as_ref()
            .map(|selection| selection.text.clone())
            .unwrap_or_default(),
        error,
        can_insert,
    };
    if let Ok(mut current) = app.state::<RuntimeState>().assistant_runtime.answer.lock() {
        *current = answer.clone();
    }
    if let Ok(mut target) = app
        .state::<RuntimeState>()
        .assistant_runtime
        .answer_target
        .lock()
    {
        *target = request.target;
    }
    let _ = ensure_answer_window(app);
    if let Some(window) = app.get_webview_window(ANSWER_WINDOW_LABEL) {
        position_answer_window(
            app,
            &window,
            request
                .selection
                .as_ref()
                .and_then(|selection| selection.bounds.as_ref()),
        );
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit(ANSWER_EVENT, answer);
    }
}

fn position_answer_window(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    selection: Option<&SelectionBounds>,
) {
    let anchor = selection
        .map(|bounds| (bounds.x + bounds.width, bounds.y + bounds.height))
        .or_else(|| app.cursor_position().ok().map(|point| (point.x, point.y)));
    let Some((anchor_x, anchor_y)) = anchor else {
        return;
    };
    let Ok(Some(monitor)) = app.monitor_from_point(anchor_x, anchor_y) else {
        return;
    };
    let area = monitor.position();
    let size = monitor.size();
    let left = f64::from(area.x);
    let top = f64::from(area.y);
    let right = left + f64::from(size.width);
    let bottom = top + f64::from(size.height);
    let width = 560.0;
    let height = 420.0;
    let x = (anchor_x + 16.0)
        .min(right - width - 12.0)
        .max(left + 12.0);
    let preferred_y = anchor_y + 16.0;
    let y = if preferred_y + height <= bottom - 12.0 {
        preferred_y
    } else {
        (anchor_y - height - 16.0).max(top + 12.0)
    };
    let _ = window.set_position(tauri::PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    ));
}

fn ensure_answer_window(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window(ANSWER_WINDOW_LABEL).is_some() {
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        ANSWER_WINDOW_LABEL,
        WebviewUrl::App("assistant.html".into()),
    )
    .title("说吧！语音助手")
    .inner_size(560.0, 420.0)
    .min_inner_size(420.0, 280.0)
    .decorations(false)
    .always_on_top(true)
    .visible(false)
    .build()
    .map_err(|error| format!("创建语音助手回答窗失败：{error}"))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_assistant_answer(
    state: tauri::State<'_, RuntimeState>,
) -> Result<AssistantAnswer, String> {
    state
        .assistant_runtime
        .answer
        .lock()
        .map(|answer| answer.clone())
        .map_err(|_| "回答状态锁失败".into())
}

#[tauri::command]
pub(crate) async fn insert_assistant_answer(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let text = state
        .assistant_runtime
        .answer
        .lock()
        .map_err(|_| "回答状态锁失败")?
        .text
        .clone();
    let target = *state
        .assistant_runtime
        .answer_target
        .lock()
        .map_err(|_| "回答目标锁失败")?;
    if text.trim().is_empty() {
        return Err("没有可插入的回答".into());
    }
    if let Some(window) = app.get_webview_window(ANSWER_WINDOW_LABEL) {
        let _ = window.hide();
    }
    let target = target.ok_or_else(|| "原输入窗口已丢失，请复制回答后手动粘贴".to_string())?;
    crate::active_app_context::activate_target(target)?;
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    crate::commands::dictation::inject_text_inner(text, Some("paste".into())).await
}

#[tauri::command]
pub(crate) async fn regenerate_assistant_answer(app: AppHandle) -> Result<(), String> {
    let context = app
        .state::<RuntimeState>()
        .assistant_runtime
        .regeneration
        .lock()
        .map_err(|_| "重新生成状态锁失败")?
        .clone()
        .ok_or_else(|| "当前回答没有可重新生成的上下文".to_string())?;
    let state = app.state::<RuntimeState>();
    match process(&app, &state, &context.request, &context.spoken_text).await {
        Ok(processed) => {
            publish_answer(&app, &context.request, processed.output, None, true);
            Ok(())
        }
        Err(error) => {
            publish_answer(
                &app,
                &context.request,
                String::new(),
                Some(error.clone()),
                false,
            );
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) fn close_assistant_answer(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(ANSWER_WINDOW_LABEL) {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

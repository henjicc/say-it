use crate::persistence::save_persisted_state;
use crate::prelude::*;
use crate::state::*;

#[tauri::command]
pub(crate) fn get_dictation_settings(
    state: tauri::State<'_, RuntimeState>,
) -> Result<DictationSettings, String> {
    state
        .dictation
        .lock()
        .map_err(|_| "Dictation lock failed".to_string())
        .map(|v| v.clone())
}

#[tauri::command]
pub(crate) fn set_dictation_settings(
    app: tauri::AppHandle,
    mut settings: DictationSettings,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    settings.inject_method = normalize_inject_method(Some(&settings.inject_method))
        .unwrap_or_else(|| "paste".to_string());
    for profile in &mut settings.shortcut_profiles {
        profile.id = profile.id.trim().to_string();
        profile.name = profile.name.trim().to_string();
        profile.key_code = profile.key_code.trim().to_string();
        profile.smart_template_id = profile
            .smart_template_id
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        profile.inject_method = normalize_inject_method(profile.inject_method.as_deref());
    }

    crate::commands::shortcuts::replace_dictation_settings(&app, &state, settings)
}

fn normalize_inject_method(value: Option<&str>) -> Option<String> {
    match value {
        Some("paste") => Some("paste".to_string()),
        Some("type") => Some("type".to_string()),
        _ => None,
    }
}

#[tauri::command]
pub(crate) fn resolve_application_identity(
    path: String,
) -> Result<crate::active_app_context::AppIdentity, String> {
    let path = std::path::PathBuf::from(path);
    #[cfg(windows)]
    {
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        {
            return Err("请选择有效的 EXE 格式 Windows 应用程序".into());
        }
        let process_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "无法读取 Windows 应用程序文件名".to_string())?
            .to_string();
        let app_name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(&process_name)
            .to_string();
        Ok(crate::active_app_context::AppIdentity {
            process_name,
            app_name,
            window_title: None,
        })
    }
    #[cfg(target_os = "macos")]
    {
        let identity = crate::macos_native::application_bundle(&path)?;
        Ok(crate::active_app_context::AppIdentity {
            process_name: identity.process_name,
            app_name: identity.app_name,
            window_title: None,
        })
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
        Err("当前平台不支持从本地选择应用程序".into())
    }
}

#[cfg(test)]
fn validate_dictation_settings(
    settings: &DictationSettings,
    state: &RuntimeState,
) -> Result<(), String> {
    let subtitle = state
        .subtitle_shortcut
        .lock()
        .map_err(|_| "Subtitle shortcut lock failed".to_string())?
        .clone();
    crate::commands::shortcuts::validate_shortcut_settings(settings, &subtitle, state)
}

/// 读取启动设置：`autostart` 查询当前平台的系统自启状态，`silent_start` 取本地持久化值。
#[tauri::command]
pub(crate) fn get_startup_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<StartupStatus, String> {
    let autostart = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| format!("读取开机自启状态失败：{error}"))?;
    let silent_start = state
        .startup
        .lock()
        .map_err(|_| "Startup lock failed".to_string())?
        .silent_start;
    Ok(StartupStatus {
        autostart,
        silent_start,
    })
}

/// 写入启动设置：通过当前平台的系统自启机制切换状态，并持久化静默启动偏好。
#[tauri::command]
pub(crate) fn set_startup_settings(
    app: tauri::AppHandle,
    autostart: bool,
    silent_start: bool,
    state: tauri::State<'_, RuntimeState>,
) -> Result<StartupStatus, String> {
    let manager = app.autolaunch();
    let currently = manager
        .is_enabled()
        .map_err(|error| format!("读取开机自启状态失败：{error}"))?;
    if autostart && !currently {
        manager
            .enable()
            .map_err(|e| format!("开启开机自启失败：{e}"))?;
    } else if !autostart && currently {
        manager
            .disable()
            .map_err(|e| format!("关闭开机自启失败：{e}"))?;
    }
    {
        let mut guard = state
            .startup
            .lock()
            .map_err(|_| "Startup lock failed".to_string())?;
        guard.silent_start = silent_start;
    }
    save_persisted_state(&app, &state)?;
    let autostart = manager
        .is_enabled()
        .map_err(|error| format!("确认开机自启状态失败：{error}"))?;
    Ok(StartupStatus {
        autostart,
        silent_start,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardDisposition {
    RestorePrevious,
    KeepResult,
}

pub(crate) async fn write_clipboard_text_inner(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|error| format!("打开剪贴板失败：{error}"))?;
        clipboard
            .set_text(text)
            .map_err(|error| format!("写入剪贴板失败：{error}"))
    })
    .await
    .map_err(|error| format!("剪贴板任务失败：{error}"))?
}

/// 把文本注入当前拥有键盘焦点的窗口。
/// - paste：备份剪贴板 → 写入文本 → 模拟平台粘贴键 → 还原剪贴板（更适合长中文）。
/// - type：逐字 Unicode 模拟输入。
pub(crate) async fn inject_text_inner(text: String, method: Option<String>) -> Result<(), String> {
    inject_text_with_policy(text, method, ClipboardDisposition::RestorePrevious).await
}

pub(crate) async fn inject_text_with_policy(
    text: String,
    method: Option<String>,
    clipboard_disposition: ClipboardDisposition,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let text = text.trim_end_matches(['\r', '\n']).to_string();
    if text.is_empty() {
        return Ok(());
    }
    let method = method.unwrap_or_else(|| "paste".to_string());
    let char_count = text.chars().count();
    dlog!("[inject] 开始注入：方式={method}，{char_count} 字");
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        if clipboard_disposition == ClipboardDisposition::KeepResult {
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败: {e}"))?;
            clipboard
                .set_text(text.clone())
                .map_err(|e| format!("写入剪贴板失败: {e}"))?;
            if method == "type" {
                #[cfg(target_os = "macos")]
                {
                    return crate::macos_native::type_text(&text);
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let mut enigo = Enigo::new(&EnigoSettings::default())
                        .map_err(|e| format!("初始化输入失败: {e}"))?;
                    return enigo
                        .text(&text)
                        .map_err(|e| format!("模拟输入失败: {e}"));
                }
            }
            std::thread::sleep(Duration::from_millis(60));
            #[cfg(target_os = "macos")]
            {
                return crate::macos_native::paste_current_clipboard();
            }
            #[cfg(not(target_os = "macos"))]
            return send_keep_result_paste_shortcut();
        }
        if method == "type" {
            #[cfg(target_os = "macos")]
            {
                return crate::macos_native::type_text(&text);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let mut enigo = Enigo::new(&EnigoSettings::default())
                    .map_err(|e| format!("初始化输入失败: {e}"))?;
                enigo
                    .text(&text)
                    .map_err(|e| format!("模拟输入失败: {e}"))?;
                return Ok(());
            }
        }

        #[cfg(target_os = "macos")]
        {
            // 原生桥接会完整保存剪贴板中的图片、文件和富文本等所有类型，并且仅在
            // 剪贴板未被其他进程再次修改时恢复，避免听写覆盖用户的新复制内容。
            return crate::macos_native::paste_text(&text);
        }

        #[cfg(windows)]
        {
            return crate::windows_native::paste_text(&text);
        }

        #[cfg(not(any(target_os = "macos", windows)))]
        {
            // paste 模式
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败: {e}"))?;
            let previous = clipboard.get_text().ok();
            clipboard
                .set_text(text.clone())
                .map_err(|e| format!("写入剪贴板失败: {e}"))?;
            // 给系统一点时间让剪贴板生效。
            std::thread::sleep(Duration::from_millis(60));

            let paste_result = send_paste_shortcut();

            // 等目标窗口完成粘贴后再还原剪贴板，避免把内容清掉。发送失败时也要
            // 立即还原，不能把识别结果意外留在用户剪贴板里。
            if paste_result.is_ok() {
                std::thread::sleep(Duration::from_millis(180));
            }
            if let Some(prev) = previous {
                let _ = clipboard.set_text(prev);
            }
            paste_result
        }
    })
    .await
    .map_err(|e| format!("注入任务失败: {e}"))?;
    match &result {
        Ok(()) => dlog!("[inject] 注入完成"),
        Err(e) => dlog!("[inject] 注入失败: {e}"),
    }
    crate::application::performance::record("text.injection", started.elapsed().as_millis() as u64);
    result
}

#[cfg(not(target_os = "macos"))]
fn send_keep_result_paste_shortcut() -> Result<(), String> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("初始化输入失败: {e}"))?;
    let modifier = Key::Control;
    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| format!("模拟粘贴失败: {e}"))?;
    let click = enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format!("模拟粘贴失败: {e}"));
    let release = enigo
        .key(modifier, Direction::Release)
        .map_err(|e| format!("释放粘贴按键失败: {e}"));
    click?;
    release
}

#[cfg(not(target_os = "macos"))]
fn send_paste_shortcut() -> Result<(), String> {
    let mut enigo =
        Enigo::new(&EnigoSettings::default()).map_err(|e| format!("初始化输入失败: {e}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| format!("模拟粘贴失败: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format!("模拟粘贴失败: {e}"))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| format!("模拟粘贴失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, name: &str, key_code: &str) -> DictationShortcutProfile {
        DictationShortcutProfile {
            id: id.into(),
            name: name.into(),
            enabled: true,
            key_code: key_code.into(),
            ctrl: true,
            shift: false,
            alt: false,
            meta: false,
            processing_mode: ShortcutProcessingMode::FollowScene,
            trigger_mode: ShortcutTriggerMode::Toggle,
            smart_template_id: None,
            smart_processing_min_chars: None,
            inject_method: None,
        }
    }

    #[test]
    fn duplicate_dictation_shortcuts_are_rejected() {
        let state = RuntimeState::default();
        let settings = DictationSettings {
            key_code: "F9".into(),
            ctrl: true,
            shortcut_profiles: vec![profile("one", "方案一", "F9")],
            ..Default::default()
        };
        let error = validate_dictation_settings(&settings, &state).unwrap_err();
        assert!(error.contains("相同快捷键"));
    }

    #[test]
    fn same_shortcut_with_different_trigger_modes_is_allowed() {
        let state = RuntimeState::default();
        let mut hold = profile("hold", "长按方案", "F9");
        hold.trigger_mode = ShortcutTriggerMode::PressHold;
        let settings = DictationSettings {
            key_code: "F9".into(),
            ctrl: true,
            press_hold_mode: false,
            shortcut_profiles: vec![hold],
            ..Default::default()
        };
        assert!(validate_dictation_settings(&settings, &state).is_ok());
    }

    #[test]
    fn same_profile_shortcut_and_trigger_mode_is_rejected() {
        let state = RuntimeState::default();
        let mut first = profile("one", "长按一", "F9");
        first.trigger_mode = ShortcutTriggerMode::PressHold;
        let mut second = profile("two", "长按二", "F9");
        second.trigger_mode = ShortcutTriggerMode::PressHold;
        let settings = DictationSettings {
            key_code: String::new(),
            shortcut_profiles: vec![first, second],
            ..Default::default()
        };
        assert!(validate_dictation_settings(&settings, &state)
            .unwrap_err()
            .contains("相同快捷键和触发方式"));
    }

    #[test]
    fn enabled_profile_requires_a_key_but_disabled_draft_does_not() {
        let state = RuntimeState::default();
        let mut draft = profile("draft", "草稿", "");
        draft.enabled = false;
        let settings = DictationSettings {
            shortcut_profiles: vec![draft.clone()],
            ..Default::default()
        };
        assert!(validate_dictation_settings(&settings, &state).is_ok());

        draft.enabled = true;
        let settings = DictationSettings {
            shortcut_profiles: vec![draft],
            ..Default::default()
        };
        assert!(validate_dictation_settings(&settings, &state)
            .unwrap_err()
            .contains("尚未设置快捷键"));
    }

    #[test]
    fn shortcut_profile_limits_and_ids_are_validated() {
        let state = RuntimeState::default();
        let mut settings = DictationSettings {
            shortcut_profiles: (0..=MAX_DICTATION_SHORTCUT_PROFILES)
                .map(|index| profile(&format!("id-{index}"), &format!("方案{index}"), "F9"))
                .collect(),
            ..Default::default()
        };
        assert!(validate_dictation_settings(&settings, &state)
            .unwrap_err()
            .contains("不能超过"));

        settings.shortcut_profiles = vec![
            profile("same", "方案一", "F9"),
            profile("same", "方案二", "F10"),
        ];
        assert!(validate_dictation_settings(&settings, &state)
            .unwrap_err()
            .contains("ID"));
    }
}

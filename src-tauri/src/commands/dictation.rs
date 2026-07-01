use crate::prelude::*;
use crate::persistence::save_persisted_state;
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
    settings: DictationSettings,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    if settings.key_code.trim().is_empty() {
        return Err("快捷键不能为空".to_string());
    }
    // 先尝试应用到钩子，确认键码受支持，再写入并持久化。
    apply_dictation_hotkey(&settings)?;
    {
        let mut guard = state
            .dictation
            .lock()
            .map_err(|_| "Dictation lock failed".to_string())?;
        guard.key_code = settings.key_code;
        guard.ctrl = settings.ctrl;
        guard.shift = settings.shift;
        guard.alt = settings.alt;
        guard.meta = settings.meta;
        guard.inject_method = if settings.inject_method == "type" {
            "type".to_string()
        } else {
            "paste".to_string()
        };
    }
    save_persisted_state(&app, &state)?;
    Ok(())
}

/// 读取启动设置：`autostart` 查询系统注册表实际状态，`silent_start` 取本地持久化值。
#[tauri::command]
pub(crate) fn get_startup_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<StartupStatus, String> {
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
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

/// 写入启动设置：开关开机自启（写系统注册表），并持久化静默启动偏好。
#[tauri::command]
pub(crate) fn set_startup_settings(
    app: tauri::AppHandle,
    autostart: bool,
    silent_start: bool,
    state: tauri::State<'_, RuntimeState>,
) -> Result<StartupStatus, String> {
    let manager = app.autolaunch();
    let currently = manager.is_enabled().unwrap_or(false);
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
    let autostart = manager.is_enabled().unwrap_or(autostart);
    Ok(StartupStatus {
        autostart,
        silent_start,
    })
}

/// 把文本注入当前拥有键盘焦点的窗口。
/// - paste：备份剪贴板 → 写入文本 → 模拟 Ctrl+V → 还原剪贴板（更适合长中文）。
/// - type：逐字 Unicode 模拟输入。
#[tauri::command]
pub(crate) async fn inject_text(text: String, method: Option<String>) -> Result<(), String> {
    let text = text.trim_end_matches(['\r', '\n']).to_string();
    if text.is_empty() {
        return Ok(());
    }
    let method = method.unwrap_or_else(|| "paste".to_string());
    let char_count = text.chars().count();
    dlog!("[inject] 开始注入：方式={method}，{char_count} 字");
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        if method == "type" {
            let mut enigo = Enigo::new(&EnigoSettings::default())
                .map_err(|e| format!("初始化输入失败: {e}"))?;
            enigo
                .text(&text)
                .map_err(|e| format!("模拟输入失败: {e}"))?;
            return Ok(());
        }

        // paste 模式
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败: {e}"))?;
        let previous = clipboard.get_text().ok();
        clipboard
            .set_text(text.clone())
            .map_err(|e| format!("写入剪贴板失败: {e}"))?;
        // 给系统一点时间让剪贴板生效。
        std::thread::sleep(Duration::from_millis(60));

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

        // 等目标窗口完成粘贴后再还原剪贴板，避免把内容清掉。
        std::thread::sleep(Duration::from_millis(180));
        if let Some(prev) = previous {
            let _ = clipboard.set_text(prev);
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("注入任务失败: {e}"))?;
    match &result {
        Ok(()) => dlog!("[inject] 注入完成"),
        Err(e) => dlog!("[inject] 注入失败: {e}"),
    }
    result
}


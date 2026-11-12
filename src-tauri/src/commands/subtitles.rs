use crate::state::*;
use serde::Serialize;

/// 字幕快捷键配置，外加一个只存在于运行时的标志：它是否真的被全局接管了。
/// 配置本身不带这个字段，所以用 flatten 包一层，不污染持久化结构。
#[derive(Debug, Serialize)]
pub(crate) struct SubtitleShortcutResponse {
    #[serde(flatten)]
    settings: SubtitleShortcutSettings,
    /// 为真时前端不得再装焦点兜底：全局路径已经会触发一次。
    global_registered: bool,
}

#[tauri::command]
pub(crate) fn get_subtitle_shortcut(
    state: tauri::State<'_, RuntimeState>,
) -> Result<SubtitleShortcutResponse, String> {
    let settings = state
        .subtitle_shortcut
        .lock()
        .map_err(|_| "Subtitle shortcut lock failed".to_string())?
        .clone();
    Ok(SubtitleShortcutResponse {
        settings,
        global_registered: crate::hotkey::subtitle_hotkey_registered(),
    })
}

#[tauri::command]
pub(crate) fn set_subtitle_shortcut(
    app: tauri::AppHandle,
    mut settings: SubtitleShortcutSettings,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    settings.key_code = settings.key_code.trim().to_string();
    crate::commands::shortcuts::replace_subtitle_shortcut(&app, &state, settings)
}


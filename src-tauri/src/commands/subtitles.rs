use crate::persistence::save_persisted_state;
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

fn normalize_translation_model(
    model: &str,
    plugins: Option<&crate::providers::plugin::PluginRegistry>,
) -> Result<String, String> {
    let model = model.trim();
    match model {
        "" | "none" => Ok("none".to_string()),
        "qwen-mt-flash" | "qwen-mt-plus" | "qwen-mt-lite" => Ok(model.to_string()),
        _ if plugins.is_some_and(|plugins| {
            plugins.model(model).is_some_and(|info| {
                info.category == "translation"
                    && info
                        .scenes
                        .iter()
                        .any(|scene| scene == "subtitleTranslation")
            })
        }) =>
        {
            Ok(model.to_string())
        }
        _ => Err(format!("不支持的字幕翻译模型：{model}")),
    }
}

#[tauri::command]
pub(crate) fn get_subtitle_translation_model(
    state: tauri::State<'_, RuntimeState>,
) -> Result<String, String> {
    let model = state
        .subtitle_translation_model
        .lock()
        .map_err(|_| "Subtitle translation model lock failed".to_string())?;
    let plugins = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?;
    normalize_translation_model(&model, Some(&plugins))
}

#[tauri::command]
pub(crate) fn set_subtitle_translation_model(
    app: tauri::AppHandle,
    model: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let model = {
        let plugins = state
            .plugin_registry
            .lock()
            .map_err(|_| "插件注册表锁失败".to_string())?;
        normalize_translation_model(&model, Some(&plugins))?
    };
    {
        let mut guard = state
            .subtitle_translation_model
            .lock()
            .map_err(|_| "Subtitle translation model lock failed".to_string())?;
        *guard = model;
    }
    save_persisted_state(&app, &state)
}

#[cfg(test)]
mod tests {
    use super::normalize_translation_model;

    #[test]
    fn translation_model_keeps_none_and_rejects_unknown_values() {
        assert_eq!(normalize_translation_model("", None).unwrap(), "none");
        assert_eq!(normalize_translation_model("none", None).unwrap(), "none");
        assert_eq!(
            normalize_translation_model("qwen-mt-plus", None).unwrap(),
            "qwen-mt-plus"
        );
        assert!(normalize_translation_model("unknown", None).is_err());
    }
}

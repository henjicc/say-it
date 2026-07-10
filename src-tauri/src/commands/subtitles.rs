use crate::persistence::save_persisted_state;
use crate::state::*;

#[tauri::command]
pub(crate) fn get_subtitle_shortcut(
    state: tauri::State<'_, RuntimeState>,
) -> Result<SubtitleShortcutSettings, String> {
    state
        .subtitle_shortcut
        .lock()
        .map_err(|_| "Subtitle shortcut lock failed".to_string())
        .map(|v| v.clone())
}

#[tauri::command]
pub(crate) fn set_subtitle_shortcut(
    app: tauri::AppHandle,
    settings: SubtitleShortcutSettings,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    if !settings.key_code.trim().is_empty() {
        let dictation = state
            .dictation
            .lock()
            .map_err(|_| "Dictation lock failed".to_string())?;
        if dictation.key_code == settings.key_code
            && dictation_mods(&dictation) == subtitle_shortcut_mods(&settings)
        {
            return Err("该快捷键已被语音输入占用".to_string());
        }
    }
    apply_subtitle_hotkey(&settings)?;
    {
        let mut guard = state
            .subtitle_shortcut
            .lock()
            .map_err(|_| "Subtitle shortcut lock failed".to_string())?;
        *guard = settings;
    }
    save_persisted_state(&app, &state)?;
    Ok(())
}

fn normalize_translation_model(model: &str) -> Result<String, String> {
    let model = model.trim();
    match model {
        "" | "none" => Ok("none".to_string()),
        "qwen-mt-flash" | "qwen-mt-plus" | "qwen-mt-lite" => Ok(model.to_string()),
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
    normalize_translation_model(&model)
}

#[tauri::command]
pub(crate) fn set_subtitle_translation_model(
    app: tauri::AppHandle,
    model: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let model = normalize_translation_model(&model)?;
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
        assert_eq!(normalize_translation_model("").unwrap(), "none");
        assert_eq!(normalize_translation_model("none").unwrap(), "none");
        assert_eq!(normalize_translation_model("qwen-mt-plus").unwrap(), "qwen-mt-plus");
        assert!(normalize_translation_model("unknown").is_err());
    }
}

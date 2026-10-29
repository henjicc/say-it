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
    read_translation_model(&model, Some(&plugins))
}

/// 读取已保存的字幕翻译模型。
///
/// 读取不得因为那个选择不再可用就失败。提供译文模型的插件被卸载/停用后，
/// 这里原本直接返回 Err，而前端的 `loadTranslationModel` 是启动恢复那批 loader 里唯一
/// 会把错误招出去的：一旦 reject，监听各 runtime、revision 对账那整段运行时投影恢复全部
/// 被跳过，界面停在空状态，只留一行 console.error。
///
/// 存储里的值不动：卸载逻辑本来就故意保留一份配置，插件重装后选择自动恢复。
/// 写入侧（`set_subtitle_translation_model`）仍然严格拒绝未知 id——那是用户的一次主动选择。
fn read_translation_model(
    model: &str,
    plugins: Option<&crate::providers::plugin::PluginRegistry>,
) -> Result<String, String> {
    Ok(normalize_translation_model(model, plugins).unwrap_or_else(|_| "none".to_string()))
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
    use super::{normalize_translation_model, read_translation_model};

    /// 写入拒绝的 id，读取必须降级为 none 而不是跟着失败。
    ///
    /// 两侧语义一旦拉齐成「都报错」，提供译文模型的插件被卸载后，主窗口启动时的
    /// 整段运行时投影恢复都会被这一条 reject 跳过。
    #[test]
    fn an_unavailable_saved_model_reads_back_as_none() {
        assert!(normalize_translation_model("plugin-gone", None).is_err());
        assert_eq!(read_translation_model("plugin-gone", None).unwrap(), "none");
    }

    #[test]
    fn reading_keeps_a_still_available_model() {
        assert_eq!(
            read_translation_model("qwen-mt-flash", None).unwrap(),
            "qwen-mt-flash"
        );
        assert_eq!(read_translation_model("", None).unwrap(), "none");
    }

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

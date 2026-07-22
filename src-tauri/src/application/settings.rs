use crate::persistence::save_persisted_state_with_app_settings;
use crate::state::RuntimeState;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, State};

pub(crate) const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSettings {
    #[serde(default = "schema_version")]
    pub(crate) schema_version: u32,
    #[serde(default)]
    pub(crate) legacy_imported: bool,
    #[serde(default = "empty_object")]
    pub(crate) dictation_prefs: Value,
    #[serde(default = "empty_object")]
    pub(crate) subtitle_prefs: Value,
    #[serde(default = "empty_object")]
    pub(crate) compare_prefs: Value,
    /// 全局热词与上下文，见 `application::customization`。
    #[serde(default = "empty_object")]
    pub(crate) customization_prefs: Value,
    #[serde(default = "default_theme")]
    pub(crate) theme: Value,
    #[serde(default)]
    pub(crate) custom_cue_start: Option<CustomCueFile>,
    #[serde(default)]
    pub(crate) custom_cue_end: Option<CustomCueFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomCueFile {
    pub(crate) relative_path: String,
    pub(crate) mime_type: String,
}

fn schema_version() -> u32 {
    SETTINGS_SCHEMA_VERSION
}
fn empty_object() -> Value {
    serde_json::json!({})
}
fn default_theme() -> Value {
    serde_json::json!({"tone":"dark","accent":"#5199FF"})
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: schema_version(),
            legacy_imported: false,
            dictation_prefs: empty_object(),
            subtitle_prefs: empty_object(),
            compare_prefs: empty_object(),
            customization_prefs: empty_object(),
            theme: default_theme(),
            custom_cue_start: None,
            custom_cue_end: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacySettingsImport {
    pub(crate) dictation_prefs: Option<Value>,
    pub(crate) subtitle_prefs: Option<Value>,
    pub(crate) compare_prefs: Option<Value>,
    pub(crate) theme: Option<Value>,
    pub(crate) custom_cue_start: Option<String>,
    pub(crate) custom_cue_end: Option<String>,
}

fn valid_object(value: &Value) -> Result<(), String> {
    if value.is_object() {
        Ok(())
    } else {
        Err("配置必须是 JSON 对象".into())
    }
}

#[tauri::command]
pub(crate) fn import_legacy_settings(
    app: AppHandle,
    state: State<'_, RuntimeState>,
    legacy: LegacySettingsImport,
    retry: Option<bool>,
) -> Result<AppSettings, String> {
    let mut current = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败")?
        .clone();
    if current.legacy_imported && retry != Some(true) {
        return Ok(current);
    }
    if current.dictation_prefs == empty_object() {
        if let Some(v) = legacy.dictation_prefs {
            valid_object(&v)?;
            current.dictation_prefs = v;
        }
    }
    if current.subtitle_prefs == empty_object() {
        if let Some(v) = legacy.subtitle_prefs {
            valid_object(&v)?;
            current.subtitle_prefs = v;
        }
    }
    if current.compare_prefs == empty_object() {
        if let Some(v) = legacy.compare_prefs {
            valid_object(&v)?;
            current.compare_prefs = v;
        }
    }
    if current.theme == default_theme() {
        if let Some(v) = legacy.theme {
            valid_object(&v)?;
            current.theme = v;
        }
    }
    if current.custom_cue_start.is_none() {
        if let Some(data) = legacy.custom_cue_start {
            current.custom_cue_start = Some(store_cue(&app, "start", &data)?);
        }
    }
    if current.custom_cue_end.is_none() {
        if let Some(data) = legacy.custom_cue_end {
            current.custom_cue_end = Some(store_cue(&app, "end", &data)?);
        }
    }
    current.legacy_imported = true;
    save_settings_then_commit(&app, &state, current.clone())?;
    Ok(current)
}

#[tauri::command]
pub(crate) fn update_app_settings(
    app: AppHandle,
    state: State<'_, RuntimeState>,
    domain: String,
    value: Value,
) -> Result<AppSettings, String> {
    valid_object(&value)?;
    if domain == "dictation" {
        crate::application::dictation::validate_dictation_settings_value(&value)?;
    }
    if domain == "customization" {
        crate::application::customization::validate_customization_settings_value(&value)?;
    }
    let mut next = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败")?
        .clone();
    match domain.as_str() {
        "dictation" => next.dictation_prefs = value,
        "subtitles" => next.subtitle_prefs = value,
        "comparison" => next.compare_prefs = value,
        "customization" => next.customization_prefs = value,
        "theme" => next.theme = value,
        _ => return Err(format!("未知配置领域：{domain}")),
    }
    // 模板目录和快捷键方案分属两份持久化设置。后端在同一保存路径清理孤立引用，
    // 即使调用方不是当前前端，也不会留下启动后才报错的快捷键方案。
    let previous_dictation = if domain == "dictation" {
        let mut dictation = state
            .dictation
            .lock()
            .map_err(|_| "Dictation lock failed".to_string())?;
        let previous = dictation.clone();
        prune_shortcut_template_references(&mut dictation, &next.dictation_prefs);
        (previous != *dictation).then_some(previous)
    } else {
        None
    };
    if let Err(error) = save_settings_then_commit(&app, &state, next.clone()) {
        if let Some(previous) = previous_dictation {
            if let Ok(mut dictation) = state.dictation.lock() {
                *dictation = previous;
            }
        }
        return Err(error);
    }
    Ok(next)
}

fn prune_shortcut_template_references(
    dictation: &mut crate::state::DictationSettings,
    prefs: &Value,
) {
    let valid_templates = prefs
        .get("smartTemplates")
        .and_then(Value::as_array)
        .map(|templates| {
            templates
                .iter()
                .filter_map(|template| template.get("id").and_then(Value::as_str))
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    for profile in &mut dictation.shortcut_profiles {
        if profile
            .smart_template_id
            .as_deref()
            .is_some_and(|id| !valid_templates.contains(id))
        {
            profile.smart_template_id = None;
        }
    }
}

#[tauri::command]
pub(crate) fn update_custom_cue(
    app: AppHandle,
    state: State<'_, RuntimeState>,
    which: String,
    data_url: String,
) -> Result<AppSettings, String> {
    let mut next = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败")?
        .clone();
    let stored = store_cue(&app, &which, &data_url)?;
    match which.as_str() {
        "start" => next.custom_cue_start = Some(stored),
        "end" => next.custom_cue_end = Some(stored),
        _ => return Err("未知提示音位置".into()),
    }
    save_settings_then_commit(&app, &state, next.clone())?;
    Ok(next)
}

fn store_cue(app: &AppHandle, which: &str, data_url: &str) -> Result<CustomCueFile, String> {
    if which != "start" && which != "end" {
        return Err("未知提示音位置".into());
    }
    if data_url.len() > 20 * 1024 * 1024 {
        return Err("提示音数据超过 20 MiB".into());
    }
    let (header, encoded) = data_url.split_once(',').ok_or("提示音 Data URL 无效")?;
    let mime = header
        .strip_prefix("data:")
        .and_then(|v| v.strip_suffix(";base64"))
        .filter(|v| v.starts_with("audio/"))
        .ok_or("提示音 MIME 类型无效")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("提示音 Base64 无效：{e}"))?;
    let dir = crate::application::data_root::data_subdir(app, "cues")
        .map_err(|e| format!("创建提示音目录失败：{e}"))?;
    let relative = format!("cues/{which}.audio");
    let file = dir.join(format!("{which}.audio"));
    let tmp = dir.join(format!("{which}.audio.tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| format!("写入提示音失败：{e}"))?;
    if file.exists() {
        std::fs::remove_file(&file).map_err(|e| format!("替换提示音失败：{e}"))?;
    }
    std::fs::rename(tmp, file).map_err(|e| format!("提交提示音失败：{e}"))?;
    Ok(CustomCueFile {
        relative_path: relative,
        mime_type: mime.to_string(),
    })
}

fn save_settings_then_commit(
    app: &AppHandle,
    state: &State<'_, RuntimeState>,
    next: AppSettings,
) -> Result<(), String> {
    save_persisted_state_with_app_settings(app, state, Some(&next))?;
    *state.app_settings.lock().map_err(|_| "应用配置锁失败")? = next;
    crate::application::contract::next_revision(&state.snapshot_revision);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_json_gets_safe_defaults() {
        let v: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(v.schema_version, 1);
        assert!(v.dictation_prefs.is_object());
    }
    #[test]
    fn rejects_non_object_domain() {
        assert!(valid_object(&serde_json::json!([])).is_err());
    }
    #[test]
    fn deleting_a_template_downgrades_shortcut_override_to_inherit() {
        let mut dictation = crate::state::DictationSettings::default();
        dictation
            .shortcut_profiles
            .push(crate::state::DictationShortcutProfile {
                id: "shortcut".into(),
                name: "方案".into(),
                enabled: false,
                key_code: String::new(),
                ctrl: false,
                shift: false,
                alt: false,
                meta: false,
                processing_mode: crate::state::ShortcutProcessingMode::FollowScene,
                smart_template_id: Some("deleted".into()),
                smart_processing_min_chars: None,
                inject_method: None,
            });
        prune_shortcut_template_references(
            &mut dictation,
            &serde_json::json!({"smartTemplates":[{"id":"remaining"}]}),
        );
        assert!(dictation.shortcut_profiles[0].smart_template_id.is_none());
    }
}

use crate::obs_overlay::ObsOverlaySettings;
use crate::prelude::*;
use crate::state::*;
use std::path::Path;

const STATE_FILE_NAME: &str = "say-it-state.json";
const LEGACY_APP_IDENTIFIERS: &[&str] = &["com.vibecode.sayit"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PersistedData {
    #[serde(default = "default_schema_version")]
    pub(crate) schema_version: u32,
    #[serde(default)]
    pub(crate) app_settings: crate::application::settings::AppSettings,
    #[serde(default)]
    pub(crate) providers: ProviderSettings,
    #[serde(default)]
    pub(crate) dictation: DictationSettings,
    #[serde(default)]
    pub(crate) subtitle_shortcut: SubtitleShortcutSettings,
    #[serde(default = "default_subtitle_translation_model")]
    pub(crate) subtitle_translation_model: String,
    #[serde(default)]
    pub(crate) startup: StartupSettings,
    #[serde(default)]
    pub(crate) obs_overlay: ObsOverlaySettings,
}

fn default_schema_version() -> u32 { 1 }

fn default_subtitle_translation_model() -> String {
    "none".to_string()
}

pub(crate) fn state_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(STATE_FILE_NAME))
}

fn legacy_state_file_paths(app: &tauri::AppHandle) -> Result<Vec<PathBuf>, String> {
    let current_dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let parent = current_dir
        .parent()
        .ok_or_else(|| "无法定位应用数据目录父级".to_string())?;
    Ok(LEGACY_APP_IDENTIFIERS
        .iter()
        .map(|identifier| parent.join(identifier).join(STATE_FILE_NAME))
        .collect())
}

pub(crate) fn save_persisted_state(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    save_persisted_state_with_app_settings(app, state, None)
}

pub(crate) fn save_persisted_state_with_app_settings(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    app_settings_override: Option<&crate::application::settings::AppSettings>,
) -> Result<(), String> {
    let providers = state
        .providers
        .lock()
        .map_err(|_| "Provider settings lock failed".to_string())?
        .clone();
    let dictation = state
        .dictation
        .lock()
        .map_err(|_| "Dictation lock failed".to_string())?
        .clone();
    let subtitle_shortcut = state
        .subtitle_shortcut
        .lock()
        .map_err(|_| "Subtitle shortcut lock failed".to_string())?
        .clone();
    let subtitle_translation_model = state
        .subtitle_translation_model
        .lock()
        .map_err(|_| "Subtitle translation model lock failed".to_string())?
        .clone();
    let startup = state
        .startup
        .lock()
        .map_err(|_| "Startup lock failed".to_string())?
        .clone();
    let obs_overlay = state
        .obs_overlay_settings
        .lock()
        .map_err(|_| "OBS overlay settings lock failed".to_string())?
        .clone();
    let data = PersistedData {
        schema_version: default_schema_version(),
        app_settings: match app_settings_override {
            Some(settings) => settings.clone(),
            None => state.app_settings.lock().map_err(|_| "App settings lock failed".to_string())?.clone(),
        },
        providers: normalize_settings(providers),
        dictation,
        subtitle_shortcut,
        subtitle_translation_model: if subtitle_translation_model.trim().is_empty() {
            default_subtitle_translation_model()
        } else {
            subtitle_translation_model
        },
        startup,
        obs_overlay,
    };
    let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
    let file = state_file_path(app)?;
    atomic_write_with_backup(&file, &bytes)
}

fn atomic_write_with_backup(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = file.with_extension("json.tmp");
    let backup = file.with_extension("json.bak");
    {
        let mut output = fs::File::create(&tmp).map_err(|e| format!("创建配置临时文件失败：{e}"))?;
        use std::io::Write;
        output.write_all(bytes).map_err(|e| format!("写入配置临时文件失败：{e}"))?;
        output.sync_all().map_err(|e| format!("刷新配置临时文件失败：{e}"))?;
    }
    if file.exists() { fs::copy(file, &backup).map_err(|e| format!("备份原配置失败：{e}"))?; fs::remove_file(file).map_err(|e| format!("替换配置前移除旧文件失败：{e}"))?; }
    if let Err(error) = fs::rename(&tmp, file) {
        if backup.exists() { let _ = fs::copy(&backup, file); }
        return Err(format!("提交配置文件失败，已尝试恢复备份：{error}"));
    }
    Ok(())
}

pub(crate) fn load_persisted_state(
    app: &tauri::AppHandle,
) -> Result<Option<PersistedData>, String> {
    let file = state_file_path(app)?;
    let source = if file.exists() {
        Some(file)
    } else {
        legacy_state_file_paths(app)?
            .into_iter()
            .find(|legacy| legacy.exists())
    };
    let Some(source) = source else {
        return Ok(None);
    };
    let text = fs::read_to_string(&source).map_err(|e| e.to_string())?;
    let mut data = match serde_json::from_str::<PersistedData>(&text) {
        Ok(data) => data,
        Err(primary) => {
            let backup = source.with_extension("json.bak");
            let backup_text = fs::read_to_string(&backup).map_err(|_| format!("配置文件损坏且备份不可用：{primary}"))?;
            serde_json::from_str(&backup_text).map_err(|backup_error| format!("配置文件及备份均损坏：主文件 {primary}；备份 {backup_error}"))?
        }
    };
    data.providers = normalize_settings(data.providers);
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_state_defaults_subtitle_translation_to_none() {
        let data: PersistedData = serde_json::from_str("{}").unwrap();
        assert_eq!(data.subtitle_translation_model, "none");
        assert_eq!(data.schema_version, 1);
    }

    #[test]
    fn atomic_write_keeps_previous_backup() {
        let dir = std::env::temp_dir().join(format!("say-it-persistence-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir); fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        atomic_write_with_backup(&file, b"one").unwrap(); atomic_write_with_backup(&file, b"two").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "two"); assert_eq!(fs::read_to_string(file.with_extension("json.bak")).unwrap(), "one");
        fs::remove_dir_all(dir).unwrap();
    }
}

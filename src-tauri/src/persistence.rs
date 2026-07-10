use crate::obs_overlay::ObsOverlaySettings;
use crate::prelude::*;
use crate::state::*;

const STATE_FILE_NAME: &str = "say-it-state.json";
const LEGACY_APP_IDENTIFIERS: &[&str] = &["com.vibecode.sayit"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PersistedData {
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
    fs::write(file, bytes).map_err(|e| e.to_string())
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
    let text = fs::read_to_string(source).map_err(|e| e.to_string())?;
    let mut data = serde_json::from_str::<PersistedData>(&text).map_err(|e| e.to_string())?;
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
    }
}

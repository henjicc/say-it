use crate::prelude::*;
use crate::state::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PersistedData {
    #[serde(default)]
    pub(crate) providers: ProviderSettings,
    #[serde(default)]
    pub(crate) dictation: DictationSettings,
    #[serde(default)]
    pub(crate) startup: StartupSettings,
}

pub(crate) fn state_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("say-it-state.json"))
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
    let startup = state
        .startup
        .lock()
        .map_err(|_| "Startup lock failed".to_string())?
        .clone();
    let data = PersistedData {
        providers: normalize_settings(providers),
        dictation,
        startup,
    };
    let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
    let file = state_file_path(app)?;
    fs::write(file, bytes).map_err(|e| e.to_string())
}

pub(crate) fn load_persisted_state(app: &tauri::AppHandle) -> Result<Option<PersistedData>, String> {
    let file = state_file_path(app)?;
    if !file.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    let mut data = serde_json::from_str::<PersistedData>(&text).map_err(|e| e.to_string())?;
    data.providers = normalize_settings(data.providers);
    Ok(Some(data))
}

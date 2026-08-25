use crate::obs_overlay::ObsOverlaySettings;
use crate::prelude::*;
use crate::state::*;
use std::path::Path;

const STATE_FILE_NAME: &str = "say-it-state.json";
const LEGACY_APP_IDENTIFIERS: &[&str] = &["com.vibecode.sayit"];
const CURRENT_STATE_SCHEMA_VERSION: u32 = 2;
const FOUR_CLICK_DEFAULT_SCHEMA_VERSION: u32 = 2;

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
    #[serde(default)]
    pub(crate) assistant_shortcuts: crate::application::assistant::AssistantShortcutSettings,
    #[serde(default = "default_subtitle_translation_model")]
    pub(crate) subtitle_translation_model: String,
    #[serde(default)]
    pub(crate) startup: StartupSettings,
    #[serde(default)]
    pub(crate) obs_overlay: ObsOverlaySettings,
    #[serde(default)]
    pub(crate) floating_orb: FloatingOrbSettings,
    #[serde(default)]
    pub(crate) mouse_gesture: MouseGestureSettings,
}

fn default_schema_version() -> u32 {
    1
}

fn default_subtitle_translation_model() -> String {
    "none".to_string()
}

pub(crate) fn state_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    crate::application::data_root::data_file(app, STATE_FILE_NAME)
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
    let assistant_shortcuts = state
        .assistant_shortcuts
        .lock()
        .map_err(|_| "智能助手快捷键配置锁失败".to_string())?
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
    let floating_orb = state
        .floating_orb
        .lock()
        .map_err(|_| "Floating orb settings lock failed".to_string())?
        .clone();
    let mouse_gesture = state
        .mouse_gesture
        .lock()
        .map_err(|_| "Mouse gesture settings lock failed".to_string())?
        .clone();
    let data = PersistedData {
        schema_version: CURRENT_STATE_SCHEMA_VERSION,
        app_settings: match app_settings_override {
            Some(settings) => settings.clone(),
            None => state
                .app_settings
                .lock()
                .map_err(|_| "App settings lock failed".to_string())?
                .clone(),
        },
        providers: normalize_settings(providers),
        dictation,
        subtitle_shortcut,
        assistant_shortcuts,
        subtitle_translation_model: if subtitle_translation_model.trim().is_empty() {
            default_subtitle_translation_model()
        } else {
            subtitle_translation_model
        },
        startup,
        obs_overlay,
        floating_orb,
        mouse_gesture,
    };
    let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
    let file = state_file_path(app)?;
    atomic_write_with_backup(&file, &bytes)
}

fn atomic_write_with_backup(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = file.with_extension("json.tmp");
    let backup = file.with_extension("json.bak");
    {
        let mut output =
            fs::File::create(&tmp).map_err(|e| format!("创建配置临时文件失败：{e}"))?;
        use std::io::Write;
        output
            .write_all(bytes)
            .map_err(|e| format!("写入配置临时文件失败：{e}"))?;
        output
            .sync_all()
            .map_err(|e| format!("刷新配置临时文件失败：{e}"))?;
    }
    if file.exists() {
        fs::copy(file, &backup).map_err(|e| format!("备份原配置失败：{e}"))?;
        // Unix（包括 macOS）的 rename 可原子替换同目录目标。先删除旧文件会制造一个
        // 掉电窗口，使主配置暂时不存在；Windows 则仍需先移除目标文件。
        #[cfg(windows)]
        fs::remove_file(file).map_err(|e| format!("替换配置前移除旧文件失败：{e}"))?;
    }
    if let Err(error) = fs::rename(&tmp, file) {
        if !file.exists() && backup.exists() {
            let _ = fs::copy(&backup, file);
        }
        let _ = fs::remove_file(&tmp);
        return Err(format!("提交配置文件失败，已尝试恢复备份：{error}"));
    }
    Ok(())
}

fn persisted_state_files_exist(file: &Path) -> bool {
    file.exists() || file.with_extension("json.bak").exists()
}

fn migrate_persisted_data(data: &mut PersistedData) {
    if data.schema_version < FOUR_CLICK_DEFAULT_SCHEMA_VERSION
        && data.mouse_gesture.rapid_click_count == 3
    {
        data.mouse_gesture.rapid_click_count = DEFAULT_MOUSE_RAPID_CLICK_COUNT;
    }
    if data.schema_version < CURRENT_STATE_SCHEMA_VERSION {
        data.schema_version = CURRENT_STATE_SCHEMA_VERSION;
    }
}

fn load_persisted_data_from_path(file: &Path) -> Result<PersistedData, String> {
    let backup = file.with_extension("json.bak");
    if !file.exists() {
        let text = fs::read_to_string(&backup)
            .map_err(|error| format!("主配置不存在且读取备份失败：{error}"))?;
        return serde_json::from_str(&text)
            .map_err(|error| format!("主配置不存在且备份损坏：{error}"));
    }

    let text = fs::read_to_string(file).map_err(|error| format!("读取配置文件失败：{error}"))?;
    match serde_json::from_str::<PersistedData>(&text) {
        Ok(data) => Ok(data),
        Err(primary) => {
            let backup_text = fs::read_to_string(&backup)
                .map_err(|_| format!("配置文件损坏且备份不可用：{primary}"))?;
            serde_json::from_str(&backup_text).map_err(|backup_error| {
                format!("配置文件及备份均损坏：主文件 {primary}；备份 {backup_error}")
            })
        }
    }
}

pub(crate) fn load_persisted_state(
    app: &tauri::AppHandle,
) -> Result<Option<PersistedData>, String> {
    let file = state_file_path(app)?;
    let source = if persisted_state_files_exist(&file) {
        Some(file)
    } else {
        legacy_state_file_paths(app)?
            .into_iter()
            .find(|legacy| persisted_state_files_exist(legacy))
    };
    let Some(source) = source else {
        return Ok(None);
    };
    let mut data = load_persisted_data_from_path(&source)?;
    migrate_persisted_data(&mut data);
    data.providers = normalize_settings(data.providers);
    crate::application::dictation::repair_empty_asr_model(&mut data.app_settings.dictation_prefs);
    crate::application::customization::migrate_legacy_provider_hotwords(
        &mut data.app_settings,
        &mut data.providers,
    );
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
        assert!(!data.floating_orb.enabled);
        assert!(data.floating_orb.position.is_none());
        assert_eq!(data.floating_orb.size_percent, 45);
        assert_eq!(data.floating_orb.opacity, 100);
        assert!(!data.floating_orb.glass_enabled);
        assert_eq!(
            data.floating_orb.glass_material,
            FloatingOrbGlassMaterial::Sidebar
        );
        assert_eq!(data.floating_orb.glass_tint, 8);
        assert_eq!(data.floating_orb.glass_border, 0);
        assert!(!data.mouse_gesture.enabled);
        assert_eq!(data.mouse_gesture.mode, MouseGestureMode::Confirm);
        assert_eq!(data.mouse_gesture.sensitivity, 50);
        assert!(data.mouse_gesture.rapid_click_enabled);
        assert_eq!(data.mouse_gesture.rapid_click_count, 4);
    }

    #[test]
    fn mouse_gesture_sensitivity_is_clamped_when_normalized() {
        let settings: MouseGestureSettings = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "mode": "direct",
            "sensitivity": 240,
            "rapidClickEnabled": true,
            "rapidClickCount": 99
        }))
        .unwrap();
        let settings = settings.normalized();
        assert_eq!(settings.sensitivity, 100);
        assert_eq!(settings.mode, MouseGestureMode::Direct);
        assert!(settings.rapid_click_enabled);
        assert_eq!(settings.rapid_click_count, 10);
    }

    #[test]
    fn old_three_click_default_migrates_once_but_new_explicit_three_is_preserved() {
        let mut legacy: PersistedData = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "mouse_gesture": {
                "rapidClickCount": 3
            }
        }))
        .unwrap();
        migrate_persisted_data(&mut legacy);
        assert_eq!(legacy.schema_version, CURRENT_STATE_SCHEMA_VERSION);
        assert_eq!(legacy.mouse_gesture.rapid_click_count, 4);

        let mut current: PersistedData = serde_json::from_value(serde_json::json!({
            "schema_version": CURRENT_STATE_SCHEMA_VERSION,
            "mouse_gesture": {
                "rapidClickCount": 3
            }
        }))
        .unwrap();
        migrate_persisted_data(&mut current);
        assert_eq!(current.mouse_gesture.rapid_click_count, 3);
    }

    #[test]
    fn atomic_write_keeps_previous_backup() {
        let dir = std::env::temp_dir().join(format!("say-it-persistence-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        atomic_write_with_backup(&file, b"one").unwrap();
        atomic_write_with_backup(&file, b"two").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "two");
        assert_eq!(
            fs::read_to_string(file.with_extension("json.bak")).unwrap(),
            "one"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_primary_state_recovers_from_backup() {
        let dir = std::env::temp_dir().join(format!(
            "say-it-persistence-recovery-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        fs::write(file.with_extension("json.bak"), b"{}").unwrap();

        assert!(persisted_state_files_exist(&file));
        let data = load_persisted_data_from_path(&file).unwrap();
        assert_eq!(data.schema_version, 1);

        fs::remove_dir_all(dir).unwrap();
    }
}

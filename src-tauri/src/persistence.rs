use crate::obs_overlay::ObsOverlaySettings;
use crate::prelude::*;
use crate::providers::credential_store::{
    key_for_profile, redact_error, CredentialKey, CredentialStore, LocalCredentialStore,
};
use crate::state::*;
use std::path::Path;

const STATE_FILE_NAME: &str = "say-it-state.json";
const LEGACY_APP_IDENTIFIERS: &[&str] = &["com.vibecode.sayit"];
const CURRENT_STATE_SCHEMA_VERSION: u32 = 4;
const FOUR_CLICK_DEFAULT_SCHEMA_VERSION: u32 = 2;
const CREDENTIAL_MIGRATION_SCHEMA_VERSION: u32 = 4;

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
    let providers = normalize_settings(providers);
    ensure_no_plaintext_credentials(&providers)?;
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
        providers,
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
    if data.schema_version < FOUR_CLICK_DEFAULT_SCHEMA_VERSION {
        data.schema_version = FOUR_CLICK_DEFAULT_SCHEMA_VERSION;
    }
}

fn migrate_provider_ids(settings: &mut ProviderSettings) {
    for profile in &mut settings.profiles {
        if profile.id == "funasr" {
            profile.id = crate::providers::BAILIAN_PROVIDER_ID.into();
        }
        if profile.kind == "alibabacloud-funasr" {
            profile.kind = "sdk:bailian".into();
        }
    }
    if settings.defaults.asr == "funasr" {
        settings.defaults.asr = crate::providers::BAILIAN_PROVIDER_ID.into();
    }
    if settings.defaults.translation == "funasr" {
        settings.defaults.translation = crate::providers::BAILIAN_PROVIDER_ID.into();
    }
    if settings.defaults.llm == "funasr" {
        settings.defaults.llm = crate::providers::GROQ_LLM_PROVIDER_ID.into();
    }
}

fn migrate_app_provider_ids(settings: &mut crate::application::settings::AppSettings) {
    fn replace_named(value: &mut Value, key_name: &str, replacement: &str) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if key == key_name && value.as_str() == Some("funasr") {
                        *value = replacement.into();
                    } else {
                        replace_named(value, key_name, replacement);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    replace_named(value, key_name, replacement);
                }
            }
            _ => {}
        }
    }
    replace_named(
        &mut settings.dictation_prefs,
        "smartLlmProviderId",
        crate::providers::GROQ_LLM_PROVIDER_ID,
    );
    replace_named(
        &mut settings.assistant_prefs,
        "llmProviderId",
        crate::providers::GROQ_LLM_PROVIDER_ID,
    );
}

fn ensure_no_plaintext_credentials(settings: &ProviderSettings) -> Result<(), String> {
    for profile in &settings.profiles {
        for field in crate::providers::config_fields_for(profile)
            .into_iter()
            .filter(|field| field.secret)
        {
            if profile.config.get(&field.key).is_some() {
                return Err(format!(
                    "拒绝持久化供应商 {} 的明文凭据字段 {}",
                    profile.id, field.key
                ));
            }
        }
        if profile.id == "funasr" || profile.kind == "alibabacloud-funasr" {
            return Err("拒绝持久化已废弃的 funasr 供应商标识".into());
        }
    }
    Ok(())
}

fn collect_plaintext_credentials(
    settings: &ProviderSettings,
) -> Result<Vec<(CredentialKey, String)>, String> {
    let mut collected = Vec::new();
    for profile in &settings.profiles {
        let mut destination = profile.clone();
        if destination.id == "funasr" {
            destination.id = crate::providers::BAILIAN_PROVIDER_ID.into();
        }
        if destination.kind == "alibabacloud-funasr" {
            destination.kind = "sdk:bailian".into();
        }
        let mut secret_fields = crate::providers::config_fields_for(profile)
            .into_iter()
            .filter(|field| field.secret)
            .map(|field| field.key)
            .collect::<Vec<_>>();
        // 旧 kind 只在 schema v2 迁移器中识别；运行时和 catalog 不保留 alias。
        if profile.kind == "alibabacloud-funasr"
            && !secret_fields.iter().any(|field| field == "apiKey")
        {
            secret_fields.push("apiKey".into());
        }
        for field in secret_fields {
            let Some(value) = profile.config.get(&field) else {
                continue;
            };
            let value = value
                .as_str()
                .ok_or_else(|| format!("供应商 {} 的凭据字段 {} 格式异常", profile.id, field))?
                .trim();
            if !value.is_empty() {
                collected.push((key_for_profile(&destination, &field)?, value.to_string()));
            }
        }
    }
    Ok(collected)
}

fn strip_plaintext_credentials(settings: &mut ProviderSettings) {
    for profile in &mut settings.profiles {
        let secret_fields = crate::providers::config_fields_for(profile)
            .into_iter()
            .filter(|field| field.secret)
            .map(|field| field.key)
            .collect::<Vec<_>>();
        if let Some(config) = profile.config.as_object_mut() {
            for field in secret_fields {
                config.remove(&field);
            }
        }
    }
}

fn rollback_credentials(
    store: &dyn CredentialStore,
    journal: &[(CredentialKey, Option<String>)],
) -> Result<(), String> {
    let mut errors = Vec::new();
    for (key, previous) in journal.iter().rev() {
        let result = match previous {
            Some(value) => store.set(key, value),
            None => store.delete(key),
        };
        if let Err(error) = result {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("凭据回滚失败：{}", errors.join("；")))
    }
}

fn write_sanitized_state_pair(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let backup = file.with_extension("json.bak");
    let primary_tmp = file.with_extension("json.migration.tmp");
    let backup_tmp = file.with_extension("json.bak.migration.tmp");
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败：{error}"))?;
    }
    let write_tmp = |path: &Path| -> Result<(), String> {
        use std::io::Write;
        let mut output =
            fs::File::create(path).map_err(|error| format!("创建迁移临时文件失败：{error}"))?;
        output
            .write_all(bytes)
            .and_then(|_| output.sync_all())
            .map_err(|error| format!("写入迁移临时文件失败：{error}"))
    };
    write_tmp(&primary_tmp)?;
    if let Err(error) = write_tmp(&backup_tmp) {
        let _ = fs::remove_file(&primary_tmp);
        return Err(error);
    }
    #[cfg(windows)]
    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| format!("替换迁移备份失败：{error}"))?;
    }
    if let Err(error) = fs::rename(&backup_tmp, &backup) {
        let _ = fs::remove_file(&primary_tmp);
        let _ = fs::remove_file(&backup_tmp);
        return Err(format!("提交无明文备份失败：{error}"));
    }
    #[cfg(windows)]
    if file.exists() {
        fs::remove_file(file).map_err(|error| format!("替换迁移主配置失败：{error}"))?;
    }
    if let Err(error) = fs::rename(&primary_tmp, file) {
        let _ = fs::remove_file(&primary_tmp);
        return Err(format!("提交无明文主配置失败：{error}"));
    }
    Ok(())
}

fn migrate_credentials_and_ids(
    file: &Path,
    data: &mut PersistedData,
    store: &dyn CredentialStore,
) -> Result<(), String> {
    if data.schema_version >= CREDENTIAL_MIGRATION_SCHEMA_VERSION {
        ensure_no_plaintext_credentials(&data.providers)?;
        return Ok(());
    }
    let credentials = collect_plaintext_credentials(&data.providers)?;
    let secrets = credentials
        .iter()
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    let mut journal = Vec::new();
    let migrate_result = (|| -> Result<(), String> {
        for (key, value) in &credentials {
            match store.get(key)? {
                Some(existing) if existing == *value => continue,
                Some(_) => return Err("本地加密凭据库已有不同值，拒绝覆盖".into()),
                None => {
                    store.set(key, value)?;
                    journal.push((key.clone(), None));
                    if store.get(key)?.as_deref() != Some(value.as_str()) {
                        return Err("本地加密凭据写入后校验不一致".into());
                    }
                }
            }
        }
        migrate_provider_ids(&mut data.providers);
        migrate_app_provider_ids(&mut data.app_settings);
        strip_plaintext_credentials(&mut data.providers);
        data.schema_version = CREDENTIAL_MIGRATION_SCHEMA_VERSION;
        ensure_no_plaintext_credentials(&data.providers)?;
        let bytes = serde_json::to_vec_pretty(data).map_err(|error| error.to_string())?;
        write_sanitized_state_pair(file, &bytes)
    })();
    if let Err(error) = migrate_result {
        let rollback = rollback_credentials(store, &journal);
        let error = redact_error(&error, &secrets);
        return match rollback {
            Ok(()) => Err(format!(
                "凭据与供应商 ID 迁移失败，原配置保留且下次会重试：{error}"
            )),
            Err(rollback) => Err(format!(
                "凭据与供应商 ID 迁移失败；原配置仍保留，但本地加密凭据回滚异常：{error}；{}",
                redact_error(&rollback, &secrets)
            )),
        };
    }
    Ok(())
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
    migrate_credentials_and_ids(&source, &mut data, &LocalCredentialStore::default())?;
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
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeCredentialStore {
        values: Mutex<HashMap<CredentialKey, String>>,
        fail_set_for: Mutex<Option<CredentialKey>>,
        deletes: Mutex<usize>,
        reads: Mutex<usize>,
    }

    impl CredentialStore for FakeCredentialStore {
        fn get(&self, key: &CredentialKey) -> Result<Option<String>, String> {
            *self.reads.lock().unwrap() += 1;
            Ok(self.values.lock().unwrap().get(key).cloned())
        }

        fn set(&self, key: &CredentialKey, value: &str) -> Result<(), String> {
            if self.fail_set_for.lock().unwrap().as_ref() == Some(key) {
                return Err(format!("fake set failure for {value}"));
            }
            self.values
                .lock()
                .unwrap()
                .insert(key.clone(), value.to_string());
            Ok(())
        }

        fn delete(&self, key: &CredentialKey) -> Result<(), String> {
            *self.deletes.lock().unwrap() += 1;
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn legacy_data() -> PersistedData {
        serde_json::from_value(serde_json::json!({
            "schema_version": 2,
            "app_settings": {
                "dictationPrefs": {"smartLlmProviderId": "funasr"},
                "assistantPrefs": {"editSelection": {"llmProviderId": "funasr"}}
            },
            "providers": {
                "profiles": [
                    {
                        "id": "funasr",
                        "kind": "alibabacloud-funasr",
                        "displayName": "阿里云百炼",
                        "authKind": "api-key",
                        "capabilities": ["asr", "translation"],
                        "enabled": true,
                        "config": {"apiKey": "bailian-test-secret", "languageHints": ["zh"]}
                    },
                    {
                        "id": "llm-groq",
                        "kind": "llm:groq",
                        "displayName": "Groq",
                        "authKind": "api-key",
                        "capabilities": ["llm"],
                        "enabled": true,
                        "config": {"apiKey": "groq-test-secret", "model": "openai/gpt-oss-20b"}
                    }
                ],
                "defaults": {"asr": "funasr", "llm": "funasr", "translation": "funasr"}
            }
        }))
        .unwrap()
    }

    fn migration_file(label: &str, data: &PersistedData) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "say-it-credential-migration-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        let bytes = serde_json::to_vec_pretty(data).unwrap();
        fs::write(&file, &bytes).unwrap();
        fs::write(file.with_extension("json.bak"), &bytes).unwrap();
        (dir, file)
    }

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
        assert_eq!(legacy.schema_version, FOUR_CLICK_DEFAULT_SCHEMA_VERSION);
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

    #[test]
    fn credential_migration_moves_all_secrets_and_sanitizes_primary_and_backup() {
        let mut data = legacy_data();
        let store = FakeCredentialStore::default();
        let (dir, file) = migration_file("success", &data);

        migrate_credentials_and_ids(&file, &mut data, &store).unwrap();

        assert_eq!(data.schema_version, CREDENTIAL_MIGRATION_SCHEMA_VERSION);
        assert_eq!(data.providers.defaults.asr, "bailian");
        assert_eq!(data.providers.defaults.translation, "bailian");
        assert_eq!(data.providers.defaults.llm, "llm-groq");
        assert_eq!(
            data.app_settings.dictation_prefs["smartLlmProviderId"],
            "llm-groq"
        );
        assert_eq!(
            data.app_settings.assistant_prefs["editSelection"]["llmProviderId"],
            "llm-groq"
        );
        assert!(data
            .providers
            .profiles
            .iter()
            .any(|profile| profile.id == "bailian" && profile.kind == "sdk:bailian"));
        for path in [&file, &file.with_extension("json.bak")] {
            let text = fs::read_to_string(path).unwrap();
            assert!(!text.contains("test-secret"));
            assert!(!text.contains("\"apiKey\""));
            assert!(!text.contains("\"funasr\""));
        }
        assert_eq!(
            store
                .get(&CredentialKey::provider("bailian", "apiKey").unwrap())
                .unwrap()
                .as_deref(),
            Some("bailian-test-secret")
        );
        assert_eq!(
            store
                .get(&CredentialKey::provider("llm-groq", "apiKey").unwrap())
                .unwrap()
                .as_deref(),
            Some("groq-test-secret")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn schema_v2_migrates_into_real_local_vault_without_plaintext() {
        let mut data = legacy_data();
        let (dir, file) = migration_file("local-vault", &data);
        let store = LocalCredentialStore::from_directory(dir.join("credentials"));

        migrate_credentials_and_ids(&file, &mut data, &store).unwrap();

        assert_eq!(data.schema_version, 4);
        assert_eq!(
            store
                .get(&CredentialKey::provider("bailian", "apiKey").unwrap())
                .unwrap()
                .as_deref(),
            Some("bailian-test-secret")
        );
        let vault = fs::read_to_string(dir.join("credentials/vault.json")).unwrap();
        assert!(!vault.contains("bailian-test-secret"));
        assert!(!vault.contains("groq-test-secret"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn schema_v3_without_plaintext_advances_to_v4_without_legacy_store_reads() {
        let mut data = legacy_data();
        migrate_provider_ids(&mut data.providers);
        migrate_app_provider_ids(&mut data.app_settings);
        strip_plaintext_credentials(&mut data.providers);
        data.schema_version = 3;
        let store = FakeCredentialStore::default();
        let (dir, file) = migration_file("v3-no-system-read", &data);

        migrate_credentials_and_ids(&file, &mut data, &store).unwrap();

        assert_eq!(data.schema_version, 4);
        assert_eq!(*store.reads.lock().unwrap(), 0);
        assert!(store.values.lock().unwrap().is_empty());
        assert_eq!(
            serde_json::from_str::<PersistedData>(&fs::read_to_string(file).unwrap())
                .unwrap()
                .schema_version,
            4
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn credential_migration_failure_keeps_plaintext_file_and_rolls_back_written_items() {
        let mut data = legacy_data();
        let store = FakeCredentialStore::default();
        let groq_key = CredentialKey::provider("llm-groq", "apiKey").unwrap();
        *store.fail_set_for.lock().unwrap() = Some(groq_key);
        let (dir, file) = migration_file("rollback", &data);

        let error = migrate_credentials_and_ids(&file, &mut data, &store).unwrap_err();

        assert!(error.contains("下次会重试"));
        assert!(!error.contains("bailian-test-secret"));
        assert!(!error.contains("groq-test-secret"));
        assert_eq!(data.schema_version, 2);
        let primary = fs::read_to_string(&file).unwrap();
        let backup = fs::read_to_string(file.with_extension("json.bak")).unwrap();
        assert!(primary.contains("bailian-test-secret"));
        assert!(backup.contains("groq-test-secret"));
        assert!(store.values.lock().unwrap().is_empty());
        assert_eq!(*store.deletes.lock().unwrap(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn credential_migration_is_idempotent_and_current_state_rejects_plaintext() {
        let mut data = legacy_data();
        let store = FakeCredentialStore::default();
        let (dir, file) = migration_file("idempotent", &data);
        migrate_credentials_and_ids(&file, &mut data, &store).unwrap();
        let snapshot = store.values.lock().unwrap().clone();

        migrate_credentials_and_ids(&file, &mut data, &store).unwrap();
        assert_eq!(*store.values.lock().unwrap(), snapshot);

        data.providers.profiles[0].config["apiKey"] = serde_json::json!("must-not-persist");
        assert!(migrate_credentials_and_ids(&file, &mut data, &store)
            .unwrap_err()
            .contains("拒绝持久化"));
        fs::remove_dir_all(dir).unwrap();
    }
}

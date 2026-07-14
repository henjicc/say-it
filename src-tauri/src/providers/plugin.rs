use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;

use super::registry::ModelInfo;
use super::{ProviderConfigField, ProviderProfile, ProviderSettings};

pub const PLUGIN_API_VERSION: u32 = 3;
pub const PLUGIN_HOST_API_VERSION: u32 = 1;
const MANIFEST_FILE_NAME: &str = "manifest.json";
const ALLOWED_PERMISSIONS: &[&str] = &["network", "browserSession", "cookies"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub provider: PluginProviderManifest,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
    pub runtime: PluginRuntimeManifest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_session: Option<PluginBrowserSessionManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<PluginIntegrityManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<PluginSignatureManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginBrowserSessionManifest {
    pub login_url: String,
    pub allowed_urls: Vec<String>,
    /// 同步会话前必须能读取到的 Cookie 名称。由插件声明，宿主只做通用完整性校验。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_cookie_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialization_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginIntegrityManifest {
    pub algorithm: String,
    pub files: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSignatureManifest {
    pub algorithm: String,
    pub key_id: String,
    pub public_key: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginProviderManifest {
    pub id: String,
    pub display_name: String,
    #[serde(default = "default_auth_kind")]
    pub auth_kind: String,
    #[serde(default = "default_capabilities")]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub config_fields: Vec<ProviderConfigField>,
    #[serde(default)]
    pub actions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntimeManifest {
    #[serde(default = "default_runtime_kind")]
    pub kind: String,
    pub entrypoint: String,
    #[serde(default = "default_host_api_version")]
    pub host_api_version: u32,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub network: PluginNetworkManifest,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginNetworkManifest {
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct InstalledPlugin {
    pub root: PathBuf,
    pub manifest: PluginManifest,
    pub trust: String,
}

#[derive(Clone, Debug)]
pub struct PluginRuntimeSpec {
    pub plugin_id: String,
    pub root: PathBuf,
    pub entrypoint: PathBuf,
    pub permissions: Vec<String>,
    pub allowed_hosts: Vec<String>,
    pub data_dir: PathBuf,
    pub trust: String,
}

#[derive(Clone, Debug, Default)]
pub struct PluginRegistry {
    plugins: Vec<InstalledPlugin>,
    errors: Vec<PluginLoadError>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginLoadError {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub provider_id: String,
    pub permissions: Vec<String>,
    pub models: Vec<String>,
    pub trust: String,
    pub actions: Vec<String>,
    pub has_browser_session: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegistrySnapshot {
    pub api_version: u32,
    pub plugins: Vec<PluginSummary>,
    pub errors: Vec<PluginLoadError>,
}

fn default_auth_kind() -> String {
    "custom".into()
}
fn default_capabilities() -> Vec<String> {
    vec!["asr".into()]
}
fn default_runtime_kind() -> String {
    "javascript".into()
}
fn default_host_api_version() -> u32 {
    PLUGIN_HOST_API_VERSION
}

pub fn plugins_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("plugins");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

pub fn load_registry(app: &tauri::AppHandle) -> Result<PluginRegistry, String> {
    let trusted = super::plugin_package::load_trusted_keys(app)?;
    load_registry_from_with_trust(&plugins_dir(app)?, &trusted)
}

#[cfg(test)]
pub fn load_registry_from(root: &Path) -> Result<PluginRegistry, String> {
    load_registry_from_with_trust(root, &HashMap::new())
}

fn load_registry_from_with_trust(
    root: &Path,
    trusted: &HashMap<String, String>,
) -> Result<PluginRegistry, String> {
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let mut registry = PluginRegistry::default();
    let mut ids = HashSet::new();
    let mut provider_ids = super::builtin_profiles()
        .into_iter()
        .map(|profile| profile.id)
        .collect::<HashSet<_>>();
    let mut model_ids = super::registry::models()
        .iter()
        .map(|model| model.id.clone())
        .collect::<HashSet<_>>();
    let mut entries = std::fs::read_dir(root)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let plugin_root = entry.path();
        let manifest_path = plugin_root.join(MANIFEST_FILE_NAME);
        if !manifest_path.exists() {
            continue;
        }
        match load_manifest(&plugin_root, &manifest_path, trusted) {
            Ok(plugin) => {
                let manifest = &plugin.manifest;
                let duplicate = ids.contains(&manifest.id)
                    || provider_ids.contains(&manifest.provider.id)
                    || manifest
                        .models
                        .iter()
                        .any(|model| model_ids.contains(&model.id));
                if duplicate {
                    registry.errors.push(PluginLoadError {
                        path: manifest_path.display().to_string(),
                        message: "插件、供应商或模型 ID 与已加载插件重复".into(),
                    });
                } else {
                    ids.insert(manifest.id.clone());
                    provider_ids.insert(manifest.provider.id.clone());
                    model_ids.extend(manifest.models.iter().map(|model| model.id.clone()));
                    registry.plugins.push(plugin);
                }
            }
            Err(message) => registry.errors.push(PluginLoadError {
                path: manifest_path.display().to_string(),
                message,
            }),
        }
    }
    Ok(registry)
}

fn load_manifest(
    plugin_root: &Path,
    manifest_path: &Path,
    trusted: &HashMap<String, String>,
) -> Result<InstalledPlugin, String> {
    let text = std::fs::read_to_string(manifest_path).map_err(|error| error.to_string())?;
    let manifest: PluginManifest =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    validate_manifest(plugin_root, &manifest)?;
    let trust = super::plugin_package::verify_installation(plugin_root, &manifest, trusted)?;
    Ok(InstalledPlugin {
        root: plugin_root.to_path_buf(),
        manifest,
        trust,
    })
}

pub(crate) fn validate_plugin_dir(root: &Path) -> Result<PluginManifest, String> {
    let path = root.join(MANIFEST_FILE_NAME);
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let manifest: PluginManifest =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    validate_manifest(root, &manifest)?;
    Ok(manifest)
}

fn validate_manifest(root: &Path, manifest: &PluginManifest) -> Result<(), String> {
    if manifest.api_version < PLUGIN_API_VERSION {
        return Err("旧进程插件不兼容，请用新版 Skill 重新生成".into());
    }
    if manifest.api_version != PLUGIN_API_VERSION {
        return Err(format!("不支持的插件 API 版本：{}", manifest.api_version));
    }
    validate_id("插件", &manifest.id)?;
    validate_id("供应商", &manifest.provider.id)?;
    if manifest.name.trim().is_empty() || manifest.version.trim().is_empty() {
        return Err("插件名称和版本不能为空".into());
    }
    if !manifest
        .provider
        .capabilities
        .iter()
        .any(|capability| matches!(capability.as_str(), "asr" | "translation" | "customization"))
    {
        return Err("插件供应商未声明受支持的能力".into());
    }
    if !manifest.provider.config.is_object() {
        return Err("provider.config 必须是 JSON 对象".into());
    }
    let mut config_keys = HashSet::new();
    for field in &manifest.provider.config_fields {
        if field.key.trim().is_empty() || !config_keys.insert(field.key.clone()) {
            return Err(format!("配置字段 key 为空或重复：{}", field.key));
        }
        if !matches!(
            field.field_type.as_str(),
            "text" | "password" | "number" | "boolean"
        ) {
            return Err(format!(
                "配置字段 {} 使用了未知类型：{}",
                field.key, field.field_type
            ));
        }
    }
    let mut actions = HashSet::new();
    for action in &manifest.provider.actions {
        validate_action_id(action)?;
        if !actions.insert(action) {
            return Err(format!("插件操作重复：{action}"));
        }
    }
    if manifest.runtime.kind != "javascript" {
        return Err(format!("不支持的插件运行时：{}", manifest.runtime.kind));
    }
    if manifest.runtime.host_api_version != PLUGIN_HOST_API_VERSION {
        return Err(format!(
            "不支持的宿主 API 版本：{}",
            manifest.runtime.host_api_version
        ));
    }
    for permission in &manifest.runtime.permissions {
        if !ALLOWED_PERMISSIONS.contains(&permission.as_str()) {
            return Err(format!("未知插件权限：{permission}"));
        }
    }
    if manifest
        .runtime
        .permissions
        .iter()
        .any(|value| value == "network")
        && manifest.runtime.network.allowed_hosts.is_empty()
    {
        return Err("声明 network 权限时 runtime.network.allowedHosts 不能为空".into());
    }
    for host in &manifest.runtime.network.allowed_hosts {
        validate_allowed_host(host)?;
    }
    if let Some(browser) = &manifest.browser_session {
        if !manifest
            .runtime
            .permissions
            .iter()
            .any(|permission| permission == "browserSession")
            || !manifest
                .runtime
                .permissions
                .iter()
                .any(|permission| permission == "cookies")
        {
            return Err("browserSession 配置必须同时声明 browserSession 与 cookies 权限".into());
        }
        validate_https_url("loginUrl", &browser.login_url)?;
        if browser.allowed_urls.is_empty() {
            return Err("browserSession.allowedUrls 不能为空".into());
        }
        for value in &browser.allowed_urls {
            validate_https_url("allowedUrls", value)?;
        }
        let mut required_cookie_names = HashSet::new();
        for name in &browser.required_cookie_names {
            if name.is_empty()
                || name.len() > 128
                || !name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b'!' | b'#'
                                | b'$'
                                | b'%'
                                | b'&'
                                | b'\''
                                | b'*'
                                | b'+'
                                | b'-'
                                | b'.'
                                | b'^'
                                | b'_'
                                | b'`'
                                | b'|'
                                | b'~'
                        )
                })
                || !required_cookie_names.insert(name)
            {
                return Err(format!(
                    "browserSession.requiredCookieNames 包含非法或重复 Cookie 名：{name}"
                ));
            }
        }
        if browser
            .initialization_script
            .as_ref()
            .is_some_and(|script| script.len() > 64 * 1024)
        {
            return Err("browserSession.initializationScript 超过 64 KiB".into());
        }
        for required in ["openLogin", "syncSession", "clearSession"] {
            if !manifest
                .provider
                .actions
                .iter()
                .any(|action| action == required)
            {
                return Err(format!("browserSession 插件必须声明操作：{required}"));
            }
        }
    }
    let entrypoint = safe_entrypoint(root, &manifest.runtime.entrypoint)?;
    if !entrypoint.is_file() {
        return Err(format!("插件入口不存在：{}", entrypoint.display()));
    }
    if !matches!(
        entrypoint.extension().and_then(|value| value.to_str()),
        Some("js" | "mjs")
    ) {
        return Err("JavaScript 插件入口必须使用 .js 或 .mjs 后缀".into());
    }
    let canonical_root = root.canonicalize().map_err(|error| error.to_string())?;
    let canonical_entrypoint = entrypoint
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !canonical_entrypoint.starts_with(&canonical_root) {
        return Err("插件入口不能通过符号链接跳出插件目录".into());
    }
    if manifest.models.is_empty() {
        return Err("插件至少需要声明一个模型".into());
    }
    let mut model_ids = HashSet::new();
    for model in &manifest.models {
        validate_id("模型", &model.id)?;
        if !model_ids.insert(model.id.clone()) {
            return Err(format!("模型 ID 重复：{}", model.id));
        }
        if model.provider_id != manifest.provider.id {
            return Err(format!(
                "模型 {} 的 providerId 必须为 {}",
                model.id, manifest.provider.id
            ));
        }
        let valid_model = match model.category.as_str() {
            "realtime" => {
                manifest
                    .provider
                    .capabilities
                    .iter()
                    .any(|value| value == "asr")
                    && model.protocol == "plugin-realtime-v1"
                    && model
                        .scenes
                        .iter()
                        .any(|scene| scene == "dictationRealtime" || scene == "subtitles")
            }
            "file" => {
                manifest
                    .provider
                    .capabilities
                    .iter()
                    .any(|value| value == "asr")
                    && model.protocol == "plugin-file-v1"
                    && model
                        .scenes
                        .iter()
                        .any(|scene| scene == "dictationFile" || scene == "transcription")
            }
            "translation" => {
                manifest
                    .provider
                    .capabilities
                    .iter()
                    .any(|value| value == "translation")
                    && model.protocol == "plugin-translation-v1"
                    && model
                        .scenes
                        .iter()
                        .any(|scene| scene == "subtitleTranslation")
            }
            _ => false,
        };
        if !valid_model {
            return Err(format!("模型 {} 的类别、协议或场景组合不受支持", model.id));
        }
    }
    Ok(())
}

fn validate_allowed_host(value: &str) -> Result<(), String> {
    let host = value.strip_prefix("*.").unwrap_or(value);
    if host.is_empty()
        || host.contains('/')
        || host.contains(':')
        || host.starts_with('.')
        || host.ends_with('.')
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        return Err(format!("非法网络白名单主机：{value}"));
    }
    Ok(())
}

fn validate_https_url(label: &str, value: &str) -> Result<(), String> {
    let url = url::Url::parse(value).map_err(|error| format!("{label} 不是合法 URL：{error}"))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(format!("{label} 必须是带主机名的 HTTPS URL"));
    }
    Ok(())
}

fn validate_id(label: &str, id: &str) -> Result<(), String> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} ID 只能包含小写字母、数字、点和连字符：{id}"
        ))
    }
}

fn validate_action_id(id: &str) -> Result<(), String> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "操作 ID 只能包含 ASCII 字母、数字、点和连字符：{id}"
        ))
    }
}

fn safe_entrypoint(root: &Path, value: &str) -> Result<PathBuf, String> {
    let relative = Path::new(value);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("插件入口必须是插件目录内的相对路径".into());
    }
    Ok(root.join(relative))
}

fn merge_missing_config(target: &mut Value, defaults: &Value) {
    let (Some(target), Some(defaults)) = (target.as_object_mut(), defaults.as_object()) else {
        return;
    };
    for (key, value) in defaults {
        target.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

impl PluginRegistry {
    pub fn snapshot_with_provider_settings(
        &self,
        settings: Option<&super::ProviderSettings>,
    ) -> PluginRegistrySnapshot {
        PluginRegistrySnapshot {
            api_version: PLUGIN_API_VERSION,
            plugins: self
                .plugins
                .iter()
                .map(|plugin| PluginSummary {
                    id: plugin.manifest.id.clone(),
                    name: plugin.manifest.name.clone(),
                    version: plugin.manifest.version.clone(),
                    provider_id: plugin.manifest.provider.id.clone(),
                    permissions: plugin.manifest.runtime.permissions.clone(),
                    models: plugin
                        .manifest
                        .models
                        .iter()
                        .map(|model| model.id.clone())
                        .collect(),
                    trust: plugin.trust.clone(),
                    actions: plugin.manifest.provider.actions.clone(),
                    has_browser_session: plugin.manifest.browser_session.is_some(),
                    enabled: settings
                        .and_then(|settings| {
                            settings
                                .profiles
                                .iter()
                                .find(|profile| profile.id == plugin.manifest.provider.id)
                        })
                        .map(|profile| profile.enabled)
                        .unwrap_or(true),
                })
                .collect(),
            errors: self.errors.clone(),
        }
    }

    pub fn models(&self) -> impl Iterator<Item = &ModelInfo> {
        self.plugins
            .iter()
            .flat_map(|plugin| plugin.manifest.models.iter())
    }

    pub fn model(&self, id: &str) -> Option<&ModelInfo> {
        self.models().find(|model| model.id == id.trim())
    }

    pub fn provider_id_for_model(&self, model: &str) -> Option<String> {
        self.model(model).map(|model| model.provider_id.clone())
    }

    pub fn runtime_for_provider(
        &self,
        provider_id: &str,
    ) -> Result<Option<PluginRuntimeSpec>, String> {
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.manifest.provider.id == provider_id)
        else {
            return Ok(None);
        };
        Ok(Some(PluginRuntimeSpec {
            plugin_id: plugin.manifest.id.clone(),
            root: plugin.root.clone(),
            entrypoint: safe_entrypoint(&plugin.root, &plugin.manifest.runtime.entrypoint)?,
            permissions: plugin.manifest.runtime.permissions.clone(),
            allowed_hosts: plugin.manifest.runtime.network.allowed_hosts.clone(),
            data_dir: plugin
                .root
                .parent()
                .and_then(Path::parent)
                .unwrap_or(&plugin.root)
                .join("plugin-data")
                .join(&plugin.manifest.id),
            trust: plugin.trust.clone(),
        }))
    }

    pub fn browser_for_provider(&self, provider_id: &str) -> Option<PluginBrowserSessionManifest> {
        self.plugins
            .iter()
            .find(|plugin| plugin.manifest.provider.id == provider_id)
            .and_then(|plugin| plugin.manifest.browser_session.clone())
    }

    pub fn provider_id_for_plugin(&self, plugin_id: &str) -> Option<&str> {
        self.plugins
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
            .map(|plugin| plugin.manifest.provider.id.as_str())
    }

    pub fn runtime_for_provider_id_by_plugin(
        &self,
        plugin_id: &str,
    ) -> Result<Option<(String, PluginRuntimeSpec)>, String> {
        let Some(plugin) = self.plugins.iter().find(|plugin| plugin.manifest.id == plugin_id) else {
            return Ok(None);
        };
        let provider_id = plugin.manifest.provider.id.clone();
        Ok(self
            .runtime_for_provider(&provider_id)?
            .map(|spec| (provider_id, spec)))
    }

    pub fn merge_provider_profiles(&self, settings: &mut ProviderSettings) {
        let installed: HashMap<_, _> = self
            .plugins
            .iter()
            .map(|plugin| (plugin.manifest.provider.id.clone(), plugin))
            .collect();
        for profile in &mut settings.profiles {
            if profile.kind.starts_with("plugin:") && !installed.contains_key(&profile.id) {
                profile.enabled = false;
            }
        }
        for (provider_id, plugin) in installed {
            let provider = &plugin.manifest.provider;
            match settings
                .profiles
                .iter_mut()
                .find(|profile| profile.id == provider_id)
            {
                Some(profile) => {
                    profile.kind = format!("plugin:{}", plugin.manifest.id);
                    profile.display_name = provider.display_name.clone();
                    profile.auth_kind = provider.auth_kind.clone();
                    profile.capabilities = provider.capabilities.clone();
                    profile.config_fields = provider.config_fields.clone();
                    profile.actions = provider.actions.clone();
                    merge_missing_config(&mut profile.config, &provider.config);
                }
                None => settings.profiles.push(ProviderProfile {
                    id: provider.id.clone(),
                    kind: format!("plugin:{}", plugin.manifest.id),
                    display_name: provider.display_name.clone(),
                    auth_kind: provider.auth_kind.clone(),
                    capabilities: provider.capabilities.clone(),
                    enabled: true,
                    config: provider.config.clone(),
                    config_fields: provider.config_fields.clone(),
                    actions: provider.actions.clone(),
                }),
            }
        }
        *settings = super::normalize_settings(settings.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_entrypoint_escape() {
        let root = std::env::temp_dir();
        assert!(safe_entrypoint(&root, "../bad.js").is_err());
        assert!(safe_entrypoint(&root, "connector/index.js").is_ok());
    }

    #[test]
    fn plugin_ids_are_portable() {
        assert!(validate_id("插件", "web-provider.1").is_ok());
        assert!(validate_id("插件", "供应商").is_err());
        assert!(validate_id("插件", "Upper").is_err());
    }

    #[test]
    fn loads_manifest_and_merges_provider_profile() {
        let root = std::env::temp_dir().join(format!("sayit-plugin-test-{}", std::process::id()));
        let plugin = root.join("test-provider");
        let connector = plugin.join("connector");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&connector).unwrap();
        std::fs::write(connector.join("index.js"), b"export default () => ({})").unwrap();
        std::fs::write(
            plugin.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "apiVersion": 3,
                "id": "test-provider",
                "name": "Test Provider",
                "version": "1.0.0",
                "provider": {
                    "id": "test-provider",
                    "displayName": "Test Provider",
                    "capabilities": ["asr"],
                    "config": { "token": "" },
                    "configFields": [{ "key": "token", "label": "Token", "fieldType": "password", "secret": true }]
                },
                "models": [{
                    "id": "test-realtime",
                    "label": "Test Realtime",
                    "providerId": "test-provider",
                    "category": "realtime",
                    "protocol": "plugin-realtime-v1",
                    "supportsVocabulary": false,
                    "supportsAlignmentTimestamps": false,
                    "scenes": ["dictationRealtime"],
                    "isDefaultRealtime": false,
                    "isDefaultFile": false
                }],
                "runtime": { "kind": "javascript", "entrypoint": "connector/index.js", "hostApiVersion": 1, "permissions": ["network"], "network": {"allowedHosts": ["api.example.com"]} }
            }))
            .unwrap(),
        )
        .unwrap();
        let registry = load_registry_from(&root).unwrap();
        assert_eq!(registry.snapshot_with_provider_settings(None).plugins.len(), 1);
        let mut settings = ProviderSettings::default();
        registry.merge_provider_profiles(&mut settings);
        {
            let profile = settings
                .profiles
                .iter_mut()
                .find(|profile| profile.id == "test-provider")
                .unwrap();
            assert_eq!(profile.kind, "plugin:test-provider");
            assert_eq!(profile.config_fields[0].key, "token");
            profile.enabled = false;
        }
        registry.merge_provider_profiles(&mut settings);
        let disabled = settings
            .profiles
            .iter()
            .find(|profile| profile.id == "test-provider")
            .unwrap();
        assert!(!disabled.enabled);
        let snapshot = registry.snapshot_with_provider_settings(Some(&settings));
        assert!(!snapshot.plugins[0].enabled);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_v3_privileged_multicapability_manifest_without_provider_specific_code() {
        let root = std::env::temp_dir().join(format!("sayit-plugin-v2-{}", uuid::Uuid::new_v4()));
        let plugin = root.join("web-provider");
        std::fs::create_dir_all(plugin.join("connector")).unwrap();
        std::fs::write(
            plugin.join("connector/index.js"),
            b"export default () => ({})",
        )
        .unwrap();
        std::fs::write(
            plugin.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "apiVersion": 3,
                "id": "web-provider", "name": "Web Provider", "version": "1.0.0",
                "provider": {
                    "id": "web-provider", "displayName": "Web Provider",
                    "capabilities": ["asr", "translation", "customization"], "config": {},
                    "actions": ["openLogin", "syncSession", "clearSession", "diagnose"]
                },
                "models": [
                    {"id":"web-live","label":"Live","providerId":"web-provider","category":"realtime","protocol":"plugin-realtime-v1","supportsVocabulary":true,"supportsAlignmentTimestamps":false,"scenes":["dictationRealtime","subtitles"],"isDefaultRealtime":false,"isDefaultFile":false},
                    {"id":"web-file","label":"File","providerId":"web-provider","category":"file","protocol":"plugin-file-v1","supportsVocabulary":true,"supportsAlignmentTimestamps":true,"scenes":["dictationFile","transcription"],"isDefaultRealtime":false,"isDefaultFile":false},
                    {"id":"web-translation","label":"Translation","providerId":"web-provider","category":"translation","protocol":"plugin-translation-v1","supportsVocabulary":false,"supportsAlignmentTimestamps":false,"scenes":["subtitleTranslation"],"isDefaultRealtime":false,"isDefaultFile":false}
                ],
                "runtime": {"entrypoint":"connector/index.js","hostApiVersion":1,"permissions":["network","browserSession","cookies"],"network":{"allowedHosts":["vendor.example"]}},
                "browserSession": {"loginUrl":"https://vendor.example/login","allowedUrls":["https://vendor.example/"],"requiredCookieNames":["sessionid"],"initializationScript":"window.__capture = true;"}
            })).unwrap(),
        ).unwrap();
        let registry = load_registry_from(&root).unwrap();
        let snapshot = registry.snapshot_with_provider_settings(None);
        assert_eq!(snapshot.plugins.len(), 1);
        assert!(snapshot.plugins[0].has_browser_session);
        assert_eq!(
            registry
                .browser_for_provider("web-provider")
                .unwrap()
                .required_cookie_names,
            vec!["sessionid"]
        );
        assert_eq!(registry.models().count(), 3);
        assert_eq!(
            registry.model("web-translation").unwrap().category,
            "translation"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_legacy_process_plugin_with_actionable_message() {
        let root =
            std::env::temp_dir().join(format!("sayit-plugin-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("manifest.json"), r#"{"apiVersion":2,"id":"legacy","name":"Legacy","version":"1","provider":{"id":"legacy","displayName":"Legacy"},"models":[],"runtime":{"kind":"process","entrypoint":"connector.exe"}}"#).unwrap();
        let error = validate_plugin_dir(&root).unwrap_err();
        assert!(error.contains("旧进程插件不兼容"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

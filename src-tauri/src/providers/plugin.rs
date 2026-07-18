use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::registry::ModelInfo;
use super::{ProviderConfigField, ProviderProfile, ProviderSettings};

pub const PLUGIN_API_VERSION: u32 = 4;
/// 宿主兼容的最低插件 API 版本：v3 在线插件（asr/translation/customization）继续可用；
/// ocr 能力、localNetwork 权限等 v4 语义仅在 apiVersion = 4 时允许声明。
pub const PLUGIN_MIN_API_VERSION: u32 = 3;
pub const PLUGIN_HOST_API_VERSION: u32 = 1;
const MANIFEST_FILE_NAME: &str = "manifest.json";
const ALLOWED_PERMISSIONS: &[&str] = &["network", "browserSession", "cookies", "localNetwork"];

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
    pub model_pack: Option<ModelPackManifest>,
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
    /// 可选的短时 URL 捕获规则。捕获 Cookie 的值为 Base64URL 编码 JSON，包含 `issuedAt` 与 `url`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_url_cookie: Option<PluginCapturedUrlCookieManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapturedUrlCookieManifest {
    pub cookie_name: String,
    pub max_age_ms: u64,
    #[serde(default)]
    pub freshness_slack_ms: u64,
    pub url: PluginCapturedUrlManifest,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCapturedUrlManifest {
    pub scheme: String,
    pub host: String,
    pub path: String,
    #[serde(default)]
    pub required_query_names: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    #[serde(default = "default_host_api_version")]
    pub host_api_version: u32,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub network: PluginNetworkManifest,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPackManifest {
    pub engine: String,
    pub files: Vec<ModelPackFileManifest>,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPackFileManifest {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<ModelPackDownloadManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPackDownloadManifest {
    pub url: String,
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
    pub browser_session: Option<PluginBrowserSessionManifest>,
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
    pub runtime_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<ModelPackSummary>,
}

#[derive(Clone, Debug)]
pub struct LocalModelSpec {
    pub plugin_id: String,
    pub provider_id: String,
    pub engine: String,
    pub model_dir: PathBuf,
    pub files: Vec<ModelPackFileManifest>,
    pub params: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPackSummary {
    pub engine: String,
    pub state: String,
    pub total_bytes: u64,
    pub ready_bytes: u64,
    pub downloadable: bool,
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
    crate::application::data_root::data_subdir(app, "plugins")
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
    let trust = if manifest.runtime.kind == "model-pack" {
        super::plugin_package::verify_installed_model_pack(plugin_root, &manifest, trusted)?
    } else {
        super::plugin_package::verify_installation(plugin_root, &manifest, trusted)?
    };
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
    if manifest.api_version < PLUGIN_MIN_API_VERSION {
        return Err("旧进程插件不兼容，请用新版 Skill 重新生成".into());
    }
    if manifest.api_version > PLUGIN_API_VERSION {
        return Err(format!("不支持的插件 API 版本：{}", manifest.api_version));
    }
    validate_id("插件", &manifest.id)?;
    validate_id("供应商", &manifest.provider.id)?;
    if manifest.name.trim().is_empty() || manifest.version.trim().is_empty() {
        return Err("插件名称和版本不能为空".into());
    }
    if manifest.api_version < 4
        && manifest
            .provider
            .capabilities
            .iter()
            .any(|capability| capability == "ocr")
    {
        return Err("ocr 能力需要插件 API 版本 4".into());
    }
    if !manifest.provider.capabilities.iter().any(|capability| {
        matches!(
            capability.as_str(),
            "asr" | "translation" | "customization" | "ocr"
        )
    }) {
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
    if !matches!(manifest.runtime.kind.as_str(), "javascript" | "model-pack") {
        return Err(format!("不支持的插件运行时：{}", manifest.runtime.kind));
    }
    if manifest.runtime.kind == "model-pack"
        && (manifest.provider.capabilities.is_empty()
            || manifest
                .provider
                .capabilities
                .iter()
                .any(|capability| !matches!(capability.as_str(), "asr" | "ocr")))
    {
        return Err("模型包能力只允许 asr 或 ocr".into());
    }
    let is_model_pack = manifest.runtime.kind == "model-pack";
    if is_model_pack && manifest.api_version < 4 {
        return Err("model-pack 运行时需要插件 API 版本 4".into());
    }
    if !is_model_pack && manifest.runtime.host_api_version != PLUGIN_HOST_API_VERSION {
        return Err(format!(
            "不支持的宿主 API 版本：{}",
            manifest.runtime.host_api_version
        ));
    }
    for permission in &manifest.runtime.permissions {
        if !ALLOWED_PERMISSIONS.contains(&permission.as_str()) {
            return Err(format!("未知插件权限：{permission}"));
        }
        if permission == "localNetwork" && manifest.api_version < 4 {
            return Err("localNetwork 权限需要插件 API 版本 4".into());
        }
    }
    if is_model_pack && !manifest.runtime.permissions.is_empty() {
        return Err("模型包不得声明任何运行时权限".into());
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
    if is_model_pack && manifest.browser_session.is_some() {
        return Err("模型包不得声明 browserSession".into());
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
            if !valid_cookie_name(name) || !required_cookie_names.insert(name) {
                return Err(format!(
                    "browserSession.requiredCookieNames 包含非法或重复 Cookie 名：{name}"
                ));
            }
        }
        if let Some(capture) = &browser.captured_url_cookie {
            validate_captured_url_cookie(capture)?;
            if !required_cookie_names.contains(&capture.cookie_name) {
                return Err(
                    "browserSession.capturedUrlCookie.cookieName 必须同时声明在 requiredCookieNames"
                        .into(),
                );
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
    if is_model_pack {
        if manifest.runtime.entrypoint.is_some() {
            return Err("模型包不得声明 entrypoint".into());
        }
        validate_model_pack(root, manifest)?;
    } else {
        if manifest.model_pack.is_some() {
            return Err("JavaScript 插件不得声明 modelPack".into());
        }
        let entrypoint = manifest
            .runtime
            .entrypoint
            .as_deref()
            .ok_or("JavaScript 插件必须声明 entrypoint")?;
        let entrypoint = safe_entrypoint(root, entrypoint)?;
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
    }
    // OCR 能力按供应商而非模型选择，纯 OCR 插件允许不声明模型；其余能力仍要求至少一个模型。
    let ocr_only = manifest
        .provider
        .capabilities
        .iter()
        .all(|capability| matches!(capability.as_str(), "ocr" | "customization"))
        && manifest
            .provider
            .capabilities
            .iter()
            .any(|capability| capability == "ocr");
    if manifest.models.is_empty() && !ocr_only {
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
        let valid_model =
            if is_model_pack {
                match manifest
                    .model_pack
                    .as_ref()
                    .map(|pack| pack.engine.as_str())
                {
                    Some("sherpa-onnx-online") => {
                        model.category == "realtime"
                            && model.protocol == "local-sherpa-online"
                            && model
                                .scenes
                                .iter()
                                .any(|scene| scene == "dictationRealtime" || scene == "subtitles")
                    }
                    Some("sherpa-onnx-offline") => {
                        matches!(model.category.as_str(), "realtime" | "file")
                            && model.protocol == "local-sherpa-offline"
                            && match model.category.as_str() {
                                "realtime" => model.scenes.iter().any(|scene| {
                                    scene == "dictationRealtime" || scene == "subtitles"
                                }),
                                "file" => model.scenes.iter().any(|scene| {
                                    scene == "dictationFile" || scene == "transcription"
                                }),
                                _ => false,
                            }
                    }
                    Some("ppocr-mnn") => manifest
                        .provider
                        .capabilities
                        .iter()
                        .any(|value| value == "ocr"),
                    _ => false,
                }
            } else {
                match model.category.as_str() {
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
                }
            };
        if !valid_model {
            return Err(format!("模型 {} 的类别、协议或场景组合不受支持", model.id));
        }
    }
    Ok(())
}

fn validate_model_pack(root: &Path, manifest: &PluginManifest) -> Result<(), String> {
    let pack = manifest
        .model_pack
        .as_ref()
        .ok_or("模型包缺少 modelPack 配置")?;
    if !matches!(
        pack.engine.as_str(),
        "sherpa-onnx-online" | "sherpa-onnx-offline" | "ppocr-mnn"
    ) {
        return Err(format!(
            "当前版本不支持模型引擎：{}；请升级软件",
            pack.engine
        ));
    }
    if !pack.params.is_object() {
        return Err("modelPack.params 必须是 JSON 对象".into());
    }
    if pack.files.is_empty() {
        return Err("modelPack.files 不能为空".into());
    }
    if !manifest.provider.actions.is_empty()
        || !manifest.provider.config_fields.is_empty()
        || manifest
            .provider
            .config
            .as_object()
            .is_some_and(|value| !value.is_empty())
    {
        return Err("模型包不得声明操作或供应商配置".into());
    }
    let mut paths = HashSet::new();
    for file in &pack.files {
        let relative = safe_model_file_path(&file.path)?;
        if !paths.insert(file.path.clone()) {
            return Err(format!("模型文件路径重复：{}", file.path));
        }
        if file.size_bytes == 0 {
            return Err(format!("模型文件大小必须大于 0：{}", file.path));
        }
        if file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("模型文件 SHA256 非法：{}", file.path));
        }
        let embedded = root.join(&relative);
        let installed = models_dir_from_plugin_root(root)
            .join(&manifest.id)
            .join(&relative);
        let installed_ready = installed.is_file()
            && super::model_download::verify_model_file(&installed, file).is_ok();
        if !embedded.is_file() && !installed_ready && file.download.is_none() {
            return Err(format!("模型文件既未内嵌也未提供下载地址：{}", file.path));
        }
        if let Some(download) = &file.download {
            let url = url::Url::parse(&download.url)
                .map_err(|error| format!("模型下载 URL 非法：{error}"))?;
            if url.scheme() != "https" || url.host_str().is_none() {
                return Err(format!("模型下载地址必须为 HTTPS：{}", file.path));
            }
        }
    }
    let required_params: &[&str] = match pack.engine.as_str() {
        "sherpa-onnx-online" => &["encoder", "decoder", "tokens"],
        "sherpa-onnx-offline" => &["model", "tokens", "vadModel"],
        "ppocr-mnn" => &[],
        _ => unreachable!(),
    };
    for key in required_params {
        let path = pack
            .params
            .get(*key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("modelPack.params 缺少文件参数：{key}"))?;
        if !paths.contains(path) {
            return Err(format!("modelPack.params.{key} 指向未声明文件：{path}"));
        }
    }
    fn visit(root: &Path, directory: &Path, actual: &mut HashSet<String>) -> Result<(), String> {
        for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_symlink() {
                return Err("模型包不能包含符号链接".into());
            }
            if kind.is_dir() {
                visit(root, &entry.path(), actual)?;
            } else if kind.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                if !matches!(relative.as_str(), "manifest.json" | "sayit-package.json") {
                    actual.insert(relative);
                }
            }
        }
        Ok(())
    }
    let mut actual = HashSet::new();
    visit(root, root, &mut actual)?;
    if let Some(unexpected) = actual.difference(&paths).next() {
        return Err(format!("模型包包含未声明的数据或代码文件：{unexpected}"));
    }
    Ok(())
}

pub(crate) fn safe_model_file_path(value: &str) -> Result<PathBuf, String> {
    let relative = Path::new(value);
    if value.trim().is_empty()
        || value.contains('\\')
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("模型文件必须使用包内相对路径：{value}"));
    }
    Ok(relative.to_path_buf())
}

fn valid_cookie_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().all(|byte| {
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
}

fn validate_captured_url_cookie(capture: &PluginCapturedUrlCookieManifest) -> Result<(), String> {
    if !valid_cookie_name(&capture.cookie_name) {
        return Err("browserSession.capturedUrlCookie.cookieName 非法".into());
    }
    if capture.max_age_ms == 0 || capture.max_age_ms > 24 * 60 * 60 * 1_000 {
        return Err("browserSession.capturedUrlCookie.maxAgeMs 必须介于 1ms 和 24 小时之间".into());
    }
    if capture.freshness_slack_ms > capture.max_age_ms {
        return Err("browserSession.capturedUrlCookie.freshnessSlackMs 不能大于 maxAgeMs".into());
    }
    if !matches!(capture.url.scheme.as_str(), "https" | "wss") {
        return Err("browserSession.capturedUrlCookie.url.scheme 仅支持 https 或 wss".into());
    }
    if capture.url.host.starts_with("*.") {
        return Err("browserSession.capturedUrlCookie.url.host 不支持通配符".into());
    }
    validate_allowed_host(&capture.url.host)?;
    if !capture.url.path.starts_with('/')
        || capture.url.path.contains('?')
        || capture.url.path.contains('#')
    {
        return Err(
            "browserSession.capturedUrlCookie.url.path 必须是不含查询参数的绝对路径".into(),
        );
    }
    let mut query_names = HashSet::new();
    for name in &capture.url.required_query_names {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || !query_names.insert(name)
        {
            return Err(format!(
                "browserSession.capturedUrlCookie.url.requiredQueryNames 包含非法或重复参数：{name}"
            ));
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
                    runtime_kind: plugin.manifest.runtime.kind.clone(),
                    model_pack: plugin.manifest.model_pack.as_ref().map(|pack| {
                        let model_dir =
                            models_dir_from_plugin_root(&plugin.root).join(&plugin.manifest.id);
                        let status = super::model_download::inspect_pack(&model_dir, pack);
                        ModelPackSummary {
                            engine: pack.engine.clone(),
                            state: status.state,
                            total_bytes: status.total_bytes,
                            ready_bytes: status.ready_bytes,
                            downloadable: pack.files.iter().any(|file| file.download.is_some()),
                        }
                    }),
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
        if plugin.manifest.runtime.kind != "javascript" {
            return Ok(None);
        }
        let entrypoint = plugin
            .manifest
            .runtime
            .entrypoint
            .as_deref()
            .ok_or("JavaScript 插件缺少 entrypoint")?;
        Ok(Some(PluginRuntimeSpec {
            plugin_id: plugin.manifest.id.clone(),
            root: plugin.root.clone(),
            entrypoint: safe_entrypoint(&plugin.root, entrypoint)?,
            permissions: plugin.manifest.runtime.permissions.clone(),
            allowed_hosts: plugin.manifest.runtime.network.allowed_hosts.clone(),
            browser_session: plugin.manifest.browser_session.clone(),
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

    pub fn local_model_for_model(&self, model_id: &str) -> Option<LocalModelSpec> {
        let plugin = self.plugins.iter().find(|plugin| {
            plugin.manifest.runtime.kind == "model-pack"
                && plugin
                    .manifest
                    .models
                    .iter()
                    .any(|model| model.id == model_id.trim())
        })?;
        let pack = plugin.manifest.model_pack.as_ref()?;
        Some(LocalModelSpec {
            plugin_id: plugin.manifest.id.clone(),
            provider_id: plugin.manifest.provider.id.clone(),
            engine: pack.engine.clone(),
            model_dir: models_dir_from_plugin_root(&plugin.root).join(&plugin.manifest.id),
            files: pack.files.clone(),
            params: pack.params.clone(),
        })
    }

    pub fn model_pack_for_plugin(&self, plugin_id: &str) -> Option<LocalModelSpec> {
        let plugin = self.plugins.iter().find(|plugin| {
            plugin.manifest.id == plugin_id && plugin.manifest.runtime.kind == "model-pack"
        })?;
        let pack = plugin.manifest.model_pack.as_ref()?;
        Some(LocalModelSpec {
            plugin_id: plugin.manifest.id.clone(),
            provider_id: plugin.manifest.provider.id.clone(),
            engine: pack.engine.clone(),
            model_dir: models_dir_from_plugin_root(&plugin.root).join(&plugin.manifest.id),
            files: pack.files.clone(),
            params: pack.params.clone(),
        })
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
    ) -> Result<Option<(String, Option<PluginRuntimeSpec>)>, String> {
        let Some(plugin) = self
            .plugins
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
        else {
            return Ok(None);
        };
        let provider_id = plugin.manifest.provider.id.clone();
        let spec = self.runtime_for_provider(&provider_id)?;
        Ok(Some((provider_id, spec)))
    }

    pub fn merge_provider_profiles(&self, settings: &mut ProviderSettings) {
        let installed: HashMap<_, _> = self
            .plugins
            .iter()
            .map(|plugin| (plugin.manifest.provider.id.clone(), plugin))
            .collect();
        for profile in &mut settings.profiles {
            if (profile.kind.starts_with("plugin:") || profile.kind.starts_with("model-pack:"))
                && !installed.contains_key(&profile.id)
            {
                profile.enabled = false;
            }
        }
        for (provider_id, plugin) in installed {
            let provider = &plugin.manifest.provider;
            let profile_kind = if plugin.manifest.runtime.kind == "model-pack" {
                format!("model-pack:{}", plugin.manifest.id)
            } else {
                format!("plugin:{}", plugin.manifest.id)
            };
            match settings
                .profiles
                .iter_mut()
                .find(|profile| profile.id == provider_id)
            {
                Some(profile) => {
                    profile.kind = profile_kind.clone();
                    profile.display_name = provider.display_name.clone();
                    profile.auth_kind = provider.auth_kind.clone();
                    profile.capabilities = provider.capabilities.clone();
                    profile.config_fields = provider.config_fields.clone();
                    profile.actions = provider.actions.clone();
                    merge_missing_config(&mut profile.config, &provider.config);
                }
                None => settings.profiles.push(ProviderProfile {
                    id: provider.id.clone(),
                    kind: profile_kind,
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

fn models_dir_from_plugin_root(plugin_root: &Path) -> PathBuf {
    plugin_root
        .parent()
        .and_then(Path::parent)
        .unwrap_or(plugin_root)
        .join("models")
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
        assert_eq!(
            registry.snapshot_with_provider_settings(None).plugins.len(),
            1
        );
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
                "browserSession": {
                    "loginUrl":"https://vendor.example/login",
                    "allowedUrls":["https://vendor.example/"],
                    "requiredCookieNames":["sessionid","temporary-url"],
                    "initializationScript":"window.__capture = true;",
                    "capturedUrlCookie": {
                        "cookieName":"temporary-url",
                        "maxAgeMs":60000,
                        "freshnessSlackMs":5000,
                        "url": {"scheme":"wss","host":"stream.vendor.example","path":"/v1/live","requiredQueryNames":["signature"]}
                    }
                }
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
            vec!["sessionid", "temporary-url"]
        );
        assert_eq!(
            registry
                .runtime_for_provider("web-provider")
                .unwrap()
                .unwrap()
                .browser_session
                .unwrap()
                .captured_url_cookie
                .unwrap()
                .url
                .host,
            "stream.vendor.example"
        );
        assert_eq!(registry.models().count(), 3);
        assert_eq!(
            registry.model("web-translation").unwrap().category,
            "translation"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn write_ocr_manifest(root: &Path, api_version: u32) {
        std::fs::create_dir_all(root.join("connector")).unwrap();
        std::fs::write(
            root.join("connector/index.js"),
            b"export default () => ({})",
        )
        .unwrap();
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "apiVersion": api_version,
                "id": "ocr-provider",
                "name": "OCR Provider",
                "version": "1.0.0",
                "provider": {
                    "id": "ocr-provider",
                    "displayName": "OCR Provider",
                    "capabilities": ["ocr"],
                    "config": {}
                },
                "models": [],
                "runtime": { "kind": "javascript", "entrypoint": "connector/index.js", "hostApiVersion": 1 }
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn v4_ocr_only_plugin_loads_without_models() {
        let root = std::env::temp_dir().join(format!("sayit-plugin-ocr-{}", uuid::Uuid::new_v4()));
        write_ocr_manifest(&root, 4);
        let manifest = validate_plugin_dir(&root).unwrap();
        assert_eq!(manifest.provider.capabilities, vec!["ocr"]);
        assert!(manifest.models.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v3_plugin_cannot_declare_ocr_capability() {
        let root =
            std::env::temp_dir().join(format!("sayit-plugin-ocr-v3-{}", uuid::Uuid::new_v4()));
        write_ocr_manifest(&root, 3);
        let error = validate_plugin_dir(&root).unwrap_err();
        assert!(error.contains("ocr 能力需要插件 API 版本 4"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v3_plugin_cannot_declare_local_network_permission() {
        let root =
            std::env::temp_dir().join(format!("sayit-plugin-localnet-{}", uuid::Uuid::new_v4()));
        let connector = root.join("connector");
        std::fs::create_dir_all(&connector).unwrap();
        std::fs::write(connector.join("index.js"), b"export default () => ({})").unwrap();
        let manifest = |api_version: u32| {
            serde_json::json!({
                "apiVersion": api_version,
                "id": "local-provider",
                "name": "Local Provider",
                "version": "1.0.0",
                "provider": { "id": "local-provider", "displayName": "Local", "capabilities": ["asr"], "config": {} },
                "models": [{
                    "id": "local-live", "label": "Local Live", "providerId": "local-provider",
                    "category": "realtime", "protocol": "plugin-realtime-v1",
                    "supportsVocabulary": false, "supportsAlignmentTimestamps": false,
                    "scenes": ["dictationRealtime"], "isDefaultRealtime": false, "isDefaultFile": false
                }],
                "runtime": {
                    "kind": "javascript", "entrypoint": "connector/index.js", "hostApiVersion": 1,
                    "permissions": ["localNetwork"]
                }
            })
        };
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest(3)).unwrap(),
        )
        .unwrap();
        let error = validate_plugin_dir(&root).unwrap_err();
        assert!(error.contains("localNetwork 权限需要插件 API 版本 4"));

        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest(4)).unwrap(),
        )
        .unwrap();
        let manifest = validate_plugin_dir(&root).unwrap();
        assert_eq!(manifest.runtime.permissions, vec!["localNetwork"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_api_version_above_current() {
        let root = std::env::temp_dir().join(format!("sayit-plugin-v5-{}", uuid::Uuid::new_v4()));
        write_ocr_manifest(&root, 5);
        let error = validate_plugin_dir(&root).unwrap_err();
        assert!(error.contains("不支持的插件 API 版本：5"));
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

    fn model_pack_manifest() -> Value {
        serde_json::json!({
            "apiVersion": 4,
            "id": "local-paraformer",
            "name": "Local Paraformer",
            "version": "1.0.0",
            "provider": {
                "id": "local-paraformer",
                "displayName": "Local Paraformer",
                "capabilities": ["asr"],
                "config": {}
            },
            "models": [{
                "id": "local-paraformer-live",
                "label": "Local Paraformer",
                "providerId": "local-paraformer",
                "category": "realtime",
                "protocol": "local-sherpa-online",
                "supportsVocabulary": false,
                "supportsAlignmentTimestamps": false,
                "scenes": ["dictationRealtime", "subtitles"],
                "isDefaultRealtime": false,
                "isDefaultFile": false
            }],
            "runtime": { "kind": "model-pack" },
            "modelPack": {
                "engine": "sherpa-onnx-online",
                "files": [{
                    "path": "model.bin",
                    "sha256": "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
                    "sizeBytes": 1
                }],
                "params": { "encoder": "model.bin", "decoder": "model.bin", "tokens": "model.bin" }
            }
        })
    }

    #[test]
    fn v4_embedded_model_pack_is_valid_without_javascript() {
        let root = std::env::temp_dir().join(format!("sayit-model-pack-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("model.bin"), b"x").unwrap();
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&model_pack_manifest()).unwrap(),
        )
        .unwrap();
        let manifest = validate_plugin_dir(&root).unwrap();
        assert_eq!(manifest.runtime.kind, "model-pack");
        assert!(manifest.runtime.entrypoint.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn model_pack_rejects_code_network_and_translation() {
        let root =
            std::env::temp_dir().join(format!("sayit-model-pack-bad-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("model.bin"), b"x").unwrap();
        let mut value = model_pack_manifest();
        value["runtime"]["entrypoint"] = Value::String("connector/index.js".into());
        value["runtime"]["permissions"] = serde_json::json!(["network"]);
        value["provider"]["capabilities"] = serde_json::json!(["translation"]);
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let error = validate_plugin_dir(&root).unwrap_err();
        assert!(error.contains("模型包能力只允许 asr 或 ocr"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

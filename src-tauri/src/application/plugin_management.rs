use crate::persistence::save_persisted_state;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::commands::common::read_provider_settings;
use crate::providers::browser_session_capture::{
    requires_capture, validate_capture_for_sync,
};
use crate::providers::find_profile;
use crate::providers::plugin::{
    load_registry, PluginBrowserSessionManifest, PluginRegistrySnapshot, PluginRuntimeSpec,
};
use crate::providers::{model_download, plugin_package, plugin_runtime, plugin_secrets};
use crate::state::RuntimeState;
use serde_json::{json, Value};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const BROWSER_SYNC_TIMEOUT_MS: u64 = 3_000;
const CAPTURE_SYNC_TIMEOUT_MS: u64 = 10_000;
pub(crate) const PLUGIN_IMPORT_REQUESTED_EVENT: &str = "provider-plugin-import-requested";

fn sayit_paths_from_args(args: &[String], cwd: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    for argument in args {
        let candidate = PathBuf::from(argument);
        if !candidate
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(plugin_package::SAYIT_PACKAGE_EXTENSION))
        {
            continue;
        }
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            cwd.join(candidate)
        };
        let Ok(candidate) = candidate.canonicalize() else {
            continue;
        };
        if !candidate.is_file() {
            continue;
        }
        let value = candidate.to_string_lossy().into_owned();
        if !paths.contains(&value) {
            paths.push(value);
        }
    }
    paths
}

pub(crate) fn queue_provider_plugin_imports(
    app: &tauri::AppHandle,
    args: &[String],
    cwd: &Path,
) -> Result<usize, String> {
    let paths = sayit_paths_from_args(args, cwd);
    if paths.is_empty() {
        return Ok(0);
    }
    let state = app.state::<RuntimeState>();
    let mut pending = state
        .pending_plugin_imports
        .lock()
        .map_err(|_| "待导入说吧包队列锁失败".to_string())?;
    let mut added = 0;
    for path in paths {
        if !pending.contains(&path) {
            pending.push_back(path);
            added += 1;
        }
    }
    Ok(added)
}

#[tauri::command]
pub(crate) fn take_pending_provider_plugin_imports(
    state: tauri::State<'_, RuntimeState>,
) -> Result<Vec<String>, String> {
    let mut pending = state
        .pending_plugin_imports
        .lock()
        .map_err(|_| "待导入说吧包队列锁失败".to_string())?;
    Ok(pending.drain(..).collect())
}

pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    let registry = load_registry(app)?;
    let state = app.state::<RuntimeState>();
    {
        let mut providers = state
            .providers
            .lock()
            .map_err(|_| "供应商配置锁失败".to_string())?;
        registry.merge_provider_profiles(&mut providers);
    }
    *state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())? = registry;
    Ok(())
}

#[tauri::command]
pub(crate) fn list_provider_plugins(
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    let providers = state
        .providers
        .lock()
        .map_err(|_| "供应商配置锁失败".to_string())?
        .clone();
    state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())
        .map(|registry| registry.snapshot_with_provider_settings(Some(&providers)))
}

#[tauri::command]
pub(crate) fn reload_provider_plugins(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    let registry = load_registry(&app)?;
    {
        let mut providers = state
            .providers
            .lock()
            .map_err(|_| "供应商配置锁失败".to_string())?;
        registry.merge_provider_profiles(&mut providers);
    }
    *state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())? = registry;
    let providers = state
        .providers
        .lock()
        .map_err(|_| "供应商配置锁失败".to_string())?
        .clone();
    let snapshot = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .snapshot_with_provider_settings(Some(&providers));
    save_persisted_state(&app, &state)?;
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn install_provider_plugin(
    app: tauri::AppHandle,
    source_path: String,
    expected_archive_sha256: Option<String>,
    allow_unsigned: bool,
    trust_signing_key: bool,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    let install_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        plugin_package::install_from_path(
            &install_app,
            Path::new(&source_path),
            expected_archive_sha256.as_deref(),
            allow_unsigned,
            trust_signing_key,
        )
    })
    .await
    .map_err(|error| format!("插件安装任务失败：{error}"))??;
    reload_provider_plugins(app, state)
}

#[tauri::command]
pub(crate) async fn preview_provider_plugin(
    app: tauri::AppHandle,
    source_path: String,
) -> Result<plugin_package::PackagePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        plugin_package::preview_from_path(&app, Path::new(&source_path))
    })
    .await
    .map_err(|error| format!("插件包预览任务失败：{error}"))?
}

#[tauri::command]
pub(crate) async fn download_provider_model_pack(
    app: tauri::AppHandle,
    plugin_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    let spec = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .model_pack_for_plugin(&plugin_id)
        .ok_or_else(|| format!("模型包 {plugin_id} 不存在"))?;
    let pack = crate::providers::plugin::ModelPackManifest {
        engine: spec.engine,
        files: spec.files,
        params: spec.params,
    };
    model_download::download_pack(&app, &plugin_id, &spec.model_dir, &pack).await?;
    list_provider_plugins(state)
}

#[tauri::command]
pub(crate) fn set_provider_plugin_enabled(
    app: tauri::AppHandle,
    plugin_id: String,
    enabled: bool,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    let provider_id = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .provider_id_for_plugin(&plugin_id)
        .map(str::to_owned)
        .ok_or_else(|| format!("插件 {plugin_id} 不存在"))?;

    let providers = {
        let mut providers = state
            .providers
            .lock()
            .map_err(|_| "供应商配置锁失败".to_string())?;
        let profile = providers
            .profiles
            .iter_mut()
            .find(|profile| {
                profile.id == provider_id
                    && (profile.kind.starts_with("plugin:")
                        || profile.kind.starts_with("model-pack:"))
            })
            .ok_or_else(|| format!("插件 {plugin_id} 的供应商配置不存在"))?;
        profile.enabled = enabled;
        *providers = crate::providers::normalize_settings(providers.clone());
        providers.clone()
    };
    let snapshot = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .snapshot_with_provider_settings(Some(&providers));
    save_persisted_state(&app, &state)?;
    Ok(snapshot)
}

#[tauri::command]
pub(crate) fn uninstall_provider_plugin(
    app: tauri::AppHandle,
    plugin_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    let (provider_id, spec) = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .runtime_for_provider_id_by_plugin(&plugin_id)?
        .ok_or_else(|| format!("插件 {plugin_id} 不存在"))?;
    if let Some(spec) = spec {
        plugin_secrets::clear_session(&spec)?;
        if let Some(window) = app.get_webview_window(&login_window_label(&provider_id)) {
            window
                .clear_all_browsing_data()
                .map_err(|error| format!("清除插件浏览数据失败：{error}"))?;
            window.close().map_err(|error| error.to_string())?;
        }
    }
    plugin_package::uninstall(&app, &plugin_id)?;
    {
        let mut providers = state
            .providers
            .lock()
            .map_err(|_| "供应商配置锁失败".to_string())?;
        providers.profiles.retain(|profile| {
            profile.id != provider_id
                || !(profile.kind.starts_with("plugin:") || profile.kind.starts_with("model-pack:"))
        });
        *providers = crate::providers::normalize_settings(providers.clone());
    }
    reload_provider_plugins(app, state)
}

#[tauri::command]
pub(crate) async fn run_provider_plugin_action(
    app: tauri::AppHandle,
    provider_id: String,
    action: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<Value, String> {
    let (spec, browser, profile) = {
        let registry = state
            .plugin_registry
            .lock()
            .map_err(|_| "插件注册表锁失败".to_string())?;
        let spec = registry
            .runtime_for_provider(&provider_id)?
            .ok_or_else(|| format!("供应商 {provider_id} 不是 JavaScript 插件"))?;
        let browser = registry.browser_for_provider(&provider_id);
        let settings = read_provider_settings(&state)?;
        let profile = find_profile(&settings, &provider_id)
            .cloned()
            .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
        (spec, browser, profile)
    };
    if !profile.actions.iter().any(|declared| declared == &action) {
        return Err(format!("插件未声明操作：{action}"));
    }
    if spec.trust == "signed-untrusted" {
        return Err("插件签名密钥尚未受信任，请先在插件管理中确认发布者".into());
    }
    match action.as_str() {
        "openLogin" => {
            let browser = browser.ok_or("该插件没有声明 browserSession")?;
            open_login_window(&app, &provider_id, &spec.plugin_id, &browser)?;
            Ok(json!({ "status": "opened" }))
        }
        "syncSession" => {
            let browser = browser.ok_or("该插件没有声明 browserSession")?;
            let window = app
                .get_webview_window(&login_window_label(&provider_id))
                .ok_or("找不到插件登录窗口，请先打开登录窗口")?;
            refresh_and_save_browser_session(&window, &browser, &spec, "sync").await
        }
        "clearSession" => {
            plugin_secrets::clear_session(&spec)?;
            reset_login_webview_profile(&app, &provider_id, &spec.plugin_id)?;
            Ok(json!({ "status": "cleared" }))
        }
        other => {
            plugin_runtime::invoke(
                &spec,
                &profile,
                "action",
                json!({ "action": other }),
                plugin_runtime::DEFAULT_INVOKE_TIMEOUT,
                |_| {},
            )
            .await
        }
    }
}

pub(crate) async fn refresh_browser_session_before_runtime(
    app: &tauri::AppHandle,
    state: &RuntimeState,
    provider_id: &str,
    spec: &PluginRuntimeSpec,
) -> Result<(), String> {
    let browser = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .browser_for_provider(provider_id)
        .ok_or("插件没有声明 browserSession")?;
    if !requires_capture(&browser) {
        return Ok(());
    }
    let window = ensure_login_window(app, provider_id, &spec.plugin_id, &browser, false)?;
    refresh_and_save_browser_session(&window, &browser, spec, "auto-sync")
        .await
        .map(|_| ())
        .map_err(|error| {
            format!("浏览器临时会话自动续期失败：{error}。请打开登录窗口确认登录状态后重试")
        })
}

pub(crate) async fn refresh_browser_session_before_recording(
    app: &tauri::AppHandle,
    state: &RuntimeState,
    model_id: &str,
) -> Result<(), String> {
    let target = {
        let registry = state
            .plugin_registry
            .lock()
            .map_err(|_| "插件注册表锁失败".to_string())?;
        let Some(provider_id) = registry.provider_id_for_model(model_id) else {
            return Ok(());
        };
        let spec = registry.runtime_for_provider(&provider_id)?;
        spec.map(|spec| (provider_id, spec))
    };
    let Some((provider_id, spec)) = target else {
        return Ok(());
    };
    refresh_browser_session_before_runtime(app, state, &provider_id, &spec).await
}

async fn refresh_and_save_browser_session(
    window: &WebviewWindow,
    browser: &PluginBrowserSessionManifest,
    spec: &PluginRuntimeSpec,
    trace_scope: &str,
) -> Result<Value, String> {
    let sync_started_ms = now_epoch_ms();
    let trace = |_message: &str| {};
    let capture_status = |cookies: &[Value]| -> Result<(), String> {
        validate_capture_for_sync(browser, cookies, sync_started_ms, now_epoch_ms())
    };
    if trace_scope == "auto-sync" {
        let cookies = read_browser_cookies(window, browser)?;
        if missing_required_cookies(&cookies, browser).is_empty()
            && capture_status(&cookies).is_ok()
        {
            trace("reuse recently refreshed capture");
            return save_protected_browser_session(spec, cookies);
        }
    }
    trace(&format!(
        "start required=[{}] allowedUrls={}",
        browser.required_cookie_names.join(","),
        browser.allowed_urls.len()
    ));
    if let Some(script) = browser.initialization_script.as_deref() {
        trace("eval initialization script");
        window.eval(script).map_err(|error| error.to_string())?;
        let timeout_ms = if requires_capture(browser) {
            CAPTURE_SYNC_TIMEOUT_MS
        } else {
            BROWSER_SYNC_TIMEOUT_MS
        };
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            let cookies = read_browser_cookies(window, browser)?;
            let missing = missing_required_cookies(&cookies, browser);
            let capture_result = capture_status(&cookies).err().unwrap_or_default();
            trace(&format!(
                "poll cookies={} names=[{}] missing=[{}]{}",
                cookies.len(),
                browser_cookie_names(&cookies).join(","),
                missing.join(","),
                if capture_result.is_empty() {
                    String::new()
                } else {
                    format!(" capture={capture_result}")
                }
            ));
            if !cookies.is_empty()
                && missing.is_empty()
                && capture_status(&cookies).is_ok()
            {
                break;
            }
            if std::time::Instant::now() >= deadline {
                trace("poll timeout");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
    let cookies = read_browser_cookies(window, browser)?;
    trace(&format!(
        "final cookies={} names=[{}]",
        cookies.len(),
        browser_cookie_names(&cookies).join(",")
    ));
    if cookies.is_empty() {
        trace("failed: no cookies");
        return Err("未读取到允许域名的 Cookie，请确认已完成登录".into());
    }
    let missing = missing_required_cookies(&cookies, browser);
    if !missing.is_empty() {
        trace(&format!(
            "failed: missing required cookies [{}]",
            missing.join(",")
        ));
        return Err(format!(
            "登录会话未完整，缺少必要 Cookie：{}。请在登录窗口中确认账号已登录、等待页面加载完成后再次获取登录会话",
            missing.join("、")
        ));
    }
    if let Err(reason) = capture_status(&cookies) {
        trace(&format!("failed: invalid captured URL cookie [{reason}]"));
        return Err(format!(
            "浏览器临时会话凭据尚未刷新完成：{reason}。请等待页面完全加载后重试"
        ));
    }
    let cookie_count = cookies.len();
    let result = save_protected_browser_session(spec, cookies)?;
    trace(&format!("saved cookieCount={cookie_count}"));
    Ok(result)
}

fn read_browser_cookies(
    window: &WebviewWindow,
    browser: &PluginBrowserSessionManifest,
) -> Result<Vec<Value>, String> {
    let mut cookies = HashMap::new();
    for value in &browser.allowed_urls {
        let url = url::Url::parse(value).map_err(|error| error.to_string())?;
        for cookie in window
            .cookies_for_url(url)
            .map_err(|error| error.to_string())?
        {
            let domain = cookie.domain().unwrap_or_default().to_string();
            let path = cookie.path().unwrap_or("/").to_string();
            cookies.insert(
                format!("{}|{}|{}", cookie.name(), domain, path),
                json!({
                    "name": cookie.name(),
                    "value": cookie.value(),
                    "domain": domain,
                    "path": path,
                    "httpOnly": cookie.http_only(),
                    "secure": cookie.secure(),
                }),
            );
        }
    }
    Ok(cookies.into_values().collect())
}

fn browser_cookie_names(cookies: &[Value]) -> Vec<String> {
    let mut names = cookies
        .iter()
        .filter_map(|cookie| cookie.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn missing_required_cookies(
    cookies: &[Value],
    browser: &PluginBrowserSessionManifest,
) -> Vec<String> {
    let cookie_names = cookies
        .iter()
        .filter_map(|cookie| cookie.get("name").and_then(Value::as_str))
        .collect::<HashSet<_>>();
    browser
        .required_cookie_names
        .iter()
        .filter(|name| !cookie_names.contains(name.as_str()))
        .cloned()
        .collect()
}

fn save_protected_browser_session(
    spec: &PluginRuntimeSpec,
    cookies: Vec<Value>,
) -> Result<Value, String> {
    let cookie_count = cookies.len();
    plugin_secrets::save_session(
        spec,
        &json!({ "cookies": cookies, "capturedAtMs": now_epoch_ms() }),
    )?;
    Ok(json!({
        "status": "saved",
        "message": format!("已保护 {cookie_count} 个 Cookie，登录会话已验证。"),
        "cookieCount": cookie_count,
        "protected": true
    }))
}

fn open_login_window(
    app: &tauri::AppHandle,
    provider_id: &str,
    plugin_id: &str,
    browser: &PluginBrowserSessionManifest,
) -> Result<(), String> {
    ensure_login_window(app, provider_id, plugin_id, browser, true).map(|_| ())
}

fn ensure_login_window(
    app: &tauri::AppHandle,
    provider_id: &str,
    plugin_id: &str,
    browser: &PluginBrowserSessionManifest,
    visible: bool,
) -> Result<WebviewWindow, String> {
    let label = login_window_label(provider_id);
    if let Some(window) = app.get_webview_window(&label) {
        if visible {
            window.show().map_err(|error| error.to_string())?;
            window.set_focus().map_err(|error| error.to_string())?;
        }
        return Ok(window);
    }
    let url = url::Url::parse(&browser.login_url).map_err(|error| error.to_string())?;
    let data_dir = crate::application::data_root::data_root(app)?
        .join("plugin-webviews")
        .join(plugin_id);
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(browser.window_title.as_deref().unwrap_or("供应商登录"))
        .inner_size(1200.0, 860.0)
        .resizable(true)
        .focused(visible)
        .visible(visible)
        .decorations(true)
        .data_directory(data_dir);
    if let Some(user_agent) = browser.user_agent.as_deref() {
        builder = builder.user_agent(user_agent);
    }
    if let Some(script) = browser.initialization_script.as_deref() {
        builder = builder.initialization_script(script);
    }
    builder.build().map_err(|error| error.to_string())
}

fn reset_login_webview_profile(
    app: &tauri::AppHandle,
    provider_id: &str,
    plugin_id: &str,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(&login_window_label(provider_id)) {
        window.close().map_err(|error| error.to_string())?;
    }

    let data_dir = crate::application::data_root::data_root(app)?
        .join("plugin-webviews")
        .join(plugin_id);
    if data_dir.exists() {
        std::fs::remove_dir_all(&data_dir)
            .map_err(|error| format!("清除插件登录浏览数据失败：{error}"))?;
    }
    Ok(())
}

fn login_window_label(provider_id: &str) -> String {
    format!(
        "plugin-login-{}",
        provider_id
            .chars()
            .map(|character| if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            })
            .collect::<String>()
    )
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sayit_args_resolve_relative_paths_and_ignore_other_inputs() {
        let root = std::env::temp_dir().join(format!(
            "sayit-import-args-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let package = root.join("demo.SAYIT");
        std::fs::write(&package, b"package").unwrap();

        let paths = sayit_paths_from_args(
            &[
                "say-it.exe".into(),
                "demo.SAYIT".into(),
                "missing.sayit".into(),
                "notes.txt".into(),
            ],
            &root,
        );

        assert_eq!(
            paths,
            vec![package
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned()]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sayit_args_deduplicate_the_same_file() {
        let root = std::env::temp_dir().join(format!(
            "sayit-import-args-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let package = root.join("demo.sayit");
        std::fs::write(&package, b"package").unwrap();

        let value = package.to_string_lossy().into_owned();
        let paths = sayit_paths_from_args(&[value.clone(), value], &root);

        assert_eq!(paths.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

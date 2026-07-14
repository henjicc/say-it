use crate::persistence::save_persisted_state;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::commands::common::read_provider_settings;
use crate::providers::find_profile;
use crate::providers::plugin::{load_registry, PluginRegistrySnapshot};
use crate::providers::{plugin_package, plugin_runtime, plugin_secrets};
use crate::state::RuntimeState;
use serde_json::{json, Value};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

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
    state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())
        .map(|registry| registry.snapshot())
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
    let snapshot = registry.snapshot();
    *state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())? = registry;
    save_persisted_state(&app, &state)?;
    Ok(snapshot)
}

#[tauri::command]
pub(crate) fn install_provider_plugin(
    app: tauri::AppHandle,
    source_path: String,
    allow_unsigned: bool,
    trust_signing_key: bool,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    plugin_package::install_from_path(
        &app,
        Path::new(&source_path),
        allow_unsigned,
        trust_signing_key,
    )?;
    reload_provider_plugins(app, state)
}

#[tauri::command]
pub(crate) fn list_provider_plugin_backups(
    app: tauri::AppHandle,
) -> Result<Vec<plugin_package::PluginBackup>, String> {
    plugin_package::list_backups(&app)
}

#[tauri::command]
pub(crate) fn rollback_provider_plugin(
    app: tauri::AppHandle,
    plugin_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<PluginRegistrySnapshot, String> {
    plugin_package::rollback(&app, &plugin_id)?;
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
            let cookies = cookies.into_values().collect::<Vec<_>>();
            if cookies.is_empty() {
                return Err("未读取到允许域名的 Cookie，请确认已完成登录".into());
            }
            let cookie_names = cookies
                .iter()
                .filter_map(|cookie| cookie.get("name").and_then(Value::as_str))
                .collect::<HashSet<_>>();
            let missing = browser
                .required_cookie_names
                .iter()
                .filter(|name| !cookie_names.contains(name.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(format!(
                    "登录会话未完整，缺少必要 Cookie：{}。请在登录窗口中确认账号已登录、等待页面加载完成后再次获取登录会话",
                    missing.join("、")
                ));
            }
            let cookie_count = cookies.len();
            plugin_secrets::save_session(
                &spec,
                &json!({ "cookies": cookies, "capturedAtMs": now_epoch_ms() }),
            )?;
            Ok(json!({
                "status": "saved",
                "message": format!("已保护 {cookie_count} 个 Cookie，登录会话已验证。"),
                "cookieCount": cookie_count,
                "protected": true
            }))
        }
        "clearSession" => {
            plugin_secrets::clear_session(&spec)?;
            if let Some(window) = app.get_webview_window(&login_window_label(&provider_id)) {
                window
                    .clear_all_browsing_data()
                    .map_err(|error| error.to_string())?;
            }
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

fn open_login_window(
    app: &tauri::AppHandle,
    provider_id: &str,
    plugin_id: &str,
    browser: &crate::providers::plugin::PluginBrowserSessionManifest,
) -> Result<(), String> {
    let label = login_window_label(provider_id);
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let url = url::Url::parse(&browser.login_url).map_err(|error| error.to_string())?;
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("plugin-webviews")
        .join(plugin_id);
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(browser.window_title.as_deref().unwrap_or("供应商登录"))
        .inner_size(1200.0, 860.0)
        .resizable(true)
        .focused(true)
        .visible(true)
        .decorations(true)
        .data_directory(data_dir);
    if let Some(user_agent) = browser.user_agent.as_deref() {
        builder = builder.user_agent(user_agent);
    }
    if let Some(script) = browser.initialization_script.as_deref() {
        builder = builder.initialization_script(script);
    }
    builder.build().map_err(|error| error.to_string())?;
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

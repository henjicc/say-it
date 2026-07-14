use base64::Engine;
use serde_json::Value;
use url::Url;

use super::plugin::{PluginBrowserSessionManifest, PluginCapturedUrlCookieManifest};

pub fn requires_capture(browser: &PluginBrowserSessionManifest) -> bool {
    browser.captured_url_cookie.is_some()
}

pub fn validate_capture_for_sync(
    browser: &PluginBrowserSessionManifest,
    cookies: &[Value],
    sync_started_ms: u64,
    current_time_ms: u64,
) -> Result<(), String> {
    let Some(capture) = browser.captured_url_cookie.as_ref() else {
        return Ok(());
    };
    let (issued_at, url) = parse_capture(cookies, capture)?;
    if issued_at < sync_started_ms.saturating_sub(capture.freshness_slack_ms) {
        return Err(format!(
            "会话捕获 Cookie 过旧 issuedAt={issued_at} syncStartedAt={sync_started_ms}"
        ));
    }
    validate_url(capture, url, issued_at, current_time_ms)
}

pub fn validate_capture_for_runtime(
    browser: Option<&PluginBrowserSessionManifest>,
    session: &Value,
    current_time_ms: u64,
) -> Result<(), String> {
    let Some(browser) = browser else {
        return Ok(());
    };
    let Some(capture) = browser.captured_url_cookie.as_ref() else {
        return Ok(());
    };
    let cookies = session
        .get("cookies")
        .and_then(Value::as_array)
        .ok_or_else(|| "登录会话缺少 cookies".to_string())?;
    let (issued_at, url) = parse_capture(cookies, capture)?;
    validate_url(capture, url, issued_at, current_time_ms)
}

fn parse_capture(
    cookies: &[Value],
    capture: &PluginCapturedUrlCookieManifest,
) -> Result<(u64, Url), String> {
    let marker = cookies
        .iter()
        .find(|cookie| cookie.get("name").and_then(Value::as_str) == Some(&capture.cookie_name))
        .ok_or_else(|| format!("缺少会话捕获 Cookie：{}", capture.cookie_name))?;
    let marker_value = marker
        .get("value")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("会话捕获 Cookie 为空：{}", capture.cookie_name))?;
    let normalized = marker_value.replace('-', "+").replace('_', "/");
    let padding = "=".repeat((4 - normalized.len() % 4) % 4);
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(format!("{normalized}{padding}"))
        .map_err(|_| "会话捕获 Cookie 不是合法 Base64".to_string())?;
    let payload: Value = serde_json::from_slice(&decoded)
        .map_err(|_| "会话捕获 Cookie 不是合法 JSON".to_string())?;
    let issued_at = payload
        .get("issuedAt")
        .and_then(Value::as_u64)
        .ok_or_else(|| "会话捕获 Cookie 缺少 issuedAt".to_string())?;
    let raw_url = payload
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "会话捕获 Cookie 缺少 url".to_string())?;
    let url = Url::parse(raw_url).map_err(|_| "会话捕获 Cookie url 非法".to_string())?;
    Ok((issued_at, url))
}

fn validate_url(
    capture: &PluginCapturedUrlCookieManifest,
    url: Url,
    issued_at: u64,
    current_time_ms: u64,
) -> Result<(), String> {
    let age_ms = current_time_ms.saturating_sub(issued_at);
    if age_ms > capture.max_age_ms {
        return Err(format!("会话捕获 Cookie 已过期 ageMs={age_ms}"));
    }
    if url.scheme() != capture.url.scheme
        || url.host_str() != Some(capture.url.host.as_str())
        || url.path() != capture.url.path
    {
        return Err("会话捕获 Cookie url 目标不匹配".into());
    }
    let query = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    for name in &capture.url.required_query_names {
        if !query.contains_key(name.as_str()) {
            return Err(format!("会话捕获 Cookie url 缺少关键参数：{name}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::plugin::{PluginBrowserSessionManifest, PluginCapturedUrlManifest};

    fn browser() -> PluginBrowserSessionManifest {
        PluginBrowserSessionManifest {
            login_url: "https://vendor.example/login".into(),
            allowed_urls: vec!["https://vendor.example/".into()],
            required_cookie_names: vec!["temporary-url".into()],
            user_agent: None,
            initialization_script: None,
            window_title: None,
            captured_url_cookie: Some(PluginCapturedUrlCookieManifest {
                cookie_name: "temporary-url".into(),
                max_age_ms: 240_000,
                freshness_slack_ms: 15_000,
                url: PluginCapturedUrlManifest {
                    scheme: "wss".into(),
                    host: "stream.vendor.example".into(),
                    path: "/v1/stream".into(),
                    required_query_names: vec!["client".into(), "signature".into()],
                },
            }),
        }
    }

    fn cookie(issued_at: u64, url: &str) -> Value {
        let payload = serde_json::json!({ "issuedAt": issued_at, "url": url });
        serde_json::json!({
            "name": "temporary-url",
            "value": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string()),
        })
    }

    #[test]
    fn accepts_a_fresh_capture_for_any_declared_provider() {
        let cookies = vec![cookie(
            995_000,
            "wss://stream.vendor.example/v1/stream?client=desktop&signature=valid",
        )];
        assert!(validate_capture_for_sync(&browser(), &cookies, 1_000_000, 1_010_000).is_ok());
    }

    #[test]
    fn rejects_a_capture_from_before_the_current_sync() {
        let cookies = vec![cookie(
            980_000,
            "wss://stream.vendor.example/v1/stream?client=desktop&signature=valid",
        )];
        assert!(
            validate_capture_for_sync(&browser(), &cookies, 1_000_000, 1_010_000)
                .unwrap_err()
                .contains("过旧")
        );
    }

    #[test]
    fn rejects_a_capture_missing_required_url_parameters() {
        let cookies = vec![cookie(
            1_000_000,
            "wss://stream.vendor.example/v1/stream?client=desktop",
        )];
        assert!(validate_capture_for_runtime(
            Some(&browser()),
            &serde_json::json!({ "cookies": cookies }),
            1_010_000,
        )
        .unwrap_err()
        .contains("signature"));
    }
}

use serde_json::Value;

pub(super) async fn read_json_response(
    resp: reqwest::Response,
    action: &str,
) -> Result<Value, String> {
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取{action}响应失败：{e}"))?;
    let value = serde_json::from_str::<Value>(&text)
        .map_err(|e| format!("{action}响应解析失败：{e}（{}）", truncate(&text, 200)))?;
    if !status.is_success() {
        return Err(format!(
            "{action}返回 {status}：{}",
            extract_error_message(&value, &text)
        ));
    }
    Ok(value)
}

pub(super) fn extract_error_message(value: &Value, text: &str) -> String {
    value
        .get("message")
        .or_else(|| value.get("msg"))
        .or_else(|| value.pointer("/error/message"))
        .or_else(|| value.pointer("/output/message"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| truncate(text, 200))
}

pub(super) fn format_task_error(prefix: &str, code: Option<&str>, message: Option<&str>) -> String {
    match (
        code.filter(|v| !v.is_empty()),
        message.filter(|v| !v.is_empty()),
    ) {
        (Some(code), Some(message)) => format!("{prefix} [{code}]：{message}"),
        (Some(code), None) => format!("{prefix} [{code}]"),
        (None, Some(message)) => format!("{prefix}：{message}"),
        (None, None) => prefix.to_string(),
    }
}

pub(super) fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

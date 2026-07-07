use serde_json::{json, Value};

/// Qwen-MT 文本翻译所用的固定接口地址（华北2/北京，与百炼默认地域一致）。
const TEXT_GENERATION_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation";

/// 支持增量流式输出的模型（每次只返回新增内容，需要在本地累加成完整译文）；
/// 不支持的模型（如 qwen-mt-plus）每次事件都返回当前已生成的完整序列，直接替换即可。
fn supports_incremental_output(model: &str) -> bool {
    matches!(model, "qwen-mt-flash" | "qwen-mt-lite")
}

/// 调用 Qwen-MT 翻译一段文本，通过 SSE 流式读取响应。每收到一次内容更新就用 `on_delta`
/// 回调"当前这句到目前为止的完整译文"（屏蔽增量/非增量差异），请求正常结束后返回最终完整译文。
pub async fn translate_streaming<F>(
    api_key: &str,
    model: &str,
    text: &str,
    source_lang: &str,
    target_lang: &str,
    mut on_delta: F,
) -> Result<String, String>
where
    F: FnMut(&str) + Send,
{
    if api_key.trim().is_empty() {
        return Err("请先在设置中填写阿里云百炼 API Key".to_string());
    }
    if text.trim().is_empty() {
        return Ok(String::new());
    }

    let incremental = supports_incremental_output(model);
    let body = json!({
        "model": model,
        "input": {
            "messages": [{ "role": "user", "content": text }]
        },
        "parameters": {
            "translation_options": {
                "source_lang": source_lang,
                "target_lang": target_lang,
            },
            "incremental_output": incremental,
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(TEXT_GENERATION_URL)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json")
        .header("X-DashScope-SSE", "enable")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求翻译接口失败：{e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp
            .text()
            .await
            .map_err(|e| format!("读取翻译接口响应失败：{e}"))?;
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("message")
                    .or_else(|| value.get("msg"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .unwrap_or(text);
        return Err(format!("翻译接口返回 {status}：{message}"));
    }

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut full_text = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取翻译响应失败：{e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let line: String = buffer.drain(..=pos).collect();
            let Some(data) = line.trim().strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            let Some(delta) = event
                .pointer("/output/choices/0/message/content")
                .and_then(Value::as_str)
            else {
                continue;
            };
            if delta.is_empty() {
                continue;
            }
            if incremental {
                full_text.push_str(delta);
            } else {
                full_text = delta.to_string();
            }
            on_delta(&full_text);
        }
    }

    Ok(full_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_output_flag_matches_documented_model_support() {
        assert!(supports_incremental_output("qwen-mt-flash"));
        assert!(supports_incremental_output("qwen-mt-lite"));
        assert!(!supports_incremental_output("qwen-mt-plus"));
        assert!(!supports_incremental_output("qwen-mt-turbo"));
    }
}

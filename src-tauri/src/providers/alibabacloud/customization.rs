use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::protocol::FUN_ASR_MODEL;

/// 定制热词列表所用的固定接口地址（华北2/北京）。
const CUSTOMIZATION_URL: &str = "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/customization";
/// 热词列表前缀，仅允许数字和小写字母、长度不超过 10 个字符。
const VOCABULARY_PREFIX: &str = "deskhw";

/// 单条热词（文本 + 权重），用于创建/更新热词列表。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotwordEntry {
    pub text: String,
    pub weight: i32,
}

async fn post_customization(api_key: &str, body: Value) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(CUSTOMIZATION_URL)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求热词接口失败：{e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取热词接口响应失败：{e}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("热词接口响应解析失败：{e}（{}）", truncate(&text, 200)))?;
    if !status.is_success() {
        let message = value
            .get("message")
            .or_else(|| value.get("msg"))
            .and_then(Value::as_str)
            .unwrap_or(&text);
        return Err(format!("热词接口返回 {status}：{message}"));
    }
    Ok(value)
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// 创建一个新热词列表，返回服务端分配的 `vocabulary_id`。
pub async fn create_vocabulary(api_key: &str, vocabulary: &[HotwordEntry]) -> Result<String, String> {
    let body = json!({
        "model": "speech-biasing",
        "input": {
            "action": "create_vocabulary",
            "target_model": FUN_ASR_MODEL,
            "prefix": VOCABULARY_PREFIX,
            "vocabulary": vocabulary,
        }
    });
    let value = post_customization(api_key, body).await?;
    value
        .pointer("/output/vocabulary_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| "创建热词列表失败：响应缺少 vocabulary_id".to_string())
}

/// 用新内容完全替换已有热词列表。
pub async fn update_vocabulary(
    api_key: &str,
    vocabulary_id: &str,
    vocabulary: &[HotwordEntry],
) -> Result<(), String> {
    let body = json!({
        "model": "speech-biasing",
        "input": {
            "action": "update_vocabulary",
            "vocabulary_id": vocabulary_id,
            "vocabulary": vocabulary,
        }
    });
    post_customization(api_key, body).await?;
    Ok(())
}

/// 删除一个热词列表。
pub async fn delete_vocabulary(api_key: &str, vocabulary_id: &str) -> Result<(), String> {
    let body = json!({
        "model": "speech-biasing",
        "input": {
            "action": "delete_vocabulary",
            "vocabulary_id": vocabulary_id,
        }
    });
    post_customization(api_key, body).await?;
    Ok(())
}

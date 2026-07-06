use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 定制热词列表所用的固定接口地址（华北2/北京）。
const CUSTOMIZATION_URL: &str = "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/customization";

/// 需要建热词表的模型（与 target_model 一一对应）及各自的列表前缀。
/// 阿里云要求识别时使用的模型必须与词表创建时的 target_model 完全一致，否则热词静默不生效，
/// 因此每个模型各建一份独立词表；前缀各不相同，方便按前缀从云端精确恢复某个模型对应的词表。
///
/// 与模型注册表的一致性关系：本表列出的模型必须是注册表中 `supportsVocabulary: true` 的模型；
/// 注册表中标记支持热词的模型未必都出现在这里（例如 paraformer 系列支持 vocabulary_id 参数，
/// 但本应用未为其建独立词表）。两者一致性由单元测试确保。
pub const VOCABULARY_TARGETS: &[(&str, &str)] = &[
    ("fun-asr-realtime-2026-02-28", "deskhwrl"),
    ("fun-asr-realtime", "deskhwrs"),
    ("fun-asr", "deskhwfa"),
];

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
pub async fn create_vocabulary(
    api_key: &str,
    target_model: &str,
    prefix: &str,
    vocabulary: &[HotwordEntry],
) -> Result<String, String> {
    let body = json!({
        "model": "speech-biasing",
        "input": {
            "action": "create_vocabulary",
            "target_model": target_model,
            "prefix": prefix,
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

#[derive(Debug, Deserialize)]
struct VocabularySummary {
    vocabulary_id: String,
    status: String,
    #[serde(default)]
    gmt_modified: String,
}

/// 按前缀批量查询该账号下已创建的热词列表，返回可用（status=OK）的词表 ID，
/// 按修改时间倒序排列（最新的在前）。
pub async fn list_vocabulary(api_key: &str, prefix: &str) -> Result<Vec<String>, String> {
    let body = json!({
        "model": "speech-biasing",
        "input": {
            "action": "list_vocabulary",
            "prefix": prefix,
            "page_index": 0,
            "page_size": 10,
        }
    });
    let value = post_customization(api_key, body).await?;
    let list = value
        .pointer("/output/vocabulary_list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut summaries: Vec<VocabularySummary> = list
        .into_iter()
        .filter_map(|item| serde_json::from_value(item).ok())
        .filter(|item: &VocabularySummary| item.status == "OK")
        .collect();
    summaries.sort_by(|a, b| b.gmt_modified.cmp(&a.gmt_modified));
    Ok(summaries.into_iter().map(|item| item.vocabulary_id).collect())
}

/// 查询指定热词列表的完整内容。
pub async fn query_vocabulary(api_key: &str, vocabulary_id: &str) -> Result<Vec<HotwordEntry>, String> {
    let body = json!({
        "model": "speech-biasing",
        "input": {
            "action": "query_vocabulary",
            "vocabulary_id": vocabulary_id,
        }
    });
    let value = post_customization(api_key, body).await?;
    let vocabulary: Vec<HotwordEntry> = value
        .pointer("/output/vocabulary")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| "查询热词列表失败：响应缺少 vocabulary".to_string())?;
    Ok(vocabulary)
}

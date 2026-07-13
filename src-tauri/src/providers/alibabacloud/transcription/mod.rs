mod async_oss;
mod http;
mod media_encode;
mod sync_flash;
mod sync_qwen;
mod types;

use serde_json::json;

use super::customization::HotwordEntry;

pub use async_oss::{fetch_transcription_result, query_transcription_task, submit_transcription_task};
pub use types::{TranscriptionParams, TranscriptionResult, TranscriptionTaskStatus};

const MULTIMODAL_GENERATION_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TranscriptionModelFamily {
    FunAsr,
    FunAsrFlash,
    Paraformer,
    QwenFlash,
    QwenFiletrans,
}

pub async fn recognize_short_audio(
    api_key: &str,
    file_path: &str,
    params: &TranscriptionParams,
    hotwords: &[HotwordEntry],
) -> Result<TranscriptionResult, String> {
    if api_key.trim().is_empty() {
        return Err("请先保存阿里云百炼 API Key".to_string());
    }
    let model = params.model_id();
    let family = transcription_model_family(&model);
    // 同步短音频接口（multimodal-generation）不支持解析 OSS 临时上传返回的 oss:// 私有资源地址，
    // 必须直接传入公网 URL 或 Base64 Data URI；本地文件走 Data URI。
    // 请求体大小受限，这里统一先把原始文件转成单声道 16kHz PCM，再按模型分别压缩：
    // fun-asr-flash 的 parameters.format 文档明确支持 opus，直接用 Opus 压到足够小；
    // qwen3-asr-flash 没有 format 字段、全靠 Data URI 的 mediatype 判断格式，只用文档验证过的 mp3。
    let body = match family {
        TranscriptionModelFamily::FunAsrFlash => {
            let data_uri = media_encode::build_opus_data_uri(file_path)?;
            let mut messages = Vec::new();
            // fun-asr-flash 不支持 vocabulary_id，改用文档里的“上下文增强”：把热词词表拼成一条
            // input_text 消息放在音频消息之前，模型据此提升这些词的识别概率。
            if !hotwords.is_empty() {
                let vocabulary_text = hotwords
                    .iter()
                    .map(|item| item.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                messages.push(json!({
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": vocabulary_text,
                        }
                    ]
                }));
            }
            messages.push(json!({
                "role": "user",
                "content": [
                    {
                        "type": "input_audio",
                        "input_audio": {
                            "data": data_uri,
                        }
                    }
                ]
            }));
            json!({
                "model": model,
                "input": {
                    "messages": messages
                },
                "parameters": {
                    "format": "opus",
                    "sample_rate": crate::audio_prep::TARGET_SAMPLE_RATE.to_string(),
                }
            })
        }
        TranscriptionModelFamily::QwenFlash => {
            let data_uri = media_encode::build_mp3_data_uri(file_path)?;
            let mut asr_options = json!({
                "enable_itn": false,
            });
            if let Some(language) = params
                .language_hints
                .iter()
                .map(|item| item.trim())
                .find(|item| !item.is_empty())
            {
                asr_options["language"] = json!(language);
            }
            json!({
                "model": model,
                "input": {
                    "messages": [
                        {
                            "role": "system",
                            "content": [{ "text": "" }]
                        },
                        {
                            "role": "user",
                            "content": [{ "audio": data_uri }]
                        }
                    ]
                },
                "parameters": {
                    "asr_options": asr_options,
                }
            })
        }
        other => {
            return Err(format!("模型 {other:?} 不支持同步短音频识别"));
        }
    };

    let client = reqwest::Client::new();
    let mut request = client
        .post(MULTIMODAL_GENERATION_URL)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json");
    // fun-asr-flash 的非流式响应只返回“最后一句”的 sentence.words，无法还原多句音频的完整
    // 逐词时间戳（详见音频规格文档的 SSE 累积处理说明）；改用流式模式，累积每个
    // sentence_end=true 事件即可拿到完整的分句/逐词时间戳。qwen3-asr-flash 的响应本身就不含
    // 时间戳字段（无论流式与否都一样），维持非流式即可。
    if family == TranscriptionModelFamily::FunAsrFlash {
        request = request.header("X-DashScope-SSE", "enable");
    }
    let resp = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("提交短音频识别失败：{e}"))?;

    if family == TranscriptionModelFamily::FunAsrFlash {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("读取短音频识别响应失败：{e}"))?;
        if crate::debug_log_enabled() {
            let short = http::truncate(&text, 2000);
            crate::dlog!("[recognize_short_audio family={family:?}] response={short}");
        }
        if !status.is_success() {
            return Err(format!(
                "提交短音频识别返回 {status}：{}",
                sync_flash::extract_sse_error_message(&text)
            ));
        }
        return sync_flash::parse_fun_asr_flash_sse(&text);
    }

    let value = http::read_json_response(resp, "提交短音频识别").await?;
    if crate::debug_log_enabled() {
        let short = http::truncate(&value.to_string(), 2000);
        crate::dlog!("[recognize_short_audio family={family:?}] response={short}");
    }
    sync_qwen::parse_short_audio_result(value)
}

fn default_transcription_model() -> String {
    crate::providers::registry::default_file_model().to_string()
}

pub fn uses_async_transcription_task(model: &str) -> bool {
    crate::providers::registry::uses_async_transcription_task(model)
}

fn transcription_model_family(model: &str) -> TranscriptionModelFamily {
    use crate::providers::registry::{file_transcription_route, FileTranscriptionRoute};

    match file_transcription_route(model) {
        FileTranscriptionRoute::AsyncOss => {
            // AsyncOss 需要进一步区分 FunAsr / Paraformer / QwenFiletrans
            let normalized = model.trim();
            if normalized.starts_with("qwen3-asr-flash-filetrans") {
                TranscriptionModelFamily::QwenFiletrans
            } else if normalized.starts_with("paraformer") {
                TranscriptionModelFamily::Paraformer
            } else {
                TranscriptionModelFamily::FunAsr
            }
        }
        FileTranscriptionRoute::SyncFunAsrFlash => TranscriptionModelFamily::FunAsrFlash,
        FileTranscriptionRoute::SyncQwen => TranscriptionModelFamily::QwenFlash,
    }
}

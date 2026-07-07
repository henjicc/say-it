use serde_json::{json, Value};

use super::types::{
    RawTranscriptionResult, SubmitResponse, TaskResponse, TranscriptionParams, TranscriptionResult,
    TranscriptionTaskStatus,
};
use super::TranscriptionModelFamily;

const TRANSCRIPTION_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/transcription";
const TASK_URL_PREFIX: &str = "https://dashscope.aliyuncs.com/api/v1/tasks";

pub async fn submit_transcription_task(
    api_key: &str,
    file_url: &str,
    params: &TranscriptionParams,
    vocabulary_id: &str,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("请先保存阿里云百炼 API Key".to_string());
    }
    if file_url.trim().is_empty() {
        return Err("录音识别文件 URL 不能为空".to_string());
    }

    let model = params.model_id();
    let family = super::transcription_model_family(&model);
    let body = json!({
        "model": model,
        "input": transcription_input_value(family, file_url),
        "parameters": params.parameters_value(family, vocabulary_id),
    });
    let client = reqwest::Client::new();
    let mut request = client
        .post(TRANSCRIPTION_URL)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Content-Type", "application/json")
        .header("X-DashScope-Async", "enable")
        .json(&body);
    if file_url.trim_start().starts_with("oss://") {
        request = request.header("X-DashScope-OssResourceResolve", "enable");
    }
    let resp = request
        .send()
        .await
        .map_err(|e| format!("提交录音识别任务失败：{e}"))?;
    let value = super::http::read_json_response(resp, "提交录音识别任务").await?;
    let response: SubmitResponse =
        serde_json::from_value(value).map_err(|e| format!("解析录音识别提交响应失败：{e}"))?;
    if response.output.task_id.trim().is_empty() {
        return Err("提交录音识别任务失败：响应缺少 task_id".to_string());
    }
    Ok(response.output.task_id)
}

pub async fn query_transcription_task(
    api_key: &str,
    task_id: &str,
) -> Result<TranscriptionTaskStatus, String> {
    if task_id.trim().is_empty() {
        return Err("录音识别任务 ID 不能为空".to_string());
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/{}", TASK_URL_PREFIX, task_id.trim()))
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .send()
        .await
        .map_err(|e| format!("查询录音识别任务失败：{e}"))?;
    let value = super::http::read_json_response(resp, "查询录音识别任务").await?;
    let response: TaskResponse =
        serde_json::from_value(value).map_err(|e| format!("解析录音识别任务响应失败：{e}"))?;
    let result = response
        .output
        .result
        .or_else(|| response.output.results.into_iter().next());
    Ok(TranscriptionTaskStatus {
        task_status: response.output.task_status,
        result,
        code: response.output.code,
        message: response.output.message,
    })
}

pub async fn fetch_transcription_result(url: &str) -> Result<TranscriptionResult, String> {
    if url.trim().is_empty() {
        return Err("录音识别结果地址不能为空".to_string());
    }
    let client = reqwest::Client::new();
    let resp = client
        .get(url.trim())
        .send()
        .await
        .map_err(|e| format!("下载录音识别结果失败：{e}"))?;
    let value = super::http::read_json_response(resp, "下载录音识别结果").await?;
    let raw: RawTranscriptionResult =
        serde_json::from_value(value).map_err(|e| format!("解析录音识别结果失败：{e}"))?;
    Ok(TranscriptionResult {
        duration_ms: raw.properties.original_duration_in_milliseconds,
        transcripts: raw.transcripts,
    })
}

fn transcription_input_value(family: TranscriptionModelFamily, file_url: &str) -> Value {
    match family {
        TranscriptionModelFamily::QwenFiletrans => json!({
            "file_url": file_url.trim(),
        }),
        TranscriptionModelFamily::FunAsr | TranscriptionModelFamily::Paraformer => json!({
            "file_urls": [file_url.trim()],
        }),
        TranscriptionModelFamily::FunAsrFlash | TranscriptionModelFamily::QwenFlash => {
            unreachable!("同步短音频模型不应走异步 transcription 接口")
        }
    }
}

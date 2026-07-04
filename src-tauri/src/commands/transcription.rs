use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::commands::common::*;
use crate::prelude::*;
use crate::providers::alibabacloud::{
    fetch_transcription_result, query_transcription_task, recognize_short_audio,
    submit_transcription_task, upload_for_model, uses_async_transcription_task,
    TranscriptionParams, TranscriptionTaskStatus,
};
use crate::state::*;
use crate::text_align::{align_script, AlignOutput, AlignWord};

const TRANSCRIPTION_EVENT: &str = "transcription-event";
const FIRST_POLL_DELAY: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_secs(4);
const POLL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

type CancelFlag = Arc<AtomicBool>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptionStartResponse {
    pub(crate) job_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalFileInfo {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) size: u64,
}

#[tauri::command]
pub(crate) async fn get_local_file_info(file_path: String) -> Result<LocalFileInfo, String> {
    if file_path.trim().is_empty() {
        return Err("文件路径不能为空".to_string());
    }
    let path = Path::new(&file_path);
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|err| format!("读取文件信息失败：{err}"))?;
    if !metadata.is_file() {
        return Err("请选择一个本地文件".to_string());
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("未命名文件")
        .to_string();
    Ok(LocalFileInfo {
        path: file_path,
        name,
        size: metadata.len(),
    })
}

#[tauri::command]
pub(crate) async fn save_text_file(path: String, content: String) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("保存路径不能为空".to_string());
    }
    let path = Path::new(&path);
    if path.is_dir() {
        return Err("保存路径不能是文件夹".to_string());
    }
    tokio::fs::write(path, content)
        .await
        .map_err(|err| format!("写入文件失败：{err}"))
}

#[tauri::command]
pub(crate) fn align_transcript(
    words: Vec<AlignWord>,
    script_lines: Vec<String>,
) -> Result<AlignOutput, String> {
    if script_lines.iter().all(|line| line.trim().is_empty()) {
        return Err("请先输入文稿内容".to_string());
    }
    align_script(&words, &script_lines)
}

#[tauri::command]
pub(crate) async fn transcription_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    file_path: String,
    params: Option<TranscriptionParams>,
) -> Result<TranscriptionStartResponse, String> {
    if file_path.trim().is_empty() {
        return Err("请选择要识别的音视频文件".to_string());
    }

    let api_key_result = funasr_api_key(&state);
    let params = params.unwrap_or_default();
    let job_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state
            .transcriptions
            .lock()
            .map_err(|_| "录音识别任务表锁定失败".to_string())?;
        jobs.insert(job_id.clone(), cancel.clone());
    }

    let jobs = state.transcriptions.clone();
    let task_job_id = job_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = match api_key_result {
            Ok(api_key) if !api_key.is_empty() => {
                run_transcription_job(
                    app.clone(),
                    task_job_id.clone(),
                    api_key,
                    file_path,
                    params,
                    cancel,
                )
                .await
            }
            Ok(_) => Err("请先保存阿里云百炼 API Key".to_string()),
            Err(err) => Err(err),
        };
        if let Err(message) = result {
            emit_transcription_event(&app, &task_job_id, "error", json!({ "message": message }));
        }
        if let Ok(mut guard) = jobs.lock() {
            guard.remove(&task_job_id);
        }
    });

    Ok(TranscriptionStartResponse { job_id })
}

#[tauri::command]
pub(crate) fn transcription_cancel(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    job_id: String,
) -> Result<(), String> {
    let flag = {
        let guard = state
            .transcriptions
            .lock()
            .map_err(|_| "录音识别任务表锁定失败".to_string())?;
        guard.get(&job_id).cloned()
    }
    .ok_or_else(|| "未找到录音识别任务".to_string())?;
    flag.store(true, Ordering::Relaxed);
    emit_transcription_event(
        &app,
        &job_id,
        "error",
        json!({
            "message": "录音识别已取消：仅停止本地轮询，云端任务可能仍在继续",
            "cancelled": true,
        }),
    );
    Ok(())
}

async fn run_transcription_job(
    app: tauri::AppHandle,
    job_id: String,
    api_key: String,
    file_path: String,
    params: TranscriptionParams,
    cancel: CancelFlag,
) -> Result<(), String> {
    let model = params.model_id();
    emit_transcription_event(
        &app,
        &job_id,
        "uploading",
        json!({
            "filePath": &file_path,
            "model": &model,
        }),
    );

    if !uses_async_transcription_task(&model) {
        // 同步短音频接口（fun-asr-flash / qwen3-asr-flash）直接读取本地文件识别，
        // 不经过临时 OSS 上传：OSS 返回的 oss:// 资源地址仅异步转写接口能解析。
        emit_transcription_event(
            &app,
            &job_id,
            "submitted",
            json!({
                "taskId": "",
            }),
        );
        let result = recognize_short_audio(&api_key, &file_path, &params).await?;
        if is_cancelled(&cancel) {
            return Ok(());
        }
        emit_transcription_event(
            &app,
            &job_id,
            "completed",
            json!({
                "taskId": "",
                "result": result,
            }),
        );
        return Ok(());
    }

    let file_url = upload_for_model(&api_key, &model, &file_path).await?;
    if is_cancelled(&cancel) {
        return Ok(());
    }

    let task_id = submit_transcription_task(&api_key, &file_url, &params).await?;
    emit_transcription_event(
        &app,
        &job_id,
        "submitted",
        json!({
            "taskId": &task_id,
            "fileUrl": &file_url,
        }),
    );
    sleep(FIRST_POLL_DELAY).await;

    let started_at = Instant::now();
    let mut poll_count = 0_u32;
    loop {
        if is_cancelled(&cancel) {
            return Ok(());
        }
        if started_at.elapsed() >= POLL_TIMEOUT {
            return Err("录音识别任务轮询超时，请稍后重试".to_string());
        }
        poll_count += 1;
        let status = query_transcription_task(&api_key, &task_id).await?;
        emit_transcription_event(
            &app,
            &job_id,
            "polling",
            json!({
                "taskId": &task_id,
                "pollCount": poll_count,
                "taskStatus": &status.task_status,
            }),
        );
        let task_status = normalized_status(&status);
        match task_status.as_str() {
            "PENDING" | "RUNNING" => sleep(POLL_INTERVAL).await,
            "SUCCEEDED" => {
                let result_url = status.successful_transcription_url()?;
                let result = fetch_transcription_result(&result_url).await?;
                if is_cancelled(&cancel) {
                    return Ok(());
                }
                emit_transcription_event(
                    &app,
                    &job_id,
                    "completed",
                    json!({
                        "taskId": &task_id,
                        "result": result,
                    }),
                );
                return Ok(());
            }
            "FAILED" => {
                return Err(format_failed_task(&status));
            }
            other => {
                return Err(format!("录音识别任务返回未知状态：{other}"));
            }
        }
    }
}

fn normalized_status(status: &TranscriptionTaskStatus) -> String {
    status.task_status.trim().to_ascii_uppercase()
}

fn format_failed_task(status: &TranscriptionTaskStatus) -> String {
    match (
        status.code.as_deref().filter(|v| !v.is_empty()),
        status.message.as_deref().filter(|v| !v.is_empty()),
    ) {
        (Some(code), Some(message)) => format!("录音识别任务失败 [{code}]：{message}"),
        (Some(code), None) => format!("录音识别任务失败 [{code}]"),
        (None, Some(message)) => format!("录音识别任务失败：{message}"),
        (None, None) => "录音识别任务失败".to_string(),
    }
}

fn is_cancelled(cancel: &CancelFlag) -> bool {
    cancel.load(Ordering::Relaxed)
}

fn funasr_api_key(state: &tauri::State<'_, RuntimeState>) -> Result<String, String> {
    let settings = read_provider_settings(state)?;
    let profile = find_profile(&settings, FUNASR_PROVIDER_ID)
        .ok_or_else(|| "未找到 Fun-ASR 供应商配置".to_string())?;
    Ok(profile
        .config
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string())
}

fn emit_transcription_event(app: &tauri::AppHandle, job_id: &str, stage: &str, payload: Value) {
    let mut value = match payload {
        Value::Object(map) => Value::Object(map),
        other => json!({ "data": other }),
    };
    if let Value::Object(map) = &mut value {
        map.insert("jobId".to_string(), json!(job_id));
        map.insert("stage".to_string(), json!(stage));
    }
    if debug_log_enabled() {
        let short = job_id.get(..8).unwrap_or(job_id);
        let mut summary = value.to_string();
        if summary.chars().count() > 300 {
            summary = summary.chars().take(300).collect::<String>() + "…";
        }
        dlog!("[transcription {short}] {summary}");
    }
    let _ = app.emit(TRANSCRIPTION_EVENT, value);
}

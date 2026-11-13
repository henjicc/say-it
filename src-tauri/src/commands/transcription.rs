mod startup;

use std::path::Path;
use std::sync::atomic::Ordering;
use crate::cancellation::CancellationFlag;

use crate::application::subtitle_document::{to_srt, SubtitleCue};
use crate::application::transcription::DEFAULT_TRANSCRIPTION_JOB_KIND;
use crate::commands::common::*;
use crate::prelude::*;
use crate::providers::capabilities::{
    file_recognition_for_with_extensions, FileRecognitionProvider, TranscriptionParams,
};
use crate::state::*;
use crate::text_align::{align_script, AlignOutput, AlignWord};

const TRANSCRIPTION_EVENT: &str = "transcription-event";
type CancelFlag = Arc<CancellationFlag>;

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
pub(crate) async fn save_subtitle_srt(path: String, cues: Vec<SubtitleCue>) -> Result<(), String> {
    save_text_file(path, to_srt(cues)).await
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
    kind: Option<String>,
) -> Result<TranscriptionStartResponse, String> {
    let kind = kind.unwrap_or_else(|| DEFAULT_TRANSCRIPTION_JOB_KIND.to_string());
    // 用户在转写、生成字幕、文稿对齐里选的文件可能反复识别，复用上次的结果；
    // 听写、模型对比、会议录音每次都是新录音，对比本身也要求真的再识别一次。
    let reuse_cached = matches!(kind.as_str(), "transcribe" | "align");
    transcription_start_inner(app, &state, file_path, params, &kind, reuse_cached).await
}

pub(crate) async fn transcription_start_inner(
    app: tauri::AppHandle,
    state: &RuntimeState,
    file_path: String,
    params: Option<TranscriptionParams>,
    kind: &str,
    reuse_cached: bool,
) -> Result<TranscriptionStartResponse, String> {
    transcription_start_with_recording(app, state, file_path, params, kind, reuse_cached, None, |_| Ok(()))
        .await
}

pub(crate) async fn transcription_start_with_recording(
    app: tauri::AppHandle,
    state: &RuntimeState,
    file_path: String,
    params: Option<TranscriptionParams>,
    kind: &str,
    reuse_cached: bool,
    recording: Option<Arc<crate::audio_wav::RecordedWav>>,
    register_owner: impl FnOnce(&str) -> Result<(), String> + Send,
) -> Result<TranscriptionStartResponse, String> {
    if file_path.trim().is_empty() {
        return Err("请选择要识别的音视频文件".to_string());
    }

    let params = params.unwrap_or_default();
    let provider_result = resolve_file_recognition_provider(&state, &params.model);
    let job_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(CancellationFlag::new(false));
    startup::register_job(state, &job_id, kind, cancel.clone(), register_owner)?;

    let jobs = state.transcriptions.clone();
    let task_job_id = job_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = match provider_result {
            Ok(provider) => {
                run_transcription_job(
                    app.clone(),
                    task_job_id.clone(),
                    provider,
                    file_path,
                    params,
                    reuse_cached,
                    cancel,
                )
                .await
            }
            Err(err) => Err(err),
        };
        if let Err(message) = result {
            emit_transcription_event(&app, &task_job_id, "error", json!({ "message": message }));
        }
        // 取消标记/完成事件不代表供应商已释放文件；实际工作退出后才归还录音所有权。
        drop(recording);
        if let Ok(mut guard) = jobs.lock() {
            guard.remove(&task_job_id);
        }
        if let Err(error) = app.state::<RuntimeState>().transcription_runtime.finish(&task_job_id) {
            crate::application::diagnostics::event("error", "transcription.projectionCleanupFailed", json!({ "jobId": task_job_id, "error": error }));
        }
        crate::application::idle_reclaim::request(&app);
    });

    Ok(TranscriptionStartResponse { job_id })
}

#[tauri::command]
pub(crate) fn transcription_cancel(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    job_id: String,
) -> Result<(), String> {
    transcription_cancel_inner(&app, &state, &job_id)
}

pub(crate) fn transcription_cancel_inner(
    app: &tauri::AppHandle,
    state: &RuntimeState,
    job_id: &str,
) -> Result<(), String> {
    let flag = {
        let guard = state
            .transcriptions
            .lock()
            .map_err(|_| "录音识别任务表锁定失败".to_string())?;
        guard.get(job_id).cloned()
    }
    .ok_or_else(|| "未找到录音识别任务".to_string())?;
    flag.store(true, Ordering::Relaxed);
    emit_transcription_event(
        app,
        job_id,
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
    provider: FileRecognitionProvider,
    file_path: String,
    params: TranscriptionParams,
    reuse_cached: bool,
    cancel: CancelFlag,
) -> Result<(), String> {
    let model = params.model_id();
    let cache = if reuse_cached {
        let key = crate::application::transcription_cache::key(
            &file_path,
            &params,
            provider.customization(),
            &provider.implementation_revision(),
        )?;
        let dir = crate::application::data_root::data_subdir(
            &app,
            crate::application::transcription_cache::DIR,
        )?;
        if let Some(result) = crate::application::transcription_cache::load(&dir, &key) {
            emit_transcription_event(
                &app,
                &job_id,
                "completed",
                json!({ "taskId": "", "result": result, "cached": true }),
            );
            return Ok(());
        }
        Some((dir, key))
    } else {
        None
    };
    emit_transcription_event(
        &app,
        &job_id,
        "uploading",
        json!({
            "filePath": &file_path,
            "model": &model,
        }),
    );

    // 内置百炼 SDK、v5 插件与本地模型都由各自能力模块完成上传/轮询；
    // 宿主只持有一个可取消的能力执行，不再暴露供应商任务协议。
    emit_transcription_event(
        &app,
        &job_id,
        "submitted",
        json!({ "taskId": "", "moduleModel": model }),
    );
    let result = provider
        .recognize_short(&file_path, &params, Some(cancel.clone()))
        .await?;
    if is_cancelled(&cancel) {
        return Ok(());
    }
    if let Some((dir, key)) = &cache {
        if let Err(error) = crate::application::transcription_cache::store(dir, key, &result) {
            crate::application::diagnostics::event(
                "warn",
                "transcription.cacheStoreFailed",
                json!({ "error": error }),
            );
        }
    }
    emit_transcription_event(
        &app,
        &job_id,
        "completed",
        json!({ "taskId": "", "result": result }),
    );
    Ok(())
}

fn is_cancelled(cancel: &CancelFlag) -> bool {
    cancel.load(Ordering::Relaxed)
}

/// 已保存的「文件」识别参数：存在默认识别供应商配置的 `transcription` 字段里。
/// 没有主窗口时（会议录制条停止录制）由这里取参数开始识别。
pub(crate) fn stored_transcription_params(state: &RuntimeState) -> Result<TranscriptionParams, String> {
    let settings = read_provider_settings(state)?;
    let provider = crate::providers::default_provider_id(&settings, "asr");
    let stored = settings
        .profiles
        .iter()
        .find(|profile| profile.id == provider)
        .and_then(|profile| profile.config.get("transcription"));
    let registry = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?;
    Ok(normalize_stored_params(stored, |model| {
        crate::providers::registry::model_info(model)
            .is_some_and(|info| info.scenes.iter().any(|scene| scene == "transcription"))
            || registry.provider_id_for_model(model).is_some()
            || registry.local_model_for_model(model).is_some()
    }))
}

/// 把存着的识别参数收敛成合法的一份：模型已不能用于文件识别就用默认模型，说话人数取整且只在
/// 开启说话人分离时生效。这份配置由界面按 JavaScript 的宽松规则写入和读回，这里对
/// 开关和人数的取值保持同样宽松。
fn normalize_stored_params(
    stored: Option<&Value>,
    model_exists: impl Fn(&str) -> bool,
) -> TranscriptionParams {
    let mut params = TranscriptionParams::default();
    let Some(stored) = stored else {
        return params;
    };
    if let Some(model) = stored["model"].as_str().map(str::trim) {
        if !model.is_empty() && model_exists(model) {
            params.model = model.to_string();
        }
    }
    params.language_hints = stored["languageHints"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|hint| !hint.is_empty())
        .map(str::to_string)
        .collect();
    let diarization = match &stored["diarizationEnabled"] {
        Value::Null => false,
        Value::Bool(enabled) => *enabled,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        _ => true,
    };
    params.diarization_enabled = Some(diarization);
    // 旧版本存过小数，按截断取整。
    let speaker_count = &stored["speakerCount"];
    params.speaker_count = speaker_count
        .as_f64()
        .or_else(|| speaker_count.as_str()?.trim().parse().ok())
        .map(f64::trunc)
        .filter(|count| diarization && *count >= 1.0 && *count <= f64::from(u32::MAX))
        .map(|count| count as u32);
    params
}

fn resolve_file_recognition_provider(
    state: &RuntimeState,
    model: &str,
) -> Result<FileRecognitionProvider, String> {
    let (plugin_model_provider, local_model) = {
        let registry = state
            .plugin_registry
            .lock()
            .map_err(|_| "插件注册表锁失败".to_string())?;
        (
            registry.provider_id_for_model(model),
            registry.local_model_for_model(model),
        )
    };
    let model_provider = crate::providers::registry::model_info(model)
        .map(|model| model.provider_id.clone())
        .or(plugin_model_provider);
    let provider_id = resolve_provider_id(state, "asr", model_provider)?;
    let profile = provider_profile_for_execution(state, &provider_id)?;
    let plugin = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .runtime_for_provider(&provider_id)?
        .map(|spec| spec.bind_credentials(state.credentials.clone()));
    file_recognition_for_with_extensions(
        &profile,
        plugin,
        local_model,
        state.credentials.clone(),
        crate::application::customization::resolve_for_model(state, model),
    )
    .map_err(|error| error.to_string())
}

fn emit_transcription_event(app: &tauri::AppHandle, job_id: &str, stage: &str, payload: Value) {
    let state = app.state::<RuntimeState>();
    let value = match state
        .transcription_runtime
        .apply_event(job_id, stage, payload)
    {
        Ok(Some(value)) => value,
        Ok(None) => return,
        Err(error) => {
            crate::application::diagnostics::event(
                "error",
                "transcription.projectionFailed",
                json!({ "jobId": job_id, "error": error }),
            );
            return;
        }
    };
    if debug_log_enabled() {
        let short = job_id.get(..8).unwrap_or(job_id);
        let mut summary = value.to_string();
        if summary.chars().count() > 300 {
            summary = summary.chars().take(300).collect::<String>() + "…";
        }
        dlog!("[transcription {short}] {summary}");
    }
    if let Some(state) = app.try_state::<RuntimeState>() {
        let revision = crate::application::contract::next_revision(&state.snapshot_revision);
        let _ = app.emit(
            "domain-event",
            crate::application::contract::DomainEventEnvelope {
                revision,
                domain: "transcription".into(),
                event_type: "jobUpdated".into(),
                session_id: Some(job_id.to_string()),
                payload: value.clone(),
            },
        );
    }
    let _ = app.emit(TRANSCRIPTION_EVENT, &value);
    crate::application::events::publish_backend_event(
        &state,
        crate::application::events::BackendEvent::Transcription {
            job_id: job_id.to_string(),
            stage: stage.to_string(),
            payload: value,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_params_fall_back_to_defaults_and_only_keep_valid_values() {
        let exists = |model: &str| model == "known-model";
        let defaults = normalize_stored_params(None, exists);
        assert_eq!(defaults.model, TranscriptionParams::default().model);
        assert_eq!(defaults.speaker_count, None);

        let params = normalize_stored_params(
            Some(&json!({
                "model": "known-model",
                "languageHints": ["zh", 5, ""],
                "diarizationEnabled": true,
                "speakerCount": 3.7,
            })),
            exists,
        );
        assert_eq!(params.model, "known-model");
        assert_eq!(params.language_hints, ["zh"]);
        assert_eq!(params.diarization_enabled, Some(true));
        assert_eq!(params.speaker_count, Some(3));

        let params = normalize_stored_params(
            Some(&json!({ "model": "removed-model", "speakerCount": 4 })),
            exists,
        );
        assert_eq!(params.model, TranscriptionParams::default().model);
        assert_eq!(params.speaker_count, None, "没开说话人分离时不带人数");
        for (count, expected) in [(json!(0.4), None), (json!(-1), None), (json!("3"), Some(3))] {
            let stored = json!({ "diarizationEnabled": true, "speakerCount": count });
            assert_eq!(normalize_stored_params(Some(&stored), exists).speaker_count, expected);
        }
    }
}

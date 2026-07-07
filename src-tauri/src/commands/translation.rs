use crate::commands::common::*;
use crate::prelude::*;
use crate::state::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslateSubtitleRequest {
    /// 由前端生成的会话代次标识：停止/重连字幕后旧代次的迟到事件会被前端据此丢弃。
    pub(crate) request_id: String,
    /// 该句在当前会话里的单调递增序号，用于按顺序拼出多句译文。
    pub(crate) segment_seq: u64,
    pub(crate) text: String,
    pub(crate) model: String,
    pub(crate) source_lang: String,
    pub(crate) target_lang: String,
}

fn emit_subtitle_translation_event(
    app: &tauri::AppHandle,
    request_id: &str,
    segment_seq: u64,
    text: &str,
    done: bool,
    error: Option<String>,
) {
    let _ = app.emit(
        "subtitle-translation-event",
        json!({
            "requestId": request_id,
            "segmentSeq": segment_seq,
            "text": text,
            "done": done,
            "error": error,
        }),
    );
}

/// 对实时字幕的一句定稿文本发起 Qwen-MT 翻译，异步流式返回；不阻塞调用方，
/// 结果通过 `subtitle-translation-event` 事件回传（可能多次：每次增量 + 一次 done）。
#[tauri::command]
pub(crate) fn translate_subtitle_start(
    app: tauri::AppHandle,
    request: TranslateSubtitleRequest,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let text = request.text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }

    let provider_id = resolve_provider_id(&state, "llm", None)?;
    let settings = read_provider_settings(&state)?;
    let profile = find_profile(&settings, &provider_id)
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    let api_key = profile
        .config
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if api_key.trim().is_empty() {
        return Err("请先在设置中填写阿里云百炼 API Key".to_string());
    }

    let TranslateSubtitleRequest {
        request_id,
        segment_seq,
        model,
        source_lang,
        target_lang,
        ..
    } = request;

    tauri::async_runtime::spawn(async move {
        let delta_app = app.clone();
        let delta_request_id = request_id.clone();
        let result = crate::providers::alibabacloud::translate_streaming(
            &api_key,
            &model,
            &text,
            &source_lang,
            &target_lang,
            move |partial| {
                emit_subtitle_translation_event(
                    &delta_app,
                    &delta_request_id,
                    segment_seq,
                    partial,
                    false,
                    None,
                );
            },
        )
        .await;
        match result {
            Ok(full_text) => {
                emit_subtitle_translation_event(&app, &request_id, segment_seq, &full_text, true, None);
            }
            Err(err) => {
                emit_subtitle_translation_event(&app, &request_id, segment_seq, "", true, Some(err));
            }
        }
    });

    Ok(())
}

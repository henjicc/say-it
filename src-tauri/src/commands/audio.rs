use crate::prelude::*;
use crate::state::*;

pub(crate) fn emit_asr_stream_event(app: &tauri::AppHandle, session_id: &str, kind: &str, payload: Value) {
    // 仅在调试日志开启时构建摘要字符串，避免识别期间每条结果的无谓开销。
    if debug_log_enabled() {
        let short = session_id.get(..8).unwrap_or(session_id);
        let mut summary = payload.to_string();
        if summary.chars().count() > 300 {
            summary = summary.chars().take(300).collect::<String>() + "…";
        }
        dlog!("[asr {short}] {kind} {summary}");
    }
    if let Some(state) = app.try_state::<RuntimeState>() {
        state.backend_events.publish(crate::application::events::BackendEvent::Asr {
            session_id: session_id.to_string(), kind: kind.to_string(), payload: payload.clone(),
        });
    }
    let _ = app.emit(
        "asr-stream-event",
        json!({
          "session_id": session_id,
          "kind": kind,
          "payload": payload
        }),
    );
}



#[tauri::command]
pub(crate) fn process_audio_offline(request: AudioProcessRequest) -> Result<OfflineResult, String> {
    let samples = decode_f32_base64(&request.samples_base64)?;
    Ok(process_offline(
        &samples,
        request.sample_rate,
        &request.params,
    ))
}



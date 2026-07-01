use crate::commands::audio::emit_asr_stream_event;
use crate::commands::common::*;
use crate::prelude::*;
use crate::state::*;

const ASR_FINISH_TIMEOUT: Duration = Duration::from_secs(8);

struct FunAsrProtocol {
    task_id: String,
    started: bool,
    pending: Vec<Vec<u8>>,
}

#[tauri::command]
pub(crate) async fn start_asr_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    provider_id: Option<String>,
    sample_rate: Option<u32>,
    params: Option<DspParams>,
) -> Result<AsrStreamStartResponse, String> {
    let provider_id = resolve_provider_id(&state, "asr", provider_id)?;
    let settings = read_provider_settings(&state)?;
    let profile = find_profile(&settings, &provider_id)
        .cloned()
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    if profile.kind != "alibabacloud-funasr" {
        return Err(format!("当前版本不支持供应商类型：{}", profile.kind));
    }

    let funasr_params: FunAsrParams =
        serde_json::from_value(profile.config.clone()).map_err(|e| e.to_string())?;
    if funasr_params.api_key.trim().is_empty() {
        return Err("请先在设置中填写 Fun-ASR 的 API Key".to_string());
    }
    let req = funasr_ws_request(&funasr_params.api_key)?;
    let (ws_stream, _) = connect_async(req).await.map_err(|e| e.to_string())?;
    let (mut writer, mut reader) = ws_stream.split();
    let task_id = Uuid::new_v4().to_string();
    writer
        .send(build_run_task_message(&task_id, &funasr_params))
        .await
        .map_err(|e| e.to_string())?;

    let mut protocol = FunAsrProtocol {
        task_id,
        started: false,
        pending: Vec::new(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
    let session_id = Uuid::new_v4().to_string();

    {
        let mut streams = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        streams.insert(session_id.clone(), AsrStreamHandle { tx: tx.clone() });
    }

    let streams = state.asr_streams.clone();
    let app_handle = app.clone();
    let task_session_id = session_id.clone();
    let stream_sample_rate = sample_rate.unwrap_or(48_000);
    let dsp_info = params.as_ref().map(|p| {
        json!({
            "sample_rate": stream_sample_rate,
            "denoise_enabled": p.denoise_enabled,
            "target_lufs": p.target_lufs,
            "max_gain_db": p.max_gain_db,
            "peak_limit_dbfs": p.peak_limit_dbfs,
            "vad_gate": p.vad_gate,
        })
    });
    let mut dsp = params.map(|p| StreamDsp::new(p, stream_sample_rate));

    tauri::async_runtime::spawn(async move {
        emit_asr_stream_event(
            &app_handle,
            &task_session_id,
            "opened",
            json!({
                "message": "funasr websocket opened",
                "dsp_enabled": dsp.is_some(),
                "dsp": dsp_info,
            }),
        );
        let mut should_exit = false;
        let mut audio_chunks: u64 = 0;
        let mut audio_bytes: u64 = 0;
        let mut finish_sent_at: Option<Instant> = None;

        loop {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    AsrStreamInput::Audio(bytes) => {
                        if send_or_queue_audio(
                            &mut writer,
                            &mut protocol,
                            bytes,
                            &app_handle,
                            &task_session_id,
                            &mut audio_chunks,
                            &mut audio_bytes,
                        )
                        .await
                        .is_err()
                        {
                            should_exit = true;
                            break;
                        }
                    }
                    AsrStreamInput::RawF32(samples) => {
                        let bytes = if let Some(dsp) = dsp.as_mut() {
                            dsp.process(&samples)
                        } else {
                            Vec::new()
                        };
                        if bytes.is_empty() {
                            continue;
                        }
                        if send_or_queue_audio(
                            &mut writer,
                            &mut protocol,
                            bytes,
                            &app_handle,
                            &task_session_id,
                            &mut audio_chunks,
                            &mut audio_bytes,
                        )
                        .await
                        .is_err()
                        {
                            should_exit = true;
                            break;
                        }
                    }
                    AsrStreamInput::Finish => {
                        let secs = audio_bytes as f64 / (OUTPUT_RATE as f64 * 2.0);
                        dlog!(
                            "[asr {}] 发送 finish（共 {} 块 / {} 字节 ≈ {:.1}s 音频）",
                            task_session_id.get(..8).unwrap_or(&task_session_id),
                            audio_chunks,
                            audio_bytes,
                            secs
                        );
                        if let Err(err) =
                            writer.send(build_finish_task_message(&protocol.task_id)).await
                        {
                            emit_asr_stream_event(
                                &app_handle,
                                &task_session_id,
                                "error",
                                json!({ "message": format!("发送 finish 失败: {err}"), "stage": "send_finish" }),
                            );
                            should_exit = true;
                            break;
                        }
                        finish_sent_at = Some(Instant::now());
                    }
                    AsrStreamInput::Stop => {
                        let _ = writer.send(Message::Close(None)).await;
                        should_exit = true;
                        break;
                    }
                }
            }
            if should_exit {
                break;
            }
            if let Some(sent_at) = finish_sent_at {
                if sent_at.elapsed() >= ASR_FINISH_TIMEOUT {
                    emit_asr_stream_event(
                        &app_handle,
                        &task_session_id,
                        "finish_timeout",
                        json!({
                            "message": "ASR finish timeout; using latest result",
                            "timeout_ms": ASR_FINISH_TIMEOUT.as_millis()
                        }),
                    );
                    break;
                }
            }

            let message = tokio::time::timeout(Duration::from_millis(50), reader.next()).await;
            let Ok(message) = message else {
                continue;
            };
            let Some(message) = message else {
                break;
            };
            match message {
                Ok(Message::Text(text)) => match parse_funasr_message(&text) {
                    FunAsrEvent::Started => {
                        protocol.started = true;
                        let queued = std::mem::take(&mut protocol.pending);
                        let mut flush_failed = false;
                        for bytes in queued {
                            if let Err(err) = writer.send(Message::Binary(bytes.into())).await {
                                emit_asr_stream_event(
                                    &app_handle,
                                    &task_session_id,
                                    "error",
                                    json!({ "message": format!("发送缓冲音频失败: {err}"), "stage": "send_audio" }),
                                );
                                flush_failed = true;
                                break;
                            }
                        }
                        if flush_failed {
                            break;
                        }
                        emit_asr_stream_event(
                            &app_handle,
                            &task_session_id,
                            "event",
                            json!({ "message": "funasr task-started" }),
                        );
                    }
                    FunAsrEvent::Partial(text) => {
                        emit_asr_stream_event(
                            &app_handle,
                            &task_session_id,
                            "result",
                            json!({ "text": text, "final": false }),
                        );
                    }
                    FunAsrEvent::Final(text) => {
                        emit_asr_stream_event(
                            &app_handle,
                            &task_session_id,
                            "result",
                            json!({ "text": text, "final": true }),
                        );
                    }
                    FunAsrEvent::TaskFinished => {
                        emit_asr_stream_event(&app_handle, &task_session_id, "finish", json!({}));
                        break;
                    }
                    FunAsrEvent::TaskFailed { code, message } => {
                        emit_asr_stream_event(
                            &app_handle,
                            &task_session_id,
                            "error",
                            json!({ "code": code, "message": message }),
                        );
                        break;
                    }
                    FunAsrEvent::Other(value) => {
                        emit_asr_stream_event(&app_handle, &task_session_id, "event", value);
                    }
                },
                Ok(Message::Close(frame)) => {
                    emit_asr_stream_event(
                        &app_handle,
                        &task_session_id,
                        "closed",
                        json!({ "frame": frame.map(|v| format!("{v:?}")) }),
                    );
                    break;
                }
                Ok(_) => {}
                Err(err) => {
                    emit_asr_stream_event(
                        &app_handle,
                        &task_session_id,
                        "error",
                        json!({ "message": err.to_string() }),
                    );
                    break;
                }
            }
        }
        if let Ok(mut guard) = streams.lock() {
            guard.remove(&task_session_id);
        }
        emit_asr_stream_event(
            &app_handle,
            &task_session_id,
            "ended",
            json!({ "message": "asr stream ended" }),
        );
    });

    Ok(AsrStreamStartResponse { session_id })
}

async fn send_or_queue_audio(
    writer: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    protocol: &mut FunAsrProtocol,
    bytes: Vec<u8>,
    app: &tauri::AppHandle,
    session_id: &str,
    audio_chunks: &mut u64,
    audio_bytes: &mut u64,
) -> Result<(), ()> {
    if !protocol.started {
        protocol.pending.push(bytes);
        return Ok(());
    }
    let n = bytes.len();
    if let Err(err) = writer.send(Message::Binary(bytes.into())).await {
        emit_asr_stream_event(
            app,
            session_id,
            "error",
            json!({ "message": format!("发送音频失败: {err}"), "stage": "send_audio" }),
        );
        return Err(());
    }
    *audio_chunks += 1;
    *audio_bytes += n as u64;
    Ok(())
}

#[tauri::command]
pub(crate) fn asr_stream_push_chunk(
    session_id: String,
    audio_base64: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let bytes = STANDARD
        .decode(audio_base64.trim())
        .map_err(|e| format!("invalid base64 audio chunk: {e}"))?;
    if bytes.is_empty() {
        return Ok(());
    }
    let tx = {
        let guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard
            .get(&session_id)
            .ok_or_else(|| "ASR stream not found".to_string())?
            .tx
            .clone()
    };
    tx.send(AsrStreamInput::Audio(bytes))
        .map_err(|_| "ASR stream channel closed".to_string())
}

#[tauri::command]
pub(crate) fn asr_stream_push_f32_chunk(
    session_id: String,
    audio_base64: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let samples = decode_f32_base64(&audio_base64)?;
    if samples.is_empty() {
        return Ok(());
    }
    let tx = {
        let guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard
            .get(&session_id)
            .ok_or_else(|| "ASR stream not found".to_string())?
            .tx
            .clone()
    };
    tx.send(AsrStreamInput::RawF32(samples))
        .map_err(|_| "ASR stream channel closed".to_string())
}

#[tauri::command]
pub(crate) fn asr_stream_finish(
    session_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let tx = {
        let guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard
            .get(&session_id)
            .ok_or_else(|| "ASR stream not found".to_string())?
            .tx
            .clone()
    };
    tx.send(AsrStreamInput::Finish)
        .map_err(|_| "ASR stream channel closed".to_string())
}

#[tauri::command]
pub(crate) fn stop_asr_stream(
    session_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let handle = {
        let mut guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard.remove(&session_id)
    };
    if let Some(handle) = handle {
        let _ = handle.tx.send(AsrStreamInput::Stop);
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn run_asr_silence_test(
    state: tauri::State<'_, RuntimeState>,
    provider_id: Option<String>,
) -> Result<AsrResponse, String> {
    let provider_id = resolve_provider_id(&state, "asr", provider_id)?;
    let settings = read_provider_settings(&state)?;
    let profile = find_profile(&settings, &provider_id)
        .cloned()
        .ok_or_else(|| format!("供应商 {provider_id} 不存在"))?;
    run_funasr_silence_test(&profile).await
}

async fn run_funasr_silence_test(profile: &ProviderProfile) -> Result<AsrResponse, String> {
    let funasr_params: FunAsrParams =
        serde_json::from_value(profile.config.clone()).map_err(|e| e.to_string())?;
    if funasr_params.api_key.trim().is_empty() {
        return Err("请先在设置中填写 Fun-ASR 的 API Key".to_string());
    }
    let req = funasr_ws_request(&funasr_params.api_key)?;
    let (ws_stream, _) = connect_async(req).await.map_err(|e| e.to_string())?;
    let (mut writer, mut reader) = ws_stream.split();

    let task_id = Uuid::new_v4().to_string();
    writer
        .send(build_run_task_message(&task_id, &funasr_params))
        .await
        .map_err(|e| e.to_string())?;

    let mut events: Vec<Value> = Vec::new();
    let mut partials: Vec<String> = Vec::new();
    let mut final_text = String::new();
    let mut started = false;
    let silence = vec![0_u8; 8192];

    loop {
        let next = tokio::time::timeout(Duration::from_secs(20), reader.next())
            .await
            .map_err(|_| "ASR 等待超时".to_string())?;
        let Some(message) = next else { break };
        let message = message.map_err(|e| e.to_string())?;
        let Message::Text(text) = message else { continue };
        match parse_funasr_message(&text) {
            FunAsrEvent::Started => {
                started = true;
                for chunk in silence.chunks(4096) {
                    writer
                        .send(Message::Binary(chunk.to_vec().into()))
                        .await
                        .map_err(|e| e.to_string())?;
                    sleep(Duration::from_millis(40)).await;
                }
                writer
                    .send(build_finish_task_message(&task_id))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            FunAsrEvent::Partial(text) => {
                if !text.is_empty() {
                    partials.push(text);
                }
            }
            FunAsrEvent::Final(text) => {
                if !text.is_empty() {
                    final_text = text.clone();
                    partials.push(text);
                }
            }
            FunAsrEvent::TaskFinished if started => break,
            FunAsrEvent::TaskFinished => break,
            FunAsrEvent::TaskFailed { code, message } => {
                return Err(format!("Fun-ASR 上游错误 [{code}]: {message}"));
            }
            other => events.push(funasr_event_to_value(other)),
        }
    }

    partials.sort();
    partials.dedup();
    Ok(AsrResponse {
        text: final_text,
        partials,
        events,
    })
}

fn funasr_event_to_value(event: FunAsrEvent) -> Value {
    match event {
        FunAsrEvent::Started => json!({ "event": "task-started" }),
        FunAsrEvent::Partial(text) => {
            json!({ "event": "result-generated", "text": text, "final": false })
        }
        FunAsrEvent::Final(text) => {
            json!({ "event": "result-generated", "text": text, "final": true })
        }
        FunAsrEvent::TaskFinished => json!({ "event": "task-finished" }),
        FunAsrEvent::TaskFailed { code, message } => {
            json!({ "event": "task-failed", "code": code, "message": message })
        }
        FunAsrEvent::Other(value) => value,
    }
}

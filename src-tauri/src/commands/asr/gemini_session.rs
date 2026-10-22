use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::providers::gemini_live::{self, GeminiEvent, TurnTranscripts};
use crate::providers::ProviderProfile;
use crate::state::*;

const SETUP_TIMEOUT: Duration = Duration::from_secs(20);
const FINISH_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) async fn start_gemini_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
    profile: ProviderProfile,
    model: String,
    input_sample_rate: u32,
    params: Option<DspParams>,
    target_lang: Option<String>,
) -> Result<AsrStreamStartResponse, String> {
    let api_key = profile
        .config
        .get("apiKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if api_key.is_empty() {
        return Err("请先在「设置 → 密钥与识别 → Google Gemini」中填写 API Key".into());
    }
    // 目标语言在建连前先校验：不支持的语种直接失败，不发起无效连接。
    let target_code = gemini_live::map_target_language(target_lang.as_deref().unwrap_or("en"))?;

    let session_id = Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
    state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .insert(session_id.clone(), AsrStreamHandle { tx });

    let streams = state.asr_streams.clone();
    tauri::async_runtime::spawn(run_gemini_session(
        app,
        session_id.clone(),
        streams,
        rx,
        super::stream_dsp(params, input_sample_rate),
        model,
        api_key,
        target_code,
    ));
    Ok(AsrStreamStartResponse { session_id })
}

/// 事件处理后的流向：继续、正常收尾、或以某个终局事件结束会话。
enum Step {
    Continue,
    End,
    Terminal(&'static str, Value),
}

#[allow(clippy::too_many_arguments)]
async fn run_gemini_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut dsp: StreamDsp,
    model: String,
    api_key: String,
    target_code: String,
) {
    let redact = |text: String| text.replace(&api_key, "***");
    let fail = |message: String| {
        emit_asr_stream_event(&app, &session_id, "error", json!({ "message": message }));
    };

    let (ws, _) = match tokio_tungstenite::connect_async(gemini_live::endpoint_url(&api_key)).await
    {
        Ok(connected) => connected,
        Err(error) => {
            fail(redact(format!("连接 Gemini Live 失败：{error}")));
            cleanup_stream(&streams, &session_id);
            return;
        }
    };
    let (mut sink, mut stream) = ws.split();

    if let Err(error) = sink
        .send(Message::Text(
            gemini_live::setup_message(&target_code).to_string().into(),
        ))
        .await
    {
        fail(redact(format!("发送 Gemini Live 会话配置失败：{error}")));
        cleanup_stream(&streams, &session_id);
        return;
    }
    let setup_wait = async {
        while let Some(message) = stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    for event in decode_text(&text.to_string()) {
                        match event {
                            GeminiEvent::SetupComplete => return Ok(()),
                            GeminiEvent::ProviderError { code, message } => {
                                return Err(format!(
                                    "Gemini Live 拒绝会话配置（{code}）：{message}"
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Message::Binary(bytes)) => {
                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                        for event in decode_text(&text) {
                            if let GeminiEvent::SetupComplete = event {
                                return Ok(());
                            }
                        }
                    }
                }
                Ok(Message::Close(_)) => return Err("Gemini Live 在会话建立前关闭了连接".into()),
                Ok(_) => {}
                Err(error) => return Err(format!("等待 Gemini Live 会话建立时连接中断：{error}")),
            }
        }
        Err("Gemini Live 在会话建立前关闭了连接".to_string())
    };
    match tokio::time::timeout(SETUP_TIMEOUT, setup_wait).await {
        Ok(Ok(())) => {}
        Ok(Err(message)) => {
            fail(redact(message));
            cleanup_stream(&streams, &session_id);
            return;
        }
        Err(_) => {
            fail("Gemini Live 会话建立超时".into());
            cleanup_stream(&streams, &session_id);
            return;
        }
    }
    emit_asr_stream_event(
        &app,
        &session_id,
        "opened",
        json!({ "message": "Gemini Live Translate opened", "model": model }),
    );

    let mut frame_buf: Vec<u8> = Vec::new();
    let mut turn = TurnTranscripts::default();
    let mut last_source_emitted = String::new();
    let mut finishing_at: Option<Instant> = None;
    // 终局事件（error/closed）在清理后统一发出；为空表示正常结束（ended）。
    let mut terminal: Option<(&'static str, Value)> = None;

    'session: loop {
        if finishing_at.is_some_and(|deadline| Instant::now() >= deadline) {
            finalize_turn(&app, &session_id, &mut turn, &mut last_source_emitted);
            break 'session;
        }
        tokio::select! {
            input = rx.recv() => match input {
                Some(AsrStreamInput::RawF32(samples)) => {
                    if finishing_at.is_none() {
                        let pcm = dsp.process(&samples);
                        frame_buf.extend_from_slice(&pcm);
                        while frame_buf.len() >= gemini_live::AUDIO_FRAME_BYTES {
                            let frame: Vec<u8> = frame_buf
                                .drain(..gemini_live::AUDIO_FRAME_BYTES)
                                .collect();
                            if let Err(error) = sink
                                .send(Message::Text(
                                    gemini_live::audio_message(&frame).to_string().into(),
                                ))
                                .await
                            {
                                terminal = Some(("error", json!({
                                    "message": redact(format!("发送音频到 Gemini Live 失败：{error}"))
                                })));
                                break 'session;
                            }
                        }
                    }
                }
                Some(AsrStreamInput::Finish) => {
                    if finishing_at.is_none() {
                        if !frame_buf.is_empty() {
                            let mut frame = std::mem::take(&mut frame_buf);
                            frame.resize(gemini_live::AUDIO_FRAME_BYTES, 0);
                            let _ = sink
                                .send(Message::Text(
                                    gemini_live::audio_message(&frame).to_string().into(),
                                ))
                                .await;
                        }
                        let _ = sink
                            .send(Message::Text(
                                gemini_live::audio_stream_end_message().to_string().into(),
                            ))
                            .await;
                        finishing_at = Some(Instant::now() + FINISH_TIMEOUT);
                    }
                }
                Some(AsrStreamInput::Stop) | None => break 'session,
            },
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    match handle_events(
                        &app,
                        &session_id,
                        decode_text(&text.to_string()),
                        &mut turn,
                        &mut last_source_emitted,
                        finishing_at.is_some(),
                        &redact,
                    ) {
                        Step::Continue => {}
                        Step::End => break 'session,
                        Step::Terminal(kind, payload) => {
                            terminal = Some((kind, payload));
                            break 'session;
                        }
                    }
                }
                Some(Ok(Message::Binary(bytes))) => {
                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                        match handle_events(
                            &app,
                            &session_id,
                            decode_text(&text),
                            &mut turn,
                            &mut last_source_emitted,
                            finishing_at.is_some(),
                            &redact,
                        ) {
                            Step::Continue => {}
                            Step::End => break 'session,
                            Step::Terminal(kind, payload) => {
                                terminal = Some((kind, payload));
                                break 'session;
                            }
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => {
                    if finishing_at.is_none() {
                        terminal = Some(("closed", json!({
                            "message": "Gemini Live 连接已断开"
                        })));
                    }
                    break 'session;
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    if finishing_at.is_none() {
                        terminal = Some(("error", json!({
                            "message": redact(format!("Gemini Live 连接中断：{error}"))
                        })));
                    }
                    break 'session;
                }
            },
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    let _ = sink.close().await;
    cleanup_stream(&streams, &session_id);
    match terminal {
        Some((kind, payload)) => emit_asr_stream_event(&app, &session_id, kind, payload),
        None => emit_asr_stream_event(
            &app,
            &session_id,
            "ended",
            json!({ "message": "Gemini Live Translate ended" }),
        ),
    }
}

fn handle_events(
    app: &tauri::AppHandle,
    session_id: &str,
    events: Vec<GeminiEvent>,
    turn: &mut TurnTranscripts,
    last_source_emitted: &mut String,
    finishing: bool,
    redact: &impl Fn(String) -> String,
) -> Step {
    for event in events {
        match event {
            GeminiEvent::SourceTranscript { text } => {
                turn.push_source(&text);
                if turn.source != *last_source_emitted && !turn.source.is_empty() {
                    *last_source_emitted = turn.source.clone();
                    emit_asr_stream_event(
                        app,
                        session_id,
                        "result",
                        json!({ "text": turn.source, "final": false }),
                    );
                }
            }
            GeminiEvent::TranslationTranscript { text } => {
                turn.push_translation(&text);
            }
            GeminiEvent::GenerationComplete
            | GeminiEvent::TurnComplete
            | GeminiEvent::Interrupted => {
                finalize_turn(app, session_id, turn, last_source_emitted);
                if finishing {
                    return Step::End;
                }
            }
            GeminiEvent::GoAway => {
                finalize_turn(app, session_id, turn, last_source_emitted);
                return Step::Terminal(
                    "closed",
                    json!({ "message": "Gemini Live 会话被服务端结束（goAway）" }),
                );
            }
            GeminiEvent::ProviderError { code, message } => {
                return Step::Terminal(
                    "error",
                    json!({
                        "message": redact(format!("Gemini Live 返回错误（{code}）：{message}"))
                    }),
                );
            }
            GeminiEvent::SetupComplete => {}
        }
    }
    Step::Continue
}

/// 提交一轮：先发原文定稿，再发译文定稿——字幕编排按这个顺序把译文挂到
/// 刚定稿的原文分段上，顺序不可颠倒。
fn finalize_turn(
    app: &tauri::AppHandle,
    session_id: &str,
    turn: &mut TurnTranscripts,
    last_source_emitted: &mut String,
) {
    if turn.is_empty() {
        return;
    }
    let (source, translation) = turn.take();
    if !source.is_empty() {
        emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": source, "final": true }),
        );
    }
    if !translation.is_empty() {
        emit_asr_stream_event(
            app,
            session_id,
            "translation",
            json!({ "text": translation, "final": true }),
        );
    }
    last_source_emitted.clear();
}

fn decode_text(text: &str) -> Vec<GeminiEvent> {
    serde_json::from_str::<Value>(text)
        .map(|value| gemini_live::decode_message(&value))
        .unwrap_or_default()
}

fn cleanup_stream(streams: &Arc<Mutex<HashMap<String, AsrStreamHandle>>>, session_id: &str) {
    if let Ok(mut guard) = streams.lock() {
        guard.remove(session_id);
    }
}

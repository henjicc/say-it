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
/// `turnComplete` 之后转写尾巴仍可能继续到达，边界只能是临时的，等转写安静
/// 这么久再定稿。
const TURN_TAIL_QUIET: Duration = Duration::from_millis(500);
/// Gemini 的连续翻译模型可以整段不发 `turnComplete`：原文与译文都已有内容且
/// 安静这么久，就按本地检查点把这一对定稿，否则译文永远停留在草稿里。
const PAIRED_QUIET: Duration = Duration::from_millis(2_000);
/// 只有一路有内容（通常译文尚未开始）时再多等一会儿，避免把一句原文和它
/// 随后才到的译文拆到两轮；超过后仍按单路定稿，字幕不能无限悬挂。
const UNPAIRED_QUIET: Duration = Duration::from_millis(5_000);
const LOOP_TICK: Duration = Duration::from_millis(100);

/// 把 Gemini 两路转写编排成字幕层需要的事件序列：
/// - 原文/译文每次变化都以 `final: false` 发出累计草稿；
/// - 定稿时先发原文 `result(final)`，紧接着发译文 `translation(final)`，
///   字幕层按这个顺序把译文挂到刚定稿的原文分段上，顺序不可颠倒。
#[derive(Default)]
struct TurnAssembler {
    turn: TurnTranscripts,
    last_delta_at: Option<Instant>,
    /// 本轮收到过 `turnComplete`/`interrupted` 显式边界，尾巴安静后即定稿。
    explicit_boundary: bool,
    /// 自上次复位以来是否出现过服务端边界；收尾阶段用它判断服务端已交付完毕。
    boundary_seen: bool,
}

impl TurnAssembler {
    fn on_source(&mut self, app: &tauri::AppHandle, session_id: &str, delta: &str, now: Instant) {
        if !self.turn.push_source(delta) {
            return;
        }
        self.last_delta_at = Some(now);
        emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": self.turn.source(), "final": false }),
        );
    }

    fn on_translation(
        &mut self,
        app: &tauri::AppHandle,
        session_id: &str,
        delta: &str,
        now: Instant,
    ) {
        if !self.turn.push_translation(delta) {
            return;
        }
        self.last_delta_at = Some(now);
        emit_asr_stream_event(
            app,
            session_id,
            "translation",
            json!({ "text": self.turn.translation(), "final": false }),
        );
    }

    fn on_boundary(&mut self, now: Instant) {
        self.boundary_seen = true;
        if self.turn.is_empty() {
            return;
        }
        self.explicit_boundary = true;
        self.last_delta_at = Some(now);
    }

    fn is_empty(&self) -> bool {
        self.turn.is_empty()
    }

    fn quiet_for(&self, now: Instant) -> Option<Duration> {
        self.last_delta_at
            .map(|at| now.saturating_duration_since(at))
    }

    /// 周期检查是否到达定稿时机。
    fn poll(&mut self, app: &tauri::AppHandle, session_id: &str, now: Instant) {
        if self.turn.is_empty() {
            return;
        }
        let Some(quiet) = self.quiet_for(now) else {
            return;
        };
        let due = if self.explicit_boundary {
            quiet >= TURN_TAIL_QUIET
        } else if self.turn.is_paired() {
            quiet >= PAIRED_QUIET
        } else {
            quiet >= UNPAIRED_QUIET
        };
        if due {
            self.finalize(app, session_id);
        }
    }

    fn finalize(&mut self, app: &tauri::AppHandle, session_id: &str) {
        self.explicit_boundary = false;
        self.last_delta_at = None;
        if self.turn.is_empty() {
            return;
        }
        let (source, translation) = self.turn.take();
        if !source.trim().is_empty() {
            emit_asr_stream_event(
                app,
                session_id,
                "result",
                json!({ "text": source, "final": true }),
            );
        }
        if !translation.trim().is_empty() {
            emit_asr_stream_event(
                app,
                session_id,
                "translation",
                json!({ "text": translation, "final": true }),
            );
        }
    }
}

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

/// 事件处理后的流向：继续，或以某个终局事件结束会话。
enum Step {
    Continue,
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
    let mut turn = TurnAssembler::default();
    // 收到 Finish 的时刻；之后最多再等 FINISH_TIMEOUT 收尾。
    let mut finishing_at: Option<Instant> = None;
    // 终局事件（error/closed）在清理后统一发出；为空表示正常结束（ended）。
    let mut terminal: Option<(&'static str, Value)> = None;

    'session: loop {
        let now = Instant::now();
        turn.poll(&app, &session_id, now);
        if let Some(finish_at) = finishing_at {
            // 连续模型在 audioStreamEnd 之后也可能不发 turnComplete：已无待定稿
            // 内容，且（收到过服务端边界，或安静够久）就正常收尾，否则到预算上限
            // 强制定稿。
            let drained = turn.is_empty()
                && (turn.boundary_seen || now.saturating_duration_since(finish_at) >= PAIRED_QUIET);
            if drained || now >= finish_at + FINISH_TIMEOUT {
                turn.finalize(&app, &session_id);
                break 'session;
            }
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
                        finishing_at = Some(Instant::now());
                        turn.boundary_seen = false;
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
                        &redact,
                    ) {
                        Step::Continue => {}
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
                            &redact,
                        ) {
                            Step::Continue => {}
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
            _ = tokio::time::sleep(LOOP_TICK) => {}
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
    turn: &mut TurnAssembler,
    redact: &impl Fn(String) -> String,
) -> Step {
    let now = Instant::now();
    for event in events {
        match event {
            GeminiEvent::SourceTranscript { text } => turn.on_source(app, session_id, &text, now),
            GeminiEvent::TranslationTranscript { text } => {
                turn.on_translation(app, session_id, &text, now)
            }
            // generationComplete 只表示模型本次生成结束，译文转写可能还在路上，
            // 不是边界；turnComplete/interrupted 也只是临时边界，等尾巴安静再定稿。
            GeminiEvent::GenerationComplete => {}
            GeminiEvent::TurnComplete | GeminiEvent::Interrupted => turn.on_boundary(now),
            GeminiEvent::GoAway => {
                turn.finalize(app, session_id);
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

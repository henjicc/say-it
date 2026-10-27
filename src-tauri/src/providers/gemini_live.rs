//! Google Gemini Live Translate 内置供应商的线协议。
//!
//! 与 Mimi（yuxino/Mimi）中已对真实端点验证过的实现保持同一线形状：
//! WebSocket 连接 `BidiGenerateContent`，setup 时把转写选项放在 setup 顶层
//! （放进 generationConfig 会被端点以 1007 拒绝），generationConfig 只带
//! `responseModalities: ["AUDIO"]` 与 `translationConfig`。服务端返回的
//! 译文音频本体不消费，只取 input/output 两路转写文本：前者是识别原文，
//! 后者是译文。转写文本是增量片段，由会话层按轮次累积成累计文本。

use base64::Engine;
use serde_json::{json, Value};

pub const PROVIDER_ID: &str = "google-gemini";
pub const PROVIDER_KIND: &str = "gemini-live";
pub const PROTOCOL: &str = "gemini-live-translate";
pub const MODEL_ID: &str = "gemini-3.5-live-translate-preview";

pub const ENDPOINT: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
pub const SAMPLE_RATE_HZ: u32 = 16_000;
/// 100ms、16kHz、单声道、s16le 音频帧的字节数，与已验证实现的发送节奏一致。
pub const AUDIO_FRAME_BYTES: usize = 3_200;
pub const AUDIO_MIME_TYPE: &str = "audio/pcm;rate=16000";

/// 构造带 API Key 的连接地址。Key 只出现在本次连接 URL 中，不落盘、不进日志。
pub fn endpoint_url(api_key: &str) -> String {
    let mut url = url::Url::parse(ENDPOINT).expect("Gemini Live endpoint must parse");
    url.query_pairs_mut().append_pair("key", api_key);
    url.to_string()
}

pub fn setup_message(target_language_code: &str) -> Value {
    json!({
        "setup": {
            "model": format!("models/{MODEL_ID}"),
            "inputAudioTranscription": {},
            "outputAudioTranscription": {},
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "translationConfig": {
                    "targetLanguageCode": target_language_code,
                    "echoTargetLanguage": true
                }
            }
        }
    })
}

pub fn audio_message(frame: &[u8]) -> Value {
    json!({
        "realtimeInput": {
            "audio": {
                "data": base64::engine::general_purpose::STANDARD.encode(frame),
                "mimeType": AUDIO_MIME_TYPE
            }
        }
    })
}

pub fn audio_stream_end_message() -> Value {
    json!({
        "realtimeInput": {
            "audioStreamEnd": true
        }
    })
}

/// 把应用内的目标语言代码映射为 Gemini Live Translate 的线代码。
/// 语言集合以官方 live-translate 支持列表为准（与 Mimi 的已验证目录一致）；
/// 不支持的语种在会话开始前明确报错，不静默降级。
pub fn map_target_language(raw: &str) -> Result<String, String> {
    let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
    let mapped = match normalized.as_str() {
        "zh" | "zh_cn" | "zh_hans" | "cmn" => "zh-Hans",
        "zh_tw" | "zh_hk" | "zh_mo" | "zh_hant" => "zh-Hant",
        "tl" => "fil",
        "pt" => "pt-BR",
        "pt_br" => "pt-BR",
        "pt_pt" => "pt-PT",
        "yue" => {
            return Err(
                "Gemini Live Translate 暂不支持粤语作为目标语言，请改选其他目标语言".to_string(),
            )
        }
        _ => {
            const WIRE_CODES: &[&str] = &[
                "en", "ja", "ko", "ru", "es", "fr", "de", "it", "th", "vi", "id", "ms", "ar", "hi",
                "he", "ur", "bn", "pl", "nl", "tr", "km", "cs", "sv", "hu", "da", "fi", "fa", "af",
                "ak", "sq", "am", "hy", "az", "eu", "be", "bg", "my", "ca", "hr", "et", "gl", "ka",
                "el", "gu", "ha", "is", "jv", "kn", "kk", "rw", "lo", "lv", "lt", "mk", "ml", "mr",
                "mn", "ne", "no", "pa", "ro", "sr", "sd", "si", "sk", "sl", "su", "sw", "ta", "te",
                "uk", "uz", "zu", "fil",
            ];
            if WIRE_CODES.contains(&normalized.as_str()) {
                return Ok(normalized);
            }
            return Err(format!(
                "Gemini Live Translate 不支持目标语言「{raw}」，请改选其他目标语言"
            ));
        }
    };
    Ok(mapped.to_string())
}

/// 解码后的服务端事件。一条服务端报文可能同时携带原文、译文与轮次边界，
/// 解码返回全部事件并保持线序，轮次边界恒在最后。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeminiEvent {
    SetupComplete,
    SourceTranscript { text: String },
    TranslationTranscript { text: String },
    GenerationComplete,
    TurnComplete,
    Interrupted,
    GoAway,
    ProviderError { code: String, message: String },
}

pub fn decode_message(value: &Value) -> Vec<GeminiEvent> {
    if value.get("setupComplete").is_some() {
        return vec![GeminiEvent::SetupComplete];
    }
    if let Some(error) = value.get("error") {
        let status = error.get("status").and_then(Value::as_str).unwrap_or("");
        let code = if status.is_empty() {
            error
                .get("code")
                .and_then(Value::as_i64)
                .map(|code| format!("http_{code}"))
                .unwrap_or_else(|| "provider_error".to_string())
        } else {
            status.to_string()
        };
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(300)
            .collect();
        return vec![GeminiEvent::ProviderError { code, message }];
    }
    if value.get("goAway").is_some() {
        return vec![GeminiEvent::GoAway];
    }
    let Some(content) = value.get("serverContent") else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for (field, is_source) in [("inputTranscription", true), ("outputTranscription", false)] {
        if let Some(transcription) = content.get(field) {
            // Protobuf JSON 会省略空字符串；只有 languageCode 的更新是合法的静音期报文。
            let text = transcription
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if !text.is_empty() {
                events.push(if is_source {
                    GeminiEvent::SourceTranscript { text }
                } else {
                    GeminiEvent::TranslationTranscript { text }
                });
            }
        }
    }
    if content
        .get("generationComplete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        events.push(GeminiEvent::GenerationComplete);
    }
    if content
        .get("interrupted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        events.push(GeminiEvent::Interrupted);
    }
    // 边界恒在最后：同一报文里的转写必须先被累积，再提交这一轮。
    if content
        .get("turnComplete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        events.push(GeminiEvent::TurnComplete);
    }
    events
}

/// 单路转写的累积缓冲。Gemini 既可能发增量片段，也可能发整段累计快照
/// （严格扩展当前缓冲的报文），两种形状在同一会话里都出现过，必须按内容
/// 判别而不是假定其一；否则快照会被当片段重复拼接，译文/原文整段重复。
#[derive(Default, Debug)]
struct TranscriptLane {
    text: String,
    /// 本地已取走的累计前缀：本地检查点不会结束供应商的累计流，后续快照
    /// 仍带着这段前缀，需要剥掉才能得到本轮新增文本。
    checkpoint_prefix: String,
    cumulative_snapshots: bool,
}

impl TranscriptLane {
    /// 合并一条报文；返回缓冲是否发生变化。
    fn merge(&mut self, chunk: &str) -> bool {
        let continuation = (!self.checkpoint_prefix.is_empty())
            .then(|| chunk.strip_prefix(self.checkpoint_prefix.as_str()))
            .flatten();
        let chunk = continuation.unwrap_or(chunk);
        if chunk.is_empty() {
            return false;
        }
        if !self.text.is_empty() && chunk.len() > self.text.len() && chunk.starts_with(&self.text) {
            if continuation.is_none() {
                self.checkpoint_prefix.clear();
            }
            self.text.clear();
            self.text.push_str(chunk);
            self.cumulative_snapshots = true;
        } else if self.cumulative_snapshots && chunk == self.text {
            return false;
        } else {
            self.text.push_str(chunk);
        }
        true
    }

    fn take(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.checkpoint_prefix.push_str(&text);
        text
    }
}

/// 一轮对话的双路转写累积：宿主事件需要的是本轮累计文本。
#[derive(Default, Debug)]
pub struct TurnTranscripts {
    source: TranscriptLane,
    translation: TranscriptLane,
}

impl TurnTranscripts {
    /// 返回原文缓冲是否发生变化。
    pub fn push_source(&mut self, delta: &str) -> bool {
        self.source.merge(delta)
    }

    /// 返回译文缓冲是否发生变化。
    pub fn push_translation(&mut self, delta: &str) -> bool {
        self.translation.merge(delta)
    }

    pub fn source(&self) -> &str {
        &self.source.text
    }

    pub fn translation(&self) -> &str {
        &self.translation.text
    }

    pub fn is_empty(&self) -> bool {
        self.source.text.is_empty() && self.translation.text.is_empty()
    }

    /// 两路都已有实质内容（不只是空白），可以作为一对定稿。
    pub fn is_paired(&self) -> bool {
        !self.source.text.trim().is_empty() && !self.translation.text.trim().is_empty()
    }

    /// 取走本轮累计文本并复位。返回 (原文, 译文)。
    pub fn take(&mut self) -> (String, String) {
        (self.source.take(), self.translation.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_message_matches_verified_wire_shape() {
        let setup = setup_message("zh-Hans");
        assert_eq!(
            setup["setup"]["model"],
            "models/gemini-3.5-live-translate-preview"
        );
        // 转写选项必须在 setup 顶层；放进 generationConfig 会被端点拒绝。
        assert_eq!(setup["setup"]["inputAudioTranscription"], json!({}));
        assert_eq!(setup["setup"]["outputAudioTranscription"], json!({}));
        assert_eq!(
            setup["setup"]["generationConfig"]["responseModalities"],
            json!(["AUDIO"])
        );
        assert_eq!(
            setup["setup"]["generationConfig"]["translationConfig"]["targetLanguageCode"],
            "zh-Hans"
        );
        assert_eq!(
            setup["setup"]["generationConfig"]["translationConfig"]["echoTargetLanguage"],
            true
        );
    }

    #[test]
    fn target_language_mapping() {
        assert_eq!(map_target_language("zh").unwrap(), "zh-Hans");
        assert_eq!(map_target_language("zh_tw").unwrap(), "zh-Hant");
        assert_eq!(map_target_language("zh-TW").unwrap(), "zh-Hant");
        assert_eq!(map_target_language("en").unwrap(), "en");
        assert_eq!(map_target_language("pt").unwrap(), "pt-BR");
        assert!(map_target_language("yue").is_err());
        assert!(map_target_language("xx").is_err());
    }

    #[test]
    fn decode_combined_envelope_keeps_wire_order_with_boundary_last() {
        let value = json!({
            "serverContent": {
                "inputTranscription": {"text": "你好", "languageCode": "zh-CN"},
                "outputTranscription": {"text": "Hello", "languageCode": "en"},
                "turnComplete": true
            }
        });
        assert_eq!(
            decode_message(&value),
            vec![
                GeminiEvent::SourceTranscript {
                    text: "你好".into()
                },
                GeminiEvent::TranslationTranscript {
                    text: "Hello".into()
                },
                GeminiEvent::TurnComplete,
            ]
        );
    }

    #[test]
    fn decode_error_and_goaway() {
        let error =
            json!({"error": {"code": 400, "status": "INVALID_ARGUMENT", "message": "bad setup"}});
        assert_eq!(
            decode_message(&error),
            vec![GeminiEvent::ProviderError {
                code: "INVALID_ARGUMENT".into(),
                message: "bad setup".into()
            }]
        );
        assert_eq!(
            decode_message(&json!({"goAway": {"timeLeft": "30s"}})),
            vec![GeminiEvent::GoAway]
        );
        assert_eq!(
            decode_message(&json!({"setupComplete": {}})),
            vec![GeminiEvent::SetupComplete]
        );
    }

    #[test]
    fn decode_language_only_transcription_is_ignored() {
        let value = json!({
            "serverContent": {
                "inputTranscription": {"languageCode": "zh-CN"}
            }
        });
        assert!(decode_message(&value).is_empty());
    }

    #[test]
    fn turn_transcripts_accumulate_and_take() {
        let mut turn = TurnTranscripts::default();
        assert!(turn.push_source("你"));
        assert!(turn.push_source("好"));
        assert!(turn.push_translation("Hel"));
        assert!(turn.push_translation("lo"));
        assert!(turn.is_paired());
        assert_eq!(turn.take(), ("你好".to_string(), "Hello".to_string()));
        assert!(turn.is_empty());
    }

    #[test]
    fn cumulative_snapshots_replace_instead_of_append() {
        let mut turn = TurnTranscripts::default();
        assert!(turn.push_source("こんにちは"));
        assert!(turn.push_source("こんにちは、世界"));
        assert_eq!(turn.source(), "こんにちは、世界");
        // 与当前缓冲完全相同的快照是重发，不视为变化。
        assert!(!turn.push_source("こんにちは、世界"));
        // 快照模式下的新片段仍按原样追加，不按尾部匹配去重。
        assert!(turn.push_source("。"));
        assert_eq!(turn.source(), "こんにちは、世界。");
    }

    #[test]
    fn checkpoint_strips_already_taken_prefix_from_later_snapshots() {
        let mut turn = TurnTranscripts::default();
        turn.push_translation("你好");
        turn.push_translation("你好，世界");
        assert_eq!(turn.take().1, "你好，世界");
        assert!(turn.push_translation("你好，世界。再见"));
        assert_eq!(turn.translation(), "。再见");
        assert!(turn.push_translation("你好，世界。再见了"));
        assert_eq!(turn.translation(), "。再见了");
    }

    #[test]
    fn whitespace_only_lanes_are_not_paired() {
        let mut turn = TurnTranscripts::default();
        turn.push_source("原文");
        turn.push_translation(" ");
        assert!(!turn.is_paired());
        assert!(!turn.is_empty());
    }
}

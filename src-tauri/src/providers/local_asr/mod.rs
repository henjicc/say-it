use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::cancellation::CancellationFlag;

use serde_json::Value;
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig, OnlineRecognizer,
    OnlineRecognizerConfig, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};

use super::model_download;
use super::plugin::{LocalModelSpec, ModelPackManifest};

const SAMPLE_RATE: i32 = 16_000;

pub mod cache;
mod file;
pub(crate) use file::recognize_audio_file;

#[derive(Default)]
pub struct LocalAsrOutput {
    pub partial: Option<String>,
    pub finals: Vec<String>,
}

/// 一个 VAD 句段的识别结果与它在整段音频里的位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalSegment {
    pub text: String,
    pub begin_ms: u64,
    pub end_ms: u64,
}

pub struct OnlineSession {
    // 流先析构，最后一个会话释放时识别器仍然有效。
    stream: sherpa_onnx::OnlineStream,
    recognizer: Arc<Mutex<OnlineRecognizer>>,
    last_partial: String,
}

impl OnlineSession {
    pub fn create_realtime(
        spec: &LocalModelSpec,
        cancel: Option<&CancellationFlag>,
    ) -> Result<Self, String> {
        let shared = cache::acquire(spec)?;
        let cache::Recognizer::Online(recognizer) = shared.as_ref() else {
            return Err("模型引擎不是在线 ASR".into());
        };
        let recognizer = recognizer.clone();
        let stream = lock_recognizer(&recognizer, cancel)?.create_stream();
        Ok(Self {
            recognizer,
            stream,
            last_partial: String::new(),
        })
    }

    pub fn accept(
        &mut self,
        samples: &[f32],
        cancel: Option<&CancellationFlag>,
    ) -> Result<LocalAsrOutput, String> {
        self.stream.accept_waveform(SAMPLE_RATE, samples);
        self.decode_ready(cancel)
    }

    pub fn finish(mut self, cancel: Option<&CancellationFlag>) -> Result<LocalAsrOutput, String> {
        self.stream.input_finished();
        let mut output = self.decode_ready(cancel)?;
        let recognizer = lock_recognizer(&self.recognizer, cancel)?;
        if let Some(result) = recognizer.get_result(&self.stream) {
            let text = result.text.trim();
            if !text.is_empty() && output.finals.last().is_none_or(|last| last != text) {
                output.finals.push(text.to_string());
            }
        }
        output.partial = None;
        Ok(output)
    }

    fn decode_ready(
        &mut self,
        cancel: Option<&CancellationFlag>,
    ) -> Result<LocalAsrOutput, String> {
        // sherpa 类型声明了 Send/Sync，但未承诺共享识别器并发 decode；每步单独串行化。
        loop {
            let recognizer = lock_recognizer(&self.recognizer, cancel)?;
            if !recognizer.is_ready(&self.stream) {
                break;
            }
            recognizer.decode(&self.stream);
        }
        let recognizer = lock_recognizer(&self.recognizer, cancel)?;
        let mut output = LocalAsrOutput::default();
        let result = recognizer.get_result(&self.stream);
        if recognizer.is_endpoint(&self.stream) {
            if let Some(result) = result {
                let text = result.text.trim();
                if !text.is_empty() {
                    output.finals.push(text.to_string());
                }
            }
            recognizer.reset(&self.stream);
            self.last_partial.clear();
        } else if let Some(result) = result {
            let text = result.text.trim();
            if !text.is_empty() && text != self.last_partial {
                self.last_partial = text.to_string();
                output.partial = Some(text.to_string());
            }
        }
        Ok(output)
    }
}

fn check_cancel(cancel: Option<&CancellationFlag>) -> Result<(), String> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        Err("本地语音识别已取消".into())
    } else {
        Ok(())
    }
}

fn lock_recognizer<'a, T>(
    recognizer: &'a Mutex<T>,
    cancel: Option<&CancellationFlag>,
) -> Result<MutexGuard<'a, T>, String> {
    let guard = recognizer.lock().map_err(|_| "本地识别器锁失败")?;
    // 等待另一个实时句段时，停止或超时可能已经取消了本次会话。
    check_cancel(cancel)?;
    Ok(guard)
}

fn create_online_recognizer(spec: &LocalModelSpec) -> Result<OnlineRecognizer, String> {
    ensure_ready(spec)?;
    let mut config = OnlineRecognizerConfig::default();
    config.model_config.paraformer.encoder = Some(param_path(spec, "encoder")?);
    config.model_config.paraformer.decoder = Some(param_path(spec, "decoder")?);
    config.model_config.tokens = Some(param_path(spec, "tokens")?);
    config.model_config.num_threads = int_param(&spec.params, "numThreads", 2);
    config.model_config.provider = Some("cpu".into());
    config.decoding_method = Some(string_param(
        &spec.params,
        "decodingMethod",
        "greedy_search",
    ));
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = float_param(&spec.params, "rule1MinTrailingSilence", 2.4);
    config.rule2_min_trailing_silence = float_param(&spec.params, "rule2MinTrailingSilence", 1.2);
    config.rule3_min_utterance_length = float_param(&spec.params, "rule3MinUtteranceLength", 20.0);
    let recognizer = OnlineRecognizer::create(&config)
        .ok_or("创建 sherpa-onnx 在线识别器失败；请检查模型包文件与参数")?;
    Ok(recognizer)
}

pub struct OfflineEngine {
    recognizer: Arc<Mutex<OfflineRecognizer>>,
}

impl OfflineEngine {
    fn create_realtime(spec: &LocalModelSpec) -> Result<Self, String> {
        let shared = cache::acquire(spec)?;
        let cache::Recognizer::Offline(recognizer) = shared.as_ref() else {
            return Err("模型引擎不是离线 ASR".into());
        };
        let recognizer = recognizer.clone();
        Ok(Self { recognizer })
    }

    fn create_temporary(spec: &LocalModelSpec) -> Result<Self, String> {
        Ok(Self {
            recognizer: Arc::new(Mutex::new(create_offline_recognizer(spec)?)),
        })
    }

    pub fn recognize(
        &self,
        samples: &[f32],
        cancel: Option<&CancellationFlag>,
    ) -> Result<String, String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        // 只有实时会话共享识别器，锁只覆盖一个句段。
        let recognizer = lock_recognizer(&self.recognizer, cancel)?;
        let stream = recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE, samples);
        check_cancel(cancel)?;
        recognizer.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text.trim().to_string())
            .ok_or("离线模型未返回识别结果".into())
    }
}

fn create_offline_recognizer(spec: &LocalModelSpec) -> Result<OfflineRecognizer, String> {
    ensure_ready(spec)?;
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
        model: Some(param_path(spec, "model")?),
        language: Some(string_param(&spec.params, "language", "auto")),
        use_itn: bool_param(&spec.params, "useItn", true),
    };
    config.model_config.tokens = Some(param_path(spec, "tokens")?);
    config.model_config.num_threads = int_param(&spec.params, "numThreads", 2);
    config.model_config.provider = Some("cpu".into());
    let recognizer = OfflineRecognizer::create(&config)
        .ok_or("创建 sherpa-onnx 离线识别器失败；请检查模型包文件与参数")?;
    Ok(recognizer)
}

pub struct OfflineVadSession {
    engine: OfflineEngine,
    vad: VoiceActivityDetector,
    /// 每次喂给 VAD 的样本数，等于 Silero 的窗口大小。
    window_size: usize,
    /// 本轮 VAD（上次 reset 之后）已喂入的样本数。
    fed_samples: u64,
    /// 之前各轮 VAD 累计的样本数。`SpeechSegment::start` 只在本轮内计数，
    /// reset 会清零，因此句段的绝对位置必须由这里补齐。
    base_samples: u64,
}

impl OfflineVadSession {
    pub fn create_realtime(spec: &LocalModelSpec) -> Result<Self, String> {
        Self::with_engine(spec, OfflineEngine::create_realtime(spec)?)
    }

    pub fn create_temporary(spec: &LocalModelSpec) -> Result<Self, String> {
        Self::with_engine(spec, OfflineEngine::create_temporary(spec)?)
    }

    fn with_engine(spec: &LocalModelSpec, engine: OfflineEngine) -> Result<Self, String> {
        let window_size = int_param(&spec.params, "vadWindowSize", 512).max(1);
        let mut config = VadModelConfig::default();
        config.silero_vad = SileroVadModelConfig {
            model: Some(param_path(spec, "vadModel")?),
            threshold: float_param(&spec.params, "vadThreshold", 0.5),
            min_silence_duration: float_param(&spec.params, "vadMinSilenceDuration", 0.6),
            min_speech_duration: float_param(&spec.params, "vadMinSpeechDuration", 0.25),
            window_size,
            max_speech_duration: float_param(&spec.params, "vadMaxSpeechDuration", 30.0),
        };
        config.sample_rate = SAMPLE_RATE;
        config.num_threads = 1;
        config.provider = Some("cpu".into());
        let vad = VoiceActivityDetector::create(&config, 120.0)
            .ok_or("创建 Silero VAD 失败；请检查 vadModel 参数")?;
        Ok(Self {
            engine,
            vad,
            window_size: window_size as usize,
            fed_samples: 0,
            base_samples: 0,
        })
    }

    /// 调用方可以传入任意长度的音频；这里负责切成 VAD 能正确处理的窗口。
    pub fn accept(
        &mut self,
        samples: &[f32],
        cancel: Option<&CancellationFlag>,
    ) -> Result<Vec<LocalSegment>, String> {
        // 必须按窗口大小小块喂入：单次传入过长音频会让 sherpa VAD 在语音确认前
        // 裁掉缓冲里的开头。实测 5.6s 音频，≤0.3s 的块识别完整，0.5s 的块丢失开头
        // 数字，整段一次性传入只剩一个语气词。
        let mut results = Vec::new();
        for window in samples.chunks(self.window_size) {
            check_cancel(cancel)?;
            self.vad.accept_waveform(window);
            self.fed_samples += window.len() as u64;
            results.extend(self.drain(cancel)?);
        }
        Ok(results)
    }

    pub fn finish(
        mut self,
        cancel: Option<&CancellationFlag>,
    ) -> Result<Vec<LocalSegment>, String> {
        self.flush_and_reset(cancel)
    }

    pub fn flush_and_reset(
        &mut self,
        cancel: Option<&CancellationFlag>,
    ) -> Result<Vec<LocalSegment>, String> {
        check_cancel(cancel)?;
        self.vad.flush();
        let results = self.drain(cancel)?;
        self.vad.reset();
        // reset 之后 SpeechSegment::start 重新从 0 计数，把本轮长度并入基准偏移。
        self.base_samples += self.fed_samples;
        self.fed_samples = 0;
        Ok(results)
    }

    fn drain(&mut self, cancel: Option<&CancellationFlag>) -> Result<Vec<LocalSegment>, String> {
        let mut results = Vec::new();
        while let Some(segment) = self.vad.front() {
            let text = self.engine.recognize(segment.samples(), cancel)?;
            if !text.is_empty() {
                let start = self.base_samples + segment.start().max(0) as u64;
                let end = start + segment.n().max(0) as u64;
                results.push(LocalSegment {
                    text,
                    begin_ms: samples_to_ms(start),
                    end_ms: samples_to_ms(end),
                });
            }
            drop(segment);
            self.vad.pop();
        }
        // 这里不能 reset：Silero VAD 依赖跨窗口的循环状态累积语音概率，语音刚起始
        // 时 detected() 仍为 false，此刻 reset 会清空模型状态与尚未成段的缓冲，使
        // detected() 永远无法置位、reset 每块反复触发，VAD 再也切不出任何句段。
        // 文件模式的缓冲增长由 file 模块的周期性 flush_and_reset 收口。
        Ok(results)
    }
}

// 保留原整段算法作识别质量与时间轴的独立回归参照。
#[cfg(test)]
fn recognize_file_segments(
    spec: &LocalModelSpec,
    samples: &[f32],
) -> Result<Vec<LocalSegment>, String> {
    let mut session = OfflineVadSession::create_temporary(spec)?;
    let mut results = Vec::new();
    for (index, chunk) in samples.chunks(SAMPLE_RATE as usize * 10).enumerate() {
        results.extend(session.accept(chunk, None)?);
        // 即使输入长时间没有静音，也每分钟强制收口一次，避免 sherpa VAD 的内部
        // 环形缓冲区随整段文件持续增长；边界处 flush 不丢样本，只多一个句段边界。
        if (index + 1) % 6 == 0 {
            results.extend(session.flush_and_reset(None)?);
        }
    }
    results.extend(session.finish(None)?);
    Ok(results)
}

fn samples_to_ms(samples: u64) -> u64 {
    samples.saturating_mul(1_000) / SAMPLE_RATE as u64
}

fn ensure_ready(spec: &LocalModelSpec) -> Result<(), String> {
    let pack = ModelPackManifest {
        engine: spec.engine.clone(),
        files: spec.files.clone(),
        params: spec.params.clone(),
    };
    model_download::verify_pack(&spec.model_dir, &pack)
        .map_err(|error| format!("本地模型尚未就绪，请在插件管理中下载或重新安装：{error}"))
}

fn param_path(spec: &LocalModelSpec, key: &str) -> Result<String, String> {
    let relative = spec
        .params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("模型包参数缺少 {key}"))?;
    let declared = spec
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<HashSet<_>>();
    if !declared.contains(relative) {
        return Err(format!("模型包参数 {key} 指向未声明文件：{relative}"));
    }
    let path = spec.model_dir.join(PathBuf::from(relative));
    Ok(path.to_string_lossy().into_owned())
}

fn string_param(value: &Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

fn bool_param(value: &Value, key: &str, fallback: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(fallback)
}

fn int_param(value: &Value, key: &str, fallback: i32) -> i32 {
    value
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(fallback)
}

fn float_param(value: &Value, key: &str, fallback: f32) -> f32 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::super::plugin::ModelPackFileManifest;
    use super::*;

    #[test]
    fn cancellation_while_waiting_for_recognizer_skips_decode_after_lock() {
        use std::sync::atomic::AtomicBool;
        let recognizer = Arc::new(Mutex::new(()));
        let cancel = Arc::new(CancellationFlag::new(false));
        let decoded = Arc::new(AtomicBool::new(false));
        let held = recognizer.lock().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let task = {
            let recognizer = recognizer.clone();
            let cancel = cancel.clone();
            let decoded = decoded.clone();
            std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                lock_recognizer(&recognizer, Some(&cancel)).map(|_guard| {
                    decoded.store(true, Ordering::Release);
                })
            })
        };
        started_rx.recv().unwrap();
        cancel.store(true, Ordering::Release);
        drop(held);
        assert_eq!(task.join().unwrap().unwrap_err(), "本地语音识别已取消");
        assert!(!decoded.load(Ordering::Acquire));
        assert!(
            lock_recognizer(&recognizer, None).is_ok(),
            "取消必须释放识别器锁"
        );
    }

    #[test]
    fn rejects_params_that_reference_undeclared_files() {
        let spec = LocalModelSpec {
            manifest_version: "1.0.0".into(),
            plugin_id: "test".into(),
            provider_id: "test".into(),
            engine: "sherpa-onnx-online".into(),
            model_dir: PathBuf::from("models"),
            files: Vec::new(),
            params: serde_json::json!({ "encoder": "missing.onnx" }),
        };
        assert!(param_path(&spec, "encoder").is_err());
    }

    #[test]
    #[ignore = "需要 SAYIT_SHERPA_POC_DIR 指向官方 Paraformer 模型与 test.wav"]
    fn recognizes_official_paraformer_wave() {
        let model_dir = PathBuf::from(std::env::var("SAYIT_SHERPA_POC_DIR").unwrap());
        let spec = LocalModelSpec {
            manifest_version: "1.0.0".into(),
            plugin_id: "poc".into(),
            provider_id: "poc".into(),
            engine: "sherpa-onnx-online".into(),
            model_dir: model_dir.clone(),
            files: vec![
                ModelPackFileManifest {
                    path: "encoder.int8.onnx".into(),
                    sha256: "81a70226a8934e6ed92aa1d4fc486b428b5398e2f2619ed4897b7294cab90e9a"
                        .into(),
                    size_bytes: 165_462_184,
                    download: None,
                },
                ModelPackFileManifest {
                    path: "decoder.int8.onnx".into(),
                    sha256: "f3cca9f77bb9d93c8fcbfb63ae617b6b1ee96818df3aa3b151c40658fe38594f"
                        .into(),
                    size_bytes: 71_664_561,
                    download: None,
                },
                ModelPackFileManifest {
                    path: "tokens.txt".into(),
                    sha256: "59aba8873a2ed1e122c25fee421e25f283b63290efbde85c1f01a853d83cb6e6"
                        .into(),
                    size_bytes: 75_756,
                    download: None,
                },
            ],
            params: serde_json::json!({
                "encoder": "encoder.int8.onnx",
                "decoder": "decoder.int8.onnx",
                "tokens": "tokens.txt",
                "numThreads": 2
            }),
        };
        let wave_path = model_dir.join("test.wav");
        let wave = sherpa_onnx::Wave::read(wave_path.to_str().unwrap()).unwrap();
        let mut session = OnlineSession::create_realtime(&spec, None).unwrap();
        let mut text = String::new();
        for chunk in wave.samples().chunks(3_200) {
            let output = session.accept(chunk, None).unwrap();
            if let Some(partial) = output.partial {
                text = partial;
            }
            if let Some(final_text) = output.finals.last() {
                text = final_text.clone();
            }
        }
        if let Some(final_text) = session.finish(None).unwrap().finals.last() {
            text = final_text.clone();
        }
        assert!(!text.trim().is_empty(), "Paraformer PoC 应输出非空文本");
        println!("Paraformer PoC: {text}");
    }

    pub(super) fn sensevoice_spec() -> LocalModelSpec {
        let model_dir = PathBuf::from(std::env::var("SAYIT_SENSEVOICE_POC_DIR").unwrap());
        LocalModelSpec {
            manifest_version: "1.0.0".into(),
            plugin_id: "poc".into(),
            provider_id: "poc".into(),
            engine: "sherpa-onnx-offline".into(),
            model_dir: model_dir.clone(),
            files: vec![
                ModelPackFileManifest {
                    path: "model.int8.onnx".into(),
                    sha256: "12ca1a2ae7ecf3e0019ef2822307ee0b5cadc9196569e379b4c4026f8205276d"
                        .into(),
                    size_bytes: 237_115_547,
                    download: None,
                },
                ModelPackFileManifest {
                    path: "tokens.txt".into(),
                    sha256: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc"
                        .into(),
                    size_bytes: 315_894,
                    download: None,
                },
                ModelPackFileManifest {
                    path: "silero_vad.onnx".into(),
                    sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6"
                        .into(),
                    size_bytes: 643_854,
                    download: None,
                },
            ],
            params: serde_json::json!({
                "model": "model.int8.onnx",
                "tokens": "tokens.txt",
                "vadModel": "silero_vad.onnx",
                "language": "auto",
                "useItn": true,
                "numThreads": 2
            }),
        }
    }

    #[test]
    #[ignore = "需要 SAYIT_SENSEVOICE_POC_DIR 指向官方 SenseVoice 模型与 test.wav"]
    fn recognizes_official_sensevoice_wave_and_vad_segment() {
        let spec = sensevoice_spec();
        let wave_path = spec.model_dir.join("test.wav");
        let wave = sherpa_onnx::Wave::read(wave_path.to_str().unwrap()).unwrap();
        let direct = OfflineEngine::create_temporary(&spec)
            .unwrap()
            .recognize(wave.samples(), None)
            .unwrap();
        assert!(
            !direct.trim().is_empty(),
            "SenseVoice 直接整句识别应输出文本"
        );
        let segments = recognize_file_segments(&spec, wave.samples()).unwrap();
        let segmented = joined(&segments);
        assert!(
            !segmented.trim().is_empty(),
            "SenseVoice VAD 分段识别应输出文本"
        );
        // 只断言"非空"会放过退化结果：历史上 VAD 丢掉开头、只切出一个语气词也算
        // 通过，因此要求分段结果与整句识别的长度相当。
        let expected_len = direct.trim().chars().count();
        assert!(
            segmented.trim().chars().count() * 2 >= expected_len,
            "VAD 分段结果明显短于整句识别，疑似丢弃语音：分段={segmented:?} 整句={direct:?}"
        );
        let wave_ms = samples_to_ms(wave.samples().len() as u64);
        assert_timeline(&segments, wave_ms);

        // 实时听写按小块喂入，是 VAD 状态最容易被破坏的路径（drain 里误加 reset 会
        // 让这里一个句段都切不出来），必须与文件模式分开回归。
        let mut session = OfflineVadSession::create_temporary(&spec).unwrap();
        let mut realtime = Vec::new();
        for chunk in wave.samples().chunks(1_600) {
            realtime.extend(session.accept(chunk, None).unwrap());
        }
        realtime.extend(session.finish(None).unwrap());
        let realtime = joined(&realtime);
        assert!(
            !realtime.trim().is_empty(),
            "SenseVoice 实时小块喂入应输出文本（VAD 未切出任何句段）"
        );
        assert!(
            realtime.trim().chars().count() * 2 >= expected_len,
            "实时小块识别结果明显偏短：实时={realtime:?} 整句={direct:?}"
        );

        let ten_minutes = SAMPLE_RATE as usize * 10 * 60;
        let repeats = ten_minutes.div_ceil(wave.samples().len());
        let mut long_audio = Vec::with_capacity(ten_minutes);
        for _ in 0..repeats {
            long_audio.extend_from_slice(wave.samples());
        }
        long_audio.truncate(ten_minutes);
        let long_segments = recognize_file_segments(&spec, &long_audio).unwrap();
        assert!(
            !joined(&long_segments).trim().is_empty(),
            "SenseVoice 十分钟音频分段识别应输出文本"
        );
        // 十分钟音频会多次触发 flush_and_reset，reset 后 SpeechSegment::start 重新从 0
        // 计数。若基准偏移没补齐，这里的时间轴会突然回退或整体压缩到前一分钟内。
        assert_timeline(&long_segments, samples_to_ms(long_audio.len() as u64));
        let last_begin = long_segments.last().unwrap().begin_ms;
        assert!(
            last_begin > 9 * 60 * 1_000,
            "十分钟音频的末句起点应接近尾部，实际 {last_begin}ms —— 疑似 reset 后基准偏移丢失"
        );
        println!("SenseVoice PoC: {segmented}");
        println!("SenseVoice 首句时间轴: {:?}", segments.first());
    }

    fn joined(segments: &[LocalSegment]) -> String {
        segments
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 句段时间轴必须单调、不重叠，且落在音频时长内。
    fn assert_timeline(segments: &[LocalSegment], total_ms: u64) {
        assert!(!segments.is_empty(), "应至少切出一个句段");
        let mut previous_end = 0;
        for segment in segments {
            assert!(
                segment.begin_ms < segment.end_ms,
                "句段起止时间非法：{segment:?}"
            );
            assert!(
                segment.begin_ms >= previous_end,
                "句段时间轴回退或重叠：{segment:?} 上一句结束于 {previous_end}ms"
            );
            assert!(
                segment.end_ms <= total_ms + 1_000,
                "句段超出音频时长 {total_ms}ms：{segment:?}"
            );
            previous_end = segment.end_ms;
        }
    }
}

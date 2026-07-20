use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::Value;
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig, OnlineRecognizer,
    OnlineRecognizerConfig, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};

use super::model_download;
use super::plugin::{LocalModelSpec, ModelPackManifest};

const SAMPLE_RATE: i32 = 16_000;

#[derive(Default)]
pub struct LocalAsrOutput {
    pub partial: Option<String>,
    pub finals: Vec<String>,
}

pub struct OnlineSession {
    recognizer: OnlineRecognizer,
    stream: sherpa_onnx::OnlineStream,
    last_partial: String,
}

impl OnlineSession {
    pub fn create(spec: &LocalModelSpec) -> Result<Self, String> {
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
        config.rule1_min_trailing_silence =
            float_param(&spec.params, "rule1MinTrailingSilence", 2.4);
        config.rule2_min_trailing_silence =
            float_param(&spec.params, "rule2MinTrailingSilence", 1.2);
        config.rule3_min_utterance_length =
            float_param(&spec.params, "rule3MinUtteranceLength", 20.0);
        let recognizer = OnlineRecognizer::create(&config)
            .ok_or("创建 sherpa-onnx 在线识别器失败；请检查模型包文件与参数")?;
        let stream = recognizer.create_stream();
        Ok(Self {
            recognizer,
            stream,
            last_partial: String::new(),
        })
    }

    pub fn accept(&mut self, samples: &[f32]) -> LocalAsrOutput {
        self.stream.accept_waveform(SAMPLE_RATE, samples);
        self.decode_ready()
    }

    pub fn finish(mut self) -> LocalAsrOutput {
        self.stream.input_finished();
        let mut output = self.decode_ready();
        if let Some(result) = self.recognizer.get_result(&self.stream) {
            let text = result.text.trim();
            if !text.is_empty() && output.finals.last().is_none_or(|last| last != text) {
                output.finals.push(text.to_string());
            }
        }
        output.partial = None;
        output
    }

    fn decode_ready(&mut self) -> LocalAsrOutput {
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
        let mut output = LocalAsrOutput::default();
        let result = self.recognizer.get_result(&self.stream);
        if self.recognizer.is_endpoint(&self.stream) {
            if let Some(result) = result {
                let text = result.text.trim();
                if !text.is_empty() {
                    output.finals.push(text.to_string());
                }
            }
            self.recognizer.reset(&self.stream);
            self.last_partial.clear();
        } else if let Some(result) = result {
            let text = result.text.trim();
            if !text.is_empty() && text != self.last_partial {
                self.last_partial = text.to_string();
                output.partial = Some(text.to_string());
            }
        }
        output
    }
}

pub struct OfflineEngine {
    recognizer: OfflineRecognizer,
}

impl OfflineEngine {
    pub fn create(spec: &LocalModelSpec) -> Result<Self, String> {
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
            .ok_or("创建 sherpa-onnx SenseVoice 识别器失败；请检查模型包文件与参数")?;
        Ok(Self { recognizer })
    }

    pub fn recognize(&self, samples: &[f32]) -> Result<String, String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE, samples);
        self.recognizer.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text.trim().to_string())
            .ok_or("SenseVoice 未返回识别结果".into())
    }
}

pub struct OfflineVadSession {
    engine: OfflineEngine,
    vad: VoiceActivityDetector,
    /// 每次喂给 VAD 的样本数，等于 Silero 的窗口大小。
    window_size: usize,
}

impl OfflineVadSession {
    pub fn create(spec: &LocalModelSpec) -> Result<Self, String> {
        let engine = OfflineEngine::create(spec)?;
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
        })
    }

    /// 调用方可以传入任意长度的音频；这里负责切成 VAD 能正确处理的窗口。
    pub fn accept(&mut self, samples: &[f32]) -> Result<Vec<String>, String> {
        // 必须按窗口大小小块喂入：单次传入过长音频会让 sherpa VAD 在语音确认前
        // 裁掉缓冲里的开头。实测 5.6s 音频，≤0.3s 的块识别完整，0.5s 的块丢失开头
        // 数字，整段一次性传入只剩一个语气词。
        let mut results = Vec::new();
        for window in samples.chunks(self.window_size) {
            self.vad.accept_waveform(window);
            results.extend(self.drain()?);
        }
        Ok(results)
    }

    pub fn finish(mut self) -> Result<Vec<String>, String> {
        self.flush_and_reset()
    }

    pub fn flush_and_reset(&mut self) -> Result<Vec<String>, String> {
        self.vad.flush();
        let results = self.drain()?;
        self.vad.reset();
        Ok(results)
    }

    fn drain(&mut self) -> Result<Vec<String>, String> {
        let mut results = Vec::new();
        while let Some(segment) = self.vad.front() {
            let text = self.engine.recognize(segment.samples())?;
            if !text.is_empty() {
                results.push(text);
            }
            drop(segment);
            self.vad.pop();
        }
        // 这里不能 reset：Silero VAD 依赖跨窗口的循环状态累积语音概率，语音刚起始
        // 时 detected() 仍为 false，此刻 reset 会清空模型状态与尚未成段的缓冲，使
        // detected() 永远无法置位、reset 每块反复触发，VAD 再也切不出任何句段。
        // 缓冲增长由 recognize_file_segments 的周期性 flush_and_reset 收口。
        Ok(results)
    }
}

pub fn recognize_file_segments(spec: &LocalModelSpec, samples: &[f32]) -> Result<String, String> {
    let mut session = OfflineVadSession::create(spec)?;
    let mut results = Vec::new();
    for (index, chunk) in samples.chunks(SAMPLE_RATE as usize * 10).enumerate() {
        results.extend(session.accept(chunk)?);
        // 即使输入长时间没有静音，也每分钟强制收口一次，避免 sherpa VAD 的内部
        // 环形缓冲区随整段文件持续增长；边界处 flush 不丢样本，只多一个句段边界。
        if (index + 1) % 6 == 0 {
            results.extend(session.flush_and_reset()?);
        }
    }
    results.extend(session.finish()?);
    Ok(results.join("\n"))
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
    fn rejects_params_that_reference_undeclared_files() {
        let spec = LocalModelSpec {
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
        let mut session = OnlineSession::create(&spec).unwrap();
        let mut text = String::new();
        for chunk in wave.samples().chunks(3_200) {
            let output = session.accept(chunk);
            if let Some(partial) = output.partial {
                text = partial;
            }
            if let Some(final_text) = output.finals.last() {
                text = final_text.clone();
            }
        }
        if let Some(final_text) = session.finish().finals.last() {
            text = final_text.clone();
        }
        assert!(!text.trim().is_empty(), "Paraformer PoC 应输出非空文本");
        println!("Paraformer PoC: {text}");
    }

    #[test]
    #[ignore = "需要 SAYIT_SENSEVOICE_POC_DIR 指向官方 SenseVoice 模型与 test.wav"]
    fn recognizes_official_sensevoice_wave_and_vad_segment() {
        let model_dir = PathBuf::from(std::env::var("SAYIT_SENSEVOICE_POC_DIR").unwrap());
        let spec = LocalModelSpec {
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
        };
        let wave_path = model_dir.join("test.wav");
        let wave = sherpa_onnx::Wave::read(wave_path.to_str().unwrap()).unwrap();
        let direct = OfflineEngine::create(&spec)
            .unwrap()
            .recognize(wave.samples())
            .unwrap();
        assert!(
            !direct.trim().is_empty(),
            "SenseVoice 直接整句识别应输出文本"
        );
        let segmented = recognize_file_segments(&spec, wave.samples()).unwrap();
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

        // 实时听写按小块喂入，是 VAD 状态最容易被破坏的路径（drain 里误加 reset 会
        // 让这里一个句段都切不出来），必须与文件模式分开回归。
        let mut session = OfflineVadSession::create(&spec).unwrap();
        let mut realtime = Vec::new();
        for chunk in wave.samples().chunks(1_600) {
            realtime.extend(session.accept(chunk).unwrap());
        }
        realtime.extend(session.finish().unwrap());
        let realtime = realtime.join("\n");
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
        let long_text = recognize_file_segments(&spec, &long_audio).unwrap();
        assert!(
            !long_text.trim().is_empty(),
            "SenseVoice 十分钟音频分段识别应输出文本"
        );
        println!("SenseVoice PoC: {segmented}");
    }
}

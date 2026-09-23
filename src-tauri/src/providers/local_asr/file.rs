//! 文件解码与 VAD 同步衔接；只缓冲一个识别块，保留原有十秒喂入、每分钟收口的节奏。
use super::{samples_to_ms, LocalModelSpec, LocalSegment, OfflineVadSession, SAMPLE_RATE};
use std::sync::atomic::{AtomicBool, Ordering};

const CHUNK_SAMPLES: usize = SAMPLE_RATE as usize * 10;

struct FileVadSession {
    session: OfflineVadSession,
    pending: Vec<f32>,
    chunks: usize,
    results: Vec<LocalSegment>,
}

impl FileVadSession {
    fn create(spec: &LocalModelSpec) -> Result<Self, String> {
        Ok(Self {
            session: OfflineVadSession::create(spec)?,
            pending: Vec::with_capacity(CHUNK_SAMPLES),
            chunks: 0,
            results: Vec::new(),
        })
    }

    fn accept(&mut self, mut samples: &[f32]) -> Result<(), String> {
        while !samples.is_empty() {
            let count = samples.len().min(CHUNK_SAMPLES - self.pending.len());
            self.pending.extend_from_slice(&samples[..count]);
            samples = &samples[count..];
            if self.pending.len() == CHUNK_SAMPLES {
                self.consume_pending()?;
            }
        }
        Ok(())
    }

    fn consume_pending(&mut self) -> Result<(), String> {
        self.results.extend(self.session.accept(&self.pending)?);
        self.pending.clear();
        self.chunks += 1;
        // 包大小与解码器有关，不能把包边界直接变成 VAD 的语音/重置边界。
        if self.chunks % 6 == 0 {
            self.results.extend(self.session.flush_and_reset()?);
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<LocalSegment>, String> {
        if !self.pending.is_empty() {
            self.consume_pending()?;
        }
        self.results.extend(self.session.finish()?);
        Ok(self.results)
    }
}

/// 返回完整的句段与实际音频时长；取消或解码失败时不返回部分结果。
pub(crate) fn recognize_audio_file(
    spec: &LocalModelSpec,
    path: &str,
    cancel: Option<&AtomicBool>,
) -> Result<(Vec<LocalSegment>, u64), String> {
    let check_cancel = || {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            Err("录音识别已取消".to_string())
        } else {
            Ok(())
        }
    };
    let mut session = None;
    let count = crate::audio_prep::decode_mono_16k_chunks(path, check_cancel, |chunk| {
        if session.is_none() {
            session = Some(FileVadSession::create(spec)?);
            check_cancel()?;
        }
        session.as_mut().unwrap().accept(chunk)
    })?;
    check_cancel()?;
    let segments = match session {
        Some(session) => session.finish()?,
        None => Vec::new(),
    };
    check_cancel()?;
    Ok((segments, samples_to_ms(count)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_precedes_file_io_and_model_loading() {
        let spec = LocalModelSpec {
            plugin_id: "test".into(),
            provider_id: "test".into(),
            engine: "sherpa-onnx-offline".into(),
            model_dir: "missing".into(),
            files: Vec::new(),
            params: serde_json::json!({}),
        };
        let cancel = AtomicBool::new(true);
        assert_eq!(
            recognize_audio_file(&spec, "missing.wav", Some(&cancel)).unwrap_err(),
            "录音识别已取消"
        );
    }

    #[test]
    #[ignore = "需要 SAYIT_SENSEVOICE_POC_DIR 指向官方 SenseVoice 模型与 test.wav"]
    fn streamed_sensevoice_matches_offline_text_and_timestamps() {
        let spec = super::super::tests::sensevoice_spec();
        let source = spec.model_dir.join("test.wav");
        let wave = sherpa_onnx::Wave::read(source.to_str().unwrap()).unwrap();
        // 覆盖不足一块、整十秒、整一分钟及跨一分钟后带余数的尾块。
        // 每个夹具都重新加载会话，确保收口/释放后可继续下一次识别。
        for count in [
            wave.samples().len(),
            CHUNK_SAMPLES,
            CHUNK_SAMPLES * 6,
            CHUNK_SAMPLES * 6 + 19_937,
        ] {
            let input: Vec<f32> = wave.samples().iter().copied().cycle().take(count).collect();
            let rate = 44_100;
            let input = crate::audio_dsp::resample_linear(&input, SAMPLE_RATE as u32, rate);
            let path =
                std::env::temp_dir().join(format!("say-it-file-vad-{}.wav", uuid::Uuid::new_v4()));
            assert!(sherpa_onnx::write(
                path.to_str().unwrap(),
                &input,
                rate as i32
            ));
            let samples = crate::audio_prep::decode_to_mono_16k(path.to_str().unwrap()).unwrap();
            let expected = super::super::recognize_file_segments(&spec, &samples).unwrap();
            assert!(!expected.is_empty());
            let (actual, duration_ms) =
                recognize_audio_file(&spec, path.to_str().unwrap(), None).unwrap();
            assert_eq!(duration_ms, samples_to_ms(samples.len() as u64));
            assert_eq!(actual, expected, "样本数 {count} 的文本或时间戳发生变化");
            std::fs::remove_file(path).unwrap();
        }
    }
}

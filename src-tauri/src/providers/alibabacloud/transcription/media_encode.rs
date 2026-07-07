use base64::{engine::general_purpose::STANDARD, Engine as _};

/// 解码任意输入文件、下混单声道并重采样到 16kHz 后，编码为 Ogg-Opus 并打包成 Data URI。
/// 供 fun-asr-flash 使用：其 `parameters.format` 字段文档明确支持 `opus`。
pub(super) fn build_opus_data_uri(file_path: &str) -> Result<String, String> {
    let mono16k = crate::audio_prep::decode_to_mono_16k(file_path)?;
    let pcm16 = crate::audio_prep::f32_to_i16(&mono16k);
    let opus_bytes = ogg_opus::encode::<{ crate::audio_prep::TARGET_SAMPLE_RATE }, 1>(&pcm16)
        .map_err(|e| format!("编码 Opus 音频失败：{e:?}"))?;
    Ok(format!(
        "data:audio/ogg;base64,{}",
        STANDARD.encode(opus_bytes)
    ))
}

/// 解码任意输入文件、下混单声道并重采样到 16kHz 后，编码为 MP3 并打包成 Data URI。
/// 供 qwen3-asr-flash 使用：该模型没有独立的 format 字段，音频格式全靠 Data URI 的
/// mediatype 判断，文档只验证过 `audio/wav`、`audio/mp3`，因此不能像 fun-asr-flash 一样用 Opus。
pub(super) fn build_mp3_data_uri(file_path: &str) -> Result<String, String> {
    let mono16k = crate::audio_prep::decode_to_mono_16k(file_path)?;
    let pcm16 = crate::audio_prep::f32_to_i16(&mono16k);
    let mp3_bytes = encode_mp3_mono(&pcm16, crate::audio_prep::TARGET_SAMPLE_RATE)?;
    Ok(format!(
        "data:audio/mpeg;base64,{}",
        STANDARD.encode(mp3_bytes)
    ))
}

fn encode_mp3_mono(pcm16: &[i16], sample_rate: u32) -> Result<Vec<u8>, String> {
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};

    let builder = Builder::new().ok_or_else(|| "初始化 MP3 编码器失败".to_string())?;
    let builder = builder
        .with_num_channels(1)
        .map_err(|e| format!("设置 MP3 声道数失败：{e:?}"))?;
    let builder = builder
        .with_sample_rate(sample_rate)
        .map_err(|e| format!("设置 MP3 采样率失败：{e:?}"))?;
    let builder = builder
        .with_brate(Bitrate::Kbps64)
        .map_err(|e| format!("设置 MP3 码率失败：{e:?}"))?;
    let builder = builder
        .with_quality(Quality::Best)
        .map_err(|e| format!("设置 MP3 质量失败：{e:?}"))?;
    let mut encoder = builder
        .build()
        .map_err(|e| format!("创建 MP3 编码器失败：{e:?}"))?;

    let input = MonoPcm(pcm16);
    let mut out = Vec::new();
    out.reserve(mp3lame_encoder::max_required_buffer_size(pcm16.len()));
    let n = encoder
        .encode(input, out.spare_capacity_mut())
        .map_err(|e| format!("MP3 编码失败：{e:?}"))?;
    unsafe {
        out.set_len(out.len() + n);
    }
    let n = encoder
        .flush::<FlushNoGap>(out.spare_capacity_mut())
        .map_err(|e| format!("MP3 编码收尾失败：{e:?}"))?;
    unsafe {
        out.set_len(out.len() + n);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_prep::write_test_stereo_wav;

    /// 5 秒 44.1kHz 立体声 16-bit PCM 原始体积，用作压缩率的参照基准。
    fn raw_pcm_bytes(seconds: f32, rate: u32) -> usize {
        (rate as f32 * seconds) as usize * 4
    }

    #[test]
    fn opus_data_uri_shrinks_and_is_valid() {
        let path = std::env::temp_dir().join("say_it_transcription_opus_test.wav");
        write_test_stereo_wav(&path, 5.0, 44_100);

        let data_uri = build_opus_data_uri(path.to_str().unwrap()).expect("opus encode should succeed");
        assert!(data_uri.starts_with("data:audio/ogg;base64,"));
        let b64 = data_uri.trim_start_matches("data:audio/ogg;base64,");
        let decoded = STANDARD.decode(b64).expect("base64 payload should decode");
        assert!(!decoded.is_empty());
        assert!(
            decoded.len() < raw_pcm_bytes(5.0, 44_100) / 4,
            "opus output ({} bytes) should be much smaller than raw PCM",
            decoded.len()
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mp3_data_uri_shrinks_and_is_valid() {
        let path = std::env::temp_dir().join("say_it_transcription_mp3_test.wav");
        write_test_stereo_wav(&path, 5.0, 44_100);

        let data_uri = build_mp3_data_uri(path.to_str().unwrap()).expect("mp3 encode should succeed");
        assert!(data_uri.starts_with("data:audio/mpeg;base64,"));
        let b64 = data_uri.trim_start_matches("data:audio/mpeg;base64,");
        let decoded = STANDARD.decode(b64).expect("base64 payload should decode");
        assert!(!decoded.is_empty());
        assert!(
            decoded.len() < raw_pcm_bytes(5.0, 44_100) / 4,
            "mp3 output ({} bytes) should be much smaller than raw PCM",
            decoded.len()
        );

        let _ = std::fs::remove_file(&path);
    }
}

//! 录音识别前处理：把任意输入音视频文件解码为单声道 16kHz PCM，供后续 Opus/MP3 压缩使用。
//!
//! 只服务于"同步短音频识别"（fun-asr-flash / qwen3-asr-flash）：这两个模型走
//! multimodal-generation 接口，请求体大小受限（Base64 编码后需落在文档给出的体积上限内），
//! 直接把用户选择的原始文件（可能是高采样率/多声道/未压缩 WAV）塞进请求容易超限。
//! 异步转写模型（fun-asr / paraformer / qwen3-asr-flash-filetrans）走 OSS 上传，体积上限是
//! 2GB/12 小时，不需要这道预处理。

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::audio_dsp::resample_linear;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// 解码任意音视频文件的首个可解码音轨，下混为单声道并重采样到 16kHz。
/// 返回 [-1, 1] 范围的 f32 PCM。
pub fn decode_to_mono_16k(file_path: &str) -> Result<Vec<f32>, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("打开待识别音频文件失败：{file_path}（{e}）"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(file_path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("无法识别音频文件格式：{e}"))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "音频文件中未找到可解码的音轨".to_string())?
        .clone();
    let track_id = track.id;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| "未找到音频编码参数".to_string())?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_params, &AudioDecoderOptions::default())
        .map_err(|e| format!("创建音频解码器失败：{e}"))?;

    let mut mono: Vec<f32> = Vec::new();
    let mut in_rate: Option<u32> = None;
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(e) => return Err(format!("读取音频数据失败：{e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio_buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) | Err(SymphoniaError::IoError(_)) => continue,
            Err(e) => return Err(format!("解码音频失败：{e}")),
        };
        let spec = audio_buf.spec();
        let channels = spec.channels().count().max(1);
        in_rate.get_or_insert(spec.rate());

        interleaved.resize(audio_buf.samples_interleaved(), 0.0);
        audio_buf.copy_to_slice_interleaved(&mut interleaved);
        downmix_into(&interleaved, channels, &mut mono);
    }

    if mono.is_empty() {
        return Err("音频文件中没有可用的音频数据".to_string());
    }
    let in_rate = in_rate.ok_or_else(|| "无法获取音频采样率".to_string())?;

    Ok(resample_linear(&mono, in_rate, TARGET_SAMPLE_RATE))
}

fn downmix_into(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    for frame in interleaved.chunks_exact(channels) {
        out.push(frame.iter().sum::<f32>() / channels as f32);
    }
}

/// 转成 16-bit PCM（Opus/MP3 编码器的标准输入格式）。
pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
        .collect()
}

/// 写一个最小的立体声 16-bit PCM WAV 文件，供本模块及其它模块的测试复用（无需额外依赖）。
#[cfg(test)]
pub(crate) fn write_test_stereo_wav(path: &Path, seconds: f32, rate: u32) {
    use std::io::Write;
    let num_frames = (rate as f32 * seconds) as u32;
    let data_len = num_frames * 4; // 2 channels * 2 bytes
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap(); // fmt chunk size
    f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
    f.write_all(&2u16.to_le_bytes()).unwrap(); // channels
    f.write_all(&rate.to_le_bytes()).unwrap();
    f.write_all(&(rate * 4).to_le_bytes()).unwrap(); // byte rate
    f.write_all(&4u16.to_le_bytes()).unwrap(); // block align
    f.write_all(&16u16.to_le_bytes()).unwrap(); // bits per sample
    f.write_all(b"data").unwrap();
    f.write_all(&data_len.to_le_bytes()).unwrap();
    for i in 0..num_frames {
        let t = i as f32 / rate as f32;
        let sample = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        let s16 = (sample * i16::MAX as f32) as i16;
        f.write_all(&s16.to_le_bytes()).unwrap();
        f.write_all(&s16.to_le_bytes()).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_downmixes_and_resamples_to_16k() {
        let dir = std::env::temp_dir();
        let path = dir.join("say_it_audio_prep_test.wav");
        write_test_stereo_wav(&path, 2.0, 44_100);

        let mono16k = decode_to_mono_16k(path.to_str().unwrap()).expect("decode should succeed");
        let expected_len = (2.0 * TARGET_SAMPLE_RATE as f32) as usize;
        // 允许一点重采样引入的长度误差。
        assert!(
            (mono16k.len() as i64 - expected_len as i64).abs() < (TARGET_SAMPLE_RATE / 10) as i64,
            "unexpected decoded length: {} (expected ~{})",
            mono16k.len(),
            expected_len
        );
        assert!(mono16k.iter().any(|&s| s.abs() > 0.01), "decoded audio should not be silent");

        let _ = std::fs::remove_file(&path);
    }
}

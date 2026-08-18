//! 录音识别前处理：把任意输入音视频文件解码为单声道 16kHz PCM，供后续 Opus/MP3 压缩使用。
//!
//! 只服务于"同步短音频识别"（fun-asr-flash / qwen3-asr-flash）：这两个模型走
//! multimodal-generation 接口，请求体大小受限（Base64 编码后需落在文档给出的体积上限内），
//! 直接把用户选择的原始文件（可能是高采样率/多声道/未压缩 WAV）塞进请求容易超限。
//! 异步转写模型（fun-asr / paraformer / qwen3-asr-flash-filetrans）走 OSS 上传，体积上限是
//! 2GB/12 小时，不需要这道预处理。

use std::fs::File;
use std::path::Path;

use audiopus::coder::Decoder as OpusDecoder;
use audiopus::{Channels as OpusChannels, SampleRate as OpusSampleRate};
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Duration, TimeBase, Timestamp};

use crate::audio_dsp::resample_linear;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;
const OPUS_SAMPLE_RATE: u32 = 48_000;
const OPUS_MAX_FRAMES_PER_PACKET: usize = 5_760;

/// 解码任意音视频文件的首个可解码音轨，下混为单声道并重采样到 16kHz。
/// 返回 [-1, 1] 范围的 f32 PCM。
pub fn decode_to_mono_16k(file_path: &str) -> Result<Vec<f32>, String> {
    let file =
        File::open(file_path).map_err(|e| format!("打开待识别音频文件失败：{file_path}（{e}）"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(file_path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
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

    if codec_params.codec == CODEC_ID_OPUS {
        return decode_opus_to_mono_16k(&mut *format, &track, codec_params);
    }

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

fn decode_opus_to_mono_16k(
    format: &mut dyn symphonia::core::formats::FormatReader,
    track: &symphonia::core::formats::Track,
    codec_params: &symphonia::core::codecs::audio::AudioCodecParameters,
) -> Result<Vec<f32>, String> {
    let channel_count = codec_params
        .channels
        .as_ref()
        .map(|channels| channels.count())
        .or_else(|| opus_header(codec_params.extra_data.as_deref()).map(|header| header.channels))
        .ok_or_else(|| "无法获取 Opus 音轨的声道数".to_string())?;
    let channels = match channel_count {
        1 => OpusChannels::Mono,
        2 => OpusChannels::Stereo,
        other => return Err(format!("暂不支持解码 {other} 声道的 Opus 音轨")),
    };
    let header = opus_header(codec_params.extra_data.as_deref());
    let mut decoder = OpusDecoder::new(OpusSampleRate::Hz48000, channels)
        .map_err(|e| format!("创建 Opus 音频解码器失败：{e}"))?;
    if let Some(header) = header {
        decoder
            .set_gain(i32::from(header.output_gain))
            .map_err(|e| format!("设置 Opus 输出增益失败：{e}"))?;
    }

    let mut mono = Vec::new();
    let mut decoded = vec![0.0_f32; OPUS_MAX_FRAMES_PER_PACKET * channel_count];
    let mut first_packet = true;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(e) => return Err(format!("读取 Opus 音频数据失败：{e}")),
        };
        if packet.track_id != track.id {
            continue;
        }
        if packet.data.is_empty() {
            continue;
        }

        let frames = decoder
            .decode_float(Some(packet.data.as_ref()), decoded.as_mut_slice(), false)
            .map_err(|e| format!("解码 Opus 音频失败：{e}"))?;
        let explicit_start_trim = duration_to_opus_frames(packet.trim_start, track.time_base);
        let start_trim = if explicit_start_trim > 0 {
            explicit_start_trim
        } else if first_packet {
            header.map_or(0, |header| usize::from(header.pre_skip))
        } else {
            0
        };
        let end_trim = duration_to_opus_frames(packet.trim_end, track.time_base);
        if start_trim.saturating_add(end_trim) > frames {
            return Err(format!(
                "Opus 音频的延迟或尾部填充信息无效（解码 {frames} 帧，头部跳过 {start_trim} 帧，尾部跳过 {end_trim} 帧；原始裁剪 {:?}/{:?}，时间基 {:?}）",
                packet.trim_start, packet.trim_end, track.time_base
            ));
        }

        let start = start_trim * channel_count;
        let end = (frames - end_trim) * channel_count;
        downmix_into(&decoded[start..end], channel_count, &mut mono);
        first_packet = false;
    }

    if mono.is_empty() {
        return Err("音频文件中没有可用的 Opus 音频数据".to_string());
    }
    Ok(resample_linear(&mono, OPUS_SAMPLE_RATE, TARGET_SAMPLE_RATE))
}

#[derive(Clone, Copy)]
struct OpusHeader {
    channels: usize,
    pre_skip: u16,
    output_gain: i16,
}

fn opus_header(extra_data: Option<&[u8]>) -> Option<OpusHeader> {
    let data = extra_data?;
    if data.len() < 19 || &data[..8] != b"OpusHead" {
        return None;
    }
    // ISO BMFF 的 dOps 字段为大端序，Symphonia 会补上 OpusHead 标记但保留 dOps
    // 的 version=0 与字节序；Ogg/Matroska 中的标准 OpusHead 则是 version=1、小端序。
    let (pre_skip, output_gain) = if data[8] == 0 {
        (
            u16::from_be_bytes([data[10], data[11]]),
            i16::from_be_bytes([data[16], data[17]]),
        )
    } else {
        (
            u16::from_le_bytes([data[10], data[11]]),
            i16::from_le_bytes([data[16], data[17]]),
        )
    };
    Some(OpusHeader {
        channels: usize::from(data[9]),
        pre_skip,
        output_gain,
    })
}

fn duration_to_opus_frames(duration: Duration, time_base: Option<TimeBase>) -> usize {
    if duration.is_zero() {
        return 0;
    }
    let Some(time_base) = time_base else {
        return usize::try_from(duration.get()).unwrap_or(usize::MAX);
    };
    let Ok(ticks) = i64::try_from(duration.get()) else {
        return usize::MAX;
    };
    time_base
        .calc_time(Timestamp::new(ticks))
        .map(|time| (time.as_secs_f64() * f64::from(OPUS_SAMPLE_RATE)).round() as usize)
        .unwrap_or(usize::MAX)
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
        assert!(
            mono16k.iter().any(|&s| s.abs() > 0.01),
            "decoded audio should not be silent"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn decode_supports_ogg_opus() {
        let path = std::env::temp_dir().join("say_it_audio_prep_test.opus");
        let samples: Vec<i16> = (0..(2 * OPUS_SAMPLE_RATE))
            .map(|i| {
                let t = i as f32 / OPUS_SAMPLE_RATE as f32;
                ((t * 440.0 * std::f32::consts::TAU).sin() * 0.5 * i16::MAX as f32) as i16
            })
            .collect();
        let encoded = encode_test_ogg_opus(&samples);
        std::fs::write(&path, encoded).expect("write opus fixture");

        let mono16k = decode_to_mono_16k(path.to_str().unwrap()).expect("decode should succeed");
        assert_eq!(mono16k.len(), 2 * TARGET_SAMPLE_RATE as usize);
        assert!(
            mono16k.iter().any(|&s| s.abs() > 0.01),
            "decoded audio should not be silent"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn opus_header_reads_ogg_and_mp4_byte_order() {
        let mut ogg = [0_u8; 19];
        ogg[..8].copy_from_slice(b"OpusHead");
        ogg[8] = 1;
        ogg[9] = 2;
        ogg[10..12].copy_from_slice(&312_u16.to_le_bytes());
        ogg[16..18].copy_from_slice(&(-256_i16).to_le_bytes());
        let parsed = opus_header(Some(&ogg)).unwrap();
        assert_eq!(parsed.channels, 2);
        assert_eq!(parsed.pre_skip, 312);
        assert_eq!(parsed.output_gain, -256);

        let mut mp4 = ogg;
        mp4[8] = 0;
        mp4[10..12].copy_from_slice(&312_u16.to_be_bytes());
        mp4[16..18].copy_from_slice(&(-256_i16).to_be_bytes());
        let parsed = opus_header(Some(&mp4)).unwrap();
        assert_eq!(parsed.pre_skip, 312);
        assert_eq!(parsed.output_gain, -256);
    }

    fn encode_test_ogg_opus(samples: &[i16]) -> Vec<u8> {
        use audiopus::coder::Encoder;
        use audiopus::Application;
        use ogg::writing::{PacketWriteEndInfo, PacketWriter};

        const FRAME_SIZE: usize = 960;
        let encoder = Encoder::new(
            OpusSampleRate::Hz48000,
            OpusChannels::Mono,
            Application::Audio,
        )
        .unwrap();
        let mut output = Vec::new();
        let mut writer = PacketWriter::new(&mut output);
        let mut head = [0_u8; 19];
        head[..8].copy_from_slice(b"OpusHead");
        head[8] = 1;
        head[9] = 1;
        head[12..16].copy_from_slice(&OPUS_SAMPLE_RATE.to_le_bytes());
        writer
            .write_packet(Box::new(head), 1, PacketWriteEndInfo::EndPage, 0)
            .unwrap();

        let mut tags = Vec::new();
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&6_u32.to_le_bytes());
        tags.extend_from_slice(b"say-it");
        tags.extend_from_slice(&0_u32.to_le_bytes());
        writer
            .write_packet(tags.into_boxed_slice(), 1, PacketWriteEndInfo::EndPage, 0)
            .unwrap();

        let frame_count = samples.len().div_ceil(FRAME_SIZE);
        for (index, source) in samples.chunks(FRAME_SIZE).enumerate() {
            let mut frame = [0_i16; FRAME_SIZE];
            frame[..source.len()].copy_from_slice(source);
            let mut packet = vec![0_u8; 4_000];
            let packet_len = encoder.encode(&frame, packet.as_mut_slice()).unwrap();
            packet.truncate(packet_len);
            let end = if index + 1 == frame_count {
                PacketWriteEndInfo::EndStream
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            writer
                .write_packet(
                    packet.into_boxed_slice(),
                    1,
                    end,
                    ((index + 1) * FRAME_SIZE) as u64,
                )
                .unwrap();
        }
        drop(writer);
        output
    }
}

use crate::prelude::*;
use crate::state::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioPcmResponse {
    pub(crate) sample_rate: u32,
    pub(crate) samples_base64: String,
}

/// 把「对比」功能录音阶段累积的 f32 PCM 写成本地临时 WAV 文件（16-bit 单声道），
/// 供非实时模型走 transcription_start 识别。写法参照 audio_prep.rs 测试模块里的
/// write_test_stereo_wav（44字节头 + 16bit PCM），不需要新增 wav/hound 之类的 crate。
#[tauri::command]
pub(crate) async fn encode_mono_wav_file(
    samples_base64: String,
    sample_rate: u32,
) -> Result<String, String> {
    let samples = decode_f32_base64(&samples_base64)?;
    if samples.is_empty() {
        return Err("录音数据为空".to_string());
    }
    let pcm16 = crate::audio_prep::f32_to_i16(&samples);
    let data_len = (pcm16.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // 单声道
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm16 {
        bytes.extend_from_slice(&s.to_le_bytes());
    }

    let path = std::env::temp_dir().join(format!("say-it-compare-{}.wav", Uuid::new_v4()));
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| format!("写入临时录音文件失败：{e}"))?;
    path.to_str()
        .map(ToString::to_string)
        .ok_or_else(|| "临时文件路径包含无法识别的字符".to_string())
}

/// 把任意音视频文件解码为 16kHz 单声道 PCM（复用 audio_prep::decode_to_mono_16k），
/// 供「对比」功能上传文件模式下实时模型的"播放 + 模拟实时喂入"使用。
#[tauri::command]
pub(crate) async fn decode_audio_file_pcm(file_path: String) -> Result<AudioPcmResponse, String> {
    if file_path.trim().is_empty() {
        return Err("请选择要识别的音视频文件".to_string());
    }
    let samples = crate::audio_prep::decode_to_mono_16k(&file_path)?;
    Ok(AudioPcmResponse {
        sample_rate: crate::audio_prep::TARGET_SAMPLE_RATE,
        samples_base64: encode_f32_base64(&samples),
    })
}

//! 文件对比的有界预读：保持 100ms 分包，不把完整 PCM 留在播放任务中。
use tokio::sync::mpsc;

pub(super) const PACKET_SAMPLES: usize = 1600;
pub(super) const PREFETCH_PACKETS: usize = 2;

pub(super) fn inspect(
    path: &str,
    check_cancel: impl FnMut() -> Result<(), String>,
) -> Result<u64, String> {
    // 完整检查后才开始投喂，保留坏文件不产生部分识别、进度总时长准确的语义。
    // 代价是再读一次源文件，不产生临时 PCM 文件或长驻录音缓存。
    crate::audio_prep::decode_mono_16k_chunks(path, check_cancel, |_| Ok(()))
}

pub(super) fn decode_packets(
    path: &str,
    check_cancel: impl FnMut() -> Result<(), String>,
    mut send: impl FnMut(Vec<f32>) -> Result<(), String>,
) -> Result<u64, String> {
    let mut packet = Vec::with_capacity(PACKET_SAMPLES);
    let count = crate::audio_prep::decode_mono_16k_chunks(path, check_cancel, |mut chunk| {
        while !chunk.is_empty() {
            let length = (PACKET_SAMPLES - packet.len()).min(chunk.len());
            packet.extend_from_slice(&chunk[..length]);
            chunk = &chunk[length..];
            if packet.len() == PACKET_SAMPLES {
                send(std::mem::replace(
                    &mut packet,
                    Vec::with_capacity(PACKET_SAMPLES),
                ))?;
            }
        }
        Ok(())
    })?;
    if !packet.is_empty() {
        send(packet)?;
    }
    Ok(count)
}

pub(super) fn start(
    path: String,
    check_cancel: impl FnMut() -> Result<(), String> + Send + 'static,
) -> (
    mpsc::Receiver<Vec<f32>>,
    tauri::async_runtime::JoinHandle<Result<u64, String>>,
) {
    let (tx, rx) = mpsc::channel(PREFETCH_PACKETS);
    let worker = tauri::async_runtime::spawn_blocking(move || {
        decode_packets(&path, check_cancel, |packet| {
            // 仅在解码工作线程等待；关闭接收端会立即解除背压并终止解码。
            tx.blocking_send(packet)
                .map_err(|_| "音频播放已结束".to_string())
        })
    });
    (rx, worker)
}

#[cfg(test)]
mod tests;

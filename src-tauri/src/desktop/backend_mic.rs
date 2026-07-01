use crate::prelude::*;
use crate::state::*;

const BACKEND_MIC_CHUNK_FRAMES: usize = 4096;

pub(crate) fn push_backend_mic_samples(
    mic: &Arc<Mutex<BackendMicState>>,
    input: Vec<f32>,
) {
    if input.is_empty() {
        return;
    }
    let Ok(mut guard) = mic.lock() else {
        return;
    };
    if guard.tx.is_none() && guard.session_id.is_none() {
        return;
    }
    guard.buffer.extend_from_slice(&input);
    while guard.buffer.len() >= BACKEND_MIC_CHUNK_FRAMES {
        let chunk: Vec<f32> = guard.buffer.drain(..BACKEND_MIC_CHUNK_FRAMES).collect();
        guard.chunk_count += 1;
        if let Some(tx) = guard.tx.as_ref() {
            if tx.send(AsrStreamInput::RawF32(chunk.clone())).is_ok() {
                continue;
            }
            guard.tx = None;
            guard.session_id = None;
        }
        guard.pending.push_back(chunk);
        while guard.pending.len() > 240 {
            guard.pending.pop_front();
        }
    }
}

pub(crate) fn flush_backend_mic_buffer(guard: &mut BackendMicState) -> Result<usize, String> {
    let mut flushed = 0usize;
    if !guard.buffer.is_empty() {
        let chunk = std::mem::take(&mut guard.buffer);
        guard.chunk_count += 1;
        if let Some(tx) = guard.tx.as_ref() {
            tx.send(AsrStreamInput::RawF32(chunk))
                .map_err(|_| "ASR stream channel closed".to_string())?;
            flushed += 1;
        }
    }
    while let Some(samples) = guard.pending.pop_front() {
        if let Some(tx) = guard.tx.as_ref() {
            tx.send(AsrStreamInput::RawF32(samples))
                .map_err(|_| "ASR stream channel closed".to_string())?;
            flushed += 1;
        }
    }
    Ok(flushed)
}

pub(crate) fn interleaved_to_mono_f32_from_f32(input: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return input.to_vec();
    }
    input
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect()
}

pub(crate) fn interleaved_to_mono_f32_from_i16(input: &[i16], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return input.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
    }
    input
        .chunks_exact(channels)
        .map(|frame| {
            frame
                .iter()
                .map(|&s| s as f32 / i16::MAX as f32)
                .sum::<f32>()
                / channels as f32
        })
        .collect()
}

pub(crate) fn interleaved_to_mono_f32_from_u16(input: &[u16], channels: usize) -> Vec<f32> {
    let to_f32 = |s: u16| (s as f32 / u16::MAX as f32) * 2.0 - 1.0;
    if channels <= 1 {
        return input.iter().map(|&s| to_f32(s)).collect();
    }
    input
        .chunks_exact(channels)
        .map(|frame| frame.iter().map(|&s| to_f32(s)).sum::<f32>() / channels as f32)
        .collect()
}

pub(crate) fn build_backend_mic_stream(
    mic: Arc<Mutex<BackendMicState>>,
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
) -> Result<cpal::Stream, String> {
    let stream_config: cpal::StreamConfig = config.clone().into();
    let channels = stream_config.channels.max(1) as usize;
    let err_fn = |err| dlog!("[backend-mic] 输入流错误: {err}");

    match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    push_backend_mic_samples(
                        &mic,
                        interleaved_to_mono_f32_from_f32(data, channels),
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("创建麦克风输入流失败: {e}")),
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    push_backend_mic_samples(
                        &mic,
                        interleaved_to_mono_f32_from_i16(data, channels),
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("创建麦克风输入流失败: {e}")),
        cpal::SampleFormat::U16 => device
            .build_input_stream(
                &stream_config,
                move |data: &[u16], _| {
                    push_backend_mic_samples(
                        &mic,
                        interleaved_to_mono_f32_from_u16(data, channels),
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("创建麦克风输入流失败: {e}")),
        sample_format => Err(format!("不支持的麦克风采样格式: {sample_format:?}")),
    }
}

/// 按名字在麦克风输入设备里查找；找不到（比如设备已拔出）返回 `None`，由调用方回退到默认设备。
fn find_input_device_by_name(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.input_devices()
        .ok()?
        .find(|device| device.name().map(|n| n == name).unwrap_or(false))
}

#[tauri::command]
pub(crate) fn start_backend_mic(
    device_name: Option<String>,
    state: tauri::State<'_, RuntimeState>,
) -> Result<BackendMicStartResponse, String> {
    let requested = device_name.and_then(|s| {
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    {
        let guard = state
            .backend_mic
            .lock()
            .map_err(|_| "Backend mic lock failed".to_string())?;
        if guard.worker.is_some() && guard.current_device == requested {
            return Ok(BackendMicStartResponse {
                sample_rate: guard.sample_rate,
                channels: guard.channels,
                reused: true,
                device_name: guard.current_device.clone(),
                fallback: false,
            });
        }
    }

    // 请求的设备和当前正在跑的不一致（包括从无到有第一次指定/切回默认），
    // 先停掉旧 worker 再按新设备起一个，避免同时开两路麦克风采集。
    let previous_worker = {
        let mut guard = state
            .backend_mic
            .lock()
            .map_err(|_| "Backend mic lock failed".to_string())?;
        guard.worker.take()
    };
    if let Some(worker) = previous_worker {
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        if worker
            .send(BackendMicCommand::Stop {
                reply: Some(stop_tx),
            })
            .is_ok()
        {
            let _ = stop_rx.recv_timeout(Duration::from_secs(2));
        }
    }

    let host = cpal::default_host();
    let (device, fallback) = match requested.as_deref() {
        Some(name) => match find_input_device_by_name(&host, name) {
            Some(device) => (device, false),
            None => {
                let default = host
                    .default_input_device()
                    .ok_or_else(|| "未找到默认麦克风输入设备".to_string())?;
                (default, true)
            }
        },
        None => {
            let default = host
                .default_input_device()
                .ok_or_else(|| "未找到默认麦克风输入设备".to_string())?;
            (default, false)
        }
    };
    let resolved_device_name = if fallback { None } else { requested.clone() };
    let config = device
        .default_input_config()
        .map_err(|e| format!("读取麦克风配置失败: {e}"))?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels().max(1) as usize;
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<BackendMicCommand>();
    let mic = state.backend_mic.clone();
    std::thread::spawn(move || {
        let stream = match build_backend_mic_stream(mic.clone(), &device, &config) {
            Ok(stream) => stream,
            Err(err) => {
                dlog!("[backend-mic] {err}");
                if let Ok(mut guard) = mic.lock() {
                    guard.worker = None;
                    guard.sample_rate = 0;
                    guard.channels = 0;
                }
                return;
            }
        };
        if let Err(err) = stream.play() {
            dlog!("[backend-mic] 启动麦克风输入流失败: {err}");
            if let Ok(mut guard) = mic.lock() {
                guard.worker = None;
                guard.sample_rate = 0;
                guard.channels = 0;
            }
            return;
        }
        dlog!(
            "[backend-mic] worker 已启动 sample_rate={sample_rate} channels={channels}"
        );
        let mut stop_reply: Option<std::sync::mpsc::Sender<()>> = None;
        while let Ok(command) = worker_rx.recv() {
            match command {
                BackendMicCommand::Attach {
                    session_id,
                    tx,
                    reply,
                } => {
                    let result = (|| {
                        let mut guard = mic
                            .lock()
                            .map_err(|_| "Backend mic lock failed".to_string())?;
                        guard.session_id = Some(session_id);
                        guard.tx = Some(tx.clone());
                        let mut flushed = 0usize;
                        flushed += flush_backend_mic_buffer(&mut guard)?;
                        while let Some(samples) = guard.pending.pop_front() {
                            tx.send(AsrStreamInput::RawF32(samples))
                                .map_err(|_| "ASR stream channel closed".to_string())?;
                            flushed += 1;
                        }
                        Ok(BackendMicAttachResponse {
                            flushed_chunks: flushed,
                        })
                    })();
                    let _ = reply.send(result);
                }
                BackendMicCommand::Pause { reply } => {
                    let result = (|| {
                        let mut guard = mic
                            .lock()
                            .map_err(|_| "Backend mic lock failed".to_string())?;
                        let flushed = flush_backend_mic_buffer(&mut guard)?;
                        guard.session_id = None;
                        guard.tx = None;
                        guard.pending.clear();
                        Ok(flushed)
                    })();
                    let _ = reply.send(result);
                }
                BackendMicCommand::Stop { reply } => {
                    stop_reply = reply;
                    break;
                }
            }
        }
        drop(stream);
        if let Ok(mut guard) = mic.lock() {
            guard.worker = None;
            guard.sample_rate = 0;
            guard.channels = 0;
            guard.session_id = None;
            guard.tx = None;
            guard.pending.clear();
            guard.buffer.clear();
            guard.chunk_count = 0;
            guard.current_device = None;
        }
        dlog!("[backend-mic] worker 已停止");
        if let Some(reply) = stop_reply {
            let _ = reply.send(());
        }
    });

    let mut guard = state
        .backend_mic
        .lock()
        .map_err(|_| "Backend mic lock failed".to_string())?;
    guard.worker = Some(worker_tx);
    guard.sample_rate = sample_rate;
    guard.channels = channels;
    guard.session_id = None;
    guard.tx = None;
    guard.pending.clear();
    guard.buffer.clear();
    guard.chunk_count = 0;
    guard.current_device = resolved_device_name.clone();
    dlog!("[backend-mic] 已启动后端麦克风 sample_rate={sample_rate} channels={channels} device={resolved_device_name:?}");
    Ok(BackendMicStartResponse {
        sample_rate,
        channels,
        reused: false,
        device_name: resolved_device_name,
        fallback,
    })
}

#[tauri::command]
pub(crate) fn attach_backend_mic_to_asr(
    session_id: String,
    state: tauri::State<'_, RuntimeState>,
) -> Result<BackendMicAttachResponse, String> {
    let tx = {
        let guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard
            .get(&session_id)
            .ok_or_else(|| "ASR stream not found".to_string())?
            .tx
            .clone()
    };

    let worker = {
        let guard = state
            .backend_mic
            .lock()
            .map_err(|_| "Backend mic lock failed".to_string())?;
        guard
            .worker
            .clone()
            .ok_or_else(|| "后端麦克风未启动".to_string())?
    };

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    worker
        .send(BackendMicCommand::Attach {
            session_id,
            tx,
            reply: reply_tx,
        })
        .map_err(|_| "后端麦克风线程已停止".to_string())?;
    reply_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "后端麦克风绑定超时".to_string())?
}

#[tauri::command]
pub(crate) fn pause_backend_mic(state: tauri::State<'_, RuntimeState>) -> Result<(), String> {
    let worker = {
        let guard = state
            .backend_mic
            .lock()
            .map_err(|_| "Backend mic lock failed".to_string())?;
        guard.worker.clone()
    };
    if let Some(worker) = worker {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        worker
            .send(BackendMicCommand::Pause { reply: reply_tx })
            .map_err(|_| "后端麦克风线程已停止".to_string())?;
        let flushed = reply_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "后端麦克风暂停超时".to_string())??;
        dlog!("[backend-mic] 已暂停并 flush {flushed} 块尾部音频");
    }
    let mut guard = state
        .backend_mic
        .lock()
        .map_err(|_| "Backend mic lock failed".to_string())?;
    guard.session_id = None;
    guard.tx = None;
    guard.pending.clear();
    guard.buffer.clear();
    Ok(())
}

#[tauri::command]
pub(crate) fn release_backend_mic(state: tauri::State<'_, RuntimeState>) -> Result<(), String> {
    let worker = {
        let mut guard = state
            .backend_mic
            .lock()
            .map_err(|_| "Backend mic lock failed".to_string())?;
        guard.worker.take()
    };
    if let Some(worker) = worker {
        let _ = worker.send(BackendMicCommand::Stop { reply: None });
    }
    let mut guard = state
        .backend_mic
        .lock()
        .map_err(|_| "Backend mic lock failed".to_string())?;
    guard.session_id = None;
    guard.tx = None;
    guard.pending.clear();
    guard.sample_rate = 0;
    guard.channels = 0;
    guard.chunk_count = 0;
    guard.current_device = None;
    dlog!("[backend-mic] 已释放后端麦克风");
    Ok(())
}


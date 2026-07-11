use crate::desktop::backend_mic::{
    flush_backend_mic_buffer, interleaved_to_mono_f32_from_f32, interleaved_to_mono_f32_from_i16,
    interleaved_to_mono_f32_from_u16, push_backend_mic_samples,
};
use crate::prelude::*;
use crate::state::*;

/// 系统音频采集：把播放设备（output device）当输入设备打开，cpal 的 WASAPI 后端
/// 会自动切到 loopback 模式，采集到的就是该设备正在播放的声音（见 cpal wasapi 模块文档）。
/// 除了设备来源换成 `output_devices()`，worker 线程/attach/pause 状态机与
/// `backend_mic.rs` 完全一致，复用同一套 `BackendMicState`/`BackendMicCommand` 类型和
/// `flush_backend_mic_buffer` 辅助函数。
fn find_output_device_by_name(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.output_devices()
        .ok()?
        .find(|device| device.name().map(|n| n == name).unwrap_or(false))
}

fn build_backend_system_audio_stream(
    system_audio: Arc<Mutex<BackendMicState>>,
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
) -> Result<cpal::Stream, String> {
    let stream_config: cpal::StreamConfig = config.clone().into();
    let channels = stream_config.channels.max(1) as usize;
    let err_fn = |err| dlog!("[backend-system-audio] 输入流错误: {err}");

    match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    push_backend_mic_samples(
                        &system_audio,
                        interleaved_to_mono_f32_from_f32(data, channels),
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("创建系统音频 loopback 输入流失败: {e}")),
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    push_backend_mic_samples(
                        &system_audio,
                        interleaved_to_mono_f32_from_i16(data, channels),
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("创建系统音频 loopback 输入流失败: {e}")),
        cpal::SampleFormat::U16 => device
            .build_input_stream(
                &stream_config,
                move |data: &[u16], _| {
                    push_backend_mic_samples(
                        &system_audio,
                        interleaved_to_mono_f32_from_u16(data, channels),
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("创建系统音频 loopback 输入流失败: {e}")),
        sample_format => Err(format!("不支持的系统音频采样格式: {sample_format:?}")),
    }
}

#[tauri::command]
pub(crate) fn start_backend_system_audio(
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
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
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

    let previous_worker = {
        let mut guard = state
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
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
        Some(name) => match find_output_device_by_name(&host, name) {
            Some(device) => (device, false),
            None => {
                let default = host
                    .default_output_device()
                    .ok_or_else(|| "未找到默认播放设备".to_string())?;
                (default, true)
            }
        },
        None => {
            let default = host
                .default_output_device()
                .ok_or_else(|| "未找到默认播放设备".to_string())?;
            (default, false)
        }
    };
    let resolved_device_name = if fallback { None } else { requested.clone() };
    let config = device
        .default_output_config()
        .map_err(|e| format!("读取系统音频输出配置失败: {e}"))?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels().max(1) as usize;
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<BackendMicCommand>();
    let system_audio = state.backend_system_audio.clone();
    std::thread::spawn(move || {
        let stream = match build_backend_system_audio_stream(system_audio.clone(), &device, &config)
        {
            Ok(stream) => stream,
            Err(err) => {
                dlog!("[backend-system-audio] {err}");
                if let Ok(mut guard) = system_audio.lock() {
                    guard.worker = None;
                    guard.sample_rate = 0;
                    guard.channels = 0;
                }
                return;
            }
        };
        if let Err(err) = stream.play() {
            dlog!("[backend-system-audio] 启动系统音频 loopback 流失败: {err}");
            if let Ok(mut guard) = system_audio.lock() {
                guard.worker = None;
                guard.sample_rate = 0;
                guard.channels = 0;
            }
            return;
        }
        dlog!("[backend-system-audio] worker 已启动 sample_rate={sample_rate} channels={channels}");
        let mut stop_reply: Option<std::sync::mpsc::Sender<()>> = None;
        while let Ok(command) = worker_rx.recv() {
            match command {
                BackendMicCommand::Attach {
                    session_id,
                    tx,
                    reply,
                } => {
                    let result = (|| {
                        let mut guard = system_audio
                            .lock()
                            .map_err(|_| "Backend system audio lock failed".to_string())?;
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
                BackendMicCommand::AttachRaw { tx, reply } => {
                    let result = (|| {
                        let mut guard = system_audio
                            .lock()
                            .map_err(|_| "Backend system audio lock failed".to_string())?;
                        guard.raw_txs.push(tx);
                        Ok(BackendMicAttachResponse { flushed_chunks: 0 })
                    })();
                    let _ = reply.send(result);
                }
                BackendMicCommand::Pause { reply } => {
                    let result = (|| {
                        let mut guard = system_audio
                            .lock()
                            .map_err(|_| "Backend system audio lock failed".to_string())?;
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
        if let Ok(mut guard) = system_audio.lock() {
            guard.worker = None;
            guard.sample_rate = 0;
            guard.channels = 0;
            guard.session_id = None;
            guard.tx = None;
            guard.raw_txs.clear();
            guard.pending.clear();
            guard.buffer.clear();
            guard.chunk_count = 0;
            guard.current_device = None;
        }
        dlog!("[backend-system-audio] worker 已停止");
        if let Some(reply) = stop_reply {
            let _ = reply.send(());
        }
    });

    let mut guard = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?;
    guard.worker = Some(worker_tx);
    guard.sample_rate = sample_rate;
    guard.channels = channels;
    guard.session_id = None;
    guard.tx = None;
    guard.pending.clear();
    guard.buffer.clear();
    guard.chunk_count = 0;
    guard.last_rms = 0.0;
    guard.current_device = resolved_device_name.clone();
    dlog!(
        "[backend-system-audio] 已启动系统音频采集 sample_rate={sample_rate} channels={channels} device={resolved_device_name:?}"
    );
    Ok(BackendMicStartResponse {
        sample_rate,
        channels,
        reused: false,
        device_name: resolved_device_name,
        fallback,
    })
}

#[tauri::command]
pub(crate) fn attach_backend_system_audio_raw_capture(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<BackendMicAttachResponse, String> {
    let worker = {
        let guard = state
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        guard
            .worker
            .clone()
            .ok_or_else(|| "系统音频采集未启动".to_string())?
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    worker
        .send(BackendMicCommand::AttachRaw {
            tx,
            reply: reply_tx,
        })
        .map_err(|_| "系统音频采集线程已停止".to_string())?;
    let response = reply_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "系统音频采集绑定超时".to_string())??;

    tauri::async_runtime::spawn(async move {
        while let Some(input) = rx.recv().await {
            if let AsrStreamInput::RawF32(samples) = input {
                let _ = app.emit("backend-system-audio-raw-chunk", encode_f32_base64(&samples));
            }
        }
        let _ = app.emit("backend-system-audio-raw-ended", ());
    });

    Ok(response)
}

#[tauri::command]
pub(crate) fn attach_backend_system_audio_to_asr(
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
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        guard
            .worker
            .clone()
            .ok_or_else(|| "系统音频采集未启动".to_string())?
    };

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    worker
        .send(BackendMicCommand::Attach {
            session_id,
            tx,
            reply: reply_tx,
        })
        .map_err(|_| "系统音频采集线程已停止".to_string())?;
    reply_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "系统音频采集绑定超时".to_string())?
}

#[tauri::command]
pub(crate) fn pause_backend_system_audio(
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let worker = {
        let guard = state
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        guard.worker.clone()
    };
    if let Some(worker) = worker {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        worker
            .send(BackendMicCommand::Pause { reply: reply_tx })
            .map_err(|_| "系统音频采集线程已停止".to_string())?;
        let flushed = reply_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "系统音频采集暂停超时".to_string())??;
        dlog!("[backend-system-audio] 已暂停并 flush {flushed} 块尾部音频");
    }
    let mut guard = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?;
    guard.session_id = None;
    guard.tx = None;
    guard.pending.clear();
    guard.buffer.clear();
    Ok(())
}

#[tauri::command]
pub(crate) fn release_backend_system_audio(
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    let worker = {
        let mut guard = state
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        guard.worker.take()
    };
    if let Some(worker) = worker {
        let _ = worker.send(BackendMicCommand::Stop { reply: None });
    }
    let mut guard = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?;
    guard.session_id = None;
    guard.tx = None;
    guard.raw_txs.clear();
    guard.pending.clear();
    guard.sample_rate = 0;
    guard.channels = 0;
    guard.chunk_count = 0;
    guard.current_device = None;
    guard.last_rms = 0.0;
    dlog!("[backend-system-audio] 已释放系统音频采集");
    Ok(())
}

#[tauri::command]
pub(crate) fn get_backend_system_audio_level(
    state: tauri::State<'_, RuntimeState>,
) -> Result<f32, String> {
    let guard = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?;
    Ok(guard.last_rms)
}

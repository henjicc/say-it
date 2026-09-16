use crate::desktop::backend_mic::{
    flush_backend_mic_buffer, interleaved_to_mono_f32_from_f32, interleaved_to_mono_f32_from_i16,
    interleaved_to_mono_f32_from_u16, push_backend_mic_samples, report_backend_mic_capture_error,
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
    worker: std::sync::mpsc::Sender<BackendMicCommand>,
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
) -> Result<cpal::Stream, String> {
    let stream_config: cpal::StreamConfig = config.clone().into();
    let channels = stream_config.channels.max(1) as usize;
    let capture_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // 流意外停止（拔掉/禁用播放设备、被独占模式抢占、驱动重置）必须上报给 worker，
    // 否则 worker 会一直阻塞在 recv()、raw_txs 里的 sender 不被 drop，字幕的 raw
    // consumer 永远挂着：界面仍显示「运行中」但字幕永久定格，AudioOwner::Subtitles
    // 租约也不释放，用户连听写都启动不了。此前这里只有一行 dlog，
    // 于是 worker 的 CaptureError 分支在 Windows 上根本不可达。
    let error_callback = || {
        let worker = worker.clone();
        let capture_failed = capture_failed.clone();
        move |error: cpal::StreamError| {
            let message = format!("系统音频 loopback 输入流意外停止：{error}");
            dlog!("[backend-system-audio] {message}");
            report_backend_mic_capture_error(&worker, &capture_failed, message);
        }
    };

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
                error_callback(),
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
                error_callback(),
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
                error_callback(),
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
    start_backend_system_audio_inner(device_name, &state)
}

pub(crate) fn start_backend_system_audio_inner(
    device_name: Option<String>,
    state: &RuntimeState,
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
    let worker_for_stream = worker_tx.clone();
    // 与 backend_mic 相同的启动握手。
    //
    // 原来 worker 句柄是在 `thread::spawn` **之后**才写进共享状态的，于是建流失败时
    // 线程里的「清空 worker」可能先执行、随后又被调用方无条件覆写成 Some(..)：
    // `start_backend_system_audio_inner` 照样返回成功，真实的 cpal 错误只进了 dlog，
    // 而音频一帧都不会来。更糟的是那条失败线程的收尾可能晚于下一次成功启动，
    // 把新会话的 worker 一并抹掉。
    let (startup_tx, startup_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    {
        let mut guard = system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        guard.worker = Some(worker_tx.clone());
        guard.sample_rate = sample_rate;
        guard.channels = channels;
        guard.session_id = None;
        guard.tx = None;
        guard.raw_txs.clear();
        guard.pending.clear();
        guard.buffer.clear();
        guard.chunk_count = 0;
        guard.last_rms = 0.0;
        guard.last_error = None;
        guard.current_device = resolved_device_name.clone();
    }
    std::thread::spawn(move || {
        let stream = match build_backend_system_audio_stream(
            system_audio.clone(),
            worker_for_stream,
            &device,
            &config,
        ) {
            Ok(stream) => stream,
            Err(err) => {
                dlog!("[backend-system-audio] {err}");
                if let Ok(mut guard) = system_audio.lock() {
                    guard.last_error = Some(err.clone());
                    guard.worker = None;
                    guard.sample_rate = 0;
                    guard.channels = 0;
                }
                let _ = startup_tx.send(Err(err));
                return;
            }
        };
        if let Err(err) = stream.play() {
            let message = format!("启动系统音频 loopback 流失败: {err}");
            dlog!("[backend-system-audio] {message}");
            if let Ok(mut guard) = system_audio.lock() {
                guard.last_error = Some(message.clone());
                guard.worker = None;
                guard.sample_rate = 0;
                guard.channels = 0;
            }
            let _ = startup_tx.send(Err(message));
            return;
        }
        let _ = startup_tx.send(Ok(()));
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
                BackendMicCommand::CaptureError { message } => {
                    if let Ok(mut guard) = system_audio.lock() {
                        guard.last_error = Some(message);
                    }
                    break;
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

    match startup_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => {
            let guard = state
                .backend_system_audio
                .lock()
                .map_err(|_| "Backend system audio lock failed".to_string())?;
            if guard.worker.is_none() {
                return Err(guard
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "系统音频 loopback 流已意外停止".into()));
            }
        }
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            let _ = worker_tx.send(BackendMicCommand::Stop { reply: None });
            let error = "启动系统音频 loopback 流超时".to_string();
            if let Ok(mut guard) = state.backend_system_audio.lock() {
                guard.worker = None;
                guard.sample_rate = 0;
                guard.channels = 0;
                guard.current_device = None;
                guard.last_error = Some(error.clone());
            }
            return Err(error);
        }
    }
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

pub(crate) fn attach_backend_system_audio_to_asr_inner(
    session_id: &str,
    state: &RuntimeState,
) -> Result<BackendMicAttachResponse, String> {
    let tx = {
        let guard = state
            .asr_streams
            .lock()
            .map_err(|_| "ASR stream lock failed".to_string())?;
        guard
            .get(session_id)
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
            session_id: session_id.to_string(),
            tx,
            reply: reply_tx,
        })
        .map_err(|_| "系统音频采集线程已停止".to_string())?;
    reply_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "系统音频采集绑定超时".to_string())?
}

/// 字幕应用服务直接消费系统 loopback PCM，避免完整音频经过 WebView。
pub(crate) fn attach_backend_system_audio_raw_inner(
    state: &RuntimeState,
) -> Result<
    (
        BackendMicAttachResponse,
        tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    ),
    String,
> {
    let worker = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?
        .worker
        .clone()
        .ok_or_else(|| "系统音频采集未启动".to_string())?;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
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
    Ok((response, rx))
}

pub(crate) fn pause_backend_system_audio_inner(state: &RuntimeState) -> Result<(), String> {
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
    release_backend_system_audio_inner(&state)
}

pub(crate) fn release_backend_system_audio_inner(state: &RuntimeState) -> Result<(), String> {
    let worker = {
        let mut guard = state
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        guard.worker.take()
    };
    if let Some(worker) = worker {
        // 必须等旧 worker 收尾完成再清状态。它的收尾块（drop(stream) 之后）是
        // **无条件**覆写 worker/sample_rate/raw_txs/current_device 等共享字段的，
        // 而 drop(stream) 在 WASAPI 下要等音频线程 join，通常十几到几十毫秒。
        // 不等的话，这段时间里若有新会话启动（连按两下听写热键即可），旧线程的
        // 收尾会把新会话的状态整个抹掉：raw_txs 被清空导致「麦克风采集已意外停止」，
        // 或 worker 被置 None 导致「后端麦克风未启动」；更糟的是新会话那条 cpal
        // 流由新 worker 持有而 sender 已被抹掉，谁也停不掉它，麦克风一直开着。
        // `reply` 正是为此设计的（见 state.rs 中 BackendMicCommand::Stop 的注释），
        // macOS 的系统音频路径早已这么做，这里补齐。
        let (reply, receiver) = std::sync::mpsc::channel();
        let _ = worker.send(BackendMicCommand::Stop { reply: Some(reply) });
        let _ = receiver.recv_timeout(Duration::from_secs(5));
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

#[cfg(test)]
mod tests {
    /// 系统音频的启动必须和麦克风一样：先把 worker 写进共享状态，再起线程，然后等
    /// 启动握手。
    ///
    /// 原来 worker 句柄在 `thread::spawn` **之后**才写入：建流失败时线程里的「清空
    /// worker」可能先跑，随后被调用方无条件覆写成 `Some(..)`，于是 inner 返回成功而
    /// 音频一帧都不会来，真实的 cpal 错误只进了 dlog；那条失败线程的收尾还可能晚于
    /// 下一次成功启动，把新会话的 worker 一并抹掉。
    ///
    /// 这条路径需要真实的 WASAPI loopback 设备，只能做源码契约校验。
    #[test]
    fn system_audio_start_registers_the_worker_before_spawning() {
        // 归一化行尾：按 core.autocrlf 检出时工作区是 CRLF，含 \n 的切片会失配。
        let source = include_str!("backend_system_audio.rs").replace("\r\n", "\n");
        let body = &source[..source
            .find("#[cfg(test)]")
            .expect("backend_system_audio.rs 必须有测试模块标记")];
        let start = body
            .find("pub(crate) fn start_backend_system_audio_inner")
            .expect("启动函数必须仍然存在");
        let command = &body[start..];
        let command = &command[..command.find("\n}\n").expect("函数体未闭合")];

        let register_at = command
            .find("guard.worker = Some(worker_tx.clone());")
            .expect("必须把 worker 句柄写进共享状态");
        let spawn_at = command
            .find("std::thread::spawn")
            .expect("必须仍然在独立线程里跑 worker");
        let handshake_at = command
            .find("startup_rx.recv_timeout")
            .expect("必须等待启动握手，否则建流失败也会返回成功");

        assert!(
            register_at < spawn_at,
            "worker 句柄必须在起线程之前登记，否则失败线程的清理会被调用方覆写"
        );
        assert!(
            spawn_at < handshake_at,
            "握手必须发生在起线程之后"
        );
        assert!(
            !command.contains("guard.worker = Some(worker_tx);"),
            "不得在起线程之后再无条件覆写 worker 句柄"
        );
    }
}

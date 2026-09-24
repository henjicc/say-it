use crate::prelude::*;
use crate::state::*;

const BACKEND_MIC_CHUNK_FRAMES: usize = 4096;

fn rms_f32(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples.iter().map(|s| s * s).sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}

pub(crate) fn push_backend_mic_samples(mic: &Arc<Mutex<BackendMicState>>, input: Vec<f32>) {
    if input.is_empty() {
        return;
    }
    let Ok(mut guard) = mic.lock() else {
        return;
    };
    guard.last_rms = rms_f32(&input);
    guard
        .raw_txs
        .retain(|subscriber| !subscriber.tx.is_closed());
    if !guard
        .raw_txs
        .iter()
        .any(|subscriber| subscriber.preroll == AsrPreroll::Enabled)
    {
        guard.pending.clear();
    }
    if guard.tx.is_none() && guard.session_id.is_none() && guard.raw_txs.is_empty() {
        return;
    }
    // 不把整个设备输入块先塞入 buffer 再反复 drain 搬移；只拼齐当前 4096 帧。
    let mut remaining = input.as_slice();
    while !remaining.is_empty() {
        if guard.buffer.capacity() < BACKEND_MIC_CHUNK_FRAMES {
            let additional = BACKEND_MIC_CHUNK_FRAMES - guard.buffer.len();
            guard.buffer.reserve_exact(additional);
        }
        let take = (BACKEND_MIC_CHUNK_FRAMES - guard.buffer.len()).min(remaining.len());
        guard.buffer.extend_from_slice(&remaining[..take]);
        remaining = &remaining[take..];
        if guard.buffer.len() < BACKEND_MIC_CHUNK_FRAMES {
            break;
        }
        let chunk = std::mem::take(&mut guard.buffer);
        guard.chunk_count += 1;
        let preroll = guard
            .raw_txs
            .iter()
            .any(|subscriber| subscriber.preroll == AsrPreroll::Enabled);
        let keep_original = guard.tx.is_some() || preroll;
        let (chunk, _) = fanout_raw(&mut guard.raw_txs, chunk, keep_original);
        let Some(mut chunk) = chunk else {
            continue;
        };
        if let Some(tx) = guard.tx.as_ref() {
            match tx.send(AsrStreamInput::RawF32(chunk)) {
                Ok(()) => continue,
                Err(error) => {
                    let AsrStreamInput::RawF32(samples) = error.0 else {
                        unreachable!()
                    };
                    chunk = samples;
                    guard.tx = None;
                    guard.session_id = None;
                }
            }
        }
        // 只有会重新绑定直接 ASR 的监视消费者需要预录音；文件/调校/对比已收到所有样本。
        if guard
            .raw_txs
            .iter()
            .any(|subscriber| subscriber.preroll == AsrPreroll::Enabled)
        {
            guard.pending.push_back(chunk);
            while guard.pending.len() > 240 {
                guard.pending.pop_front();
            }
        }
    }
}

fn fanout_raw(
    subscribers: &mut Vec<BackendMicRawSubscriber>,
    samples: Vec<f32>,
    keep_original: bool,
) -> (Option<Vec<f32>>, bool) {
    let mut samples = Some(samples);
    let mut delivered = false;
    let count = subscribers.len();
    let mut index = 0;
    subscribers.retain(|subscriber| {
        index += 1;
        // 无直接 ASR / 预录音时，将原分配交给最后的消费者，省去一份整块复制。
        let packet = if !keep_original && index == count {
            samples.take().unwrap()
        } else {
            samples.as_ref().unwrap().clone()
        };
        let sent = subscriber.tx.send(AsrStreamInput::RawF32(packet)).is_ok();
        delivered |= sent;
        sent
    });
    (samples, delivered)
}

/// 把攒着的音频送进 ASR 流。
///
/// 顺序很重要：`pending` 里装的是**更早**采集、因为当时还没有下游而被缓存下来的完整块，
/// `buffer` 是**此刻**这一块还没攒够的残段。原实现先发 `buffer` 再发 `pending`，等于把
/// 音频倒着送给识别侧。静音自动断开（默认 `dictationSilenceDisconnectMs = 5000`）之后
/// 重连必定走到这条路径：说一句 → 静音 5 秒 → 再开口，新句子的开头会被排在那段预滚
/// 之前，识别结果因此错乱。
pub(crate) fn flush_backend_mic_buffer(guard: &mut BackendMicState) -> Result<usize, String> {
    let mut flushed = 0usize;
    // 预滚块只补给 ASR 通道：raw_txs 在采集时就已经收到过它们了。
    while let Some(samples) = guard.pending.pop_front() {
        if let Some(tx) = guard.tx.as_ref() {
            tx.send(AsrStreamInput::RawF32(samples))
                .map_err(|_| "ASR stream channel closed".to_string())?;
            flushed += 1;
        }
    }
    if !guard.buffer.is_empty() {
        let chunk = std::mem::take(&mut guard.buffer);
        guard.chunk_count += 1;
        let keep_original = guard.tx.is_some();
        let (chunk, mut delivered) = fanout_raw(&mut guard.raw_txs, chunk, keep_original);
        if let (Some(tx), Some(chunk)) = (guard.tx.as_ref(), chunk) {
            tx.send(AsrStreamInput::RawF32(chunk))
                .map_err(|_| "ASR stream channel closed".to_string())?;
            delivered = true;
        }
        if delivered {
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

/// 向 worker 上报采集失败，同一条流只报一次。
/// 系统音频 loopback（backend_system_audio.rs）复用同一套 worker 状态机，因此共用它。
pub(crate) fn report_backend_mic_capture_error(
    worker: &std::sync::mpsc::Sender<BackendMicCommand>,
    capture_failed: &std::sync::atomic::AtomicBool,
    message: String,
) {
    if !capture_failed.swap(true, std::sync::atomic::Ordering::AcqRel) {
        let _ = worker.send(BackendMicCommand::CaptureError { message });
    }
}

pub(crate) fn build_backend_mic_stream(
    mic: Arc<Mutex<BackendMicState>>,
    worker: std::sync::mpsc::Sender<BackendMicCommand>,
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
) -> Result<cpal::Stream, String> {
    let stream_config: cpal::StreamConfig = config.clone().into();
    let channels = stream_config.channels.max(1) as usize;
    let capture_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let error_callback = || {
        let worker = worker.clone();
        let capture_failed = capture_failed.clone();
        move |error: cpal::StreamError| {
            let message = format!("麦克风输入流意外停止：{error}");
            dlog!("[backend-mic] {message}");
            report_backend_mic_capture_error(&worker, &capture_failed, message);
        }
    };

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
                error_callback(),
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
                error_callback(),
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
                error_callback(),
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
    let lease = {
        let mut current = state
            .legacy_audio_lease
            .lock()
            .map_err(|_| "音频会话锁失败")?;
        if current.is_none() {
            *current = Some(
                state
                    .audio_session
                    .acquire(crate::application::audio_session::AudioOwner::Legacy)?,
            );
        }
        current.clone()
    };
    match start_backend_mic_inner(device_name, &state) {
        Ok(response) => Ok(response),
        Err(error) => {
            if let Some(lease) = lease {
                let _ = state.audio_session.release(&lease);
            }
            if let Ok(mut current) = state.legacy_audio_lease.lock() {
                *current = None;
            }
            Err(error)
        }
    }
}

pub(crate) fn start_backend_mic_inner(
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
    let worker_for_stream = worker_tx.clone();
    let (startup_tx, startup_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let mic = state.backend_mic.clone();
    {
        let mut guard = mic
            .lock()
            .map_err(|_| "Backend mic lock failed".to_string())?;
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
        let stream =
            match build_backend_mic_stream(mic.clone(), worker_for_stream, &device, &config) {
                Ok(stream) => stream,
                Err(err) => {
                    dlog!("[backend-mic] {err}");
                    if let Ok(mut guard) = mic.lock() {
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
            let message = format!("启动麦克风输入流失败: {err}");
            dlog!("[backend-mic] {message}");
            if let Ok(mut guard) = mic.lock() {
                guard.last_error = Some(message.clone());
                guard.worker = None;
                guard.sample_rate = 0;
                guard.channels = 0;
            }
            let _ = startup_tx.send(Err(message));
            return;
        }
        let _ = startup_tx.send(Ok(()));
        dlog!("[backend-mic] worker 已启动 sample_rate={sample_rate} channels={channels}");
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
                BackendMicCommand::AttachRaw {
                    tx,
                    preroll,
                    reply,
                } => {
                    let result = (|| {
                        let mut guard = mic
                            .lock()
                            .map_err(|_| "Backend mic lock failed".to_string())?;
                        guard.raw_txs.push(BackendMicRawSubscriber { tx, preroll });
                        Ok(BackendMicAttachResponse { flushed_chunks: 0 })
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
                        guard.raw_txs.clear();
                        guard.pending.clear();
                        Ok(flushed)
                    })();
                    let _ = reply.send(result);
                }
                BackendMicCommand::CaptureError { message } => {
                    if let Ok(mut guard) = mic.lock() {
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
        if let Ok(mut guard) = mic.lock() {
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
        dlog!("[backend-mic] worker 已停止");
        if let Some(reply) = stop_reply {
            let _ = reply.send(());
        }
    });

    match startup_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => {
            let guard = state
                .backend_mic
                .lock()
                .map_err(|_| "Backend mic lock failed".to_string())?;
            if guard.worker.is_none() {
                return Err(guard
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "麦克风输入流已意外停止".into()));
            }
        }
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            let _ = worker_tx.send(BackendMicCommand::Stop { reply: None });
            let error = "启动麦克风输入流超时".to_string();
            if let Ok(mut guard) = state.backend_mic.lock() {
                guard.worker = None;
                guard.sample_rate = 0;
                guard.channels = 0;
                guard.current_device = None;
                guard.last_error = Some(error.clone());
            }
            return Err(error);
        }
    }
    dlog!("[backend-mic] 已启动后端麦克风 sample_rate={sample_rate} channels={channels} device={resolved_device_name:?}");
    Ok(BackendMicStartResponse {
        sample_rate,
        channels,
        reused: false,
        device_name: resolved_device_name,
        fallback,
    })
}

/// 应用服务直接消费原始 PCM，避免完整音频经过 WebView 事件往返。
pub(crate) fn attach_backend_mic_raw_inner(
    state: &RuntimeState,
    preroll: AsrPreroll,
) -> Result<
    (
        BackendMicAttachResponse,
        tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    ),
    String,
> {
    let worker = state
        .backend_mic
        .lock()
        .map_err(|_| "Backend mic lock failed".to_string())?
        .worker
        .clone()
        .ok_or_else(|| "后端麦克风未启动".to_string())?;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    worker
        .send(BackendMicCommand::AttachRaw {
            preroll,
            tx,
            reply: reply_tx,
        })
        .map_err(|_| "后端麦克风线程已停止".to_string())?;
    let response = reply_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "后端麦克风绑定超时".to_string())??;
    Ok((response, rx))
}

pub(crate) fn attach_backend_mic_to_asr_inner(
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
            session_id: session_id.to_string(),
            tx,
            reply: reply_tx,
        })
        .map_err(|_| "后端麦克风线程已停止".to_string())?;
    reply_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "后端麦克风绑定超时".to_string())?
}

pub(crate) fn pause_backend_mic_inner(state: &RuntimeState) -> Result<(), String> {
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
    guard.raw_txs.clear();
    guard.pending.clear();
    guard.buffer.clear();
    Ok(())
}

#[tauri::command]
pub(crate) fn release_backend_mic(state: tauri::State<'_, RuntimeState>) -> Result<(), String> {
    release_backend_mic_inner(&state)?;
    if let Some(lease) = state
        .legacy_audio_lease
        .lock()
        .map_err(|_| "音频会话锁失败")?
        .take()
    {
        state.audio_session.release(&lease)?;
    }
    Ok(())
}

pub(crate) fn release_backend_mic_inner(state: &RuntimeState) -> Result<(), String> {
    let worker = {
        let mut guard = state
            .backend_mic
            .lock()
            .map_err(|_| "Backend mic lock failed".to_string())?;
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
        .backend_mic
        .lock()
        .map_err(|_| "Backend mic lock failed".to_string())?;
    guard.session_id = None;
    guard.tx = None;
    guard.raw_txs.clear();
    guard.pending.clear();
    guard.sample_rate = 0;
    guard.channels = 0;
    guard.chunk_count = 0;
    guard.current_device = None;
    guard.last_rms = 0.0;
    guard.last_error = None;
    dlog!("[backend-mic] 已释放后端麦克风");
    Ok(())
}

#[tauri::command]
pub(crate) fn get_backend_mic_level(state: tauri::State<'_, RuntimeState>) -> Result<f32, String> {
    let guard = state
        .backend_mic
        .lock()
        .map_err(|_| "Backend mic lock failed".to_string())?;
    Ok(guard.last_rms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_sends_partial_tail_to_raw_subscribers() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut state = BackendMicState {
            raw_txs: vec![BackendMicRawSubscriber {
                tx,
                preroll: AsrPreroll::Enabled,
            }],
            buffer: vec![0.25, -0.5, 0.75],
            ..Default::default()
        };

        assert_eq!(flush_backend_mic_buffer(&mut state).unwrap(), 1);
        assert!(state.buffer.is_empty());
        match rx.try_recv().unwrap() {
            AsrStreamInput::RawF32(samples) => {
                assert_eq!(samples, vec![0.25, -0.5, 0.75]);
            }
            _ => panic!("expected raw PCM tail"),
        }
    }

    #[test]
    fn flush_does_not_replay_pending_chunks_to_raw_subscribers() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut state = BackendMicState {
            raw_txs: vec![BackendMicRawSubscriber {
                tx,
                preroll: AsrPreroll::Enabled,
            }],
            pending: VecDeque::from([vec![0.25, -0.5]]),
            ..Default::default()
        };

        assert_eq!(flush_backend_mic_buffer(&mut state).unwrap(), 0);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    /// 预滚块（更早采集）必须排在残块（此刻还没攒够的那一段）之前。
    ///
    /// 原实现先发 `buffer` 再发 `pending`，等于把音频倒着送进 ASR。静音自动断开
    /// 默认 5 秒，说一句 → 静音 → 再开口就会走到这条路径，新句子的开头被排到
    /// 上一段预滚之前，识别结果错乱。
    #[test]
    fn flush_replays_pending_chunks_before_the_partial_tail() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut state = BackendMicState {
            tx: Some(tx),
            // 时间顺序：早 → 晚
            pending: VecDeque::from([vec![1.0], vec![2.0]]),
            buffer: vec![3.0],
            ..Default::default()
        };

        assert_eq!(flush_backend_mic_buffer(&mut state).unwrap(), 3);

        let mut order = vec![];
        while let Ok(AsrStreamInput::RawF32(samples)) = rx.try_recv() {
            order.extend(samples);
        }
        assert_eq!(
            order,
            vec![1.0, 2.0, 3.0],
            "必须按采集先后送出，残块排在所有预滚块之后"
        );
        assert!(state.pending.is_empty());
        assert!(state.buffer.is_empty());
    }

    #[test]
    fn capture_error_is_forwarded_only_once() {
        let (worker, receiver) = std::sync::mpsc::channel();
        let capture_failed = std::sync::atomic::AtomicBool::new(false);

        report_backend_mic_capture_error(&worker, &capture_failed, "设备已断开".into());
        report_backend_mic_capture_error(&worker, &capture_failed, "重复错误".into());

        match receiver.recv().unwrap() {
            BackendMicCommand::CaptureError { message } => assert_eq!(message, "设备已断开"),
            _ => panic!("expected capture error"),
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }
}

#[cfg(test)]
mod capture_tests;

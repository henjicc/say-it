use crate::desktop::backend_mic::{flush_backend_mic_buffer, push_backend_mic_samples};
use crate::prelude::*;
use crate::state::*;
use std::ffi::{c_char, c_void, CStr};

const SYSTEM_AUDIO_SAMPLE_RATE: u32 = 16_000;

struct MacSystemAudioCapture {
    handle: usize,
    context: *mut MacSystemAudioContext,
}

struct MacSystemAudioContext {
    state: Arc<Mutex<BackendMicState>>,
    worker: std::sync::mpsc::Sender<BackendMicCommand>,
}

unsafe impl Send for MacSystemAudioCapture {}

impl Drop for MacSystemAudioCapture {
    fn drop(&mut self) {
        crate::macos_native::stop_system_audio(self.handle);
        unsafe {
            drop(Box::from_raw(self.context));
        }
    }
}

unsafe extern "C" fn receive_system_audio(context: *mut c_void, samples: *const f32, count: usize) {
    if context.is_null() || samples.is_null() || count == 0 {
        return;
    }
    let state = &(*(context as *const MacSystemAudioContext)).state;
    let samples = std::slice::from_raw_parts(samples, count).to_vec();
    push_backend_mic_samples(state, samples);
}

unsafe extern "C" fn receive_system_audio_error(
    context: *mut c_void,
    message: *const c_char,
) {
    if context.is_null() {
        return;
    }
    let context = &*(context as *const MacSystemAudioContext);
    let message = if message.is_null() {
        "macOS 系统音频采集意外停止".to_string()
    } else {
        CStr::from_ptr(message).to_string_lossy().into_owned()
    };
    let _ = context.worker.send(BackendMicCommand::CaptureError {
        message: format!("macOS 系统音频采集已停止：{message}"),
    });
}

fn start_native_capture(
    state: Arc<Mutex<BackendMicState>>,
    worker: std::sync::mpsc::Sender<BackendMicCommand>,
) -> Result<MacSystemAudioCapture, String> {
    let context = Box::into_raw(Box::new(MacSystemAudioContext { state, worker }));
    let handle = match unsafe {
        crate::macos_native::start_system_audio(
            receive_system_audio,
            receive_system_audio_error,
            context.cast(),
        )
    } {
        Ok(handle) => handle,
        Err(error) => {
            unsafe { drop(Box::from_raw(context)) };
            return Err(error);
        }
    };
    Ok(MacSystemAudioCapture { handle, context })
}

#[tauri::command]
pub(crate) fn start_backend_system_audio(
    device_name: Option<String>,
    state: tauri::State<'_, RuntimeState>,
) -> Result<BackendMicStartResponse, String> {
    start_backend_system_audio_inner(device_name, &state)
}

pub(crate) fn start_backend_system_audio_inner(
    _device_name: Option<String>,
    state: &RuntimeState,
) -> Result<BackendMicStartResponse, String> {
    {
        let guard = state
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        if guard.worker.is_some() {
            return Ok(BackendMicStartResponse {
                sample_rate: guard.sample_rate,
                channels: guard.channels,
                reused: true,
                device_name: None,
                fallback: false,
            });
        }
    }

    let previous_worker = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?
        .worker
        .take();
    if let Some(worker) = previous_worker {
        let (reply, receiver) = std::sync::mpsc::channel();
        let _ = worker.send(BackendMicCommand::Stop { reply: Some(reply) });
        let _ = receiver.recv_timeout(Duration::from_secs(5));
    }

    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<BackendMicCommand>();
    let capture = start_native_capture(
        Arc::clone(&state.backend_system_audio),
        worker_tx.clone(),
    )?;
    {
        let mut guard = state
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        guard.worker = Some(worker_tx.clone());
        guard.sample_rate = SYSTEM_AUDIO_SAMPLE_RATE;
        guard.channels = 1;
        guard.session_id = None;
        guard.tx = None;
        guard.raw_txs.clear();
        guard.pending.clear();
        guard.buffer.clear();
        guard.chunk_count = 0;
        guard.last_rms = 0.0;
        guard.last_error = None;
        guard.current_device = None;
    }

    let system_audio = Arc::clone(&state.backend_system_audio);
    let spawn_result = std::thread::Builder::new()
        .name("macos-system-audio".into())
        .spawn(move || {
            let _capture = capture;
            let mut stop_reply = None;
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
                            let mut flushed = flush_backend_mic_buffer(&mut guard)?;
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
                        let result = system_audio
                            .lock()
                            .map_err(|_| "Backend system audio lock failed".to_string())
                            .map(|mut guard| {
                                guard.raw_txs.push(tx);
                                BackendMicAttachResponse { flushed_chunks: 0 }
                            });
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
                            guard.raw_txs.clear();
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
            drop(_capture);
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
                guard.last_rms = 0.0;
            }
            if let Some(reply) = stop_reply {
                let _ = reply.send(());
            }
        });
    if let Err(error) = spawn_result {
        let mut guard = state
            .backend_system_audio
            .lock()
            .map_err(|_| "Backend system audio lock failed".to_string())?;
        guard.worker = None;
        guard.sample_rate = 0;
        guard.channels = 0;
        return Err(format!("启动 macOS 系统音频工作线程失败：{error}"));
    }

    Ok(BackendMicStartResponse {
        sample_rate: SYSTEM_AUDIO_SAMPLE_RATE,
        channels: 1,
        reused: false,
        device_name: None,
        fallback: false,
    })
}

pub(crate) fn attach_backend_system_audio_to_asr_inner(
    session_id: &str,
    state: &RuntimeState,
) -> Result<BackendMicAttachResponse, String> {
    let tx = state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .get(session_id)
        .ok_or_else(|| "ASR stream not found".to_string())?
        .tx
        .clone();
    let worker = system_audio_worker(state)?;
    let (reply, receiver) = std::sync::mpsc::channel();
    worker
        .send(BackendMicCommand::Attach {
            session_id: session_id.to_string(),
            tx,
            reply,
        })
        .map_err(|_| "系统音频采集线程已停止".to_string())?;
    receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "系统音频采集绑定超时".to_string())?
}

pub(crate) fn attach_backend_system_audio_raw_inner(
    state: &RuntimeState,
) -> Result<
    (
        BackendMicAttachResponse,
        tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    ),
    String,
> {
    let worker = system_audio_worker(state)?;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (reply, receiver) = std::sync::mpsc::channel();
    worker
        .send(BackendMicCommand::AttachRaw { tx, reply })
        .map_err(|_| "系统音频采集线程已停止".to_string())?;
    let response = receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "系统音频采集绑定超时".to_string())??;
    Ok((response, rx))
}

pub(crate) fn pause_backend_system_audio_inner(state: &RuntimeState) -> Result<(), String> {
    let worker = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?
        .worker
        .clone();
    if let Some(worker) = worker {
        let (reply, receiver) = std::sync::mpsc::channel();
        worker
            .send(BackendMicCommand::Pause { reply })
            .map_err(|_| "系统音频采集线程已停止".to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "系统音频采集暂停超时".to_string())??;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn release_backend_system_audio(
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    release_backend_system_audio_inner(&state)
}

pub(crate) fn release_backend_system_audio_inner(state: &RuntimeState) -> Result<(), String> {
    let worker = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?
        .worker
        .take();
    if let Some(worker) = worker {
        let (reply, receiver) = std::sync::mpsc::channel();
        let _ = worker.send(BackendMicCommand::Stop { reply: Some(reply) });
        let _ = receiver.recv_timeout(Duration::from_secs(5));
    }
    let mut guard = state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?;
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
    guard.last_rms = 0.0;
    guard.last_error = None;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_backend_system_audio_level(
    state: tauri::State<'_, RuntimeState>,
) -> Result<f32, String> {
    Ok(state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?
        .last_rms)
}

fn system_audio_worker(
    state: &RuntimeState,
) -> Result<std::sync::mpsc::Sender<BackendMicCommand>, String> {
    state
        .backend_system_audio
        .lock()
        .map_err(|_| "Backend system audio lock failed".to_string())?
        .worker
        .clone()
        .ok_or_else(|| "系统音频采集未启动".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn native_stop_error_is_forwarded_to_worker() {
        let (worker, receiver) = std::sync::mpsc::channel();
        let context = Box::into_raw(Box::new(MacSystemAudioContext {
            state: Arc::new(Mutex::new(BackendMicState::default())),
            worker,
        }));
        let message = CString::new("显示器已断开").unwrap();

        unsafe { receive_system_audio_error(context.cast(), message.as_ptr()) };

        match receiver.recv().unwrap() {
            BackendMicCommand::CaptureError { message } => {
                assert_eq!(message, "macOS 系统音频采集已停止：显示器已断开");
            }
            _ => panic!("expected capture error"),
        }
        unsafe { drop(Box::from_raw(context)) };
    }
}

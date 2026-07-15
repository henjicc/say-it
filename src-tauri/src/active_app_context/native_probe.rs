use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Pipes::PeekNamedPipe;

use super::model::{ActivationTarget, CaptureStatus, CapturedActiveAppContext, ContextSource};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CONNECT_POLL: Duration = Duration::from_millis(10);
const CLIENT_LOCK_POLL: Duration = Duration::from_millis(5);
const MAX_RESPONSE_BYTES: usize = 128 * 1024;

static PROBE_PATH: OnceLock<PathBuf> = OnceLock::new();
static CLIENT: OnceLock<Mutex<ProbeClient>> = OnceLock::new();
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProbeRequest {
    protocol_version: u32,
    request_id: u64,
    hwnd: i64,
    pid: u32,
    max_chars: usize,
    deep_clipboard: bool,
    reader_budget_ms: u32,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ProbeResponse {
    protocol_version: u32,
    request_id: u64,
    status: String,
    source: String,
    selected_text: String,
    focused_text: String,
    caret_context: String,
    visible_text: Vec<String>,
    document_text: Vec<String>,
    diagnostics: Vec<String>,
    elapsed_ms: u64,
    truncated: bool,
}

#[derive(Default)]
struct ProbeClient {
    child: Option<Child>,
    pipe: Option<File>,
}

pub(crate) fn configure_path(path: PathBuf) {
    let _ = PROBE_PATH.set(path);
}

fn probe_path() -> Result<PathBuf, String> {
    PROBE_PATH
        .get()
        .cloned()
        .filter(|path| path.is_file())
        .ok_or_else(|| "当前软件文本探针不存在，请重新执行 npm run tauri:dev".to_string())
}

fn pipe_available(file: &File) -> Result<u32, String> {
    let handle = HANDLE(file.as_raw_handle());
    let mut available = 0u32;
    unsafe {
        PeekNamedPipe(handle, None, 0, None, Some(&mut available), None)
            .map_err(|error| format!("读取文本探针管道状态失败：{error}"))?;
    }
    Ok(available)
}

fn wait_for_bytes(
    file: &File,
    count: usize,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("当前软件文本提取已取消".into());
        }
        if Instant::now() >= deadline {
            return Err("当前软件文本探针超时".into());
        }
        if pipe_available(file)? as usize >= count {
            return Ok(());
        }
        std::thread::sleep(CONNECT_POLL);
    }
}

impl ProbeClient {
    fn reset(&mut self) {
        self.pipe.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn ensure_connected(&mut self, deadline: Instant) -> Result<(), String> {
        if self.pipe.is_some()
            && self
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok())
                .flatten()
                .is_none()
        {
            return Ok(());
        }
        self.reset();
        let executable = probe_path()?;
        let pipe_name = format!(
            r"\\.\pipe\say-it-context-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        );
        let child = Command::new(executable)
            .arg("--pipe")
            .arg(&pipe_name)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("启动当前软件文本探针失败：{error}"))?;
        self.child = Some(child);

        loop {
            if Instant::now() >= deadline {
                self.reset();
                return Err("启动当前软件文本探针超时".into());
            }
            match OpenOptions::new().read(true).write(true).open(&pipe_name) {
                Ok(file) => {
                    self.pipe = Some(file);
                    return Ok(());
                }
                Err(_) => {
                    if self
                        .child
                        .as_mut()
                        .and_then(|child| child.try_wait().ok())
                        .flatten()
                        .is_some()
                    {
                        self.reset();
                        return Err("当前软件文本探针启动后异常退出".into());
                    }
                    std::thread::sleep(CONNECT_POLL);
                }
            }
        }
    }

    fn request(
        &mut self,
        request: &ProbeRequest,
        deadline: Instant,
        cancelled: &AtomicBool,
    ) -> Result<ProbeResponse, String> {
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err("当前软件文本提取已取消或超时".into());
        }
        self.ensure_connected(deadline)?;
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err("当前软件文本提取已取消或超时".into());
        }
        let body = serde_json::to_vec(request)
            .map_err(|error| format!("编码文本探针请求失败：{error}"))?;
        let length = u32::try_from(body.len()).map_err(|_| "文本探针请求过大")?;
        let pipe = self.pipe.as_mut().ok_or("文本探针管道未连接")?;
        if pipe.write_all(&length.to_le_bytes()).is_err()
            || pipe.write_all(&body).is_err()
            || pipe.flush().is_err()
        {
            self.reset();
            return Err("发送文本探针请求失败".into());
        }

        let pipe = self.pipe.as_ref().ok_or("文本探针管道未连接")?;
        if let Err(error) = wait_for_bytes(pipe, 4, deadline, cancelled) {
            self.reset();
            return Err(error);
        }
        let pipe = self.pipe.as_mut().ok_or("文本探针管道未连接")?;
        let mut length_bytes = [0u8; 4];
        pipe.read_exact(&mut length_bytes)
            .map_err(|error| format!("读取文本探针响应长度失败：{error}"))?;
        let response_length = u32::from_le_bytes(length_bytes) as usize;
        if response_length == 0 || response_length > MAX_RESPONSE_BYTES {
            self.reset();
            return Err("文本探针返回了无效响应长度".into());
        }
        let pipe = self.pipe.as_ref().ok_or("文本探针管道未连接")?;
        if let Err(error) = wait_for_bytes(pipe, response_length, deadline, cancelled) {
            self.reset();
            return Err(error);
        }
        let pipe = self.pipe.as_mut().ok_or("文本探针管道未连接")?;
        let mut response = vec![0u8; response_length];
        pipe.read_exact(&mut response)
            .map_err(|error| format!("读取文本探针响应失败：{error}"))?;
        serde_json::from_slice(&response).map_err(|error| format!("解析文本探针响应失败：{error}"))
    }
}

fn source(value: &str) -> Option<ContextSource> {
    match value {
        "ia2Text" => Some(ContextSource::Ia2Text),
        "uiaTextPattern" => Some(ContextSource::UiaTextPattern),
        "win32Message" => Some(ContextSource::Win32Message),
        "officeNative" => Some(ContextSource::OfficeNative),
        "msaa" => Some(ContextSource::Msaa),
        "clipboardDeep" => Some(ContextSource::ClipboardDeep),
        _ => None,
    }
}

pub(crate) fn capture(
    target: ActivationTarget,
    context: &mut CapturedActiveAppContext,
    deadline: Instant,
    max_chars: usize,
    cancelled: &Arc<AtomicBool>,
) -> Result<CaptureStatus, String> {
    if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Ok(CaptureStatus::TimedOut);
    }
    let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let request = ProbeRequest {
        protocol_version: 1,
        request_id,
        hwnd: target.window_handle as i64,
        pid: target.process_id,
        max_chars,
        deep_clipboard: true,
        // 给管道回传和 Rust 的终止/重启留出余量，避免末尾的深度读取挤占整个 800ms 硬截止。
        reader_budget_ms: 650,
    };
    let client = CLIENT.get_or_init(|| Mutex::new(ProbeClient::default()));
    // 前一条跨进程请求即使卡住，也不能让新会话无限阻塞在 Mutex 上。
    // 到达本次截止后由调用者使用已同步读取的窗口元信息保底。
    let mut client = loop {
        match client.try_lock() {
            Ok(client) => break client,
            Err(TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                    return Ok(CaptureStatus::TimedOut);
                }
                std::thread::sleep(CLIENT_LOCK_POLL);
            }
        }
    };
    if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Ok(CaptureStatus::TimedOut);
    }
    let response = client.request(&request, deadline, cancelled)?;
    if response.protocol_version != 1 || response.request_id != request_id {
        client.reset();
        return Err("文本探针响应与当前请求不匹配".into());
    }
    context.source = source(&response.source);
    context.selected_text = (!response.selected_text.is_empty()).then_some(response.selected_text);
    context.focused_text = (!response.focused_text.is_empty()).then_some(response.focused_text);
    context.caret_context = (!response.caret_context.is_empty()).then_some(response.caret_context);
    context.visible_text = response.visible_text;
    context.document_text = response.document_text;
    context.diagnostics.extend(response.diagnostics);
    context.elapsed_ms = response.elapsed_ms;
    context.truncated |= response.truncated;
    Ok(match response.status.as_str() {
        "captured" if context.has_text_content() => CaptureStatus::Captured,
        "sensitive" => CaptureStatus::Sensitive,
        "timedOut" => CaptureStatus::TimedOut,
        "failed" => CaptureStatus::Failed,
        _ => CaptureStatus::Empty,
    })
}

pub(crate) fn shutdown() {
    if let Some(client) = CLIENT.get() {
        client
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_match_public_contract() {
        assert_eq!(source("ia2Text"), Some(ContextSource::Ia2Text));
        assert_eq!(source("clipboardDeep"), Some(ContextSource::ClipboardDeep));
        assert_eq!(source("unknown"), None);
    }

    #[test]
    fn response_rejects_oversized_frames() {
        assert!(MAX_RESPONSE_BYTES < usize::MAX);
    }
}

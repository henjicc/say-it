use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, Lines,
};
use tokio::net::UnixListener;
use tokio::process::{Child, ChildStderr};

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::state::*;

#[derive(Debug, Deserialize)]
struct HelperEvent {
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    final_result: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    locale: String,
    #[serde(default)]
    backend: String,
    #[serde(default)]
    process_id: i32,
}

type DynReader = Box<dyn AsyncRead + Unpin + Send>;
type DynWriter = Box<dyn AsyncWrite + Unpin + Send>;

struct SessionTransport {
    writer: Option<DynWriter>,
    lines: Lines<BufReader<DynReader>>,
    child: Option<Child>,
    stderr: Option<ChildStderr>,
}

pub(super) async fn start_apple_speech_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
    model: String,
    input_sample_rate: u32,
    params: Option<DspParams>,
) -> Result<AsrStreamStartResponse, String> {
    let mut capability = crate::providers::apple_speech::refresh_status();
    if !capability.identity_valid {
        return Err(if capability.message.trim().is_empty() {
            "Apple 语音识别助手缺少 macOS 权限身份，请重新构建开发版或重新安装应用".into()
        } else {
            capability.message
        });
    }
    if !capability.available {
        return Err(if capability.message.trim().is_empty() {
            "当前设备或系统语言不支持 Apple 纯本地语音识别".into()
        } else {
            capability.message
        });
    }
    if matches!(capability.authorization.as_str(), "denied" | "restricted") {
        return Err(if capability.message.trim().is_empty() {
            "请在 macOS“系统设置 → 隐私与安全性 → 语音识别”中允许“说吧！”".into()
        } else {
            capability.message
        });
    }
    if !capability.installed {
        capability = crate::providers::apple_speech::prepare()
            .await
            .map_err(|error| format!("macOS 自动准备本地语音识别资源失败：{error}"))?;
    }
    if !capability.available || !capability.installed {
        return Err("macOS 未能准备当前系统语言的本地语音识别资源".into());
    }

    let session_id = Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
    state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .insert(session_id.clone(), AsrStreamHandle { tx });

    let streams = state.asr_streams.clone();
    let task_id = session_id.clone();
    tauri::async_runtime::spawn(run_apple_session(
        app,
        task_id,
        streams,
        rx,
        StreamDsp::new(params.unwrap_or_default(), input_sample_rate),
        model,
    ));
    Ok(AsrStreamStartResponse { session_id })
}

async fn run_apple_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut dsp: StreamDsp,
    model: String,
) {
    let transport = match open_transport(OUTPUT_RATE).await {
        Ok(transport) => transport,
        Err(error) => {
            emit_asr_stream_event(&app, &session_id, "error", json!({ "message": error }));
            cleanup_stream(&streams, &session_id);
            return;
        }
    };
    let SessionTransport {
        mut writer,
        mut lines,
        mut child,
        mut stderr,
    } = transport;
    let mut opened = false;
    let mut terminal_event = false;
    let mut stopped = false;
    let mut helper_pid = 0;

    loop {
        tokio::select! {
            input = rx.recv() => match input {
                Some(AsrStreamInput::RawF32(samples)) => {
                    let pcm = dsp.process(&samples);
                    if pcm.is_empty() { continue; }
                    let bytes = pcm16_as_f32_bytes(&pcm);
                    let Some(channel) = writer.as_mut() else { continue; };
                    if let Err(error) = channel.write_all(&bytes).await {
                        emit_asr_stream_event(
                            &app,
                            &session_id,
                            "error",
                            json!({ "message": format!("发送音频到 Apple 系统本地识别失败：{error}") }),
                        );
                        terminal_event = true;
                        break;
                    }
                }
                Some(AsrStreamInput::Finish) => {
                    if let Some(mut channel) = writer.take() {
                        let _ = channel.shutdown().await;
                    }
                }
                Some(AsrStreamInput::Stop) | None => {
                    stopped = true;
                    writer.take();
                    if let Some(process) = child.as_mut() {
                        let _ = process.kill().await;
                    } else if helper_pid > 0 {
                        terminate_process(helper_pid);
                    }
                    break;
                }
            },
            line = lines.next_line() => match line {
                Ok(Some(line)) => match parse_helper_event(&line) {
                    Ok(event) => match event.kind.as_str() {
                        "connected" => helper_pid = event.process_id,
                        "opened" => {
                            opened = true;
                            emit_asr_stream_event(
                                &app,
                                &session_id,
                                "opened",
                                json!({
                                    "message": "Apple system speech opened",
                                    "model": model,
                                    "locale": event.locale,
                                    "backend": event.backend,
                                    "onDevice": true
                                }),
                            );
                        }
                        "result" if !event.text.is_empty() => emit_asr_stream_event(
                            &app,
                            &session_id,
                            "result",
                            json!({ "text": event.text, "final": event.final_result }),
                        ),
                        "finish" => {
                            terminal_event = true;
                            emit_asr_stream_event(&app, &session_id, "finish", json!({}));
                            break;
                        }
                        "error" => {
                            terminal_event = true;
                            emit_asr_stream_event(
                                &app,
                                &session_id,
                                "error",
                                json!({ "message": event.message }),
                            );
                            break;
                        }
                        _ => {}
                    },
                    Err(error) => {
                        terminal_event = true;
                        emit_asr_stream_event(
                            &app,
                            &session_id,
                            "error",
                            json!({ "message": error }),
                        );
                        break;
                    }
                },
                Ok(None) => break,
                Err(error) => {
                    terminal_event = true;
                    emit_asr_stream_event(
                        &app,
                        &session_id,
                        "error",
                        json!({ "message": format!("读取 Apple 系统本地识别结果失败：{error}") }),
                    );
                    break;
                }
            }
        }
    }

    writer.take();
    let exit_status = if let Some(process) = child.as_mut() {
        process.wait().await.ok()
    } else {
        None
    };
    if !stopped && !terminal_event {
        let mut detail = String::new();
        if let Some(mut stderr) = stderr.take() {
            let _ = stderr.read_to_string(&mut detail).await;
        }
        let message = if !detail.trim().is_empty() {
            detail.trim().to_string()
        } else if !opened {
            match exit_status.as_ref() {
                Some(status) if !status.success() => format!(
                    "Apple 系统本地识别助手被 macOS 异常终止（{status}）。请重新构建开发版或重新安装应用；若问题仍在，请检查“隐私与安全性 → 语音识别”权限"
                ),
                _ => "Apple 系统本地识别未能启动或开发通信已断开".to_string(),
            }
        } else {
            format!("Apple 系统本地识别意外结束（{exit_status:?}）")
        };
        emit_asr_stream_event(&app, &session_id, "error", json!({ "message": message }));
    }
    cleanup_stream(&streams, &session_id);
    emit_asr_stream_event(
        &app,
        &session_id,
        "ended",
        json!({ "message": "Apple system speech ended" }),
    );
}

async fn open_transport(sample_rate: u32) -> Result<SessionTransport, String> {
    if crate::providers::apple_speech::uses_development_bundle() {
        return open_development_transport(sample_rate).await;
    }
    let mut command = crate::providers::apple_speech::command(sample_rate);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("启动 Apple 系统本地识别失败：{error}"))?;
    let writer = child
        .stdin
        .take()
        .ok_or_else(|| "Apple 系统本地识别标准输入不可用".to_string())?;
    let reader = child
        .stdout
        .take()
        .ok_or_else(|| "Apple 系统本地识别标准输出不可用".to_string())?;
    let stderr = child.stderr.take();
    Ok(SessionTransport {
        writer: Some(Box::new(writer)),
        lines: BufReader::new(Box::new(reader) as DynReader).lines(),
        child: Some(child),
        stderr,
    })
}

async fn open_development_transport(sample_rate: u32) -> Result<SessionTransport, String> {
    let socket_path = PathBuf::from(format!("/tmp/sayit-asr-{}.sock", Uuid::new_v4().simple()));
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| format!("创建 Apple 开发语音通道失败：{error}"))?;
    let bundle = crate::providers::apple_speech::development_bundle_path();
    let output = tokio::process::Command::new("/usr/bin/open")
        .args(["-n", "-g"])
        .arg(&bundle)
        .args(["--args", "--socket"])
        .arg(&socket_path)
        .args(["--sample-rate", &sample_rate.to_string()])
        .output()
        .await
        .map_err(|error| format!("启动 Apple 开发语音助手失败：{error}"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&socket_path);
        return Err(format!(
            "启动 Apple 开发语音助手失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let accepted = tokio::time::timeout(Duration::from_secs(15), listener.accept()).await;
    let _ = std::fs::remove_file(&socket_path);
    let (stream, _) = accepted
        .map_err(|_| "等待 Apple 开发语音助手连接超时".to_string())?
        .map_err(|error| format!("Apple 开发语音助手连接失败：{error}"))?;
    let (reader, writer) = stream.into_split();
    Ok(SessionTransport {
        writer: Some(Box::new(writer)),
        lines: BufReader::new(Box::new(reader) as DynReader).lines(),
        child: None,
        stderr: None,
    })
}

fn parse_helper_event(line: &str) -> Result<HelperEvent, String> {
    #[derive(Deserialize)]
    struct WireEvent {
        kind: String,
        #[serde(default)]
        text: String,
        #[serde(default, rename = "final")]
        final_result: bool,
        #[serde(default)]
        message: String,
        #[serde(default)]
        locale: String,
        #[serde(default)]
        backend: String,
        #[serde(default, rename = "processId")]
        process_id: i32,
    }
    let event: WireEvent = serde_json::from_str(line)
        .map_err(|error| format!("解析 Apple 系统本地识别事件失败：{error}"))?;
    Ok(HelperEvent {
        kind: event.kind,
        text: event.text,
        final_result: event.final_result,
        message: event.message,
        locale: event.locale,
        backend: event.backend,
        process_id: event.process_id,
    })
}

fn terminate_process(process_id: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    const SIGTERM: i32 = 15;
    if process_id > 0 {
        unsafe {
            let _ = kill(process_id, SIGTERM);
        }
    }
}

fn pcm16_as_f32_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len() * 2);
    for sample in bytes.chunks_exact(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]) as f32 / i16::MAX as f32;
        output.extend_from_slice(&value.to_le_bytes());
    }
    output
}

fn cleanup_stream(streams: &Arc<Mutex<HashMap<String, AsrStreamHandle>>>, session_id: &str) {
    if let Ok(mut streams) = streams.lock() {
        streams.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_and_final_events() {
        let partial =
            parse_helper_event(r#"{"kind":"result","text":"你好","final":false}"#).unwrap();
        assert_eq!(partial.text, "你好");
        assert!(!partial.final_result);

        let final_result = parse_helper_event(
            r#"{"kind":"result","text":"你好。","final":true,"backend":"SFSpeechRecognizer"}"#,
        )
        .unwrap();
        assert!(final_result.final_result);
        assert_eq!(final_result.backend, "SFSpeechRecognizer");

        let connected = parse_helper_event(r#"{"kind":"connected","processId":1234}"#).unwrap();
        assert_eq!(connected.process_id, 1234);
    }

    #[test]
    fn converts_pcm16_to_little_endian_f32() {
        let bytes = pcm16_as_f32_bytes(&[0, 0, 0xff, 0x7f]);
        let values = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(values, vec![0.0, 1.0]);
    }
}

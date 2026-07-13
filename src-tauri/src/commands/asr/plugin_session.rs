use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};

use crate::commands::audio::emit_asr_stream_event;
use crate::prelude::*;
use crate::providers::plugin::{PluginProcessSpec, PROCESS_PROTOCOL_VERSION};
use crate::providers::ProviderProfile;
use crate::state::*;

const FINISH_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) async fn start_plugin_asr_stream(
    app: tauri::AppHandle,
    state: &RuntimeState,
    plugin: PluginProcessSpec,
    profile: ProviderProfile,
    model: String,
    input_sample_rate: u32,
    params: Option<DspParams>,
) -> Result<AsrStreamStartResponse, String> {
    let mut command = Command::new(&plugin.entrypoint);
    command
        .args(&plugin.args)
        .current_dir(&plugin.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("SAYIT_PLUGIN_ID", &plugin.plugin_id)
        .env(
            "SAYIT_PLUGIN_PROTOCOL",
            PROCESS_PROTOCOL_VERSION.to_string(),
        );
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动插件 {} 失败：{error}", plugin.plugin_id))?;
    let mut stdin = child.stdin.take().ok_or("插件标准输入不可用")?;
    let stdout = child.stdout.take().ok_or("插件标准输出不可用")?;
    let stderr = child.stderr.take().ok_or("插件标准错误不可用")?;
    let session_id = Uuid::new_v4().to_string();

    write_message(
        &mut stdin,
        json!({
            "type": "start",
            "protocolVersion": PROCESS_PROTOCOL_VERSION,
            "sessionId": session_id,
            "providerId": profile.id,
            "model": model,
            "sampleRate": OUTPUT_RATE,
            "config": profile.config,
            "permissions": plugin.permissions,
        }),
    )
    .await?;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<AsrStreamInput>();
    state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .insert(session_id.clone(), AsrStreamHandle { tx });

    let streams = state.asr_streams.clone();
    let task_id = session_id.clone();
    tauri::async_runtime::spawn(async move {
        let mut stderr_lines = BufReader::new(stderr).lines();
        let stderr_plugin_id = plugin.plugin_id.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(Some(line)) = stderr_lines.next_line().await {
                dlog!("[plugin {stderr_plugin_id}] {line}");
            }
        });
        run_plugin_session(
            app,
            task_id,
            streams,
            child,
            stdin,
            BufReader::new(stdout).lines(),
            rx,
            params.map(|params| StreamDsp::new(params, input_sample_rate)),
            model,
            plugin.plugin_id,
        )
        .await;
    });

    Ok(AsrStreamStartResponse { session_id })
}

#[allow(clippy::too_many_arguments)]
async fn run_plugin_session(
    app: tauri::AppHandle,
    session_id: String,
    streams: Arc<Mutex<HashMap<String, AsrStreamHandle>>>,
    mut child: tokio::process::Child,
    mut stdin: ChildStdin,
    mut stdout: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AsrStreamInput>,
    mut dsp: Option<StreamDsp>,
    model: String,
    plugin_id: String,
) {
    emit_asr_stream_event(
        &app,
        &session_id,
        "opened",
        json!({ "message": "plugin process opened", "model": model, "pluginId": plugin_id }),
    );
    let mut finish_sent_at: Option<Instant> = None;
    let mut stop = false;

    while !stop {
        while let Ok(command) = rx.try_recv() {
            match command {
                AsrStreamInput::RawF32(samples) => {
                    let bytes = dsp
                        .as_mut()
                        .map(|dsp| dsp.process(&samples))
                        .unwrap_or_default();
                    if !bytes.is_empty()
                        && write_message(
                            &mut stdin,
                            json!({ "type": "audio", "pcm16Base64": STANDARD.encode(bytes) }),
                        )
                        .await
                        .is_err()
                    {
                        emit_asr_stream_event(
                            &app,
                            &session_id,
                            "error",
                            json!({ "message": "向插件发送音频失败" }),
                        );
                        stop = true;
                        break;
                    }
                }
                AsrStreamInput::Finish => {
                    if let Err(error) = write_message(&mut stdin, json!({ "type": "finish" })).await
                    {
                        emit_asr_stream_event(
                            &app,
                            &session_id,
                            "error",
                            json!({ "message": error }),
                        );
                        stop = true;
                    } else {
                        finish_sent_at = Some(Instant::now());
                    }
                }
                AsrStreamInput::Stop => {
                    let _ = write_message(&mut stdin, json!({ "type": "stop" })).await;
                    stop = true;
                }
            }
        }
        if stop {
            break;
        }
        if finish_sent_at.is_some_and(|started| started.elapsed() >= FINISH_TIMEOUT) {
            emit_asr_stream_event(
                &app,
                &session_id,
                "finish_timeout",
                json!({ "message": "插件收尾超时" }),
            );
            break;
        }
        match tokio::time::timeout(Duration::from_millis(50), stdout.next_line()).await {
            Ok(Ok(Some(line))) => {
                if handle_plugin_event(&app, &session_id, &line) {
                    break;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(error)) => {
                emit_asr_stream_event(
                    &app,
                    &session_id,
                    "error",
                    json!({ "message": error.to_string() }),
                );
                break;
            }
            Err(_) => {}
        }
    }
    let _ = child.kill().await;
    if let Ok(mut streams) = streams.lock() {
        streams.remove(&session_id);
    }
    emit_asr_stream_event(
        &app,
        &session_id,
        "ended",
        json!({ "message": "plugin process ended" }),
    );
}

fn handle_plugin_event(app: &tauri::AppHandle, session_id: &str, line: &str) -> bool {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            emit_asr_stream_event(
                app,
                session_id,
                "error",
                json!({ "message": format!("插件输出不是合法 JSON：{error}") }),
            );
            return true;
        }
    };
    match value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "ready" => emit_asr_stream_event(
            app,
            session_id,
            "event",
            json!({ "message": "plugin ready" }),
        ),
        "partial" => emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": value.get("text").and_then(Value::as_str).unwrap_or_default(), "final": false }),
        ),
        "final" => emit_asr_stream_event(
            app,
            session_id,
            "result",
            json!({ "text": value.get("text").and_then(Value::as_str).unwrap_or_default(), "final": true }),
        ),
        "finished" => {
            emit_asr_stream_event(app, session_id, "finish", json!({}));
            return true;
        }
        "error" => {
            emit_asr_stream_event(
                app,
                session_id,
                "error",
                json!({
                    "code": value.get("code").and_then(Value::as_str).unwrap_or("plugin_error"),
                    "message": value.get("message").and_then(Value::as_str).unwrap_or("插件执行失败")
                }),
            );
            return true;
        }
        "event" => emit_asr_stream_event(app, session_id, "event", value),
        other => emit_asr_stream_event(
            app,
            session_id,
            "event",
            json!({ "message": "unknown plugin event", "type": other }),
        ),
    }
    false
}

async fn write_message(stdin: &mut ChildStdin, value: Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    stdin
        .write_all(&bytes)
        .await
        .map_err(|error| error.to_string())?;
    stdin.flush().await.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_protocol_messages_are_json_lines() {
        let bytes = serde_json::to_vec(&json!({ "type": "finish" })).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), r#"{"type":"finish"}"#);
    }
}

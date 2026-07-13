use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

use super::plugin::{PluginProcessSpec, PROCESS_PROTOCOL_VERSION};
use super::plugin_secrets;
use super::ProviderProfile;

pub const DEFAULT_INVOKE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub async fn invoke<F>(
    spec: &PluginProcessSpec,
    profile: &ProviderProfile,
    operation: &str,
    payload: Value,
    timeout: Duration,
    on_event: F,
) -> Result<Value, String>
where
    F: FnMut(&Value) + Send,
{
    invoke_cancellable(spec, profile, operation, payload, timeout, None, on_event).await
}

pub async fn invoke_cancellable<F>(
    spec: &PluginProcessSpec,
    profile: &ProviderProfile,
    operation: &str,
    payload: Value,
    timeout: Duration,
    cancel: Option<Arc<AtomicBool>>,
    mut on_event: F,
) -> Result<Value, String>
where
    F: FnMut(&Value) + Send,
{
    if spec.trust == "signed-untrusted" {
        return Err(format!(
            "插件 {} 的签名密钥尚未受信任，请从插件管理重新安装并确认发布者",
            spec.plugin_id
        ));
    }
    if spec.protocol_version < 2 {
        return Err(format!(
            "插件 {} 使用进程协议 v{}，不支持 {operation}",
            spec.plugin_id, spec.protocol_version
        ));
    }
    std::fs::create_dir_all(&spec.data_dir).map_err(|error| error.to_string())?;
    let session = plugin_secrets::load_session(spec)?;
    let mut command = Command::new(&spec.entrypoint);
    command
        .args(&spec.args)
        .current_dir(&spec.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("SAYIT_PLUGIN_ID", &spec.plugin_id)
        .env("SAYIT_PLUGIN_PROTOCOL", spec.protocol_version.to_string())
        .env("SAYIT_PLUGIN_DATA_DIR", &spec.data_dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动插件 {} 失败：{error}", spec.plugin_id))?;
    let mut stdin = child.stdin.take().ok_or("插件标准输入不可用")?;
    let stdout = child.stdout.take().ok_or("插件标准输出不可用")?;
    let stderr = child.stderr.take().ok_or("插件标准错误不可用")?;
    let request_id = Uuid::new_v4().to_string();
    let mut request = serde_json::to_vec(&json!({
        "type": "invoke",
        "protocolVersion": PROCESS_PROTOCOL_VERSION,
        "requestId": request_id,
        "operation": operation,
        "providerId": profile.id,
        "config": profile.config,
        "session": session,
        "permissions": spec.permissions,
        "payload": payload,
    }))
    .map_err(|error| error.to_string())?;
    request.push(b'\n');
    stdin
        .write_all(&request)
        .await
        .map_err(|error| format!("向插件发送请求失败：{error}"))?;
    stdin
        .shutdown()
        .await
        .map_err(|error| format!("关闭插件请求流失败：{error}"))?;

    let plugin_id = spec.plugin_id.clone();
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            crate::dlog!("[plugin {plugin_id}] {line}");
        }
    });

    let started = Instant::now();
    let mut lines = BufReader::new(stdout).lines();
    let result = loop {
        if cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
        {
            break Err("插件操作已取消".into());
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break Err(format!("插件操作 {operation} 超时"));
        }
        let wait = remaining.min(Duration::from_millis(100));
        let line = match tokio::time::timeout(wait, lines.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => break Err("插件在返回 completed 前关闭了输出".into()),
            Ok(Err(error)) => break Err(format!("读取插件输出失败：{error}")),
            Err(_) => continue,
        };
        let event: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => break Err(format!("插件输出不是合法 JSON：{error}")),
        };
        match event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "completed" => break Ok(event.get("result").cloned().unwrap_or(Value::Null)),
            "error" => {
                break Err(event
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("插件操作失败")
                    .to_string())
            }
            "delta" | "progress" | "event" => on_event(&event),
            other => break Err(format!("插件返回未知事件类型：{other}")),
        }
    };
    let _ = child.kill().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_protocol_is_reserved_for_v2() {
        assert_eq!(PROCESS_PROTOCOL_VERSION, 2);
        assert!(DEFAULT_INVOKE_TIMEOUT >= Duration::from_secs(60));
    }
}

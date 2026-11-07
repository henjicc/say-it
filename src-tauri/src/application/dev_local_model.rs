//! 开发构建专用：用 `工作区/smart-text-ft` 里微调过的 0.8B 模型整理同一段听写，和任选的云端模型并排对比
//! 首字、生成速度和总耗时。模型由 `工作区/llama.cpp` 的 llama-server（Vulkan）承载：打开对比页就预加载并常驻，
//! 应用正常退出时关闭；被强制结束的应用留下的进程仍占着端口，下次直接复用。加载时间不计入对比。

use crate::application::dev_bench::{self, BenchModel, RoundSample};
use crate::application::dev_trace;
use crate::state::RuntimeState;
use futures_util::future::join_all;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const PORT: u16 = 5157;
const MODEL_FILE: &str = "ft-0.8b-Q8_0.gguf";
const LOCAL_LABEL: &str = "本地微调 0.8B";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const SCOPE: &str = "dev-local-compare";
/// 训练时的输入格式，和 `工作区/smart-text-ft/scripts/common.py` 必须一致：模型只认这种输入，
/// 云端模板里的整理规则已经训进权重。
const SYSTEM_PROMPT: &str =
    "你是听写整理引擎：把语音识别的口述原文整理成用户真正想输出的文本，只输出整理结果。";
const SCENES: &[&str] = &["assistant", "code", "chat", "writing", "search", "general"];
/// 训练集按所在软件推断场景，规则同 `build_real_test.py`。
const ASSISTANT_APPS: &[&str] = &["claude", "chatgpt", "cursor"];

static SERVER: Mutex<Option<Child>> = Mutex::new(None);
static STATUS: Mutex<LoadStatus> = Mutex::new(LoadStatus::Idle);

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
enum LoadStatus {
    Idle,
    Loading,
    /// `load_ms` 为空表示服务早已在运行，这次没有加载。
    #[serde(rename_all = "camelCase")]
    Ready {
        load_ms: Option<u64>,
    },
    Failed {
        error: String,
    },
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct LocalCompareRequest {
    /// 参与对比的云端模型。
    models: Vec<BenchModel>,
    /// 是否带上本地模型。
    local: bool,
    /// 自定义文本；不给时重放最近一次走过智能处理的听写。
    text: Option<String>,
    /// 自定义文本的场景；重放时按当时所在软件推断。
    scene: Option<String>,
}

fn workspace() -> PathBuf {
    dev_trace::traces_dir()
        .parent()
        .expect("追踪目录在工作区下")
        .to_path_buf()
}

fn scene_of(app_name: &str) -> &'static str {
    if ASSISTANT_APPS.contains(&app_name.to_lowercase().as_str()) {
        "assistant"
    } else {
        "general"
    }
}

/// 训练格式还有可选的 `<terms>`，但训练集每条最多几个、真实测试集为空；全局热词动辄几十上百个，
/// 塞进去反而偏离模型见过的输入，所以不带。
fn user_message(scene: &str, raw: &str) -> String {
    format!("<scene>{scene}</scene>\n<text>{raw}</text>")
}

async fn healthy(client: &reqwest::Client) -> bool {
    let Ok(response) = client
        .get(format!("http://127.0.0.1:{PORT}/health"))
        .timeout(HEALTH_TIMEOUT)
        .send()
        .await
    else {
        return false;
    };
    response
        .json::<Value>()
        .await
        .is_ok_and(|body| body["status"] == "ok")
}

fn spawn_server() -> Result<Child, String> {
    let root = workspace();
    let model = root.join("smart-text-ft").join("gguf").join(MODEL_FILE);
    let server = root
        .join("llama.cpp")
        .join("bin-vulkan")
        .join("llama-server.exe");
    for path in [&model, &server] {
        if !path.exists() {
            return Err(format!("找不到 {}", path.display()));
        }
    }
    let mut command = Command::new(server);
    command
        .arg("-m")
        .arg(model)
        .args(["--port", &PORT.to_string(), "-c", "4096", "-ngl", "99"])
        .args(["--no-webui", "--reasoning-budget", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map_err(|error| format!("启动 llama-server 失败：{error}"))
}

fn set_status(status: LoadStatus) {
    if let Ok(mut current) = STATUS.lock() {
        *current = status;
    }
}

/// 保证服务可用，并把结果写进加载状态，页面据此显示进度。
async fn ensure_server(client: &reqwest::Client) -> Result<(), String> {
    if healthy(client).await {
        if let Ok(mut current) = STATUS.lock() {
            if !matches!(*current, LoadStatus::Ready { .. }) {
                *current = LoadStatus::Ready { load_ms: None };
            }
        }
        return Ok(());
    }
    set_status(LoadStatus::Loading);
    let result = start_server(client).await;
    set_status(match &result {
        Ok(load_ms) => LoadStatus::Ready {
            load_ms: Some(*load_ms),
        },
        Err(error) => LoadStatus::Failed {
            error: error.clone(),
        },
    });
    result.map(|_| ())
}

/// 返回加载花的时间。
async fn start_server(client: &reqwest::Client) -> Result<u64, String> {
    let started = Instant::now();
    {
        let mut server = SERVER.lock().map_err(|_| "本地模型进程锁失败")?;
        let running = server
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if !running {
            *server = Some(spawn_server()?);
        }
    }
    while started.elapsed() < STARTUP_TIMEOUT {
        if healthy(client).await {
            return Ok(started.elapsed().as_millis() as u64);
        }
        let exited = SERVER
            .lock()
            .map_err(|_| "本地模型进程锁失败")?
            .as_mut()
            .is_none_or(|child| !matches!(child.try_wait(), Ok(None)));
        if exited {
            return Err("llama-server 启动后退出了，可在终端手动运行它查看原因".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("等待本地模型加载超时".into())
}

fn local_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())
}

/// 解析 llama-server 流式响应的一行，返回这一块的正文增量和（最后一块才有的）输出 token 数。
fn parse_sse_line(line: &str) -> Result<Option<(String, Option<f64>)>, String> {
    let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else {
        return Ok(None);
    };
    if data == "[DONE]" {
        return Ok(None);
    }
    let event: Value =
        serde_json::from_str(data).map_err(|error| format!("本地模型返回无法解析：{error}"))?;
    if let Some(message) = event["error"]["message"].as_str() {
        return Err(message.to_string());
    }
    Ok(Some((
        event["choices"][0]["delta"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        event["usage"]["completion_tokens"].as_f64(),
    )))
}

async fn stream_local(client: &reqwest::Client, scene: &str, raw: &str) -> (RoundSample, String) {
    let body = json!({
        "messages":[
            {"role":"system","content":SYSTEM_PROMPT},
            {"role":"user","content":user_message(scene, raw)},
        ],
        "temperature":0,
        "max_tokens":raw.chars().count() * 2 + 64,
        "stream":true,
        "stream_options":{"include_usage":true},
    });
    let started = Instant::now();
    let mut sample = RoundSample::default();
    let mut output = String::new();
    let result = async {
        let response = client
            .post(format!("http://127.0.0.1:{PORT}/v1/chat/completions"))
            .json(&body)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {status}：{body}"));
        }
        let mut tokens = None;
        let mut buffer = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            buffer.extend_from_slice(&chunk.map_err(|error| error.to_string())?);
            while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = buffer.drain(..=end).collect::<Vec<_>>();
                let Some((delta, usage)) = parse_sse_line(&String::from_utf8_lossy(&line))? else {
                    continue;
                };
                if !delta.is_empty() {
                    output.push_str(&delta);
                    sample.record_progress(started.elapsed().as_millis() as u64, &output, "");
                }
                tokens = usage.or(tokens);
            }
        }
        Ok(tokens)
    }
    .await;
    let total_ms = started.elapsed().as_millis() as u64;
    sample.record_finish(total_ms, result.map(|tokens| (output.as_str(), "", tokens)));
    (sample, output.trim().to_string())
}

fn row(
    provider_id: &str,
    provider_name: &str,
    model_id: &str,
    measured: (RoundSample, String),
) -> Value {
    let (sample, output) = measured;
    json!({
        "providerId":provider_id,
        "providerName":provider_name,
        "modelId":model_id,
        "output":output,
        "sample":sample,
    })
}

async fn run_cloud(
    app: AppHandle,
    model: BenchModel,
    system_prompt: String,
    user_prompt: String,
) -> Value {
    let state = app.state::<RuntimeState>();
    let state = state.inner();
    let (provider_name, model_id, _) = dev_bench::model_labels(state, &model);
    let measured =
        dev_bench::measure_stream(state, &model, &system_prompt, &user_prompt, SCOPE, |_| {}).await;
    row(&model.provider_id, &provider_name, &model_id, measured)
}

pub(crate) async fn compare(
    app: &AppHandle,
    request: LocalCompareRequest,
) -> Result<Value, String> {
    if request.models.is_empty() && !request.local {
        return Err("至少选一个模型".into());
    }
    // 云端请求的宿主事件分不清归属，所以开始时避开正在记录的听写、对比测试和测速；
    // 只检查这一次，对比进行中再开始的听写可能混进这次请求的耗时，开发者自行避开。
    dev_trace::while_idle(|| {
        if dev_bench::is_running() {
            return Err("模型测速正在进行，结束后再试".to_string());
        }
        Ok(())
    })?;
    let state = app.state::<RuntimeState>();
    let (input, scene) = match request.text.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => {
            let scene = request.scene.unwrap_or_else(|| "general".into());
            if !SCENES.contains(&scene.as_str()) {
                return Err(format!("未知场景：{scene}"));
            }
            (dev_trace::input_from_text(state.inner(), text)?, scene)
        }
        _ => {
            let input = dev_trace::replay_prompt("latest")?;
            let scene = scene_of(&input.app_name).to_string();
            (input, scene)
        }
    };
    let client = local_client()?;
    // 先等本地模型就绪再同时发出所有请求，加载时间不算进对比；本地加载失败不影响云端照常对比。
    let local_ready = if request.local {
        Some(ensure_server(&client).await)
    } else {
        None
    };
    // 每个云端模型一个独立任务，原因同测速：同一任务里的同步准备工作会推迟其他模型的流。
    let cloud = request
        .models
        .into_iter()
        .map(|model| {
            tauri::async_runtime::spawn(run_cloud(
                app.clone(),
                model,
                input.system_prompt.clone(),
                input.user_prompt.clone(),
            ))
        })
        .collect::<Vec<_>>();
    let local = async {
        let measured = match local_ready? {
            Ok(()) => stream_local(&client, &scene, &input.raw_text).await,
            Err(error) => {
                let mut sample = RoundSample::default();
                sample.record_finish(0, Err(error));
                (sample, String::new())
            }
        };
        Some(row("local", LOCAL_LABEL, MODEL_FILE, measured))
    };
    let (local, cloud) = tokio::join!(local, join_all(cloud));
    let rows = local
        .into_iter()
        // 任务 panic 时这一行缺席，panic 信息已打到终端，同测速。
        .chain(cloud.into_iter().filter_map(Result::ok))
        .collect::<Vec<_>>();
    Ok(json!({
        "rawText":input.raw_text,
        "appName":input.app_name,
        "scene":scene,
        "rows":rows,
    }))
}

/// 应用退出时关掉本次启动的 llama-server，不留着占显存。
pub(crate) fn shutdown() {
    if let Ok(mut server) = SERVER.lock() {
        if let Some(mut child) = server.take() {
            let _ = child.kill();
        }
    }
}

#[tauri::command]
pub(crate) async fn dev_local_compare(
    app: AppHandle,
    request: LocalCompareRequest,
) -> Result<Value, String> {
    compare(&app, request).await
}

/// 后台开始加载本地模型，立即返回；进度用 `dev_local_status` 查。
#[tauri::command]
pub(crate) fn dev_local_preload() -> Result<(), String> {
    let client = local_client().inspect_err(|error| {
        set_status(LoadStatus::Failed {
            error: error.clone(),
        })
    })?;
    tauri::async_runtime::spawn(async move {
        // 失败已写进加载状态，页面会显示。
        let _ = ensure_server(&client).await;
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn dev_local_status() -> Value {
    STATUS
        .lock()
        .ok()
        .and_then(|status| serde_json::to_value(&*status).ok())
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_matches_training_format() {
        assert_eq!(
            user_message("assistant", "帮我看下"),
            "<scene>assistant</scene>\n<text>帮我看下</text>"
        );
    }

    #[test]
    fn sse_lines_yield_content_deltas_and_final_usage() {
        assert_eq!(
            parse_sse_line(r#"data: {"choices":[{"delta":{"content":"你好"}}]}"#).unwrap(),
            Some(("你好".into(), None))
        );
        assert_eq!(
            parse_sse_line(r#"data: {"choices":[],"usage":{"completion_tokens":12}}"#).unwrap(),
            Some((String::new(), Some(12.0)))
        );
        assert_eq!(parse_sse_line("data: [DONE]").unwrap(), None);
        assert_eq!(parse_sse_line("").unwrap(), None);
        assert!(parse_sse_line(r#"data: {"error":{"message":"超出上下文"}}"#).is_err());
    }
}

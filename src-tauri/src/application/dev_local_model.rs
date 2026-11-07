//! 开发构建专用：用 `工作区/smart-text-ft` 里微调过的 0.8B 模型整理同一段听写，和云端智能处理并排对比。
//! 模型由 `工作区/llama.cpp` 的 llama-server（Vulkan）承载：第一次对比时启动并常驻，应用正常退出时关闭；
//! 被强制结束的应用留下的进程仍占着端口，下次对比直接复用。

use crate::application::{dev_trace, smart_text};
use crate::state::RuntimeState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const PORT: u16 = 5157;
const MODEL_FILE: &str = "ft-0.8b-Q8_0.gguf";
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

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct LocalCompareRequest {
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

/// 返回这次启动花的时间；服务已在运行时返回 None。
async fn ensure_server(client: &reqwest::Client) -> Result<Option<u64>, String> {
    if healthy(client).await {
        return Ok(None);
    }
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
            return Ok(Some(started.elapsed().as_millis() as u64));
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

async fn run_local(client: &reqwest::Client, scene: &str, raw: &str) -> Value {
    let body = json!({
        "messages":[
            {"role":"system","content":SYSTEM_PROMPT},
            {"role":"user","content":user_message(scene, raw)},
        ],
        "temperature":0,
        "max_tokens":raw.chars().count() * 2 + 64,
    });
    let started = Instant::now();
    let result = async {
        let response = client
            .post(format!("http://127.0.0.1:{PORT}/v1/chat/completions"))
            .json(&body)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!("HTTP {status}：{}", body["error"]["message"]));
        }
        Ok(body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string())
    }
    .await;
    let ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(output) => json!({"output":output,"ms":ms}),
        Err(error) => json!({"error":error,"ms":ms}),
    }
}

async fn run_cloud(state: &RuntimeState, input: &dev_trace::ReplayPrompt) -> Value {
    let started = Instant::now();
    let result = smart_text::process_prompt(
        state,
        &input.system_prompt,
        &input.user_prompt,
        Some(input.provider_id.as_str()),
        Some(input.model.as_str()),
        SCOPE,
        false,
    )
    .await;
    let ms = started.elapsed().as_millis() as u64;
    let mut value = json!({"providerId":input.provider_id,"model":input.model,"ms":ms});
    match result {
        Ok(output) => value["output"] = json!(output),
        Err(error) => value["error"] = json!(dev_trace::strip_url_queries(&error)),
    }
    value
}

pub(crate) async fn compare(
    state: &RuntimeState,
    request: LocalCompareRequest,
) -> Result<Value, String> {
    // 云端请求的宿主事件分不清归属，所以开始时避开正在记录的听写、对比测试和测速；
    // 只检查这一次，对比进行中再开始的听写可能混进这次请求的耗时，开发者自行避开。
    dev_trace::while_idle(|| {
        if crate::application::dev_bench::is_running() {
            return Err("模型测速正在进行，结束后再试".to_string());
        }
        Ok(())
    })?;
    let (input, scene) = match request.text.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => {
            let scene = request.scene.unwrap_or_else(|| "general".into());
            if !SCENES.contains(&scene.as_str()) {
                return Err(format!("未知场景：{scene}"));
            }
            (dev_trace::input_from_text(state, text)?, scene)
        }
        _ => {
            let input = dev_trace::replay_prompt("latest")?;
            let scene = scene_of(&input.app_name).to_string();
            (input, scene)
        }
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())?;
    let (local, cloud) = tokio::join!(
        async {
            match ensure_server(&client).await {
                Ok(startup_ms) => {
                    let mut local = run_local(&client, &scene, &input.raw_text).await;
                    local["startupMs"] = json!(startup_ms);
                    local
                }
                Err(error) => json!({"error":error}),
            }
        },
        run_cloud(state, &input),
    );
    Ok(json!({
        "rawText":input.raw_text,
        "appName":input.app_name,
        "scene":scene,
        "model":MODEL_FILE,
        "local":local,
        "cloud":cloud,
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
    compare(app.state::<RuntimeState>().inner(), request).await
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
}

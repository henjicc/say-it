//! 本地听写整理模型。推理在独立进程 `sayit-llm` 里跑（见 `src-tauri/llm-engine`），这里管它的
//! 启动、加载、请求和回收。整理规则已经训进模型权重，输入只有场景、词表和原文，
//! 格式必须与训练时逐字一致。

use super::plugin::LocalModelSpec;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

pub(crate) const ENGINE: &str = "llama-cpp";
/// 用过之后模型留在内存里的时间。连续听写不用反复加载，闲下来就退出进程、让出显存。
const LINGER: Duration = Duration::from_secs(10 * 60);
const SYSTEM_PROMPT: &str =
    "你是听写整理引擎：把语音识别的口述原文整理成用户真正想输出的文本，只输出整理结果。";
/// 显卡上一两秒内就有结果，处理器上整理长文可能要几十秒；等这么久说明推理进程卡住了。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const PROMPT_TAIL: &str = "</text><|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n";
/// 提示词加输出的 token 上限，够整理两三千字的一段话。
const CONTEXT_TOKENS: u64 = 8192;
/// 一次前向验证的草稿 token 数。每多一个，显卡上要多留一份循环状态快照（4B 约 54 MB）；
/// 超过 7 个单次前向明显变贵，实测见 `docs/experience/本地听写整理模型的选型提速与显存实测.md`。
const DRAFT_TOKENS: u64 = 7;

static ENGINE_PATH: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn configure_engine_path(path: PathBuf) {
    let _ = ENGINE_PATH.set(path);
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PolishInput {
    /// 训练时用的场景标签，见 `scene_tag`。
    pub(crate) scene: &'static str,
    /// 热词和屏幕上出现的专有名词写法，只用来还原被听错的词。
    pub(crate) terms: Vec<String>,
    pub(crate) text: String,
}

/// 模型把 `<|...|>` 当控制符解析，听写文本和屏幕文字里出现的要拆开。
fn plain(text: &str) -> String {
    text.replace("<|", "< |")
}

impl PolishInput {
    /// 提示词分成三段：听写开始时就确定的、随口述增长的、收尾的。推理进程在前两段的末尾各留一个检查点。
    fn prompt(&self) -> (String, String) {
        let mut head = format!(
            "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\n<scene>{}</scene>\n",
            self.scene
        );
        if !self.terms.is_empty() {
            head.push_str(&format!("<terms>{}</terms>\n", plain(&self.terms.join("、"))));
        }
        head.push_str("<text>");
        (head, plain(&self.text))
    }
}

/// 应用按前台软件判定的场景 → 训练时的场景标签。终端里口述的多半是在给 AI 编程助手提需求。
pub(crate) fn scene_tag(app_scene: &str) -> &'static str {
    match app_scene {
        "AI 对话" | "终端" => "assistant",
        "聊天" => "chat",
        "写作" => "writing",
        "代码编辑器" => "code",
        _ => "general",
    }
}

const MAX_SCREEN_TERMS: usize = 40;
/// 词表总量上限。训练样本的词表最多三十来个，再长模型没见过，还会挤占上下文。
const MAX_TERMS: usize = 48;

/// 从屏幕文字里挑出带大写字母或数字的英文写法（产品名、缩写、文件名、版本号），按出现次数排在热词后面。
/// 纯小写的普通单词模型自己会写，不占词表。
pub(crate) fn collect_terms(hotwords: &[String], screen_text: &str) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    let is_word_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '#' | '_' | '-');
    for candidate in screen_text.split(|c: char| !is_word_char(c)) {
        let word = candidate.trim_matches(|c: char| matches!(c, '.' | '-' | '_' | '+' | '#'));
        let distinctive = word.chars().any(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        if !(2..=24).contains(&word.len())
            || !word.starts_with(|c: char| c.is_ascii_alphabetic())
            || !distinctive
        {
            continue;
        }
        match counts
            .iter_mut()
            .find(|(seen, _)| seen.eq_ignore_ascii_case(word))
        {
            Some((_, count)) => *count += 1,
            None => counts.push((word.to_string(), 1)),
        }
    }
    // 稳定排序：次数相同的保持在屏幕上出现的先后。
    counts.sort_by(|left, right| right.1.cmp(&left.1));
    let mut terms: Vec<String> = hotwords.to_vec();
    for (word, _) in counts.into_iter().take(MAX_SCREEN_TERMS) {
        if !terms.iter().any(|term| term.eq_ignore_ascii_case(&word)) {
            terms.push(word);
        }
    }
    terms.truncate(MAX_TERMS);
    terms
}

/// 与推理进程通信的两端：命令逐行写入，回复按命令编号送回等待者。
#[derive(Clone)]
struct Link {
    commands: mpsc::UnboundedSender<String>,
    replies: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
}

struct Process {
    child: Child,
    plugin_id: String,
    model: PathBuf,
    on_gpu: bool,
    link: Link,
}

/// 上一次整理完成的结果。同一段话越说越长时，新一轮的输出多半以它开头。
struct Memo {
    model: PathBuf,
    head: String,
    body: String,
    output: String,
}

#[derive(Default)]
struct Speculation {
    running: bool,
    latest: Option<(LocalModelSpec, PolishInput)>,
    /// 这次听写里模型没能加载。预先整理不再重试，等正式请求把错误报给用户。
    load_failed: bool,
}

struct Runtime {
    process: tokio::sync::Mutex<Option<Process>>,
    memo: Mutex<Option<Memo>>,
    speculation: Mutex<Speculation>,
    next_id: AtomicU64,
    last_used: Mutex<Instant>,
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime {
        process: tokio::sync::Mutex::new(None),
        memo: Mutex::new(None),
        speculation: Mutex::new(Speculation::default()),
        next_id: AtomicU64::new(1),
        last_used: Mutex::new(Instant::now()),
    })
}

fn model_path(spec: &LocalModelSpec) -> Result<PathBuf, String> {
    let file = spec
        .params
        .get("model")
        .and_then(Value::as_str)
        .ok_or("本地整理模型缺少模型文件")?;
    Ok(spec.model_dir.join(file))
}

/// 推理线程数。显卡上线程只负责喂数据；处理器上按物理核估算并留出一半给识别和系统。
fn inference_threads() -> usize {
    let logical = std::thread::available_parallelism().map_or(4, usize::from);
    (logical / 2).clamp(2, 8)
}

/// 发出一条命令并等它的回复。调用方中途放弃等待（用户取消、跳过、超时）时点名让推理进程停下这一条。
async fn send(link: &Link, mut command: Value) -> Result<Value, String> {
    struct Withdraw<'a> {
        link: &'a Link,
        id: u64,
        answered: bool,
    }
    impl Drop for Withdraw<'_> {
        fn drop(&mut self) {
            if !self.answered {
                if let Ok(mut replies) = self.link.replies.lock() {
                    replies.remove(&self.id);
                }
                let _ = self
                    .link
                    .commands
                    .send(json!({"op":"cancel","target":self.id}).to_string());
            }
        }
    }
    let id = runtime().next_id.fetch_add(1, Ordering::Relaxed);
    command["id"] = json!(id);
    let (sender, receiver) = oneshot::channel();
    link.replies
        .lock()
        .map_err(|_| "本地整理模型状态锁失败")?
        .insert(id, sender);
    let mut withdraw = Withdraw {
        link,
        id,
        answered: false,
    };
    link.commands
        .send(command.to_string())
        .map_err(|_| "本地整理模型进程已退出")?;
    let reply = receiver.await.map_err(|_| "本地整理模型进程已退出")?;
    withdraw.answered = true;
    match reply.get("error").and_then(Value::as_str) {
        Some(error) => Err(error.to_string()),
        None => Ok(reply),
    }
}

async fn spawn(spec: &LocalModelSpec) -> Result<Process, String> {
    let engine = ENGINE_PATH.get().ok_or("本地整理模型的推理程序未配置")?;
    let model = model_path(spec)?;
    // 推理程序要用 Vulkan 运行库（随显卡驱动安装），缺了它一初始化就会崩溃。
    #[cfg(windows)]
    if !std::env::var_os("SystemRoot")
        .is_some_and(|root| PathBuf::from(root).join("System32").join("vulkan-1.dll").is_file())
    {
        return Err("这台电脑没有 Vulkan 运行库（通常随显卡驱动安装），本地整理模型无法运行".into());
    }
    let mut command = Command::new(engine);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动本地整理模型失败：{error}"))?;
    let mut stdin = child.stdin.take().ok_or("本地整理模型进程没有输入管道")?;
    let stdout = child.stdout.take().ok_or("本地整理模型进程没有输出管道")?;
    let stderr = child.stderr.take().ok_or("本地整理模型进程没有错误输出管道")?;
    // 推理库只把警告和错误写到这里，比如显存不够时的原因。
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            crate::application::diagnostics::event("warn", "localLlm.engine", json!({"message":line}));
        }
    });
    let (commands, mut queue) = mpsc::unbounded_channel::<String>();
    tauri::async_runtime::spawn(async move {
        while let Some(line) = queue.recv().await {
            if stdin.write_all(line.as_bytes()).await.is_err()
                || stdin.write_all(b"\n").await.is_err()
                || stdin.flush().await.is_err()
            {
                break;
            }
        }
    });
    let replies: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> = Arc::default();
    let reader_replies = replies.clone();
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(reply) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let waiting = reply["id"]
                .as_u64()
                .and_then(|id| reader_replies.lock().ok()?.remove(&id));
            if let Some(waiting) = waiting {
                let _ = waiting.send(reply);
            }
        }
        // 进程退出：丢掉发送端，仍在等回复的请求随之收到「进程已退出」。
        if let Ok(mut waiting) = reader_replies.lock() {
            waiting.clear();
        }
    });
    let link = Link { commands, replies };
    let loaded = send(
        &link,
        json!({
            "op": "load",
            "model": model.to_string_lossy(),
            "gpu": true,
            "threads": inference_threads(),
            "contextTokens": CONTEXT_TOKENS,
            "draftTokens": DRAFT_TOKENS,
        }),
    )
    .await?;
    let on_gpu = loaded["gpu"].as_bool() == Some(true);
    crate::application::diagnostics::event(
        "info",
        "localLlm.loaded",
        json!({"pluginId":spec.plugin_id,"loadMs":loaded["loadMs"],"gpu":on_gpu}),
    );
    Ok(Process {
        child,
        plugin_id: spec.plugin_id.clone(),
        model,
        on_gpu,
        link,
    })
}

/// 保证进程在跑且加载的是这个模型，返回通信句柄和模型是否在显卡上。
async fn ensure_loaded(spec: &LocalModelSpec) -> Result<(Link, bool), String> {
    let mut slot = runtime().process.lock().await;
    let model = model_path(spec)?;
    let reusable = slot
        .as_mut()
        .is_some_and(|process| process.model == model && matches!(process.child.try_wait(), Ok(None)));
    if let Ok(mut last_used) = runtime().last_used.lock() {
        *last_used = Instant::now();
    }
    if !reusable {
        *slot = None;
        *slot = Some(spawn(spec).await?);
        tauri::async_runtime::spawn(release_when_idle());
    }
    let process = slot.as_ref().expect("上面刚确保进程存在");
    Ok((process.link.clone(), process.on_gpu))
}

/// 每个进程配一个：闲置满 `LINGER` 就结束它。
async fn release_when_idle() {
    loop {
        let idle = runtime()
            .last_used
            .lock()
            .map_or(LINGER, |last_used| last_used.elapsed());
        if idle < LINGER {
            tokio::time::sleep(LINGER - idle).await;
            continue;
        }
        let mut process = runtime().process.lock().await;
        // 等锁的这会儿可能刚好又有人用上了。
        let still_idle = runtime()
            .last_used
            .lock()
            .map_or(true, |last_used| last_used.elapsed() >= LINGER);
        if !still_idle {
            continue;
        }
        // 丢掉句柄即结束进程，显存随之释放。
        *process = None;
        return;
    }
}

fn shared_prefix_chars(left: &str, right: &str) -> usize {
    left.chars().zip(right.chars()).take_while(|(a, b)| a == b).count()
}

/// 返回 `None` 表示这次整理被推理进程中途放弃（预先整理遇到正式请求）。
async fn run(
    spec: &LocalModelSpec,
    input: &PolishInput,
    speculative: bool,
) -> Result<Option<String>, String> {
    let (head, body) = input.prompt();
    let model = model_path(spec)?;
    let previous = {
        let memo = runtime().memo.lock().map_err(|_| "本地整理模型状态锁失败")?;
        match memo.as_ref().filter(|memo| memo.model == model) {
            Some(memo) if memo.head == head && memo.body == body => {
                return Ok(Some(memo.output.clone()));
            }
            // 上一版原文有一半以上还在开头，才值得拿它的结果当草稿。
            Some(memo)
                if shared_prefix_chars(&memo.body, &body) * 2 >= memo.body.chars().count() =>
            {
                memo.output.clone()
            }
            _ => String::new(),
        }
    };
    let (link, _) = ensure_loaded(spec).await?;
    let reply = match tokio::time::timeout(
        REQUEST_TIMEOUT,
        send(
            &link,
            json!({
                "op": "run",
                "head": head,
                "body": body,
                "tail": PROMPT_TAIL,
                "source": body,
                "previous": previous,
                // 整理结果不会比原文长多少；留出编号和标点的余量。
                "maxTokens": input.text.chars().count() * 2 + 64,
                "speculative": speculative,
            }),
        ),
    )
    .await
    {
        Ok(reply) => reply?,
        Err(_) => {
            // 卡住的进程留着也没用，结束它，下次重新加载。
            *runtime().process.lock().await = None;
            return Err("本地整理模型超时没有返回".into());
        }
    };
    if reply["cancelled"].as_bool() == Some(true) {
        return Ok(None);
    }
    let output = reply["text"].as_str().unwrap_or_default().trim().to_string();
    crate::application::diagnostics::event(
        "debug",
        "localLlm.run",
        json!({
            "promptTokens":reply["promptTokens"],
            "promptEvaluated":reply["promptEvaluated"],
            "outputTokens":reply["outputTokens"],
            "forwardPasses":reply["forwardPasses"],
            "promptMs":reply["promptMs"],
            "generateMs":reply["generateMs"],
        }),
    );
    if let Ok(mut memo) = runtime().memo.lock() {
        *memo = Some(Memo {
            model,
            head,
            body,
            output: output.clone(),
        });
    }
    Ok(Some(output))
}

/// 整理一段听写原文。
pub(crate) async fn polish(spec: &LocalModelSpec, input: &PolishInput) -> Result<String, String> {
    // 正式请求到了，排队等着的预先整理不用再跑；正在跑的那次，推理进程收到这条命令就会停下。
    if let Ok(mut speculation) = runtime().speculation.lock() {
        speculation.latest = None;
    }
    run(spec, input, false)
        .await?
        .ok_or_else(|| "本地整理被取消".to_string())
}

/// 听写一开始就把模型加载好，说话的时间正好用来加载。
pub(crate) async fn prewarm(spec: &LocalModelSpec) -> Result<(), String> {
    if let Ok(mut speculation) = runtime().speculation.lock() {
        speculation.load_failed = false;
    }
    ensure_loaded(spec).await.map(drop)
}

/// 边说边整理已经识别出的文字，松手时上一版结果可以直接当草稿，只需补算最后一段。
/// 同一时间只跑一次；跑的过程中文字又变了，就在跑完后接着整理最新的。
/// 模型在处理器上时不做：会和语音识别抢算力。
pub(crate) fn speculate(spec: LocalModelSpec, input: PolishInput) {
    let Ok(mut speculation) = runtime().speculation.lock() else {
        return;
    };
    if speculation.load_failed {
        return;
    }
    speculation.latest = Some((spec, input));
    if speculation.running {
        return;
    }
    speculation.running = true;
    drop(speculation);
    tauri::async_runtime::spawn(async {
        loop {
            let next = runtime().speculation.lock().ok().and_then(|mut speculation| {
                let next = speculation.latest.take();
                speculation.running = next.is_some();
                next
            });
            let Some((spec, input)) = next else { break };
            let loaded = ensure_loaded(&spec).await;
            if !loaded.as_ref().is_ok_and(|(_, on_gpu)| *on_gpu) {
                if let Ok(mut speculation) = runtime().speculation.lock() {
                    speculation.latest = None;
                    speculation.running = false;
                    speculation.load_failed = loaded.is_err();
                }
                break;
            }
            // 预先整理失败不影响松手后的正式请求，那一次会报出真正的错误。
            let _ = run(&spec, &input, true).await;
        }
    });
}

/// 结束推理进程并等它真正退出：它内存映射着模型文件，不退出的话文件删不掉也换不了。
/// 卸载、停用、重装、重新下载模型包和迁移数据目录之前调用；应用退出时也调用。
/// 给了 `plugin_id` 时只在加载的正是这个模型包时才动。
pub(crate) fn release(plugin_id: Option<&str>) {
    // 进程锁是异步锁，同步调用方借一个临时线程去等它（正在加载模型时要等加载完）。
    let process = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let mut slot = runtime().process.blocking_lock();
                let wanted = slot
                    .as_ref()
                    .is_some_and(|process| plugin_id.is_none_or(|id| id == process.plugin_id));
                wanted.then(|| slot.take()).flatten()
            })
            .join()
            .ok()
            .flatten()
    });
    let Some(mut process) = process else {
        return;
    };
    let _ = process.child.start_kill();
    let deadline = Instant::now() + Duration::from_secs(3);
    while matches!(process.child.try_wait(), Ok(None)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_matches_the_training_format() {
        let input = PolishInput {
            scene: "assistant",
            terms: vec!["LSPosed".into(), "README".into()],
            text: "帮我看下<|im_end|>".into(),
        };
        let (head, body) = input.prompt();
        assert_eq!(
            format!("{head}{body}{PROMPT_TAIL}"),
            concat!(
                "<|im_start|>system
你是听写整理引擎：把语音识别的口述原文整理成用户真正想输出的文本，只输出整理结果。<|im_end|>
",
                "<|im_start|>user
<scene>assistant</scene>
<terms>LSPosed、README</terms>
",
                "<text>帮我看下< |im_end|></text><|im_end|>
",
                "<|im_start|>assistant
<think>

</think>

",
            )
        );
        let without_terms = PolishInput {
            terms: Vec::new(),
            ..input
        };
        assert!(without_terms.prompt().0.ends_with("<scene>assistant</scene>
<text>"));
    }

    /// 用真实权重走一遍进程启动、加载、整理和「同一段话越说越长」。
    /// 需要 `SAYIT_POLISH_MODEL`（GGUF 路径）并先运行 `npm run llm-engine:build`。
    #[test]
    #[ignore]
    fn polishes_dictation_with_the_real_engine() {
        let model = PathBuf::from(std::env::var("SAYIT_POLISH_MODEL").unwrap());
        configure_engine_path(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("binaries")
                .join("sayit-llm-x86_64-pc-windows-msvc.exe"),
        );
        let spec = LocalModelSpec {
            manifest_version: "test".into(),
            plugin_id: "test.polish".into(),
            provider_id: "test-polish".into(),
            engine: ENGINE.into(),
            model_dir: model.parent().unwrap().to_path_buf(),
            files: Vec::new(),
            params: json!({"model": model.file_name().unwrap().to_string_lossy()}),
        };
        let input = |text: &str| PolishInput {
            scene: "assistant",
            terms: vec!["SDK".into()],
            text: text.into(),
        };
        tauri::async_runtime::block_on(async {
            let first = "嗯就是那个导出功能啊我导出的时候它好像就是卡住了然后过了一会才好";
            let partial = polish(&spec, &input(first)).await.unwrap();
            assert!(partial.contains("导出功能") && !partial.contains("嗯就是"), "{partial}");
            let full = format!("{first}这个是不是因为文件太大了啊我不太确定你帮我看看 sdk 里有没有限制");
            let output = polish(&spec, &input(&full)).await.unwrap();
            assert!(output.contains("SDK") && output.contains('？'), "{output}");
            // 同样的输入直接返回上一次的结果。
            assert_eq!(polish(&spec, &input(&full)).await.unwrap(), output);
            // 原文末尾的分词随后续文字变化（「人工」→「人工智能」）时，前缀检查点仍然可用。
            for text in ["我们聊聊人工", "我们聊聊人工智能", "我们聊聊人工智能很厉害这件事"] {
                polish(&spec, &input(text)).await.unwrap();
            }
        });
        release(None);
    }

    #[test]
    fn screen_terms_keep_distinctive_spellings_after_hotwords() {
        let terms = collect_terms(
            &["痕迹".into(), "readme".into()],
            "open the README.md and run npm install, SDK v0.22.2 SDK LSPosed 管理器 a1",
        );
        assert_eq!(terms, ["痕迹", "readme", "SDK", "README.md", "v0.22.2", "LSPosed", "a1"]);
    }
}

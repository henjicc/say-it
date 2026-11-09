//! 开发构建专用的运行追踪。
//!
//! 每次听写写成一条 JSON 记录，放在仓库已 gitignore 的 `工作区/dev-traces/`，开发者页面和
//! 本地接口读的都是这份文件。记录里有听写原文和完整提示词，所以发布构建完全不编译本模块。

use crate::application::diagnostics;
use crate::application::smart_text;
use crate::state::RuntimeState;
use axum::extract::{Path as RoutePath, Query, Request, State as RouteState};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

/// 紧挨开发端口 5155。接口只接受 JSON 请求体，并校验 Host：跨站页面发不了请求，
/// DNS 重绑定也读不到记录，所以不需要再加令牌。
const API_PORT: u16 = 5156;
const RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const MAX_BYTES: u64 = 200 * 1024 * 1024;
const DEFAULT_LIST_LIMIT: usize = 50;
const BASELINE_SYSTEM_PROMPT: &str = "只回复一个字：好";
const BASELINE_USER_PROMPT: &str = "好";
const DICTATION: &str = "dictation";
const ASSISTANT: &str = "assistant";
const SMART_TEST: &str = "smart-test";
/// 线上智能处理的 scope；重放和统计只认它，学习总结等后台请求不算。
const SMART_SCOPE: &str = "smart-text";
/// 测试发出的请求都带这个前缀：测试被听写顶掉后，迟到的结果不能进听写记录。
const TEST_SCOPE_PREFIX: &str = "dev-";
/// JS 运行时启动完成。Groq 和插件模型每次请求都要新建一个运行时。
pub(crate) const RUNTIME_READY: &str = "trace.runtimeReady";
/// 宿主 HTTP 请求结束。
pub(crate) const HTTP_RESPONSE: &str = "trace.httpResponse";
/// 低于这个值的连接成本不值得单独当结论。
const CONNECTION_NOTABLE_MS: u64 = 200;
const SMART_PHASE: &str = "智能处理";
const CONTEXT_PHASE: &str = "等软件上下文";

struct PhaseSpec {
    name: &'static str,
    from: &'static str,
    to: &'static str,
    /// 松手之后用户在干等的阶段；录音阶段的时长由用户决定，不算慢。
    waiting: bool,
}

const fn phase(
    name: &'static str,
    from: &'static str,
    to: &'static str,
    waiting: bool,
) -> PhaseSpec {
    PhaseSpec {
        name,
        from,
        to,
        waiting,
    }
}

const PHASES: &[PhaseSpec] = &[
    phase("准备录音", "dictation.sessionStarted", "trace.recordingReady", false),
    phase("录音", "trace.recordingReady", "dictation.stopRequested", false),
    phase("等识别结果", "dictation.stopRequested", "trace.asrFinal", true),
    phase("保存原文", "trace.asrFinal", "trace.historySaved", true),
    phase(CONTEXT_PHASE, "trace.historySaved", "trace.contextReady", true),
    phase(SMART_PHASE, "trace.contextReady", "trace.smartDone", true),
    phase("本地规则与学习", "trace.smartDone", "trace.rulesDone", true),
    phase("输出到目标软件", "trace.rulesDone", "dictation.injectionFinished", true),
];

/// 提示词构成的字段与显示名。
const PROMPT_SOURCES: &[(&str, &str)] = &[
    ("templateChars", "模板"),
    ("textChars", "识别文本"),
    ("activeAppContextChars", "软件上下文"),
    ("globalContextChars", "全局上下文"),
    ("hotwordsChars", "热词"),
    ("learningChars", "个性化学习"),
];

/// 敏感输入框的听写和历史记录一样不留内容，这些键的值会被替换。
const CONTENT_KEYS: &[&str] = &["rawText", "outputText", "userPrompt", "output", "message"];

struct Run {
    id: String,
    kind: &'static str,
    started: Instant,
    started_at_ms: u64,
    timeline: Vec<Value>,
    llm: Vec<Value>,
    variant: Option<&'static str>,
    /// 时间线里已经归到上一次模型请求的条目数。
    attributed_entries: usize,
    sensitive: bool,
    /// 这次听写期间有测试或测速在跑：宿主事件分不清归属，耗时也不具代表性。
    overlapped: bool,
}

static ACTIVE: Mutex<Option<Run>> = Mutex::new(None);

fn new_run(kind: &'static str) -> Run {
    let started_at_ms = diagnostics::now_millis() as u64;
    Run {
        id: started_at_ms.to_string(),
        kind,
        started: Instant::now(),
        started_at_ms,
        timeline: Vec::new(),
        llm: Vec::new(),
        variant: None,
        attributed_entries: 0,
        sensitive: false,
        overlapped: false,
    }
}

pub(super) fn traces_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri 一定有上级目录")
        .join("工作区")
        .join("dev-traces")
}

pub(crate) fn initialize(app: &AppHandle) {
    let directory = traces_dir();
    let prepared = std::fs::create_dir_all(&directory)
        .map_err(|error| error.to_string())
        .and_then(|()| {
            diagnostics::cleanup_files(&directory, "runs-", None, RETENTION, MAX_BYTES)
        });
    if let Err(error) = prepared {
        eprintln!("[dev-trace] 准备追踪目录失败：{error}");
    }
    start_api(app.clone());
}

/// `diagnostics::event` 的旁路：开发构建里所有事件都进当前运行的时间线，不受详细日志开关影响。
pub(super) fn on_event(name: &str, metadata: &Value) {
    if name == "dictation.sessionStarted" {
        begin_dictation();
    }
    // 没有听写在跑时也可能有听写事件（例如空闲时取消），它们不属于正在跑的测试。
    if name.starts_with("dictation.") && active_kind() != Some(DICTATION) {
        return;
    }
    if name != "legacy.debug" {
        mark(name, metadata.clone());
    }
    match name {
        "dictation.completed" => finish("succeeded", None),
        "dictation.sessionCancelled" => finish("cancelled", None),
        diagnostics::DICTATION_REQUEST_FAILED => finish("failed", metadata["detail"].as_str()),
        _ => {}
    }
}

/// `diagnostics::content_event` 的旁路。提示词和模型输出已经完整记在 llm 调用里，这里跳过重复内容。
pub(super) fn on_content(name: &str, content: &Value) {
    let duplicate = name.starts_with("smartText.")
        || (name == "legacy.debug" && content["component"] == SMART_SCOPE);
    if !duplicate {
        mark(name, content.clone());
    }
}

pub(crate) fn mark(name: &str, data: Value) {
    let Ok(mut active) = ACTIVE.lock() else {
        return;
    };
    if let Some(run) = active.as_mut() {
        let t_ms = run.started.elapsed().as_millis() as u64;
        run.timeline
            .push(json!({"tMs":t_ms,"event":name,"data":redact_secrets(data)}));
    }
}

/// 这次听写的目标是敏感输入框：记录里只留耗时和字符数，不留内容。
pub(crate) fn mark_sensitive() {
    if let Ok(mut active) = ACTIVE.lock() {
        if let Some(run) = active.as_mut() {
            run.sensitive = true;
        }
    }
}

pub(crate) fn llm_call(mut call: Value) {
    let Ok(mut active) = ACTIVE.lock() else {
        return;
    };
    let Some(run) = active.as_mut() else {
        return;
    };
    let from_test = call["scope"]
        .as_str()
        .is_some_and(|scope| scope.starts_with(TEST_SCOPE_PREFIX));
    if from_test && run.kind != SMART_TEST {
        return;
    }
    if !run.overlapped {
        let now_ms = run.started.elapsed().as_millis() as u64;
        attribute_host_timing(&mut call, &run.timeline[run.attributed_entries..], now_ms);
    }
    if let Some(variant) = run.variant.filter(|_| from_test) {
        call["variant"] = json!(variant);
    }
    run.attributed_entries = run.timeline.len();
    run.llm.push(redact_secrets(call));
}

/// 把这次请求期间发生的宿主事件算到它头上。只看请求时长内的条目：之前的 ASR、OCR 也走宿主
/// HTTP，不能算进大模型。
fn attribute_host_timing(call: &mut Value, entries: &[Value], now_ms: u64) {
    let window_start = now_ms.saturating_sub(call["durationMs"].as_u64().unwrap_or(0));
    let entries = entries
        .iter()
        .filter(|entry| entry["tMs"].as_u64().unwrap_or(0) >= window_start)
        .collect::<Vec<_>>();
    call["runtimeStartMs"] = json!(sum_ms(&entries, RUNTIME_READY));
    call["httpMs"] = json!(sum_ms(&entries, HTTP_RESPONSE));
    if call["origin"].is_null() {
        call["origin"] = entries
            .iter()
            .rev()
            .find_map(|entry| entry["data"].get("origin").filter(|origin| !origin.is_null()))
            .cloned()
            .unwrap_or(Value::Null);
    }
}

/// 某类宿主事件的总耗时；一次都没发生时是 None，区别于耗时为 0。
fn sum_ms(entries: &[&Value], event: &str) -> Option<u64> {
    entries
        .iter()
        .filter(|entry| entry["event"] == event)
        .filter_map(|entry| entry["data"]["ms"].as_u64())
        .reduce(|total, ms| total + ms)
}

/// genai 和 AI SDK 的用量字段不一样，统一成页面和分析用的一种。
pub(super) fn normalize_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage else {
        return Value::Null;
    };
    let pick = |camel: &str, snake: &str| {
        usage
            .get(camel)
            .or_else(|| usage.get(snake))
            .cloned()
            .unwrap_or(Value::Null)
    };
    json!({
        "inputTokens":pick("inputTokens", "prompt_tokens"),
        "outputTokens":pick("outputTokens", "completion_tokens"),
        "reasoningTokens":usage
            .get("reasoningTokens")
            .or_else(|| usage.pointer("/completion_tokens_details/reasoning_tokens"))
            .cloned()
            .unwrap_or(Value::Null),
    })
}

pub(crate) fn finish_failed(error: &str) {
    finish("failed", Some(error));
}

fn active_kind() -> Option<&'static str> {
    ACTIVE.lock().ok()?.as_ref().map(|run| run.kind)
}

/// 听写优先：有测试在跑也直接占用，测试在下一次请求前发现自己被顶掉后中止。
fn begin_dictation() {
    let replaced_test = active_kind() == Some(SMART_TEST);
    // 上一次听写没有走到终态（例如被新会话顶掉）时也要落盘，否则这条记录就丢了。
    finish("abandoned", None);
    if let Ok(mut active) = ACTIVE.lock() {
        // 持锁判断：测速开始前也要拿这把锁，两者不会互相错过。
        let overlapped = replaced_test || crate::application::dev_bench::is_running();
        *active = Some(Run {
            overlapped,
            ..new_run(DICTATION)
        });
    }
}

fn finish(outcome: &str, error: Option<&str>) {
    let run = ACTIVE
        .lock()
        .ok()
        .and_then(|mut active| active.take_if(|run| run.kind == DICTATION));
    // 单元测试同样是 debug 构建，它们发出的听写事件不能写进真实记录。
    if let Some(run) = run.filter(|_| !cfg!(test)) {
        append_record(&traces_dir(), &dictation_record(&run, outcome, error));
    }
}

pub(super) fn append_record(directory: &Path, record: &Value) {
    let day = diagnostics::date_key(diagnostics::now_seconds());
    let path = directory.join(format!("runs-{day}.jsonl"));
    let written = serde_json::to_vec(record)
        .map_err(|error| error.to_string())
        .and_then(|line| {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|error| error.to_string())?;
            file.write_all(&line)
                .and_then(|()| file.write_all(b"\n"))
                .map_err(|error| error.to_string())
        });
    if let Err(error) = written {
        eprintln!("[dev-trace] 写入运行记录失败：{error}");
    }
}

fn redact_secrets(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let secret = ["apikey", "api_key", "secret", "password", "authorization"]
                        .iter()
                        .any(|needle| normalized.contains(needle));
                    let value = match value {
                        _ if secret => json!("[redacted]"),
                        Value::String(text) if matches!(normalized.as_str(), "error" | "detail") => {
                            Value::String(strip_url_queries(&text))
                        }
                        other => redact_secrets(other),
                    };
                    (key, value)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_secrets).collect()),
        other => other,
    }
}

/// 请求库的错误文本带完整地址，部分接口把密钥放在查询串里。
pub(super) fn strip_url_queries(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find("http") {
        let (before, rest) = remaining.split_at(start);
        output.push_str(before);
        let url_end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '"' | '\''))
            .unwrap_or(rest.len());
        let (url, after) = rest.split_at(url_end);
        output.push_str(url.split('?').next().unwrap_or(url));
        remaining = after;
    }
    output.push_str(remaining);
    output
}

fn scrub_content(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                if CONTENT_KEYS.contains(&key.as_str()) {
                    *field = json!("[敏感输入，未记录]");
                } else {
                    scrub_content(field);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(scrub_content),
        _ => {}
    }
}

fn event_time(timeline: &[Value], name: &str) -> Option<u64> {
    timeline
        .iter()
        .find(|entry| entry["event"] == name)
        .and_then(|entry| entry["tMs"].as_u64())
}

fn event_data<'a>(timeline: &'a [Value], name: &str) -> Option<&'a Value> {
    timeline
        .iter()
        .find(|entry| entry["event"] == name)
        .map(|entry| &entry["data"])
}

fn compute_phases(timeline: &[Value]) -> Vec<Value> {
    PHASES
        .iter()
        .filter_map(|spec| {
            let from = event_time(timeline, spec.from)?;
            let to = event_time(timeline, spec.to)?;
            Some(json!({
                "name":spec.name,
                "ms":to.saturating_sub(from),
                "waiting":spec.waiting,
            }))
        })
        .collect()
}

fn dictation_record(run: &Run, outcome: &str, error: Option<&str>) -> Value {
    let phases = compute_phases(&run.timeline);
    let wait_ms: u64 = phases
        .iter()
        .filter(|phase| phase["waiting"] == true)
        .filter_map(|phase| phase["ms"].as_u64())
        .sum();
    let prompt = event_data(&run.timeline, "trace.promptBuilt").cloned();
    let settings = event_data(&run.timeline, "trace.asrFinal").cloned();
    let assistant = settings.as_ref().is_some_and(|data| data["assistant"] == true);
    let mut record = json!({
        "id":run.id,
        "kind":if assistant { ASSISTANT } else { run.kind },
        "startedAtMs":run.started_at_ms,
        "outcome":outcome,
        "error":error.map(strip_url_queries),
        "sensitive":run.sensitive,
        "overlapped":run.overlapped,
        "totalMs":run.started.elapsed().as_millis() as u64,
        "waitMs":wait_ms,
        "phases":phases,
        "settings":settings,
        "promptBreakdown":prompt,
        "llm":run.llm,
        "analysis":analyze_dictation(&phases, wait_ms, &run.llm, prompt.as_ref()),
        "timeline":run.timeline,
    });
    if run.overlapped {
        record["analysis"]["hints"]
            .as_array_mut()
            .expect("hints 是数组")
            .insert(0, json!("这次听写期间有测试或测速在跑，耗时不具代表性，已排除出统计"));
    }
    if run.sensitive {
        scrub_content(&mut record);
    }
    record
}

pub(super) fn seconds(ms: u64) -> String {
    format!("{:.1} 秒", ms as f64 / 1000.0)
}

fn analyze_dictation(
    phases: &[Value],
    wait_ms: u64,
    llm: &[Value],
    prompt: Option<&Value>,
) -> Value {
    let slowest = phases
        .iter()
        .filter(|phase| phase["waiting"] == true)
        .max_by_key(|phase| phase["ms"].as_u64().unwrap_or(0));
    let bottleneck = slowest.and_then(|phase| phase["name"].as_str());
    let summary = match slowest {
        Some(phase) if wait_ms > 0 => {
            let ms = phase["ms"].as_u64().unwrap_or(0);
            format!(
                "松开后等了 {}，最慢的是{}（{}，占 {}%）",
                seconds(wait_ms),
                phase["name"].as_str().unwrap_or_default(),
                seconds(ms),
                ms * 100 / wait_ms,
            )
        }
        _ => "这次没有走到等待阶段".to_string(),
    };
    let call = llm
        .iter()
        .find(|call| call["scope"] == SMART_SCOPE)
        .or_else(|| llm.first());
    let mut hints = call.map(llm_hints).unwrap_or_default();
    hints.extend(prompt_hint(prompt));
    let context_ms = phases
        .iter()
        .find(|phase| phase["name"] == CONTEXT_PHASE)
        .and_then(|phase| phase["ms"].as_u64())
        .unwrap_or(0);
    if context_ms >= 800 {
        hints.push(format!(
            "等软件上下文用了 {}，来自截图识别；模板不用 {{{{active_app_context}}}} 就能省掉",
            seconds(context_ms)
        ));
    }
    json!({"bottleneck":bottleneck,"summary":summary,"hints":hints})
}

fn llm_hints(call: &Value) -> Vec<String> {
    let mut hints = Vec::new();
    if let Some(runtime_ms) = call["runtimeStartMs"].as_u64().filter(|ms| *ms >= 300) {
        hints.push(format!(
            "启动 AI SDK 运行时用了 {}，每次请求都会重新启动一次",
            seconds(runtime_ms)
        ));
    }
    let tokens = call["usage"]["reasoningTokens"].as_u64().unwrap_or(0);
    let chars = call["reasoningChars"].as_u64().unwrap_or(0);
    if tokens > 0 || chars > 0 {
        let amount = if tokens > 0 {
            format!("{tokens} token")
        } else {
            format!("{chars} 字符")
        };
        hints.push(format!(
            "模型做了思考（{amount}），这部分时间花在思考上；可以在模型设置里把推理强度改成关闭"
        ));
    }
    hints
}

fn prompt_hint(prompt: Option<&Value>) -> Option<String> {
    let prompt = prompt?;
    let total = prompt["promptChars"].as_u64()?;
    if total < 2000 {
        return None;
    }
    let (label, chars) = PROMPT_SOURCES
        .iter()
        .map(|(key, label)| (*label, prompt[*key].as_u64().unwrap_or(0)))
        .max_by_key(|(_, chars)| *chars)?;
    Some(format!(
        "提示词有 {total} 字符，最大来源是{label}（{chars} 字符）"
    ))
}

fn analyze_test(
    baseline_ms: u64,
    actual_ms: u64,
    llm: &[Value],
    prompt: Option<&Value>,
    connection: Option<&Value>,
) -> Value {
    let call = |variant: &str| llm.iter().find(|call| call["variant"] == variant);
    let http = |variant: &str| call(variant).and_then(|call| call["httpMs"].as_u64());
    let local_ms = http("actual").map(|http_ms| actual_ms.saturating_sub(http_ms));
    let (bottleneck, summary) = match local_ms {
        Some(local_ms) if local_ms * 2 >= actual_ms => (
            "local",
            format!(
                "主要慢在本机：这次请求 {}，真正花在网络和模型上的只有 {}，其余 {} 在本机",
                seconds(actual_ms),
                seconds(actual_ms - local_ms),
                seconds(local_ms)
            ),
        ),
        _ => {
            // 有宿主 HTTP 耗时就比网络部分，排除本机开销；否则只能比整次请求。
            let base = http("baseline").unwrap_or(baseline_ms);
            let real = http("actual").unwrap_or(actual_ms);
            let connection_ms = connection
                .and_then(|probe| probe["connectionMs"].as_u64())
                .unwrap_or(0);
            if connection_ms >= CONNECTION_NOTABLE_MS && connection_ms * 100 >= real * 30 {
                let reused_ms = connection
                    .and_then(|probe| probe["reusedMs"].as_u64())
                    .unwrap_or(0);
                (
                    "connection",
                    format!(
                        "主要慢在每次重新建立连接（估算）：握手约 {}，占这次网络耗时 {} 的 {}%；复用连接后约 {}",
                        seconds(connection_ms),
                        seconds(real),
                        (connection_ms * 100 / real.max(1)).min(100),
                        seconds(real.saturating_sub(connection_ms).max(reused_ms))
                    ),
                )
            } else if base * 100 >= real * 60 {
                (
                    "model",
                    format!(
                        "主要慢在模型或网络本身：极短提示词也要 {}，真实提示词 {}",
                        seconds(base),
                        seconds(real)
                    ),
                )
            } else {
                (
                    "prompt",
                    format!(
                        "主要慢在这次的输入输出：极短提示词只要 {}，真实提示词要 {}，多出来的时间来自提示词长度、输出长度或思考",
                        seconds(base),
                        seconds(real)
                    ),
                )
            }
        }
    };
    let mut hints = call("actual").map(llm_hints).unwrap_or_default();
    hints.extend(prompt_hint(prompt));
    json!({"bottleneck":bottleneck,"summary":summary,"hints":hints})
}

/// 按日期从新到旧读记录。文件内是追加写入，所以每个文件倒序扫描。
fn read_records(directory: &Path, limit: usize) -> Result<Vec<Value>, String> {
    let mut names = std::fs::read_dir(directory)
        .map_err(|error| format!("读取追踪目录失败：{error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("runs-") && name.ends_with(".jsonl"))
        .collect::<Vec<_>>();
    names.sort_unstable_by(|a, b| b.cmp(a));
    let mut records = Vec::new();
    for name in names {
        let content = std::fs::read_to_string(directory.join(&name))
            .map_err(|error| format!("读取 {name} 失败：{error}"))?;
        for line in content.lines().rev() {
            if records.len() == limit {
                return Ok(records);
            }
            if let Ok(record) = serde_json::from_str::<Value>(line) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn summarize(record: &Value) -> Value {
    let call = record["llm"]
        .as_array()
        .and_then(|calls| {
            calls
                .iter()
                .find(|call| call["variant"] == "actual" || call["scope"] == SMART_SCOPE)
                .or_else(|| calls.first())
        })
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "id":record["id"],
        "kind":record["kind"],
        "startedAtMs":record["startedAtMs"],
        "outcome":record["outcome"],
        "totalMs":record["totalMs"],
        "waitMs":record["waitMs"],
        "bottleneck":record["analysis"]["bottleneck"],
        "summary":record["analysis"]["summary"],
        "connectionMs":record["connection"]["connectionMs"],
        "providerId":call["providerId"],
        "modelId":call["modelId"],
        "llmMs":call["durationMs"],
    })
}

fn percentile(values: &mut [u64], percent: usize) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    values[(values.len() - 1) * percent / 100]
}

fn distribution(mut values: Vec<u64>) -> Value {
    json!({
        "samples":values.len(),
        "p50Ms":percentile(&mut values, 50),
        "p90Ms":percentile(&mut values, 90),
        "maxMs":values.last().copied().unwrap_or(0),
    })
}

fn compute_stats(records: &[Value]) -> Value {
    let finished = records
        .iter()
        .filter(|record| {
            record["kind"] == DICTATION
                && record["outcome"] == "succeeded"
                && record["overlapped"] != true
        })
        .collect::<Vec<_>>();
    let phases = PHASES
        .iter()
        .filter_map(|spec| {
            let values = finished
                .iter()
                .filter_map(|record| {
                    record["phases"]
                        .as_array()?
                        .iter()
                        .find(|phase| phase["name"] == spec.name)?["ms"]
                        .as_u64()
                })
                .collect::<Vec<_>>();
            if values.is_empty() {
                return None;
            }
            let mut row = distribution(values);
            row["name"] = json!(spec.name);
            row["waiting"] = json!(spec.waiting);
            Some(row)
        })
        .collect::<Vec<_>>();
    let mut by_model: BTreeMap<String, (Vec<u64>, Vec<u64>)> = BTreeMap::new();
    for call in finished
        .iter()
        .filter_map(|record| record["llm"].as_array())
        .flatten()
        .filter(|call| call["scope"] == SMART_SCOPE)
    {
        let key = format!(
            "{} / {}",
            call["providerId"].as_str().unwrap_or("?"),
            call["modelId"].as_str().unwrap_or("?")
        );
        let entry = by_model.entry(key).or_default();
        entry.0.extend(call["durationMs"].as_u64());
        entry.1.extend(call["userPromptChars"].as_u64());
    }
    let models = by_model
        .into_iter()
        .map(|(name, (durations, prompts))| {
            let mut row = distribution(durations);
            row["name"] = json!(name);
            row["avgPromptChars"] =
                json!(prompts.iter().sum::<u64>() / (prompts.len().max(1) as u64));
            row
        })
        .collect::<Vec<_>>();
    let wait = finished
        .iter()
        .filter_map(|record| record["waitMs"].as_u64())
        .collect::<Vec<_>>();
    json!({"runs":finished.len(),"wait":distribution(wait),"phases":phases,"models":models})
}

fn find_record(directory: &Path, id: &str) -> Result<Option<Value>, String> {
    if id == "latest" {
        return Ok(read_records(directory, 1)?.into_iter().next());
    }
    let records = read_records(directory, usize::MAX)?;
    Ok(records.into_iter().find(|record| record["id"] == id))
}

/// 一次线上智能处理用到的提示词和模型，重放时原样再发一次。
pub(super) struct ReplayPrompt {
    pub(super) system_prompt: String,
    pub(super) user_prompt: String,
    pub(super) provider_id: String,
    pub(super) model: String,
    pub(super) breakdown: Option<Value>,
    /// 智能处理之前的识别原文和所在软件，本地模型对比按训练格式重新拼输入时用。
    pub(super) raw_text: String,
    pub(super) app_name: String,
}

fn replay_from_record(record: &Value) -> Option<ReplayPrompt> {
    if record["sensitive"] == true {
        return None;
    }
    let call = record["llm"]
        .as_array()?
        .iter()
        .find(|call| call["scope"] == SMART_SCOPE)?;
    Some(ReplayPrompt {
        system_prompt: call["systemPrompt"].as_str().unwrap_or_default().to_string(),
        user_prompt: call["userPrompt"].as_str().unwrap_or_default().to_string(),
        provider_id: call["providerId"].as_str().unwrap_or("default").to_string(),
        model: call["modelId"].as_str().unwrap_or_default().to_string(),
        breakdown: record.get("promptBreakdown").cloned(),
        raw_text: record["settings"]["rawText"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        app_name: record["settings"]["appName"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    })
}

/// `latest` 指最近一次能重放的听写：取消的、字数不够没走智能处理的都跳过。
pub(super) fn replay_prompt(id: &str) -> Result<ReplayPrompt, String> {
    let records = read_records(&traces_dir(), usize::MAX)?;
    let found = if id == "latest" {
        records.iter().find_map(replay_from_record)
    } else {
        records
            .iter()
            .find(|record| record["id"] == id)
            .and_then(replay_from_record)
    };
    found.ok_or_else(|| "没有能重放的听写记录：需要一次走过智能处理、且不是敏感输入框的听写".to_string())
}

/// 模型基础延迟、真实提示词、流式三种请求的对比测试。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SmartTestRequest {
    /// 用这段文字走当前模板；和 from_run 都没给时重放最近一次听写。
    text: Option<String>,
    /// 重放某次运行的完整提示词，`latest` 表示最近一次。
    from_run: Option<String>,
    provider_id: Option<String>,
    model: Option<String>,
}

pub(super) fn input_from_text(state: &RuntimeState, text: &str) -> Result<ReplayPrompt, String> {
    let (template, provider_id, model) = crate::application::dictation::smart_test_defaults(state)?;
    let user_prompt =
        smart_text::build_smart_prompt(state, text, &template, "", "", &provider_id)?.text;
    Ok(ReplayPrompt {
        system_prompt: smart_text::SYSTEM_PROMPT.to_string(),
        user_prompt,
        provider_id,
        model,
        breakdown: None,
        raw_text: text.to_string(),
        app_name: String::new(),
    })
}

/// 切到下一种请求；测试已被听写顶掉时返回错误，不再发付费请求。
fn enter_variant(id: &str, variant: &'static str) -> Result<(), String> {
    let mut active = ACTIVE.lock().map_err(|_| "追踪状态锁失败")?;
    let run = active
        .as_mut()
        .filter(|run| run.id == id)
        .ok_or("测试期间开始了听写，本次测试已中止")?;
    run.variant = Some(variant);
    Ok(())
}

fn busy_message(kind: &str) -> &'static str {
    if kind == SMART_TEST {
        "对比测试正在进行，结束后再试"
    } else {
        "正在听写，结束后再试"
    }
}

/// 没有听写或对比测试在记录时才执行 start。全程持有追踪锁，检查和设置状态之间不会插进
/// 别的运行；锁的顺序固定是先追踪后测速。
pub(super) fn while_idle<T>(start: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let active = ACTIVE.lock().map_err(|_| "追踪状态锁失败")?;
    if let Some(run) = active.as_ref() {
        return Err(busy_message(run.kind).to_string());
    }
    start()
}

/// 放进独立任务：调用方断开（curl 超时、AI 工具超时）时测试照常跑完并落盘，
/// 否则这条运行会一直占着追踪状态。
pub(crate) async fn smart_test(app: &AppHandle, request: SmartTestRequest) -> Result<Value, String> {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run_smart_test(&app, request).await })
        .await
        .map_err(|error| format!("对比测试任务异常结束：{error}"))?
}

async fn run_smart_test(app: &AppHandle, request: SmartTestRequest) -> Result<Value, String> {
    let state = app.state::<RuntimeState>();
    let state: &RuntimeState = state.inner();
    let id = {
        let mut active = ACTIVE.lock().map_err(|_| "追踪状态锁失败")?;
        if let Some(run) = active.as_ref() {
            return Err(busy_message(run.kind).to_string());
        }
        if crate::application::dev_bench::is_running() {
            return Err("模型测速正在进行，结束后再试".into());
        }
        let run = new_run(SMART_TEST);
        let id = run.id.clone();
        *active = Some(run);
        id
    };
    let outcome = run_variants(state, &id, &request).await;
    let run = ACTIVE
        .lock()
        .map_err(|_| "追踪状态锁失败")?
        .take_if(|run| run.id == id)
        .ok_or("测试期间开始了听写，本次测试已中止")?;
    let (analysis, breakdown, connection, error) = match outcome {
        Ok(result) => (
            analyze_test(
                result.baseline_ms,
                result.actual_ms,
                &run.llm,
                result.breakdown.as_ref(),
                result.connection.as_ref(),
            ),
            result.breakdown,
            result.connection,
            None,
        ),
        Err(error) => {
            let error = strip_url_queries(&error);
            (
                json!({"bottleneck":null,"summary":format!("测试失败：{error}"),"hints":[]}),
                None,
                None,
                Some(error),
            )
        }
    };
    let record = json!({
        "id":run.id,
        "kind":run.kind,
        "startedAtMs":run.started_at_ms,
        "outcome":if error.is_none() { "succeeded" } else { "failed" },
        "error":error,
        "totalMs":run.started.elapsed().as_millis() as u64,
        "promptBreakdown":breakdown,
        "connection":connection,
        "llm":run.llm,
        "analysis":analysis,
        "timeline":run.timeline,
    });
    append_record(&traces_dir(), &record);
    Ok(record)
}

struct TestResult {
    baseline_ms: u64,
    actual_ms: u64,
    breakdown: Option<Value>,
    connection: Option<Value>,
}

async fn run_variants(
    state: &RuntimeState,
    id: &str,
    request: &SmartTestRequest,
) -> Result<TestResult, String> {
    let mut input = match (&request.text, &request.from_run) {
        (Some(text), _) if !text.trim().is_empty() => input_from_text(state, text)?,
        (_, Some(run)) => replay_prompt(run)?,
        _ => replay_prompt("latest")?,
    };
    if let Some(provider_id) = &request.provider_id {
        input.provider_id = provider_id.clone();
    }
    if let Some(model) = &request.model {
        input.model = model.clone();
    }
    let provider = Some(input.provider_id.as_str());
    let model = Some(input.model.as_str());
    enter_variant(id, "baseline")?;
    let started = Instant::now();
    smart_text::process_prompt(
        state,
        BASELINE_SYSTEM_PROMPT,
        BASELINE_USER_PROMPT,
        provider,
        model,
        "dev-baseline",
        false,
    )
    .await?;
    let baseline_ms = started.elapsed().as_millis() as u64;
    enter_variant(id, "actual")?;
    let started = Instant::now();
    smart_text::process_prompt(
        state,
        &input.system_prompt,
        &input.user_prompt,
        provider,
        model,
        "dev-actual",
        false,
    )
    .await?;
    let actual_ms = started.elapsed().as_millis() as u64;
    enter_variant(id, "stream")?;
    // 流式只是补充首字数据，失败已记在这次请求上，不影响前两项结论。
    run_stream_variant(state, &input).await?;
    let (built_breakdown, origin) = ACTIVE
        .lock()
        .ok()
        .and_then(|active| {
            active.as_ref().map(|run| {
                (
                    event_data(&run.timeline, "trace.promptBuilt").cloned(),
                    run.llm
                        .iter()
                        .find_map(|call| call["origin"].as_str().map(str::to_string)),
                )
            })
        })
        .unwrap_or_default();
    let connection = match origin {
        Some(origin) => probe_connection(&origin).await,
        None => None,
    };
    Ok(TestResult {
        baseline_ms,
        actual_ms,
        breakdown: input.breakdown.or(built_breakdown),
        connection,
    })
}

async fn timed_get(client: &reqwest::Client, origin: &str) -> Option<u64> {
    let started = Instant::now();
    // 读完响应体连接才会回到连接池，否则下一次还是新连接。
    client.get(origin).send().await.ok()?.bytes().await.ok()?;
    Some(started.elapsed().as_millis() as u64)
}

/// 估算每次新建连接的成本：新连接测两次取小值排除抖动，再减去复用连接的耗时。
/// 只请求站点根路径、不带密钥，不会计费。站点不支持复用时测不出，connectionMs 为空。
async fn probe_connection(origin: &str) -> Option<Value> {
    let build = || {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .ok()
    };
    let first = timed_get(&build()?, origin).await?;
    let client = build()?;
    let second = timed_get(&client, origin).await?;
    let reused_ms = timed_get(&client, origin).await?;
    let fresh_ms = first.min(second);
    Some(json!({
        "origin":origin,
        "freshMs":fresh_ms,
        "reusedMs":reused_ms,
        "connectionMs":(reused_ms < fresh_ms).then(|| fresh_ms - reused_ms),
    }))
}

/// 听写走整包请求，只能测到总耗时；这里用同样的提示词走流式，拆出首字和生成各花了多久。
async fn run_stream_variant(state: &RuntimeState, input: &ReplayPrompt) -> Result<(), String> {
    let mut call = smart_text::llm_trace_settings_for(
        state,
        Some(input.provider_id.as_str()),
        Some(input.model.as_str()),
    )?;
    let started = Instant::now();
    let mut first_reasoning_ms = None;
    let mut first_output_ms = None;
    let result = smart_text::stream_prompt(
        state,
        &input.system_prompt,
        &input.user_prompt,
        Some(input.provider_id.as_str()),
        Some(input.model.as_str()),
        None,
        "dev-stream",
        false,
        &[],
        CancellationToken::new(),
        false,
        |output, reasoning| {
            let elapsed = started.elapsed().as_millis() as u64;
            if !reasoning.is_empty() && first_reasoning_ms.is_none() {
                first_reasoning_ms = Some(elapsed);
            }
            if !output.is_empty() && first_output_ms.is_none() {
                first_output_ms = Some(elapsed);
            }
        },
    )
    .await;
    let response = result.as_ref().ok();
    call["scope"] = json!("dev-stream");
    call["stream"] = json!(true);
    call["durationMs"] = json!(started.elapsed().as_millis() as u64);
    call["firstReasoningMs"] = json!(first_reasoning_ms);
    call["firstOutputMs"] = json!(first_output_ms);
    call["userPromptChars"] = json!(input.user_prompt.chars().count());
    call["outputChars"] = json!(response.map_or(0, |r| r.output.chars().count()));
    call["reasoningChars"] = json!(response.map_or(0, |r| r.reasoning.chars().count()));
    call["usage"] = normalize_usage(response.and_then(|r| r.usage.as_ref()));
    call["output"] = json!(response.map(|r| r.output.as_str()));
    call["error"] = json!(result.as_ref().err());
    llm_call(call);
    Ok(())
}

#[tauri::command]
pub(crate) fn dev_trace_status() -> Value {
    json!({
        "tracesDir":traces_dir().display().to_string(),
        "apiUrl":format!("http://127.0.0.1:{API_PORT}"),
    })
}

/// 读写「保存听写录音」开关；`enabled` 不给时只读。
#[tauri::command]
pub(crate) fn dev_audio_recording(enabled: Option<bool>) -> Result<Value, String> {
    if let Some(enabled) = enabled {
        crate::audio_dsp::dev_record::set_enabled(enabled)?;
    }
    Ok(json!({
        "enabled":crate::audio_dsp::dev_record::enabled(),
        "dir":crate::audio_dsp::dev_record::dir().display().to_string(),
    }))
}

// 读记录的命令都放到后台线程：同步命令跑在主线程上，页面轮询时读文件会干扰正在测量的听写。
#[tauri::command(async)]
pub(crate) fn dev_trace_runs(limit: Option<usize>) -> Result<Vec<Value>, String> {
    read_records(&traces_dir(), limit.unwrap_or(DEFAULT_LIST_LIMIT))
        .map(|records| records.iter().map(summarize).collect())
}

#[tauri::command(async)]
pub(crate) fn dev_trace_run(id: String) -> Result<Option<Value>, String> {
    find_record(&traces_dir(), &id)
}

#[tauri::command(async)]
pub(crate) fn dev_trace_stats(limit: Option<usize>) -> Result<Value, String> {
    read_records(&traces_dir(), limit.unwrap_or(DEFAULT_LIST_LIMIT))
        .map(|records| compute_stats(&records))
}

#[tauri::command]
pub(crate) async fn dev_trace_smart_test(
    app: AppHandle,
    request: SmartTestRequest,
) -> Result<Value, String> {
    smart_test(&app, request).await
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn api_result(result: Result<Value, String>) -> ApiResult {
    result
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({"error":error}))))
}

async fn api_index() -> Json<Value> {
    Json(json!({
        "tracesDir":traces_dir().display().to_string(),
        "endpoints":{
            "GET /runs?limit=50":"运行记录摘要，从新到旧",
            "GET /runs/{id}":"一次运行的完整记录，id 可写 latest",
            "GET /stats?limit=50":"最近若干次听写的各阶段与各模型耗时分布",
            "POST /smart-test":"对比测试，会调用 3 次大模型并产生费用。请求体 {text?, fromRun?, providerId?, model?}，text 和 fromRun 都不给时重放最近一次听写",
            "POST /smart-eval":"模板回归评测，每条样本一次真实请求（并发，有费用）。请求体 {template?, cases:[{text, appName?, context?}]}，template 不给时用当前模板",
            "GET /models":"可以测速的大模型列表",
            "POST /model-bench":"多模型并发测速，每个模型每轮一次真实请求，有费用。请求体 {models?:[{providerId, model}], rounds?, prompt?, fromRun?}，models 不给时测当前智能处理模型，完成后返回结果",
            "GET /model-bench":"测速进度：进行中返回实时数据，结束后返回最近一次结果",
        },
    }))
}

async fn api_runs(Query(query): Query<LimitQuery>) -> ApiResult {
    api_result(
        read_records(&traces_dir(), query.limit.unwrap_or(DEFAULT_LIST_LIMIT))
            .map(|records| Value::from(records.iter().map(summarize).collect::<Vec<_>>())),
    )
}

async fn api_run(RoutePath(id): RoutePath<String>) -> ApiResult {
    match find_record(&traces_dir(), &id) {
        Ok(Some(record)) => Ok(Json(record)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"没有这条运行记录"})),
        )),
        Err(error) => api_result(Err(error)),
    }
}

async fn api_stats(Query(query): Query<LimitQuery>) -> ApiResult {
    api_result(
        read_records(&traces_dir(), query.limit.unwrap_or(DEFAULT_LIST_LIMIT))
            .map(|records| compute_stats(&records)),
    )
}

async fn api_smart_test(
    RouteState(app): RouteState<AppHandle>,
    Json(request): Json<SmartTestRequest>,
) -> ApiResult {
    api_result(smart_test(&app, request).await)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmartEvalCase {
    text: String,
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    context: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmartEvalRequest {
    /// 不给时用当前智能处理模板，便于和改过的模板对比。
    template: Option<String>,
    cases: Vec<SmartEvalCase>,
}

/// 调模板用的回归评测：每条样本按给定模板、软件名和上下文渲染后真实请求一次（并发，有费用），
/// 返回每条的输出和耗时。不写入运行记录，也不和听写互斥。
async fn smart_eval(app: &AppHandle, request: SmartEvalRequest) -> Result<Value, String> {
    let state = app.state::<RuntimeState>();
    let state: &RuntimeState = state.inner();
    let (current, provider_id, model) = crate::application::dictation::smart_test_defaults(state)?;
    let template = request.template.unwrap_or(current);
    let runs = request.cases.iter().map(|case| {
        let template = template.as_str();
        let provider_id = provider_id.as_str();
        let model = model.as_str();
        async move {
            let started = Instant::now();
            let output = async {
                let prompt = smart_text::build_smart_prompt(
                    state,
                    &case.text,
                    template,
                    &case.context,
                    &case.app_name,
                    provider_id,
                )?
                .text;
                smart_text::process_prompt(
                    state,
                    smart_text::SYSTEM_PROMPT,
                    &prompt,
                    Some(provider_id),
                    Some(model),
                    "dev-eval",
                    false,
                )
                .await
            }
            .await;
            json!({
                "appName":case.app_name,
                "text":case.text,
                "output":output.as_ref().ok(),
                "error":output.as_ref().err(),
                "ms":started.elapsed().as_millis() as u64,
            })
        }
    });
    Ok(Value::from(futures_util::future::join_all(runs).await))
}

async fn api_smart_eval(
    RouteState(app): RouteState<AppHandle>,
    Json(request): Json<SmartEvalRequest>,
) -> ApiResult {
    api_result(smart_eval(&app, request).await)
}

async fn api_models(RouteState(app): RouteState<AppHandle>) -> ApiResult {
    api_result(crate::application::dev_bench::list_models(app.state::<RuntimeState>().inner()))
}

async fn api_bench_state() -> Json<Value> {
    Json(crate::application::dev_bench::current_state())
}

async fn api_bench_run(
    RouteState(app): RouteState<AppHandle>,
    Json(request): Json<crate::application::dev_bench::BenchRequest>,
) -> ApiResult {
    api_result(crate::application::dev_bench::run(&app, request).await)
}

async fn only_loopback_host(request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let loopback = host == format!("127.0.0.1:{API_PORT}") || host == format!("localhost:{API_PORT}");
    if loopback {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

fn start_api(app: AppHandle) {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), API_PORT);
    let listener = std::net::TcpListener::bind(address).and_then(|listener| {
        listener.set_nonblocking(true)?;
        Ok(listener)
    });
    let listener = match listener {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("[dev-trace] 本地接口无法监听 {address}：{error}");
            return;
        }
    };
    let router = Router::new()
        .route("/", get(api_index))
        .route("/runs", get(api_runs))
        .route("/runs/{id}", get(api_run))
        .route("/stats", get(api_stats))
        .route("/smart-test", post(api_smart_test))
        .route("/smart-eval", post(api_smart_eval))
        .route("/models", get(api_models))
        .route("/model-bench", get(api_bench_state).post(api_bench_run))
        .layer(middleware::from_fn(only_loopback_host))
        .with_state(app);
    tauri::async_runtime::spawn(async move {
        // 和 OBS 字幕服务一样：setup 钩子是同步的，要进了 Tauri 运行时才能注册 Tokio 监听。
        let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
            return;
        };
        let _ = axum::serve(listener, router).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(events: &[(&str, u64)], llm: Vec<Value>) -> Run {
        Run {
            timeline: events
                .iter()
                .map(|(name, t_ms)| json!({"tMs":t_ms,"event":name,"data":{}}))
                .collect(),
            llm,
            ..new_run(DICTATION)
        }
    }

    #[test]
    fn record_splits_phases_and_excludes_recording_from_waiting() {
        let run = run_with(
            &[
                ("dictation.sessionStarted", 0),
                ("trace.recordingReady", 50),
                ("dictation.stopRequested", 2050),
                ("trace.asrFinal", 2250),
                ("trace.historySaved", 2260),
                ("trace.contextReady", 2260),
                ("trace.smartDone", 5260),
                ("trace.rulesDone", 5261),
                ("dictation.injectionFinished", 5361),
            ],
            vec![
                json!({"scope":"learning-summary","reasoningChars":0}),
                json!({"scope":SMART_SCOPE,"reasoningChars":120,"usage":{"reasoningTokens":900}}),
            ],
        );
        let record = dictation_record(&run, "succeeded", None);
        assert_eq!(record["waitMs"], 3311);
        assert_eq!(record["analysis"]["bottleneck"], SMART_PHASE);
        let hints = record["analysis"]["hints"].as_array().unwrap();
        assert!(hints[0].as_str().unwrap().contains("900"));
    }

    #[test]
    fn overlapped_runs_are_flagged_and_left_out_of_stats() {
        let mut run = run_with(&[("dictation.stopRequested", 0), ("trace.asrFinal", 100)], Vec::new());
        run.overlapped = true;
        let record = dictation_record(&run, "succeeded", None);
        assert!(record["analysis"]["hints"][0].as_str().unwrap().contains("不具代表性"));
        assert_eq!(compute_stats(&[record])["runs"], 0);
    }

    #[test]
    fn missing_marks_leave_phases_out_instead_of_inventing_durations() {
        let run = run_with(
            &[("dictation.sessionStarted", 0), ("dictation.stopRequested", 900)],
            Vec::new(),
        );
        assert_eq!(dictation_record(&run, "cancelled", None)["phases"], json!([]));
    }

    #[test]
    fn sensitive_runs_keep_timings_but_drop_content() {
        let mut run = run_with(&[("dictation.stopRequested", 0), ("trace.asrFinal", 100)], Vec::new());
        run.sensitive = true;
        run.timeline[1]["data"] = json!({"rawText":"我的密码是 abc","rawTextChars":9});
        run.llm = vec![json!({"scope":SMART_SCOPE,"userPrompt":"我的密码是 abc","output":"abc","durationMs":700})];
        let record = dictation_record(&run, "succeeded", None);
        let text = record.to_string();
        assert!(!text.contains("我的密码"));
        assert!(!text.contains("\"abc\""));
        assert_eq!(record["llm"][0]["durationMs"], 700);
        assert_eq!(record["settings"]["rawTextChars"], 9);
        assert!(replay_from_record(&record).is_none());
    }

    #[test]
    fn test_analysis_blames_the_model_only_when_the_baseline_is_already_slow() {
        assert_eq!(analyze_test(3000, 4000, &[], None, None)["bottleneck"], "model");
        assert_eq!(analyze_test(500, 4000, &[], None, None)["bottleneck"], "prompt");
    }

    #[test]
    fn test_analysis_separates_local_overhead_from_network_time() {
        let calls = |http_ms: u64| {
            [
                json!({"variant":"baseline","httpMs":http_ms}),
                json!({"variant":"actual","httpMs":http_ms,"runtimeStartMs":2600}),
            ]
        };
        let slow_host = analyze_test(3000, 3100, &calls(200), None, None);
        assert_eq!(slow_host["bottleneck"], "local");
        assert!(slow_host["hints"][0].as_str().unwrap().contains("2.6"));
        assert_eq!(analyze_test(3000, 3100, &calls(2900), None, None)["bottleneck"], "model");
    }

    #[test]
    fn test_analysis_blames_connection_setup_when_handshake_dominates_network_time() {
        let calls = [
            json!({"variant":"baseline","httpMs":2000}),
            json!({"variant":"actual","httpMs":2100}),
        ];
        let handshake = json!({"connectionMs":1400,"reusedMs":400});
        let analysis = analyze_test(2300, 2400, &calls, None, Some(&handshake));
        assert_eq!(analysis["bottleneck"], "connection");
        assert!(analysis["summary"].as_str().unwrap().contains("1.4"));
        let unmeasured = json!({"connectionMs":null,"reusedMs":2000});
        assert_eq!(analyze_test(2300, 2400, &calls, None, Some(&unmeasured))["bottleneck"], "model");
        // 探测值比实际网络耗时还大时，复用后的预计值不能算成 0。
        let oversized = json!({"connectionMs":3000,"reusedMs":400});
        let analysis = analyze_test(2300, 2400, &calls, None, Some(&oversized));
        assert!(analysis["summary"].as_str().unwrap().contains("复用连接后约 0.4 秒"));
    }

    #[test]
    fn host_timing_only_counts_events_inside_the_request_window() {
        let entries = [
            json!({"tMs":100,"event":HTTP_RESPONSE,"data":{"ms":900,"origin":"https://asr.example"}}),
            json!({"tMs":5100,"event":RUNTIME_READY,"data":{"ms":400}}),
            json!({"tMs":7000,"event":HTTP_RESPONSE,"data":{"ms":1800,"origin":"https://llm.example"}}),
        ];
        let mut call = json!({"durationMs":2300});
        attribute_host_timing(&mut call, &entries, 7300);
        assert_eq!(call["runtimeStartMs"], 400);
        assert_eq!(call["httpMs"], 1800);
        assert_eq!(call["origin"], "https://llm.example");
        // genai 直连不经过宿主：请求期间没有事件时不能显示别人的耗时，也不能覆盖自己的主机。
        let mut direct = json!({"durationMs":50,"origin":"https://direct.example"});
        attribute_host_timing(&mut direct, &entries, 7300);
        assert_eq!(direct["httpMs"], Value::Null);
        assert_eq!(direct["origin"], "https://direct.example");
    }

    #[test]
    fn usage_from_either_sdk_normalizes_to_the_same_fields() {
        let genai = json!({"prompt_tokens":12,"completion_tokens":34,"completion_tokens_details":{"reasoning_tokens":5}});
        let sdk = json!({"inputTokens":12,"outputTokens":34,"reasoningTokens":5});
        let expected = json!({"inputTokens":12,"outputTokens":34,"reasoningTokens":5});
        assert_eq!(normalize_usage(Some(&genai)), expected);
        assert_eq!(normalize_usage(Some(&sdk)), expected);
        assert_eq!(normalize_usage(None), Value::Null);
    }

    #[test]
    fn secrets_are_redacted_but_usage_tokens_survive() {
        let value = redact_secrets(json!({
            "apiKey":"sk-1",
            "nested":{"Authorization":"Bearer x"},
            "usage":{"prompt_tokens":12},
            "error":"error sending request for url (https://x.example/v1?key=sk-2): timed out",
        }));
        assert_eq!(value["apiKey"], "[redacted]");
        assert_eq!(value["nested"]["Authorization"], "[redacted]");
        assert_eq!(value["usage"]["prompt_tokens"], 12);
        assert_eq!(
            value["error"],
            "error sending request for url (https://x.example/v1): timed out"
        );
    }

    #[test]
    fn records_read_back_newest_first_and_feed_stats() {
        let directory = std::env::temp_dir().join(format!("sayit-dev-trace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let run = run_with(
            &[("dictation.stopRequested", 0), ("trace.asrFinal", 100)],
            vec![json!({"scope":SMART_SCOPE,"providerId":"p","modelId":"m","durationMs":700,"userPromptChars":30})],
        );
        let mut older = dictation_record(&run, "succeeded", None);
        older["id"] = json!("older");
        let mut newer = older.clone();
        newer["id"] = json!("newer");
        append_record(&directory, &older);
        append_record(&directory, &newer);
        let records = read_records(&directory, 10).unwrap();
        assert_eq!(records[0]["id"], "newer");
        assert_eq!(find_record(&directory, "latest").unwrap().unwrap()["id"], "newer");
        let stats = compute_stats(&records);
        assert_eq!(stats["runs"], 2);
        assert_eq!(stats["models"][0]["name"], "p / m");
        assert_eq!(stats["models"][0]["p50Ms"], 700);
        std::fs::remove_dir_all(&directory).unwrap();
    }
}

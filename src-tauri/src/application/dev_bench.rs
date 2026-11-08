//! 开发构建专用的多模型测速：同一段提示词并发发给多个模型，比较首字耗时和每秒 token 数。

use crate::application::dev_trace;
use crate::application::smart_text;
use crate::state::RuntimeState;
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

const SCOPE: &str = "dev-bench";
const DEFAULT_ROUNDS: u32 = 3;
const MAX_ROUNDS: u32 = 5;
/// 生成窗口太短时除出来的速度没有意义，例如供应商把全部内容一次性返回。
const MIN_RATE_WINDOW_MS: u64 = 200;
const STANDARD_SYSTEM_PROMPT: &str = "你是写作助手。直接输出正文，不要解释。";
const STANDARD_USER_PROMPT: &str =
    "请用中文写一段 300 字左右的文字，介绍语音输入相比键盘打字的优势。";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BenchModel {
    pub(super) provider_id: String,
    pub(super) model: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct BenchRequest {
    /// 不给时测当前智能处理用的模型。
    models: Vec<BenchModel>,
    rounds: Option<u32>,
    /// 自定义提示词；和 from_run 都不给时用标准提示词。
    prompt: Option<String>,
    /// 重放某次听写的完整提示词，`latest` 表示最近一次。
    from_run: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RoundSample {
    /// 第一个内容到达（思考或正文）。
    first_token_ms: Option<u64>,
    /// 第一个正文到达；有思考时比 first_token_ms 晚。
    first_output_ms: Option<u64>,
    /// 最后一个内容到达；进行中会不断更新。
    last_token_ms: u64,
    output_chars: usize,
    reasoning_chars: usize,
    output_tokens: Option<f64>,
    /// 供应商没有返回用量，output_tokens 是按字符估算的。
    tokens_estimated: bool,
    tokens_per_sec: Option<f64>,
    finished: bool,
    error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct RowSummary {
    first_token_ms: Option<u64>,
    first_output_ms: Option<u64>,
    last_token_ms: Option<u64>,
    tokens_per_sec: Option<f64>,
    output_tokens: Option<f64>,
    tokens_estimated: bool,
    failures: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelRow {
    provider_id: String,
    provider_name: String,
    model_id: String,
    reasoning_effective: String,
    samples: Vec<RoundSample>,
    summary: RowSummary,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchState {
    id: String,
    running: bool,
    rounds: u32,
    started_at_ms: u64,
    models: Vec<ModelRow>,
}

static BENCH: Mutex<Option<BenchState>> = Mutex::new(None);

pub(super) fn is_running() -> bool {
    BENCH
        .lock()
        .ok()
        .and_then(|bench| bench.as_ref().map(|state| state.running))
        .unwrap_or(false)
}

pub(crate) fn current_state() -> Value {
    BENCH
        .lock()
        .ok()
        .and_then(|bench| serde_json::to_value(bench.as_ref()?).ok())
        .unwrap_or(Value::Null)
}

pub(crate) fn list_models(state: &RuntimeState) -> Result<Value, String> {
    let settings = crate::commands::common::read_provider_settings(state)?;
    let (current_provider, current_model) =
        crate::application::dictation::smart_llm_selection(state)?;
    let current_provider = smart_text::resolve_llm_provider_id(&settings, &current_provider);
    let mut models = Vec::new();
    for profile in settings.profiles.iter().filter(|profile| {
        profile.enabled && profile.capabilities.iter().any(|value| value == "llm")
    }) {
        let mut names = crate::providers::llm_models_from_config(&profile.config)
            .into_iter()
            .filter(|model| model.availability != crate::providers::LlmModelAvailability::Missing)
            .map(|model| model.name)
            .collect::<Vec<_>>();
        let configured = profile
            .config
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        // 智能处理没单独选模型时用的是供应商配置里的模型。
        let current = if current_model.is_empty() {
            configured
        } else {
            current_model.as_str()
        };
        let selected = if profile.id == current_provider {
            current
        } else {
            ""
        };
        for name in [configured, selected] {
            if !name.is_empty() && !names.iter().any(|existing| existing == name) {
                names.insert(0, name.to_string());
            }
        }
        models.extend(names.into_iter().map(|name| {
            json!({
                "providerId":profile.id,
                "providerName":profile.display_name,
                "current":profile.id == current_provider && name == current,
                "model":name,
            })
        }));
    }
    Ok(Value::from(models))
}

fn with_sample(row: usize, round: usize, update: impl FnOnce(&mut RoundSample)) {
    if let Ok(mut bench) = BENCH.lock() {
        if let Some(sample) = bench
            .as_mut()
            .and_then(|state| state.models.get_mut(row))
            .and_then(|model| model.samples.get_mut(round))
        {
            update(sample);
        }
    }
}

fn tokens_per_sec(tokens: f64, first_ms: Option<u64>, last_ms: u64) -> Option<f64> {
    let window_ms = last_ms.checked_sub(first_ms?)?;
    (window_ms >= MIN_RATE_WINDOW_MS && tokens > 0.0).then(|| tokens * 1000.0 / window_ms as f64)
}

/// 没有用量数据时的粗略估算：中文约 1.6 字一个 token，其他文字约 4 个字符一个 token。
fn estimate_tokens(text: &str) -> f64 {
    let (cjk, other) = text.chars().fold((0u32, 0u32), |(cjk, other), c| {
        if ('\u{2E80}'..='\u{9FFF}').contains(&c) || ('\u{F900}'..='\u{FAFF}').contains(&c) {
            (cjk + 1, other)
        } else {
            (cjk, other + 1)
        }
    });
    f64::from(cjk) / 1.6 + f64::from(other) / 4.0
}

/// SDK 的错误是「前缀 + JSON + 调用栈」，页面和表格只要人能看懂的部分。
fn short_error(error: &str) -> String {
    const MARKER: &str = "[provider_error]";
    let first_line = dev_trace::strip_url_queries(error.lines().next().unwrap_or(error));
    let details = first_line
        .find(MARKER)
        .and_then(|start| serde_json::from_str::<Value>(&first_line[start + MARKER.len()..]).ok());
    match details {
        Some(details) => {
            let status = details["status"]
                .as_u64()
                .map(|status| format!("，HTTP {status}"))
                .unwrap_or_default();
            format!(
                "{}（{}{status}）",
                details["message"].as_str().unwrap_or("供应商返回错误"),
                details["code"].as_str().unwrap_or("unknown"),
            )
        }
        None => first_line,
    }
}

fn median_u64(mut values: Vec<u64>) -> Option<u64> {
    values.sort_unstable();
    values.get(values.len().checked_sub(1)? / 2).copied()
}

fn median_f64(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    values.get(values.len().checked_sub(1)? / 2).copied()
}

/// 只统计跑完且成功的轮次：失败的没有可比的耗时，单独计入 failures；还没跑的不算。
fn summarize_samples(samples: &[RoundSample]) -> RowSummary {
    let succeeded = samples
        .iter()
        .filter(|sample| sample.finished && sample.error.is_none())
        .collect::<Vec<_>>();
    RowSummary {
        first_token_ms: median_u64(succeeded.iter().filter_map(|s| s.first_token_ms).collect()),
        first_output_ms: median_u64(succeeded.iter().filter_map(|s| s.first_output_ms).collect()),
        last_token_ms: median_u64(succeeded.iter().map(|s| s.last_token_ms).collect()),
        tokens_per_sec: median_f64(succeeded.iter().filter_map(|s| s.tokens_per_sec).collect()),
        output_tokens: median_f64(succeeded.iter().filter_map(|s| s.output_tokens).collect()),
        tokens_estimated: succeeded.iter().any(|s| s.tokens_estimated),
        failures: samples.iter().filter(|sample| sample.error.is_some()).count(),
    }
}

/// 首字快的排前面，完全没成功的排最后。
fn sort_rows(rows: &mut [ModelRow]) {
    rows.sort_by_key(|row| row.summary.first_token_ms.unwrap_or(u64::MAX));
}

fn analyze(rows: &[ModelRow]) -> Value {
    let name = |row: &ModelRow| format!("{} / {}", row.provider_name, row.model_id);
    let fastest_first = rows
        .iter()
        .filter(|row| row.summary.first_token_ms.is_some())
        .min_by_key(|row| row.summary.first_token_ms);
    let fastest_rate = rows
        .iter()
        .filter(|row| row.summary.tokens_per_sec.is_some())
        .max_by(|a, b| {
            a.summary
                .tokens_per_sec
                .unwrap_or(0.0)
                .total_cmp(&b.summary.tokens_per_sec.unwrap_or(0.0))
        });
    let mut parts = Vec::new();
    if let Some(row) = fastest_first {
        parts.push(format!(
            "首字最快的是 {}（{}）",
            name(row),
            dev_trace::seconds(row.summary.first_token_ms.unwrap_or(0))
        ));
    }
    if let Some(row) = fastest_rate {
        parts.push(format!(
            "生成最快的是 {}（{:.0} token/秒）",
            name(row),
            row.summary.tokens_per_sec.unwrap_or(0.0)
        ));
    }
    let summary = if parts.is_empty() {
        "所有模型都没有测出结果".to_string()
    } else {
        parts.join("，")
    };
    let mut hints = vec![
        "首字包含建立连接的时间，现在每次请求都会重新握手；Groq 和插件模型还包含启动 JS 运行时的时间，开发构建里这部分偏大".to_string(),
    ];
    if rows
        .iter()
        .any(|row| row.summary.first_token_ms.is_none() && row.summary.last_token_ms.is_some())
    {
        hints.push("首字为空的模型不支持流式，只能看总耗时".to_string());
    }
    let failed = rows.iter().filter(|row| row.summary.failures > 0).count();
    if failed > 0 {
        hints.push(format!("{failed} 个模型有失败的轮次，详情见每轮明细"));
    }
    let rate_limited = rows.iter().flat_map(|row| &row.samples).any(|sample| {
        sample
            .error
            .as_deref()
            .is_some_and(|error| error.contains("rate_limit") || error.contains("429"))
    });
    if rate_limited {
        hints.push("有模型被限流：同一供应商同时收到太多请求时会这样，少选几个模型或减少轮数再测".to_string());
    }
    if rows.iter().any(|row| row.summary.tokens_estimated) {
        hints.push("带 ≈ 的模型没有返回 token 用量，速度是按字符估算的".to_string());
    }
    json!({"summary":summary,"hints":hints})
}

impl RoundSample {
    /// 流式收到新内容时调用；内容没有变长时不更新。
    pub(super) fn record_progress(&mut self, elapsed_ms: u64, output: &str, reasoning: &str) {
        let reasoning_grew = reasoning.chars().count() > self.reasoning_chars;
        let output_grew = output.chars().count() > self.output_chars;
        if !reasoning_grew && !output_grew {
            return;
        }
        self.first_token_ms.get_or_insert(elapsed_ms);
        if !output.is_empty() {
            self.first_output_ms.get_or_insert(elapsed_ms);
        }
        self.last_token_ms = elapsed_ms;
        self.output_chars = output.chars().count();
        self.reasoning_chars = reasoning.chars().count();
        let tokens = estimate_tokens(output) + estimate_tokens(reasoning);
        self.output_tokens = Some(tokens);
        self.tokens_estimated = true;
        self.tokens_per_sec = tokens_per_sec(tokens, self.first_token_ms, elapsed_ms);
    }

    /// 请求结束时调用；`output_tokens` 是供应商返回的用量，没有时沿用按字符的估算。
    pub(super) fn record_finish(
        &mut self,
        total_ms: u64,
        result: Result<(&str, &str, Option<f64>), String>,
    ) {
        self.finished = true;
        match result {
            Ok((output, reasoning, output_tokens)) => {
                // 整包返回的供应商不会触发增量回调，这里补上最终内容。
                if self.last_token_ms == 0 {
                    self.last_token_ms = total_ms;
                    self.output_chars = output.chars().count();
                    self.reasoning_chars = reasoning.chars().count();
                    self.output_tokens = Some(estimate_tokens(output) + estimate_tokens(reasoning));
                    self.tokens_estimated = true;
                }
                if let Some(tokens) = output_tokens.filter(|tokens| *tokens > 0.0) {
                    self.output_tokens = Some(tokens);
                    self.tokens_estimated = false;
                }
                self.tokens_per_sec = tokens_per_sec(
                    self.output_tokens.unwrap_or(0.0),
                    self.first_token_ms,
                    self.last_token_ms,
                );
            }
            Err(error) => {
                self.last_token_ms = total_ms;
                self.error = Some(short_error(&error));
            }
        }
    }
}

/// 流式发一次请求并测速，边收边把实时数据交给 `on_update`；返回最终数据和正文。
pub(super) async fn measure_stream(
    state: &RuntimeState,
    model: &BenchModel,
    system_prompt: &str,
    user_prompt: &str,
    scope: &str,
    mut on_update: impl FnMut(&RoundSample),
) -> (RoundSample, String) {
    let started = Instant::now();
    let mut sample = RoundSample::default();
    let result = smart_text::stream_prompt(
        state,
        system_prompt,
        user_prompt,
        Some(model.provider_id.as_str()),
        Some(model.model.as_str()),
        scope,
        false,
        &[],
        CancellationToken::new(),
        true,
        |output, reasoning| {
            sample.record_progress(started.elapsed().as_millis() as u64, output, reasoning);
            on_update(&sample);
        },
    )
    .await;
    let total_ms = started.elapsed().as_millis() as u64;
    let output = result
        .as_ref()
        .map(|response| response.output.clone())
        .unwrap_or_default();
    sample.record_finish(
        total_ms,
        result.as_ref().map_err(String::clone).map(|response| {
            let usage = dev_trace::normalize_usage(response.usage.as_ref());
            (
                response.output.as_str(),
                response.reasoning.as_str(),
                usage["outputTokens"].as_f64(),
            )
        }),
    );
    (sample, output)
}

async fn run_round(
    state: &RuntimeState,
    row: usize,
    round: usize,
    model: &BenchModel,
    system_prompt: &str,
    user_prompt: &str,
) {
    let (sample, _) = measure_stream(state, model, system_prompt, user_prompt, SCOPE, |live| {
        with_sample(row, round, |slot| *slot = live.clone());
    })
    .await;
    with_sample(row, round, |slot| *slot = sample);
}

async fn run_model(
    state: &RuntimeState,
    row: usize,
    model: &BenchModel,
    rounds: u32,
    system_prompt: &str,
    user_prompt: &str,
) {
    for round in 0..rounds as usize {
        run_round(state, row, round, model, system_prompt, user_prompt).await;
        if let Ok(mut bench) = BENCH.lock() {
            if let Some(model) = bench.as_mut().and_then(|state| state.models.get_mut(row)) {
                model.summary = summarize_samples(&model.samples);
            }
        }
    }
}

/// 供应商显示名、实际模型 ID 和推理强度；读不到时退回请求里的写法。
pub(super) fn model_labels(state: &RuntimeState, model: &BenchModel) -> (String, String, String) {
    let settings = smart_text::llm_trace_settings_for(
        state,
        Some(model.provider_id.as_str()),
        Some(model.model.as_str()),
    )
    .unwrap_or(Value::Null);
    let text = |key: &str| settings[key].as_str().map(str::to_string);
    (
        text("providerName").unwrap_or_else(|| model.provider_id.clone()),
        text("modelId").unwrap_or_else(|| model.model.clone()),
        text("reasoningEffective").unwrap_or_default(),
    )
}

fn new_row(state: &RuntimeState, model: &BenchModel, rounds: u32) -> ModelRow {
    let (provider_name, model_id, reasoning_effective) = model_labels(state, model);
    ModelRow {
        provider_id: model.provider_id.clone(),
        provider_name,
        model_id,
        reasoning_effective,
        samples: vec![RoundSample::default(); rounds as usize],
        summary: RowSummary::default(),
    }
}

fn resolve_models(state: &RuntimeState, request: &BenchRequest) -> Result<Vec<BenchModel>, String> {
    if !request.models.is_empty() {
        return Ok(request.models.clone());
    }
    let (_, provider_id, model) = crate::application::dictation::smart_test_defaults(state)?;
    Ok(vec![BenchModel { provider_id, model }])
}

/// 放进独立任务：调用方断开时测速照常跑完并落盘。
pub(crate) async fn run(app: &AppHandle, request: BenchRequest) -> Result<Value, String> {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run_detached(&app, request).await })
        .await
        .map_err(|error| format!("测速任务异常结束：{error}"))?
}

async fn run_detached(app: &AppHandle, request: BenchRequest) -> Result<Value, String> {
    let state = app.state::<RuntimeState>();
    let state: &RuntimeState = state.inner();
    let models = resolve_models(state, &request)?;
    let rounds = request.rounds.unwrap_or(DEFAULT_ROUNDS).clamp(1, MAX_ROUNDS);
    let (system_prompt, user_prompt, source) = match (&request.prompt, &request.from_run) {
        (Some(prompt), _) if !prompt.trim().is_empty() => {
            (STANDARD_SYSTEM_PROMPT.to_string(), prompt.clone(), "custom")
        }
        (_, Some(run)) => {
            let replay = dev_trace::replay_prompt(run)?;
            (replay.system_prompt, replay.user_prompt, "replay")
        }
        _ => (
            STANDARD_SYSTEM_PROMPT.to_string(),
            STANDARD_USER_PROMPT.to_string(),
            "standard",
        ),
    };
    let rows = models.iter().map(|model| new_row(state, model, rounds)).collect();
    let started = Instant::now();
    let started_at_ms = crate::application::diagnostics::now_millis() as u64;
    // 并发请求的宿主事件分不清归属，所以不和正在记录的听写或对比测试同时跑。
    dev_trace::while_idle(|| {
        let mut bench = BENCH.lock().map_err(|_| "测速状态锁失败")?;
        if bench.as_ref().is_some_and(|state| state.running) {
            return Err("模型测速正在进行，结束后再试".to_string());
        }
        *bench = Some(BenchState {
            id: started_at_ms.to_string(),
            running: true,
            rounds,
            started_at_ms,
            models: rows,
        });
        Ok(())
    })?;
    let system_prompt: Arc<str> = system_prompt.into();
    let user_prompt: Arc<str> = user_prompt.into();
    // 每个模型一个独立任务：请求开头有读凭据、建客户端这类同步工作，放在同一个任务里
    // 会挡住其他模型的流，把它们的首字和末字都推迟。任务异常结束时结果里缺这一轮，测速照常收尾。
    let tasks = models
        .into_iter()
        .enumerate()
        .map(|(row, model)| {
            let app = app.clone();
            let system_prompt = system_prompt.clone();
            let user_prompt = user_prompt.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<RuntimeState>();
                run_model(state.inner(), row, &model, rounds, &system_prompt, &user_prompt).await;
            })
        })
        .collect::<Vec<_>>();
    join_all(tasks).await;
    let mut bench = BENCH.lock().map_err(|_| "测速状态锁失败")?;
    let finished = bench.as_mut().ok_or("测速状态丢失")?;
    finished.running = false;
    sort_rows(&mut finished.models);
    let all_failed = finished
        .models
        .iter()
        .all(|row| row.summary.failures == row.samples.len());
    let mut record = serde_json::to_value(&*finished).map_err(|error| error.to_string())?;
    record["kind"] = json!("model-bench");
    record["outcome"] = json!(if all_failed { "failed" } else { "succeeded" });
    record["totalMs"] = json!(started.elapsed().as_millis() as u64);
    record["promptSource"] = json!(source);
    record["systemPrompt"] = json!(&*system_prompt);
    record["userPrompt"] = json!(&*user_prompt);
    record["analysis"] = analyze(&finished.models);
    drop(bench);
    dev_trace::append_record(&dev_trace::traces_dir(), &record);
    Ok(record)
}

#[tauri::command(async)]
pub(crate) fn dev_bench_models(state: tauri::State<'_, RuntimeState>) -> Result<Value, String> {
    list_models(state.inner())
}

#[tauri::command]
pub(crate) fn dev_bench_state() -> Value {
    current_state()
}

#[tauri::command]
pub(crate) async fn dev_bench_run(app: AppHandle, request: BenchRequest) -> Result<Value, String> {
    run(&app, request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(first: u64, last: u64, tokens: f64) -> RoundSample {
        RoundSample {
            first_token_ms: Some(first),
            first_output_ms: Some(first),
            last_token_ms: last,
            output_tokens: Some(tokens),
            tokens_per_sec: tokens_per_sec(tokens, Some(first), last),
            finished: true,
            ..RoundSample::default()
        }
    }

    fn row(name: &str, samples: Vec<RoundSample>) -> ModelRow {
        ModelRow {
            provider_id: "p".into(),
            provider_name: "供应商".into(),
            model_id: name.into(),
            reasoning_effective: "zero".into(),
            summary: summarize_samples(&samples),
            samples,
        }
    }

    #[test]
    fn rate_is_measured_over_the_generation_window_and_needs_a_real_window() {
        // 200 token 在首字之后 2 秒内生成完，首字前的等待不算进速度。
        assert_eq!(tokens_per_sec(200.0, Some(1000), 3000), Some(100.0));
        // 一次性返回：窗口太短，给不出有意义的速度。
        assert_eq!(tokens_per_sec(200.0, Some(1000), 1060), None);
        assert_eq!(tokens_per_sec(200.0, None, 3000), None);
    }

    #[test]
    fn provider_errors_shorten_to_a_readable_line() {
        let raw = "JavaScript 插件执行失败：Error: [provider_error]{\"code\":\"rate_limit_exceeded\",\"status\":429,\"message\":\"模型供应商请求频率受限\"}\n    at Es (eval_script:8:3932)";
        assert_eq!(short_error(raw), "模型供应商请求频率受限（rate_limit_exceeded，HTTP 429）");
        assert_eq!(
            short_error("请求失败 https://x.example/v1?key=sk-1\n    at y"),
            "请求失败 https://x.example/v1"
        );
    }

    #[test]
    fn estimate_counts_chinese_denser_than_latin() {
        assert_eq!(estimate_tokens("语音输入语音输入"), 5.0);
        assert_eq!(estimate_tokens("abcdefgh"), 2.0);
    }

    #[test]
    fn summary_uses_medians_of_successful_rounds_and_counts_failures() {
        let failed = RoundSample {
            error: Some("超时".into()),
            last_token_ms: 30000,
            finished: true,
            ..RoundSample::default()
        };
        let summary = summarize_samples(&[
            sample(900, 2900, 200.0),
            failed,
            sample(1100, 3100, 400.0),
            sample(5000, 7000, 300.0),
        ]);
        assert_eq!(summary.first_token_ms, Some(1100));
        assert_eq!(summary.last_token_ms, Some(3100));
        assert_eq!(summary.tokens_per_sec, Some(150.0));
        assert_eq!(summary.failures, 1);
    }

    #[test]
    fn rounds_still_running_or_not_started_stay_out_of_the_summary() {
        let summary = summarize_samples(&[
            sample(900, 3000, 200.0),
            RoundSample::default(),
            RoundSample::default(),
        ]);
        assert_eq!(summary.last_token_ms, Some(3000));
        assert_eq!(summary.failures, 0);
    }

    #[test]
    fn rows_sort_by_first_token_and_analysis_names_each_winner() {
        let failed = RoundSample {
            error: Some("401".into()),
            finished: true,
            ..RoundSample::default()
        };
        let mut rows = vec![
            row("全失败", vec![failed]),
            row("生成快", vec![sample(2000, 2500, 500.0)]),
            row("首字快", vec![sample(300, 4300, 400.0)]),
        ];
        sort_rows(&mut rows);
        assert_eq!(rows[0].model_id, "首字快");
        assert_eq!(rows[2].model_id, "全失败");
        let analysis = analyze(&rows);
        let summary = analysis["summary"].as_str().unwrap();
        assert!(summary.contains("首字最快的是 供应商 / 首字快"));
        assert!(summary.contains("生成最快的是 供应商 / 生成快（1000 token/秒）"));
        assert!(analysis["hints"].to_string().contains("1 个模型有失败"));
    }
}

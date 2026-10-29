//! 独立验收构建：使用真实 Tauri 事件循环和窗口生命周期，不进入常规发布版本。
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

mod compare;
mod fixture;
mod live;
mod scenarios;
mod transcription;

const IDENTIFIER: &str = "com.henjicc.sayit.acceptance";

// 隔离构建的输出边界：不向当前软件输入测试文字，也不改用户剪贴板。
pub(crate) fn capture_dictation_output(text: &str) {
    live::capture_output(text);
}

/// 仅用于定位第三方输入法注入资源；不改变系统输入法设置，也不进入正式构建。
#[cfg(windows)]
pub(crate) fn configure_process() {
    if std::env::var("SAYIT_ACCEPTANCE_NO_IME").as_deref() == Ok("1") {
        #[link(name = "imm32")]
        extern "system" {
            fn ImmDisableIME(thread_id: u32) -> i32;
        }
        assert_ne!(
            unsafe { ImmDisableIME(u32::MAX) },
            0,
            "禁用验收进程 IME 失败"
        );
    }
}

pub(crate) fn validate(app: &AppHandle) -> Result<(), String> {
    if app.config().identifier != IDENTIFIER {
        return Err("验收构建必须使用独立 identifier，禁止访问正式应用数据".into());
    }
    let root = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    if root.join("data-root.json").exists() || root.join("reset-pending.marker").exists() {
        return Err("验收数据目录不得包含重定向或重置标记".into());
    }
    output_path()?;
    Ok(())
}

fn output_path() -> Result<PathBuf, String> {
    let path = std::env::var_os("SAYIT_ACCEPTANCE_EVENTS")
        .map(PathBuf::from)
        .ok_or("缺少 SAYIT_ACCEPTANCE_EVENTS")?;
    if !path.is_absolute() {
        return Err("验收输出必须为绝对路径".into());
    }
    Ok(path)
}

struct Recorder {
    file: std::fs::File,
    started: Instant,
}

impl Recorder {
    fn record(&mut self, stage: &str, cycle: usize, elapsed_ms: Option<f64>) -> Result<(), String> {
        if stage.ends_with("-idle") && stage != "initial-idle"
            && std::env::var("SAYIT_ACCEPTANCE_OPTIMIZE_HEAP").as_deref() == Ok("1")
        {
            let before = process_heap();
            let started = Instant::now();
            optimize_idle_heap()?;
            return self.detail(stage, cycle, elapsed_ms, json!({
                "heapBeforeOptimization":before,
                "heapOptimizationMs":started.elapsed().as_secs_f64() * 1000.0
            }));
        }
        self.detail(stage, cycle, elapsed_ms, serde_json::Value::Null)
    }

    fn detail(
        &mut self,
        stage: &str,
        cycle: usize,
        elapsed_ms: Option<f64>,
        detail: serde_json::Value,
    ) -> Result<(), String> {
        let line = json!({
            "stage": stage, "cycle": cycle, "elapsedMs": elapsed_ms,
            "detail": detail,
            "automaticHeapReclaim": super::idle_reclaim::acceptance_stats(),
            "processHeap": if stage.ends_with("idle") || stage == "completed" { process_heap() } else { serde_json::Value::Null },
            "sinceStartMs": self.started.elapsed().as_secs_f64() * 1000.0,
            "timestampMs": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?.as_millis(),
            "pid": std::process::id(),
        });
        writeln!(self.file, "{line}")
            .and_then(|_| self.file.flush())
            .map_err(|e| e.to_string())
    }
}

// 只读默认进程堆的已分配/已提交量，辅助区分活对象与分配器保留；不修剪、不压缩堆。
#[cfg(windows)]
fn process_heap() -> serde_json::Value {
    use windows::Win32::System::Memory::{GetProcessHeap, HeapSummary, HEAP_SUMMARY};
    let mut summary = HEAP_SUMMARY {
        cb: std::mem::size_of::<HEAP_SUMMARY>() as u32,
        ..Default::default()
    };
    unsafe {
        match GetProcessHeap() {
            Ok(heap) if HeapSummary(heap, 0, &mut summary).as_bool() => json!({
                "allocatedBytes":summary.cbAllocated,"committedBytes":summary.cbCommitted,"reservedBytes":summary.cbReserved
            }),
            _ => json!({"unavailable":true}),
        }
    }
}

#[cfg(not(windows))]
fn process_heap() -> serde_json::Value {
    serde_json::Value::Null
}

// 仅验收构建的可选对照：释放 LFH 空闲提交页，既不操作工作集，也不改变活对象。
// https://learn.microsoft.com/en-us/windows/win32/api/heapapi/nf-heapapi-heapsetinformation
#[cfg(windows)]
fn optimize_idle_heap() -> Result<(), String> {
    super::idle_reclaim::reclaim_heap()
}

#[cfg(not(windows))]
fn optimize_idle_heap() -> Result<(), String> {
    Err("空闲堆对照仅支持 Windows".into())
}

fn recognition_rounds() -> Result<usize, String> {
    let rounds = std::env::var("SAYIT_ACCEPTANCE_RECOGNITION_ROUNDS")
        .unwrap_or_else(|_| "3".into())
        .parse::<usize>()
        .map_err(|e| e.to_string())?;
    if !(1..=12).contains(&rounds) {
        return Err("识别验收轮数须为 1～12".into());
    }
    Ok(rounds)
}

pub(crate) fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let result = run(&app).await;
        if let Err(error) = &result {
            super::diagnostics::event("error", "acceptance.failed", json!({"error":error}));
            if let Ok(path) = output_path() {
                if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path) {
                    let _ = writeln!(file, "{}", json!({"stage":"failed", "error":error}));
                }
            }
        }
        app.exit(if result.is_ok() { 0 } else { 1 });
    });
}

async fn wait_for_window(app: &AppHandle, present: bool) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let ready = match app.get_webview_window("main") {
                Some(window) => present && window.is_visible().unwrap_or(false),
                None => !present,
            };
            if ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| format!("等待主窗口 present={present} 超时"))
}

async fn run(app: &AppHandle) -> Result<(), String> {
    let blank = std::env::var("SAYIT_ACCEPTANCE_BLANK").as_deref() == Ok("1");
    let mut recorder = Recorder {
        file: std::fs::File::create(output_path()?).map_err(|e| e.to_string())?,
        started: Instant::now(),
    };
    crate::desktop::destroy_main_window(app)?;
    wait_for_window(app, false).await?;
    recorder.record("initial-idle", 0, None)?;
    tokio::time::sleep(Duration::from_secs(10)).await;
    recorder.record("scenario-preparing", 0, None)?;
    let cycles = match std::env::var("SAYIT_ACCEPTANCE_SCENARIO")
        .as_deref()
        .unwrap_or("windows")
    {
        "windows" => {
            run_windows(app, &mut recorder, blank).await?;
            10
        }
        "subtitle-preview" => {
            scenarios::subtitles(app, &mut recorder).await?;
            5
        }
        "audio-lab" => {
            scenarios::audio_lab(app, &mut recorder).await?;
            3
        }
        "transcription" => {
            transcription::run(app, &mut recorder).await?;
            recognition_rounds()?
        }
        "comparison" => {
            compare::run(app, &mut recorder).await?;
            recognition_rounds()?
        }
        "dictation" => {
            live::dictation(app, &mut recorder).await?;
            recognition_rounds()?
        }
        "subtitles" => {
            live::subtitles(app, &mut recorder).await?;
            recognition_rounds()?
        }
        other => return Err(format!("未知验收场景：{other}")),
    };
    recorder.record("final-idle", cycles, None)?;
    tokio::time::sleep(Duration::from_secs(20)).await;
    recorder.record("completed", cycles, None)
}

async fn run_windows(app: &AppHandle, recorder: &mut Recorder, blank: bool) -> Result<(), String> {
    for cycle in 1..=10 {
        recorder.record("opening", cycle, None)?;
        let started = Instant::now();
        if blank {
            let mut config = app
                .config()
                .app
                .windows
                .iter()
                .find(|c| c.label == "main")
                .ok_or("缺少 main 窗口配置")?
                .clone();
            config.url =
                tauri::WebviewUrl::External("about:blank".parse().map_err(|e| format!("{e}"))?);
            tauri::WebviewWindowBuilder::from_config(app, &config)
                .map_err(|e| e.to_string())?
                .visible(true)
                .build()
                .map_err(|e| e.to_string())?;
        } else {
            crate::desktop::ensure_main_window(app)?;
        }
        wait_for_window(app, true).await?;
        recorder.record(
            "open",
            cycle,
            Some(started.elapsed().as_secs_f64() * 1000.0),
        )?;
        tokio::time::sleep(Duration::from_secs(5)).await;
        recorder.record("closing", cycle, None)?;
        let started = Instant::now();
        // 使用真实 CloseRequested 路径，包括快捷键录入复位和 WebView 销毁。
        app.get_webview_window("main")
            .ok_or("主窗口意外消失")?
            .close()
            .map_err(|e| e.to_string())?;
        wait_for_window(app, false).await?;
        recorder.record(
            "closed",
            cycle,
            Some(started.elapsed().as_secs_f64() * 1000.0),
        )?;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Ok(())
}

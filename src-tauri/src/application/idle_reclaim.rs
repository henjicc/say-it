//! 资源释放事件触发一次延迟回收；没有任务结束时不运行定时器。
use tauri::AppHandle;

#[cfg(windows)]
use {
    crate::state::RuntimeState,
    std::sync::Mutex,
    std::time::{Duration, Instant},
    tauri::Manager,
};

#[cfg(windows)]
const IDLE_DELAY: Duration = Duration::from_secs(5);

#[cfg(all(windows, feature = "performance-acceptance"))]
static COMPLETED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(all(windows, feature = "performance-acceptance"))]
static BUSY_SKIPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(feature = "performance-acceptance")]
pub(crate) fn acceptance_stats() -> serde_json::Value {
    #[cfg(windows)]
    {
        serde_json::json!({
            "completed":COMPLETED.load(std::sync::atomic::Ordering::Relaxed),
            "busySkipped":BUSY_SKIPPED.load(std::sync::atomic::Ordering::Relaxed)
        })
    }
    #[cfg(not(windows))]
    {
        serde_json::Value::Null
    }
}

#[cfg(windows)]
#[derive(Default)]
pub(crate) struct IdleReclaim {
    plan: Mutex<Plan>,
}

#[cfg(windows)]
#[derive(Default)]
struct Plan {
    deadline: Option<Instant>,
    worker_running: bool,
}

#[cfg(windows)]
impl Plan {
    fn request(&mut self, now: Instant) -> bool {
        self.deadline = Some(now + IDLE_DELAY);
        let spawn = !self.worker_running;
        self.worker_running = true;
        spawn
    }

    fn next_deadline(&mut self) -> Option<Instant> {
        if self.deadline.is_none() {
            self.worker_running = false;
        }
        self.deadline
    }

    fn claim(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|deadline| deadline <= now) {
            self.deadline = None;
            true
        } else {
            false
        }
    }
}

/// 只合并释放通知，不在调用方持有领域锁时读取其他领域状态或执行堆回收。
#[cfg(windows)]
pub(crate) fn request(app: &AppHandle) {
    #[cfg(feature = "performance-acceptance")]
    if std::env::var("SAYIT_ACCEPTANCE_DISABLE_AUTO_RECLAIM").as_deref() == Ok("1") {
        return;
    }
    let spawn = match app.state::<RuntimeState>().idle_reclaim.plan.lock() {
        Ok(mut plan) => plan.request(Instant::now()),
        Err(_) => {
            eprintln!("[idle-reclaim] 回收调度锁失败");
            return;
        }
    };
    if spawn {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = run(app).await {
                super::diagnostics::event(
                    "warn",
                    "resources.idleReclaimFailed",
                    serde_json::json!({"error":error}),
                );
            }
        });
    }
}

#[cfg(not(windows))]
pub(crate) fn request(_app: &AppHandle) {}

#[cfg(windows)]
async fn run(app: AppHandle) -> Result<(), String> {
    loop {
        let deadline = app
            .state::<RuntimeState>()
            .idle_reclaim
            .plan
            .lock()
            .map_err(|_| "回收调度锁失败")?
            .next_deadline();
        let Some(deadline) = deadline else {
            return Ok(());
        };
        tokio::time::sleep_until(deadline.into()).await;
        if !app
            .state::<RuntimeState>()
            .idle_reclaim
            .plan
            .lock()
            .map_err(|_| "回收调度锁失败")?
            .claim(Instant::now())
        {
            continue;
        }
        let target = app.clone();
        // 堆管理器可能短暂持有内部锁，不占用界面线程或异步 I/O worker。
        let result = tauri::async_runtime::spawn_blocking(move || {
            if eligible(&target.state::<RuntimeState>()) {
                let started = Instant::now();
                reclaim_heap()?;
                #[cfg(feature = "performance-acceptance")]
                COMPLETED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                super::performance::record("idleHeapReclaim", started.elapsed().as_millis() as u64);
                super::diagnostics::event(
                    "debug",
                    "resources.idleReclaimed",
                    serde_json::json!({
                        "elapsedMs":started.elapsed().as_secs_f64() * 1000.0
                    }),
                );
            } else {
                #[cfg(feature = "performance-acceptance")]
                BUSY_SKIPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Ok::<_, String>(())
        })
        .await;
        // 失败仍让调度器处理后续通知，避免一次平台错误永久卡住 worker_running。
        if let Err(error) = result
            .map_err(|error| error.to_string())
            .and_then(|value| value)
        {
            super::diagnostics::event(
                "warn",
                "resources.idleReclaimFailed",
                serde_json::json!({"error":error}),
            );
        }
    }
}

#[cfg(windows)]
fn eligible(state: &RuntimeState) -> bool {
    use super::contract::DomainRunState;
    let settled = |state| matches!(state, DomainRunState::Idle | DomainRunState::Failed);
    !state.audio_session.is_busy()
        && state
            .asr_streams
            .lock()
            .is_ok_and(|streams| streams.is_empty())
        && state
            .transcriptions
            .lock()
            .is_ok_and(|jobs| jobs.is_empty())
        && super::dictation::domain_snapshot(state).is_ok_and(|snapshot| settled(snapshot.state))
        && super::subtitles::domain_snapshot(state).is_ok_and(|snapshot| settled(snapshot.state))
        && state.compare_runtime.is_settled()
        && state.audio_lab_runtime.is_idle_for_reclaim()
}

/// 系统堆管理器只回收空闲缓存，不修剪工作集，也不卸载仍被使用的对象。
#[cfg(windows)]
pub(crate) fn reclaim_heap() -> Result<(), String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Memory::{HeapOptimizeResources, HeapSetInformation};
    // HEAP_OPTIMIZE_RESOURCES_INFORMATION 的 ABI：Version=1、Flags=0。
    // https://learn.microsoft.com/en-us/windows/win32/api/heapapi/nf-heapapi-heapsetinformation
    #[repr(C)]
    struct Information {
        version: u32,
        flags: u32,
    }
    let information = Information {
        version: 1,
        flags: 0,
    };
    unsafe {
        HeapSetInformation(
            HANDLE::default(),
            HeapOptimizeResources,
            Some((&information as *const Information).cast()),
            std::mem::size_of::<Information>(),
        )
        .map_err(|error| format!("回收空闲堆缓存失败：{error}"))
    }
}

#[cfg(all(test, windows))]
mod tests;

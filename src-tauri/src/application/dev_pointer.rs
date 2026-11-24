//! 开发者「指向测试」的启动、进度和结果，见 `active_app_context::pointer_probe`。点开始后先等几秒，
//! 让用户切到要测的软件；结果留在内存里供界面和本地接口读，同时写进追踪目录。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const COUNTDOWN: Duration = Duration::from_secs(3);
const MAX_SECONDS: u64 = 60;

fn state() -> &'static Mutex<Value> {
    static STATE: OnceLock<Mutex<Value>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(json!({"status":"idle"})))
}

fn cancelled() -> &'static AtomicBool {
    static CANCELLED: AtomicBool = AtomicBool::new(false);
    &CANCELLED
}

fn set(value: Value) {
    if let Ok(mut current) = state().lock() {
        *current = value;
    }
}

pub(crate) fn current() -> Value {
    state().lock().map(|value| value.clone()).unwrap_or_else(|_| json!({"status":"idle"}))
}

pub(crate) fn start(seconds: u64) -> Result<Value, String> {
    if !cfg!(windows) {
        return Err("指向测试目前只支持 Windows".into());
    }
    let seconds = seconds.clamp(3, MAX_SECONDS);
    {
        let mut current = state().lock().map_err(|_| "指向测试状态锁失败")?;
        if matches!(current["status"].as_str(), Some("waiting" | "running")) {
            return Err("指向测试正在进行".into());
        }
        *current = json!({"status":"waiting","seconds":seconds,"countdownMs":COUNTDOWN.as_millis() as u64});
    }
    cancelled().store(false, Ordering::Release);
    std::thread::Builder::new()
        .name("dev-pointer-probe".into())
        .spawn(move || run(seconds))
        .map_err(|error| format!("无法启动指向测试：{error}"))?;
    Ok(current())
}

pub(crate) fn cancel() -> Value {
    cancelled().store(true, Ordering::Release);
    current()
}

#[cfg(windows)]
fn run(seconds: u64) {
    std::thread::sleep(COUNTDOWN);
    if cancelled().load(Ordering::Acquire) {
        set(json!({"status":"idle"}));
        return;
    }
    set(json!({"status":"running","seconds":seconds}));
    let result = crate::active_app_context::pointer_probe::run(Duration::from_secs(seconds), cancelled());
    let report = match result {
        Ok(report) => report,
        Err(error) => return set(json!({"status":"failed","error":error})),
    };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_millis()).unwrap_or(0);
    let path = crate::application::dev_trace::traces_dir().join(format!("pointer-probe-{stamp}.json"));
    let report = serde_json::to_value(&report).unwrap_or(Value::Null);
    let written = std::fs::create_dir_all(path.parent().expect("追踪目录有上级"))
        .and_then(|_| std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap_or_default()));
    set(json!({
        "status":"done",
        "seconds":seconds,
        "file":path.display().to_string(),
        "writeError":written.err().map(|error| error.to_string()),
        "report":report,
    }));
}

#[cfg(not(windows))]
fn run(_seconds: u64) {}

#[tauri::command]
pub(crate) fn dev_pointer_probe_start(seconds: u64) -> Result<Value, String> {
    crate::application::diagnostics::require_developer_mode()?;
    start(seconds)
}

#[tauri::command]
pub(crate) fn dev_pointer_probe_state() -> Result<Value, String> {
    crate::application::diagnostics::require_developer_mode()?;
    Ok(current())
}

#[tauri::command]
pub(crate) fn dev_pointer_probe_cancel() -> Result<Value, String> {
    crate::application::diagnostics::require_developer_mode()?;
    Ok(cancel())
}

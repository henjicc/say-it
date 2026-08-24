use crate::prelude::*;
use crate::state::{MouseGestureMode, MouseGestureSettings, RuntimeState};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;

const SAMPLE_WINDOW: Duration = Duration::from_millis(650);
const STOP_DWELL: Duration = Duration::from_millis(140);
const COOLDOWN: Duration = Duration::from_millis(1500);
const MIN_SAMPLE_DISTANCE: f64 = 3.0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MouseGestureSnapshot {
    pub(crate) enabled: bool,
    pub(crate) mode: MouseGestureMode,
    pub(crate) sensitivity: u8,
    pub(crate) available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct PointerSample {
    x: f64,
    y: f64,
    at: Instant,
    button_down: bool,
}

#[derive(Clone, Copy, Debug)]
struct GestureThresholds {
    min_path: f64,
    min_leg: f64,
    min_speed: f64,
    min_reversals: usize,
}

fn thresholds(sensitivity: u8) -> GestureThresholds {
    let amount = sensitivity.min(100) as f64 / 100.0;
    GestureThresholds {
        min_path: 420.0 - 200.0 * amount,
        min_leg: 44.0 - 22.0 * amount,
        min_speed: 900.0 - 350.0 * amount,
        min_reversals: if sensitivity <= 30 {
            5
        } else if sensitivity <= 70 {
            4
        } else {
            3
        },
    }
}

#[derive(Default)]
struct GestureRecognizer {
    samples: VecDeque<PointerSample>,
    last_motion_at: Option<Instant>,
    cooldown_until: Option<Instant>,
}

impl GestureRecognizer {
    fn reset(&mut self) {
        self.samples.clear();
        self.last_motion_at = None;
    }

    fn push(&mut self, sample: PointerSample) {
        if sample.button_down {
            self.reset();
            return;
        }
        if self
            .cooldown_until
            .is_some_and(|deadline| sample.at < deadline)
        {
            return;
        }
        while self
            .samples
            .front()
            .is_some_and(|value| sample.at.duration_since(value.at) > SAMPLE_WINDOW)
        {
            self.samples.pop_front();
        }
        if let Some(last) = self.samples.back() {
            if ((sample.x - last.x).powi(2) + (sample.y - last.y).powi(2)).sqrt()
                < MIN_SAMPLE_DISTANCE
            {
                return;
            }
        }
        self.samples.push_back(sample);
        self.last_motion_at = Some(sample.at);
    }

    fn tick(&mut self, now: Instant, sensitivity: u8) -> Option<(i32, i32)> {
        if self.cooldown_until.is_some_and(|deadline| now < deadline) {
            return None;
        }
        let last_motion = self.last_motion_at?;
        if now.duration_since(last_motion) < STOP_DWELL || self.samples.len() < 3 {
            return None;
        }
        while self
            .samples
            .front()
            .is_some_and(|value| now.duration_since(value.at) > SAMPLE_WINDOW + STOP_DWELL)
        {
            self.samples.pop_front();
        }
        let first = *self.samples.front()?;
        let last = *self.samples.back()?;
        let elapsed = last.at.duration_since(first.at).as_secs_f64().max(0.001);
        let mut path = 0.0;
        let mut x_travel = 0.0;
        let mut y_travel = 0.0;
        let mut deltas = Vec::with_capacity(self.samples.len().saturating_sub(1));
        let mut previous = first;
        for sample in self.samples.iter().skip(1).copied() {
            let dx = sample.x - previous.x;
            let dy = sample.y - previous.y;
            path += (dx * dx + dy * dy).sqrt();
            x_travel += dx.abs();
            y_travel += dy.abs();
            deltas.push((dx, dy));
            previous = sample;
        }
        let threshold = thresholds(sensitivity);
        let net = ((last.x - first.x).powi(2) + (last.y - first.y).powi(2)).sqrt();
        if path < threshold.min_path
            || path / elapsed < threshold.min_speed
            || net / path.max(1.0) > 0.35
        {
            self.reset();
            return None;
        }

        let use_x = x_travel >= y_travel;
        let mut reversals = 0usize;
        let mut direction = 0i8;
        let mut leg = 0.0;
        for (dx, dy) in deltas {
            let value = if use_x { dx } else { dy };
            if value.abs() < 0.5 {
                continue;
            }
            let next_direction = if value > 0.0 { 1 } else { -1 };
            if direction == 0 {
                direction = next_direction;
                leg = value.abs();
            } else if next_direction == direction {
                leg += value.abs();
            } else if leg >= threshold.min_leg {
                reversals += 1;
                direction = next_direction;
                leg = value.abs();
            }
        }
        if reversals < threshold.min_reversals {
            self.reset();
            return None;
        }
        self.reset();
        self.cooldown_until = Some(now + COOLDOWN);
        Some((last.x.round() as i32, last.y.round() as i32))
    }
}

static EVENT_SENDER: OnceLock<SyncSender<PointerSample>> = OnceLock::new();
static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static BUTTON_DOWN: AtomicBool = AtomicBool::new(false);
static MONITOR_STARTED_AT: OnceLock<Instant> = OnceLock::new();
static LAST_SAMPLE_US: AtomicU64 = AtomicU64::new(0);

fn send_pointer_sample(x: f64, y: f64, button_down: bool) {
    BUTTON_DOWN.store(button_down, Ordering::Release);
    let started = MONITOR_STARTED_AT.get_or_init(Instant::now);
    let now_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    if !button_down {
        let previous = LAST_SAMPLE_US.load(Ordering::Relaxed);
        if now_us.saturating_sub(previous) < 8_000 {
            return;
        }
        LAST_SAMPLE_US.store(now_us, Ordering::Relaxed);
    }
    if let Some(sender) = EVENT_SENDER.get() {
        let _ = sender.try_send(PointerSample {
            x,
            y,
            at: Instant::now(),
            button_down,
        });
    }
}

fn run_recognizer(app: tauri::AppHandle, receiver: Receiver<PointerSample>) {
    let mut recognizer = GestureRecognizer::default();
    loop {
        match receiver.recv_timeout(Duration::from_millis(16)) {
            Ok(sample) => recognizer.push(sample),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let settings = app
            .state::<RuntimeState>()
            .mouse_gesture
            .lock()
            .map(|settings| settings.clone())
            .unwrap_or_default();
        if !settings.enabled || !MONITOR_STARTED.load(Ordering::Acquire) {
            recognizer.reset();
            continue;
        }
        if BUTTON_DOWN.load(Ordering::Acquire) {
            recognizer.reset();
            continue;
        }
        if let Some(position) = recognizer.tick(Instant::now(), settings.sensitivity) {
            crate::desktop::floating_orb::request_mouse_gesture(
                app.clone(),
                position,
                settings.mode,
            );
        }
    }
}

pub(crate) fn snapshot(state: &RuntimeState) -> Result<MouseGestureSnapshot, String> {
    let settings = state
        .mouse_gesture
        .lock()
        .map_err(|_| "鼠标手势配置锁失败".to_string())?
        .clone()
        .normalized();
    let error = state
        .mouse_gesture_runtime
        .error
        .lock()
        .map_err(|_| "鼠标手势状态锁失败".to_string())?
        .clone();
    Ok(MouseGestureSnapshot {
        enabled: settings.enabled,
        mode: settings.mode,
        sensitivity: settings.sensitivity,
        available: state
            .mouse_gesture_runtime
            .listening
            .load(Ordering::Acquire),
        error,
    })
}

pub(crate) fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    if EVENT_SENDER.get().is_none() {
        let (sender, receiver) = sync_channel(1);
        let _ = EVENT_SENDER.set(sender);
        let worker_app = app.clone();
        std::thread::spawn(move || run_recognizer(worker_app, receiver));
    }
    let enabled = app
        .state::<RuntimeState>()
        .mouse_gesture
        .lock()
        .map_err(|_| "鼠标手势配置锁失败".to_string())?
        .enabled;
    if enabled {
        set_monitor_enabled(app, true)?;
    }
    Ok(())
}

fn set_monitor_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    BUTTON_DOWN.store(false, Ordering::Release);
    if enabled && !MONITOR_STARTED.swap(true, Ordering::AcqRel) {
        if let Err(error) = platform::start(send_pointer_sample) {
            MONITOR_STARTED.store(false, Ordering::Release);
            app.state::<RuntimeState>()
                .mouse_gesture_runtime
                .listening
                .store(false, Ordering::Release);
            if let Ok(mut current) = app
                .state::<RuntimeState>()
                .mouse_gesture_runtime
                .error
                .lock()
            {
                *current = Some(error.clone());
            }
            return Err(error);
        }
    } else if !enabled && MONITOR_STARTED.swap(false, Ordering::AcqRel) {
        platform::stop();
    }
    app.state::<RuntimeState>()
        .mouse_gesture_runtime
        .listening
        .store(
            enabled && MONITOR_STARTED.load(Ordering::Acquire),
            Ordering::Release,
        );
    if let Ok(mut error) = app
        .state::<RuntimeState>()
        .mouse_gesture_runtime
        .error
        .lock()
    {
        *error = None;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn set_mouse_gesture_settings(
    app: tauri::AppHandle,
    enabled: bool,
    mode: MouseGestureMode,
    sensitivity: u8,
) -> Result<MouseGestureSnapshot, String> {
    set_monitor_enabled(&app, enabled)?;
    let state = app.state::<RuntimeState>();
    let previous = state
        .mouse_gesture
        .lock()
        .map_err(|_| "鼠标手势配置锁失败".to_string())?
        .clone();
    {
        let mut settings = state
            .mouse_gesture
            .lock()
            .map_err(|_| "鼠标手势配置锁失败".to_string())?;
        *settings = MouseGestureSettings {
            enabled,
            mode,
            sensitivity: sensitivity.min(100),
        }
        .normalized();
    }
    if let Err(error) = crate::persistence::save_persisted_state(&app, &state) {
        if let Ok(mut settings) = state.mouse_gesture.lock() {
            *settings = previous.clone();
        }
        let _ = set_monitor_enabled(&app, previous.enabled);
        return Err(error);
    }
    crate::application::contract::next_revision(&state.snapshot_revision);
    snapshot(&state)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::sync::Mutex;

    type Callback = fn(f64, f64, bool);
    static CALLBACK: OnceLock<Callback> = OnceLock::new();
    static HANDLE: Mutex<Option<usize>> = Mutex::new(None);

    unsafe extern "C" fn receive(_context: *mut c_void, x: f64, y: f64, button_down: bool) {
        if let Some(callback) = CALLBACK.get() {
            callback(x, y, button_down);
        }
    }

    pub(super) fn start(callback: Callback) -> Result<(), String> {
        let _ = CALLBACK.set(callback);
        let handle = crate::macos_native::start_mouse_monitor(receive)?;
        *HANDLE.lock().map_err(|_| "macOS 鼠标监听状态锁失败")? = Some(handle);
        Ok(())
    }

    pub(super) fn stop() {
        if let Ok(mut handle) = HANDLE.lock() {
            if let Some(handle) = handle.take() {
                crate::macos_native::stop_mouse_monitor(handle);
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::sync::Mutex;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
    };
    use windows::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
        GetMessageW, PostThreadMessageW, RegisterClassW, TranslateMessage, CW_USEDEFAULT,
        HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WM_QUIT, WNDCLASSW,
    };

    type Callback = fn(f64, f64, bool);
    static CALLBACK: OnceLock<Callback> = OnceLock::new();
    static THREAD_ID: Mutex<Option<u32>> = Mutex::new(None);

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_INPUT {
            let mut point = POINT::default();
            if GetCursorPos(&mut point).is_ok() {
                let down = (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_MBUTTON.0 as i32) as u16 & 0x8000) != 0;
                if let Some(callback) = CALLBACK.get() {
                    callback(point.x as f64, point.y as f64, down);
                }
            }
        }
        DefWindowProcW(window, message, wparam, lparam)
    }

    pub(super) fn start(callback: Callback) -> Result<(), String> {
        let _ = CALLBACK.set(callback);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || unsafe {
            let instance = match GetModuleHandleW(None) {
                Ok(value) => value,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("读取 Windows 模块失败：{error}")));
                    return;
                }
            };
            let class_name = w!("SayItMouseGestureInput");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassW(&class);
            let window = match CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                PCWSTR::null(),
                WINDOW_STYLE::default(),
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                0,
                0,
                HWND_MESSAGE,
                None,
                instance,
                None,
            ) {
                Ok(value) => value,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("创建 Windows 鼠标监听窗口失败：{error}")));
                    return;
                }
            };
            let device = RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: window,
            };
            if let Err(error) =
                RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32)
            {
                let _ = DestroyWindow(window);
                let _ = ready_tx.send(Err(format!("注册 Windows 原始鼠标输入失败：{error}")));
                return;
            }
            let thread_id = GetCurrentThreadId();
            let _ = ready_tx.send(Ok(thread_id));
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let _ = DestroyWindow(window);
        });
        let thread_id = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "启动 Windows 鼠标监听超时".to_string())??;
        *THREAD_ID.lock().map_err(|_| "Windows 鼠标监听状态锁失败")? = Some(thread_id);
        Ok(())
    }

    pub(super) fn stop() {
        if let Ok(mut thread_id) = THREAD_ID.lock() {
            if let Some(thread_id) = thread_id.take() {
                unsafe {
                    let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    type Callback = fn(f64, f64, bool);
    pub(super) fn start(_callback: Callback) -> Result<(), String> {
        Err("当前平台不支持鼠标手势".into())
    }
    pub(super) fn stop() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(x: f64, y: f64, at: Instant) -> PointerSample {
        PointerSample {
            x,
            y,
            at,
            button_down: false,
        }
    }

    #[test]
    fn deliberate_horizontal_shake_is_detected_after_dwell() {
        let start = Instant::now();
        let mut recognizer = GestureRecognizer::default();
        for (index, x) in [0.0, 90.0, 10.0, 100.0, 20.0, 110.0]
            .into_iter()
            .enumerate()
        {
            recognizer.push(sample(
                x,
                20.0,
                start + Duration::from_millis(index as u64 * 70),
            ));
        }
        assert_eq!(
            recognizer.tick(start + Duration::from_millis(500), 50),
            Some((110, 20))
        );
    }

    #[test]
    fn one_way_motion_and_button_drag_do_not_trigger() {
        let start = Instant::now();
        let mut recognizer = GestureRecognizer::default();
        for index in 0..6 {
            recognizer.push(sample(
                index as f64 * 90.0,
                0.0,
                start + Duration::from_millis(index * 60),
            ));
        }
        assert_eq!(
            recognizer.tick(start + Duration::from_millis(500), 100),
            None
        );
        recognizer.push(PointerSample {
            x: 0.0,
            y: 0.0,
            at: start + Duration::from_secs(1),
            button_down: true,
        });
        assert!(recognizer.samples.is_empty());
    }

    #[test]
    fn sensitivity_changes_thresholds() {
        assert!(thresholds(0).min_path > thresholds(100).min_path);
        assert!(thresholds(0).min_speed > thresholds(100).min_speed);
        assert!(thresholds(0).min_reversals > thresholds(100).min_reversals);
    }

    #[test]
    fn diagonal_shake_is_detected_but_slow_motion_is_not() {
        let start = Instant::now();
        let mut diagonal = GestureRecognizer::default();
        for (index, value) in [0.0, 90.0, 10.0, 100.0, 20.0, 110.0]
            .into_iter()
            .enumerate()
        {
            diagonal.push(sample(
                value,
                value,
                start + Duration::from_millis(index as u64 * 70),
            ));
        }
        assert_eq!(
            diagonal.tick(start + Duration::from_millis(500), 50),
            Some((110, 110))
        );

        let mut slow = GestureRecognizer::default();
        for (index, x) in [0.0, 90.0, 10.0, 100.0, 20.0, 110.0]
            .into_iter()
            .enumerate()
        {
            slow.push(sample(
                x,
                0.0,
                start + Duration::from_millis(index as u64 * 180),
            ));
        }
        assert_eq!(slow.tick(start + Duration::from_millis(1100), 100), None);
    }

    #[test]
    fn cooldown_prevents_a_second_immediate_trigger() {
        let start = Instant::now();
        let mut recognizer = GestureRecognizer::default();
        for (index, x) in [0.0, 90.0, 10.0, 100.0, 20.0, 110.0]
            .into_iter()
            .enumerate()
        {
            recognizer.push(sample(
                x,
                0.0,
                start + Duration::from_millis(index as u64 * 70),
            ));
        }
        assert!(recognizer
            .tick(start + Duration::from_millis(500), 50)
            .is_some());
        recognizer.push(sample(0.0, 0.0, start + Duration::from_millis(600)));
        assert!(recognizer.samples.is_empty());
    }
}

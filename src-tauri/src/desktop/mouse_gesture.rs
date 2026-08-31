use crate::prelude::*;
use crate::state::{
    MouseGestureMode, MouseGestureSettings, RuntimeState, MAX_MOUSE_RAPID_CLICK_COUNT,
    MIN_MOUSE_RAPID_CLICK_COUNT,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;

const SAMPLE_WINDOW: Duration = Duration::from_millis(650);
const STOP_DWELL: Duration = Duration::from_millis(140);
const COOLDOWN: Duration = Duration::from_millis(1500);
const MIN_SAMPLE_DISTANCE: f64 = 3.0;
const RAPID_CLICK_INTERVAL: Duration = Duration::from_millis(420);
const RAPID_CLICK_PRESS_MAX: Duration = Duration::from_millis(350);
const RAPID_CLICK_RADIUS: f64 = 12.0;
const MONITOR_HEALTH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MouseGestureSnapshot {
    pub(crate) enabled: bool,
    pub(crate) mode: MouseGestureMode,
    pub(crate) sensitivity: u8,
    pub(crate) rapid_click_enabled: bool,
    pub(crate) rapid_click_count: u8,
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
    left_pressed: bool,
    left_released: bool,
    native_click_count: u8,
}

#[derive(Clone, Copy, Debug)]
struct CompletedClick {
    x: f64,
    y: f64,
    at: Instant,
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

    fn reset_all(&mut self) {
        self.reset();
        self.cooldown_until = None;
    }

    fn push(&mut self, sample: PointerSample) {
        if sample.button_down || sample.left_pressed || sample.left_released {
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

#[derive(Default)]
struct RapidClickRecognizer {
    clicks: VecDeque<CompletedClick>,
    press: Option<PointerSample>,
    dragged: bool,
    cooldown_until: Option<Instant>,
}

impl RapidClickRecognizer {
    fn reset(&mut self) {
        self.clicks.clear();
        self.press = None;
        self.dragged = false;
    }

    fn reset_all(&mut self) {
        self.reset();
        self.cooldown_until = None;
    }

    fn push(&mut self, sample: PointerSample, required_clicks: u8) -> Option<(i32, i32)> {
        if self
            .cooldown_until
            .is_some_and(|deadline| sample.at < deadline)
        {
            return None;
        }

        if sample.left_pressed {
            self.press = Some(sample);
            self.dragged = false;
            return None;
        }

        // macOS 的全局事件回调在 leftMouseUp 到达时，系统按钮状态仍可能短暂报告为按下。
        // 释放沿必须优先处理，否则整组连击永远无法形成一次完整点击。
        if sample.left_released {
            return self.finish_click(sample, required_clicks);
        }

        if sample.button_down {
            if let Some(press) = self.press {
                let distance = ((sample.x - press.x).powi(2) + (sample.y - press.y).powi(2)).sqrt();
                if distance > RAPID_CLICK_RADIUS {
                    self.dragged = true;
                    self.clicks.clear();
                }
            } else {
                self.reset();
            }
            return None;
        }

        None
    }

    fn finish_click(&mut self, sample: PointerSample, required_clicks: u8) -> Option<(i32, i32)> {
        let Some(press) = self.press.take() else {
            self.clicks.clear();
            return None;
        };
        let distance = ((sample.x - press.x).powi(2) + (sample.y - press.y).powi(2)).sqrt();
        if self.dragged
            || sample.at.duration_since(press.at) > RAPID_CLICK_PRESS_MAX
            || distance > RAPID_CLICK_RADIUS
        {
            self.reset();
            return None;
        }

        let required_clicks =
            required_clicks.clamp(MIN_MOUSE_RAPID_CLICK_COUNT, MAX_MOUSE_RAPID_CLICK_COUNT);
        if sample.native_click_count >= required_clicks {
            self.reset();
            self.cooldown_until = Some(sample.at + COOLDOWN);
            return Some((sample.x.round() as i32, sample.y.round() as i32));
        }

        if self.clicks.back().is_some_and(|last| {
            sample.at.duration_since(last.at) > RAPID_CLICK_INTERVAL
                || ((sample.x - last.x).powi(2) + (sample.y - last.y).powi(2)).sqrt()
                    > RAPID_CLICK_RADIUS
        }) {
            self.clicks.clear();
        }
        self.clicks.push_back(CompletedClick {
            x: sample.x,
            y: sample.y,
            at: sample.at,
        });
        if self.clicks.len() < required_clicks as usize {
            return None;
        }

        self.reset();
        self.cooldown_until = Some(sample.at + COOLDOWN);
        Some((sample.x.round() as i32, sample.y.round() as i32))
    }
}

static EVENT_SENDER: OnceLock<Sender<PointerSample>> = OnceLock::new();
static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static BUTTON_DOWN: AtomicBool = AtomicBool::new(false);
static MONITOR_STARTED_AT: OnceLock<Instant> = OnceLock::new();
static LAST_SAMPLE_US: AtomicU64 = AtomicU64::new(0);
static RESET_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(crate) fn reset_detection() {
    RESET_GENERATION.fetch_add(1, Ordering::AcqRel);
}

fn apply_monitor_health(app: &tauri::AppHandle, health: Result<(), String>) {
    app.state::<RuntimeState>()
        .mouse_gesture_runtime
        .listening
        .store(health.is_ok(), Ordering::Release);
    if let Ok(mut error) = app
        .state::<RuntimeState>()
        .mouse_gesture_runtime
        .error
        .lock()
    {
        *error = health.err();
    }
}

pub(crate) fn resume_detection(app: &tauri::AppHandle) {
    reset_detection();
    let enabled = app
        .state::<RuntimeState>()
        .mouse_gesture
        .lock()
        .map(|settings| settings.enabled)
        .unwrap_or(false);
    if enabled && MONITOR_STARTED.load(Ordering::Acquire) {
        apply_monitor_health(app, platform::ensure_enabled());
    }
}

fn send_pointer_sample(
    x: f64,
    y: f64,
    button_down: bool,
    left_pressed: bool,
    left_released: bool,
    native_click_count: u8,
) {
    BUTTON_DOWN.store(button_down, Ordering::Release);
    let started = MONITOR_STARTED_AT.get_or_init(Instant::now);
    let now_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    if !button_down && !left_pressed && !left_released {
        let previous = LAST_SAMPLE_US.load(Ordering::Relaxed);
        if now_us.saturating_sub(previous) < 8_000 {
            return;
        }
        LAST_SAMPLE_US.store(now_us, Ordering::Relaxed);
    }
    if let Some(sender) = EVENT_SENDER.get() {
        let _ = sender.send(PointerSample {
            x,
            y,
            at: Instant::now(),
            button_down,
            left_pressed,
            left_released,
            native_click_count,
        });
    }
}

fn run_recognizer(app: tauri::AppHandle, receiver: Receiver<PointerSample>) {
    let mut recognizer = GestureRecognizer::default();
    let mut rapid_clicks = RapidClickRecognizer::default();
    let mut reset_generation = RESET_GENERATION.load(Ordering::Acquire);
    let mut next_health_check = Instant::now() + MONITOR_HEALTH_INTERVAL;
    loop {
        let requested_reset = RESET_GENERATION.load(Ordering::Acquire);
        if requested_reset != reset_generation {
            recognizer.reset_all();
            rapid_clicks.reset_all();
            reset_generation = requested_reset;
        }
        let sample = match receiver.recv_timeout(Duration::from_millis(16)) {
            Ok(sample) => {
                recognizer.push(sample);
                Some(sample)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let settings = app
            .state::<RuntimeState>()
            .mouse_gesture
            .lock()
            .map(|settings| settings.clone())
            .unwrap_or_default();
        if !settings.enabled || !MONITOR_STARTED.load(Ordering::Acquire) {
            recognizer.reset_all();
            rapid_clicks.reset_all();
            continue;
        }
        if Instant::now() >= next_health_check {
            next_health_check = Instant::now() + MONITOR_HEALTH_INTERVAL;
            apply_monitor_health(&app, platform::ensure_enabled());
        }
        if let Some(position) = sample.and_then(|sample| {
            settings
                .rapid_click_enabled
                .then(|| rapid_clicks.push(sample, settings.rapid_click_count))
                .flatten()
        }) {
            if !crate::desktop::floating_orb::is_cursor_over_floating_orb(&app) {
                crate::desktop::floating_orb::request_mouse_gesture(
                    app.clone(),
                    position,
                    settings.mode,
                );
            }
            recognizer.reset();
            continue;
        }
        if !settings.rapid_click_enabled {
            rapid_clicks.reset();
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
        rapid_click_enabled: settings.rapid_click_enabled,
        rapid_click_count: settings.rapid_click_count,
        available: state
            .mouse_gesture_runtime
            .listening
            .load(Ordering::Acquire),
        error,
    })
}

pub(crate) fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    if EVENT_SENDER.get().is_none() {
        let (sender, receiver) = channel();
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
    rapid_click_enabled: bool,
    rapid_click_count: u8,
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
            rapid_click_enabled,
            rapid_click_count,
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

    type Callback = fn(f64, f64, bool, bool, bool, u8);
    static CALLBACK: OnceLock<Callback> = OnceLock::new();
    static HANDLE: Mutex<Option<usize>> = Mutex::new(None);

    unsafe extern "C" fn receive(
        _context: *mut c_void,
        x: f64,
        y: f64,
        button_down: bool,
        left_pressed: bool,
        left_released: bool,
        click_count: u8,
    ) {
        if let Some(callback) = CALLBACK.get() {
            callback(x, y, button_down, left_pressed, left_released, click_count);
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

    pub(super) fn ensure_enabled() -> Result<(), String> {
        let handle = HANDLE
            .lock()
            .map_err(|_| "macOS 鼠标监听状态锁失败")?
            .ok_or_else(|| "macOS 全局鼠标监听未启动".to_string())?;
        crate::macos_native::ensure_mouse_monitor_enabled(handle)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::cell::RefCell;
    use std::sync::Mutex;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
    };
    use windows::Win32::UI::Input::{
        GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
        RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
        GetMessageW, PostThreadMessageW, RegisterClassW, TranslateMessage, CW_USEDEFAULT,
        HWND_MESSAGE, MSG, RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_5_DOWN,
        RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_LEFT_BUTTON_UP, RI_MOUSE_MIDDLE_BUTTON_DOWN,
        RI_MOUSE_RIGHT_BUTTON_DOWN, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WM_QUIT, WNDCLASSW,
    };

    type Callback = fn(f64, f64, bool, bool, bool, u8);
    static CALLBACK: OnceLock<Callback> = OnceLock::new();
    static THREAD_ID: Mutex<Option<u32>> = Mutex::new(None);
    thread_local! {
        // 每次监听线程重建都有独立状态，旧线程退出不能污染新线程的按钮状态。
        static BUTTONS: RefCell<RawMouseButtons> = RefCell::new(RawMouseButtons::default());
    }

    const BUTTON_DOWN_FLAGS: u32 = RI_MOUSE_LEFT_BUTTON_DOWN
        | RI_MOUSE_RIGHT_BUTTON_DOWN
        | RI_MOUSE_MIDDLE_BUTTON_DOWN
        | RI_MOUSE_BUTTON_4_DOWN
        | RI_MOUSE_BUTTON_5_DOWN;

    #[derive(Default)]
    struct RawMouseButtons {
        down_flags: u32,
    }

    impl RawMouseButtons {
        fn push(&mut self, flags: u32, mut emit: impl FnMut(bool, bool, bool)) {
            // 同一包若同时携带按下和释放，也要形成完整点击，不能把两个沿合成一个样本。
            let packets =
                if flags & RI_MOUSE_LEFT_BUTTON_DOWN != 0 && flags & RI_MOUSE_LEFT_BUTTON_UP != 0 {
                    [
                        Some(flags & !RI_MOUSE_LEFT_BUTTON_UP),
                        Some(RI_MOUSE_LEFT_BUTTON_UP),
                    ]
                } else {
                    [Some(flags), None]
                };
            for flags in packets.into_iter().flatten() {
                self.down_flags |= flags & BUTTON_DOWN_FLAGS;
                // RAWMOUSE 每个按钮的 UP 标记紧邻其 DOWN 标记，高一位。
                self.down_flags &= !((flags >> 1) & BUTTON_DOWN_FLAGS);
                emit(
                    self.down_flags != 0,
                    flags & RI_MOUSE_LEFT_BUTTON_DOWN != 0,
                    flags & RI_MOUSE_LEFT_BUTTON_UP != 0,
                );
            }
        }
    }

    fn mouse_button_flags(raw: &RAWINPUT, copied: u32) -> Result<Option<u32>, String> {
        if copied < std::mem::size_of::<RAWINPUTHEADER>() as u32 {
            return Err("Windows 原始鼠标输入头不完整".into());
        }
        if raw.header.dwType != RIM_TYPEMOUSE.0 {
            return Ok(None);
        }
        if copied < std::mem::size_of::<RAWINPUT>() as u32 {
            return Err("Windows 原始鼠标输入数据不完整".into());
        }
        // 已验证设备类型及结构长度，才能读取对应的 union 分支。
        Ok(Some(
            unsafe { raw.data.mouse.Anonymous.Anonymous.usButtonFlags } as u32,
        ))
    }

    fn read_mouse_button_flags(lparam: LPARAM) -> Result<Option<u32>, String> {
        // 本窗口只注册鼠标；使用对齐的固定结构，不在高频输入回调里分配字节缓冲区。
        let mut raw = RAWINPUT::default();
        let mut size = std::mem::size_of::<RAWINPUT>() as u32;
        let copied = unsafe {
            GetRawInputData(
                HRAWINPUT(lparam.0 as *mut _),
                RID_INPUT,
                Some((&mut raw as *mut RAWINPUT).cast()),
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if copied == u32::MAX {
            return Err(format!(
                "读取 Windows 原始鼠标输入失败：{}",
                windows::core::Error::from_win32()
            ));
        }
        mouse_button_flags(&raw, copied)
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_INPUT {
            // WM_INPUT 是队列中的事件；GetAsyncKeyState 是处理时的最新状态，
            // 无法还原排队期间的按下/释放。连击边沿必须以当前原始输入包为准。
            match read_mouse_button_flags(lparam) {
                Ok(Some(flags)) => {
                    let mut point = POINT::default();
                    let position = GetCursorPos(&mut point);
                    BUTTONS.with(|buttons| {
                        buttons.borrow_mut().push(flags, |down, pressed, released| {
                            if position.is_ok() {
                                if let Some(callback) = CALLBACK.get() {
                                    callback(
                                        point.x as f64,
                                        point.y as f64,
                                        down,
                                        pressed,
                                        released,
                                        0,
                                    );
                                }
                            }
                        });
                    });
                    if let Err(error) = position {
                        eprintln!("[mouse-gesture] 读取鼠标位置失败：{error}");
                    }
                }
                Ok(None) => {}
                Err(error) => eprintln!("[mouse-gesture] {error}"),
            }
        }
        // 前台原始输入仍需 DefWindowProc 完成系统清理。
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
            // 全局状态只用于启动时初始化已按住的按钮，不参与后续点击边沿推断。
            BUTTONS.with(|buttons| {
                let mut buttons = buttons.borrow_mut();
                for (key, flag) in [
                    (VK_LBUTTON, RI_MOUSE_LEFT_BUTTON_DOWN),
                    (VK_RBUTTON, RI_MOUSE_RIGHT_BUTTON_DOWN),
                    (VK_MBUTTON, RI_MOUSE_MIDDLE_BUTTON_DOWN),
                    (VK_XBUTTON1, RI_MOUSE_BUTTON_4_DOWN),
                    (VK_XBUTTON2, RI_MOUSE_BUTTON_5_DOWN),
                ] {
                    if (GetAsyncKeyState(key.0 as i32) as u16 & 0x8000) != 0 {
                        buttons.down_flags |= flag;
                    }
                }
            });
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

    pub(super) fn ensure_enabled() -> Result<(), String> {
        THREAD_ID
            .lock()
            .map_err(|_| "Windows 鼠标监听状态锁失败")?
            .is_some()
            .then_some(())
            .ok_or_else(|| "Windows 全局鼠标监听未启动".to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::Win32::UI::Input::RIM_TYPEKEYBOARD;
        use windows::Win32::UI::WindowsAndMessaging::RI_MOUSE_WHEEL;

        fn packet(flags: u32) -> RAWINPUT {
            let mut raw = RAWINPUT::default();
            raw.header.dwType = RIM_TYPEMOUSE.0;
            raw.header.dwSize = std::mem::size_of::<RAWINPUT>() as u32;
            raw.data.mouse.Anonymous.Anonymous.usButtonFlags = flags as u16;
            raw
        }

        fn feed(
            buttons: &mut RawMouseButtons,
            recognizer: &mut RapidClickRecognizer,
            flags: u32,
            at: Instant,
            x: f64,
            count: u8,
        ) -> Option<(i32, i32)> {
            let raw = packet(flags);
            let flags = mouse_button_flags(&raw, raw.header.dwSize)
                .unwrap()
                .unwrap();
            let mut result = None;
            buttons.push(flags, |down, pressed, released| {
                let detected = recognizer.push(
                    PointerSample {
                        x,
                        y: 80.0,
                        at,
                        button_down: down,
                        left_pressed: pressed,
                        left_released: released,
                        native_click_count: 0,
                    },
                    count,
                );
                if detected.is_some() {
                    assert!(result.is_none(), "同一输入包不能重复触发");
                    result = detected;
                }
            });
            result
        }

        #[test]
        fn queued_raw_edges_trigger_three_to_ten_clicks_without_mouse_motion() {
            // 即使处理消息时按钮已经全部松开，原始包仍保留每次按下和释放。
            for count in MIN_MOUSE_RAPID_CLICK_COUNT..=MAX_MOUSE_RAPID_CLICK_COUNT {
                let start = Instant::now();
                let mut buttons = RawMouseButtons::default();
                let mut recognizer = RapidClickRecognizer::default();
                for index in 0..count {
                    let at = start + Duration::from_millis(index as u64 * 160);
                    assert_eq!(
                        feed(
                            &mut buttons,
                            &mut recognizer,
                            RI_MOUSE_LEFT_BUTTON_DOWN,
                            at,
                            100.0,
                            count
                        ),
                        None
                    );
                    assert_eq!(
                        feed(
                            &mut buttons,
                            &mut recognizer,
                            RI_MOUSE_LEFT_BUTTON_UP,
                            at + Duration::from_millis(35),
                            100.0,
                            count
                        ),
                        (index + 1 == count).then_some((100, 80)),
                    );
                }
                assert_eq!(buttons.down_flags, 0);
                assert_eq!(
                    feed(
                        &mut buttons,
                        &mut recognizer,
                        RI_MOUSE_LEFT_BUTTON_DOWN | RI_MOUSE_LEFT_BUTTON_UP,
                        start + Duration::from_millis(count as u64 * 160),
                        100.0,
                        count
                    ),
                    None
                );
            }
        }

        #[test]
        fn combined_press_and_release_packet_keeps_both_edges() {
            let mut buttons = RawMouseButtons::default();
            let mut events = Vec::new();
            buttons.push(
                RI_MOUSE_LEFT_BUTTON_DOWN | RI_MOUSE_LEFT_BUTTON_UP,
                |down, pressed, released| {
                    events.push((down, pressed, released));
                },
            );
            assert_eq!(events, [(true, true, false), (false, false, true)]);
            assert_eq!(buttons.down_flags, 0);
        }

        #[test]
        fn motion_and_wheel_packets_do_not_invent_button_edges() {
            for button in [
                RI_MOUSE_LEFT_BUTTON_DOWN,
                RI_MOUSE_RIGHT_BUTTON_DOWN,
                RI_MOUSE_MIDDLE_BUTTON_DOWN,
                RI_MOUSE_BUTTON_4_DOWN,
                RI_MOUSE_BUTTON_5_DOWN,
            ] {
                let mut buttons = RawMouseButtons::default();
                buttons.push(button, |down, _, _| assert!(down));
                for flags in [0, RI_MOUSE_WHEEL, 0] {
                    buttons.push(flags, |down, pressed, released| {
                        assert_eq!((down, pressed, released), (true, false, false))
                    });
                }
                buttons.push(button << 1, |down, pressed, released| {
                    assert_eq!(
                        (down, pressed, released),
                        (false, false, button == RI_MOUSE_LEFT_BUTTON_DOWN)
                    );
                });
            }
        }

        #[test]
        fn raw_button_state_preserves_drag_rejection_even_when_cursor_returns() {
            let start = Instant::now();
            let mut buttons = RawMouseButtons::default();
            let mut recognizer = RapidClickRecognizer::default();
            for index in 0..3 {
                let at = start + Duration::from_millis(index * 160);
                assert_eq!(
                    feed(
                        &mut buttons,
                        &mut recognizer,
                        RI_MOUSE_LEFT_BUTTON_DOWN,
                        at,
                        100.0,
                        3
                    ),
                    None
                );
                assert_eq!(
                    feed(
                        &mut buttons,
                        &mut recognizer,
                        0,
                        at + Duration::from_millis(10),
                        130.0,
                        3
                    ),
                    None
                );
                assert_eq!(
                    feed(
                        &mut buttons,
                        &mut recognizer,
                        0,
                        at + Duration::from_millis(20),
                        100.0,
                        3
                    ),
                    None
                );
                assert_eq!(
                    feed(
                        &mut buttons,
                        &mut recognizer,
                        RI_MOUSE_LEFT_BUTTON_UP,
                        at + Duration::from_millis(35),
                        100.0,
                        3
                    ),
                    None
                );
            }
            assert!(recognizer.clicks.is_empty());
        }

        #[test]
        fn raw_input_decoder_rejects_truncated_mouse_packets_and_ignores_other_devices() {
            let mut raw = packet(RI_MOUSE_LEFT_BUTTON_UP);
            assert_eq!(
                mouse_button_flags(&raw, raw.header.dwSize).unwrap(),
                Some(RI_MOUSE_LEFT_BUTTON_UP)
            );
            assert!(mouse_button_flags(&raw, 0).is_err());
            assert!(mouse_button_flags(&raw, raw.header.dwSize - 1).is_err());
            raw.header.dwType = RIM_TYPEKEYBOARD.0;
            assert_eq!(mouse_button_flags(&raw, raw.header.dwSize).unwrap(), None);
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    type Callback = fn(f64, f64, bool, bool, bool, u8);
    pub(super) fn start(_callback: Callback) -> Result<(), String> {
        Err("当前平台不支持鼠标手势".into())
    }
    pub(super) fn stop() {}
    pub(super) fn ensure_enabled() -> Result<(), String> {
        Err("当前平台不支持鼠标手势".into())
    }
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
            left_pressed: false,
            left_released: false,
            native_click_count: 0,
        }
    }

    fn click(
        recognizer: &mut RapidClickRecognizer,
        x: f64,
        y: f64,
        at: Instant,
        required_clicks: u8,
    ) -> Option<(i32, i32)> {
        recognizer.push(
            PointerSample {
                x,
                y,
                at,
                button_down: true,
                left_pressed: true,
                left_released: false,
                native_click_count: 0,
            },
            required_clicks,
        );
        recognizer.push(
            PointerSample {
                x,
                y,
                at: at + Duration::from_millis(45),
                button_down: false,
                left_pressed: false,
                left_released: true,
                native_click_count: 0,
            },
            required_clicks,
        )
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
            left_pressed: false,
            left_released: false,
            native_click_count: 0,
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

    #[test]
    fn session_cleanup_clears_gesture_and_click_cooldowns() {
        let start = Instant::now();
        let mut gesture = GestureRecognizer {
            cooldown_until: Some(start + COOLDOWN),
            ..Default::default()
        };
        let mut clicks = RapidClickRecognizer {
            cooldown_until: Some(start + COOLDOWN),
            ..Default::default()
        };
        gesture.reset_all();
        clicks.reset_all();
        assert!(gesture.cooldown_until.is_none());
        assert!(clicks.cooldown_until.is_none());
    }

    #[test]
    fn three_rapid_left_clicks_trigger_on_the_final_release() {
        let start = Instant::now();
        let mut recognizer = RapidClickRecognizer::default();
        assert_eq!(click(&mut recognizer, 100.0, 80.0, start, 3), None);
        assert_eq!(
            click(
                &mut recognizer,
                102.0,
                81.0,
                start + Duration::from_millis(180),
                3,
            ),
            None,
        );
        assert_eq!(
            click(
                &mut recognizer,
                101.0,
                79.0,
                start + Duration::from_millis(360),
                3,
            ),
            Some((101, 79)),
        );
    }

    #[test]
    fn rapid_click_count_and_timeout_are_enforced() {
        let start = Instant::now();
        let mut recognizer = RapidClickRecognizer::default();
        for index in 0..3 {
            assert_eq!(
                click(
                    &mut recognizer,
                    40.0,
                    40.0,
                    start + Duration::from_millis(index * 160),
                    4,
                ),
                None,
            );
        }
        assert_eq!(
            click(
                &mut recognizer,
                40.0,
                40.0,
                start + Duration::from_millis(480),
                4,
            ),
            Some((40, 40)),
        );

        let mut slow = RapidClickRecognizer::default();
        assert_eq!(click(&mut slow, 10.0, 10.0, start, 3), None);
        assert_eq!(
            click(
                &mut slow,
                10.0,
                10.0,
                start + RAPID_CLICK_INTERVAL + Duration::from_millis(1),
                3,
            ),
            None,
        );
        assert_eq!(
            click(
                &mut slow,
                10.0,
                10.0,
                start + RAPID_CLICK_INTERVAL + Duration::from_millis(150),
                3,
            ),
            None,
        );
    }

    #[test]
    fn dragging_does_not_count_as_a_rapid_click() {
        let start = Instant::now();
        let mut recognizer = RapidClickRecognizer::default();
        recognizer.push(
            PointerSample {
                x: 0.0,
                y: 0.0,
                at: start,
                button_down: true,
                left_pressed: true,
                left_released: false,
                native_click_count: 0,
            },
            3,
        );
        recognizer.push(
            PointerSample {
                x: 30.0,
                y: 0.0,
                at: start + Duration::from_millis(40),
                button_down: true,
                left_pressed: false,
                left_released: false,
                native_click_count: 0,
            },
            3,
        );
        assert_eq!(
            recognizer.push(
                PointerSample {
                    x: 30.0,
                    y: 0.0,
                    at: start + Duration::from_millis(80),
                    button_down: false,
                    left_pressed: false,
                    left_released: true,
                    native_click_count: 0,
                },
                3,
            ),
            None,
        );
        assert!(recognizer.clicks.is_empty());
    }

    #[test]
    fn macos_release_edge_counts_even_if_combined_button_state_is_still_down() {
        let start = Instant::now();
        let mut recognizer = RapidClickRecognizer::default();
        for index in 0..3 {
            let at = start + Duration::from_millis(index * 160);
            assert_eq!(
                recognizer.push(
                    PointerSample {
                        x: 50.0,
                        y: 50.0,
                        at,
                        button_down: true,
                        left_pressed: true,
                        left_released: false,
                        native_click_count: 0,
                    },
                    3,
                ),
                None,
            );
            let result = recognizer.push(
                PointerSample {
                    x: 50.0,
                    y: 50.0,
                    at: at + Duration::from_millis(45),
                    button_down: true,
                    left_pressed: false,
                    left_released: true,
                    native_click_count: 0,
                },
                3,
            );
            if index < 2 {
                assert_eq!(result, None);
            } else {
                assert_eq!(result, Some((50, 50)));
            }
        }
    }

    #[test]
    fn macos_native_click_count_triggers_without_rebuilding_prior_click_history() {
        let start = Instant::now();
        let mut recognizer = RapidClickRecognizer::default();
        assert_eq!(
            recognizer.push(
                PointerSample {
                    x: 75.0,
                    y: 90.0,
                    at: start,
                    button_down: true,
                    left_pressed: true,
                    left_released: false,
                    native_click_count: 3,
                },
                3,
            ),
            None,
        );
        assert_eq!(
            recognizer.push(
                PointerSample {
                    x: 75.0,
                    y: 90.0,
                    at: start + Duration::from_millis(40),
                    button_down: false,
                    left_pressed: false,
                    left_released: true,
                    native_click_count: 3,
                },
                3,
            ),
            Some((75, 90)),
        );
    }
}

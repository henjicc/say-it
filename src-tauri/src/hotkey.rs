//! 全局热键（Windows 低级键盘钩子实现）。
//!
//! 为什么不用 `RegisterHotKey` / global-shortcut 插件：
//! - `RegisterHotKey` 无法注册 CapsLock 这类锁定键；
//! - 即使能注册，按下 CapsLock 仍会切换大小写状态。
//!
//! 低级钩子（`WH_KEYBOARD_LL`）可以捕获任意按键，并在命中目标键时“吞掉”事件，
//! 这样把 CapsLock 用作语音输入键时不会真的切换大小写。

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, HC_ACTION,
    KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

pub const MOD_CTRL: u8 = 1;
pub const MOD_SHIFT: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_WIN: u8 = 8;

const VK_CAPITAL: u16 = 0x14;
const VK_NUMLOCK: u16 = 0x90;
const VK_SCROLL: u16 = 0x91;

// 目标键与修饰键（由设置写入，钩子回调读取）。
static TARGET_VK: AtomicU16 = AtomicU16::new(VK_CAPITAL);
static TARGET_MODS: AtomicU8 = AtomicU8::new(0);
// 目标键当前是否处于“已触发未释放”，用于长按去重和成对吞掉 keyup。
static TRIGGERED: AtomicBool = AtomicBool::new(false);
static PRESS_HOLD_MODE: AtomicBool = AtomicBool::new(false);
static PRESS_HOLD_STARTED: AtomicBool = AtomicBool::new(false);
static PRESS_HOLD_START_MS: AtomicU32 = AtomicU32::new(260);
static PRESS_HOLD_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

// 实时字幕专用的第二路目标键，与语音输入完全独立。0 表示未设置。
static SUB_TARGET_VK: AtomicU16 = AtomicU16::new(0);
static SUB_TARGET_MODS: AtomicU8 = AtomicU8::new(0);
static SUB_TRIGGERED: AtomicBool = AtomicBool::new(false);

// 正在“设置快捷键”界面等待用户按键：此时任意锁定键（CapsLock/NumLock/ScrollLock）
// 一律吞掉、只上报按键，不让它真的切换锁定状态——避免设置过程中意外切换大小写。
static CAPTURING: AtomicBool = AtomicBool::new(false);

static APP: OnceLock<AppHandle> = OnceLock::new();
// 钩子回调 → 发送线程的信号通道。钩子回调里只做 send()（极快、非阻塞），
// 真正的 app.emit 放到独立线程，避免在低级钩子回调里耗时被系统超时卸载。
static TOGGLE_TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
static PRESS_TX: OnceLock<Mutex<Sender<PressSignal>>> = OnceLock::new();
static CANCEL_TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

enum PressSignal {
    Start { sequence: u32, delay_ms: u32 },
    End,
}
static SUB_TOGGLE_TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
static CAPTURE_TX: OnceLock<Mutex<Sender<u16>>> = OnceLock::new();
static DICTATION_ACTIVE: AtomicBool = AtomicBool::new(false);
static ESCAPE_TRIGGERED: AtomicBool = AtomicBool::new(false);

const VK_ESCAPE: u16 = 0x1B;

/// 保存 AppHandle 并安装一次键盘钩子（带消息循环的专用线程）。
pub fn init(app: AppHandle) {
    let _ = APP.set(app);
    if HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    // 发送线程：把钩子信号转成前端事件。
    let (tx, rx) = channel::<()>();
    let _ = TOGGLE_TX.set(Mutex::new(tx));
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            emit_toggle();
        }
    });

    let (press_tx, press_rx) = channel::<PressSignal>();
    let _ = PRESS_TX.set(Mutex::new(press_tx));
    std::thread::spawn(move || {
        while let Ok(signal) = press_rx.recv() {
            match signal {
                PressSignal::Start { sequence, delay_ms } => emit_press_start(sequence, delay_ms),
                PressSignal::End => emit_press_end(),
            }
        }
    });

    let (cancel_tx, cancel_rx) = channel::<()>();
    let _ = CANCEL_TX.set(Mutex::new(cancel_tx));
    std::thread::spawn(move || {
        while cancel_rx.recv().is_ok() {
            emit_cancel();
        }
    });

    let (sub_tx, sub_rx) = channel::<()>();
    let _ = SUB_TOGGLE_TX.set(Mutex::new(sub_tx));
    std::thread::spawn(move || {
        while sub_rx.recv().is_ok() {
            emit_subtitle_toggle();
        }
    });

    let (capture_tx, capture_rx) = channel::<u16>();
    let _ = CAPTURE_TX.set(Mutex::new(capture_tx));
    std::thread::spawn(move || {
        while let Ok(vk) = capture_rx.recv() {
            emit_capture_lock_key(vk);
        }
    });

    // 钩子线程（带消息循环）。
    std::thread::spawn(|| unsafe {
        let hook = match SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            HINSTANCE::default(),
            0,
        ) {
            Ok(h) => h,
            Err(e) => {
                HOOK_INSTALLED.store(false, Ordering::SeqCst);
                crate::dlog!("[hotkey] SetWindowsHookExW 失败: {e}");
                if let Some(app) = APP.get() {
                    let _ = app.emit(
                        "dictation-shortcut-error",
                        json!({ "message": format!("安装键盘钩子失败: {e}"), "key_code": "" }),
                    );
                }
                return;
            }
        };
        let _ = hook; // 进程存活期间一直挂着，无需主动卸载。
        crate::dlog!("[hotkey] 键盘钩子已安装");

        // WH_KEYBOARD_LL 要求安装线程有消息循环来分发钩子回调。
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// 钩子回调里调用：发一个非阻塞信号给发送线程。
fn signal_toggle() {
    if let Some(lock) = TOGGLE_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(());
        }
    }
}

fn signal_press_start(sequence: u32, delay_ms: u32) {
    if let Some(lock) = PRESS_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(PressSignal::Start { sequence, delay_ms });
        }
    }
}

fn signal_press_end() {
    if let Some(lock) = PRESS_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(PressSignal::End);
        }
    }
}

fn signal_cancel() {
    if let Some(lock) = CANCEL_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(());
        }
    }
}

fn signal_subtitle_toggle() {
    if let Some(lock) = SUB_TOGGLE_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(());
        }
    }
}

pub fn set_dictation_active(active: bool) {
    DICTATION_ACTIVE.store(active, Ordering::SeqCst);
    if !active {
        ESCAPE_TRIGGERED.store(false, Ordering::SeqCst);
    }
}

/// 更新当前热键（仅改原子，钩子已常驻）。
pub fn set_hotkey(vk: u16, mods: u8, press_hold_mode: bool) {
    TARGET_VK.store(vk, Ordering::SeqCst);
    TARGET_MODS.store(mods, Ordering::SeqCst);
    PRESS_HOLD_MODE.store(press_hold_mode, Ordering::SeqCst);
    PRESS_HOLD_STARTED.store(false, Ordering::SeqCst);
    TRIGGERED.store(false, Ordering::SeqCst);
    // 普通模式下锁定键会被无条件吞掉，因此先关闭锁定状态，避免卡在开启。
    if is_lock_key(vk) && !press_hold_mode {
        force_lock_off(vk);
    }
}

/// 清除语音输入热键（恢复未设置状态，语音输入将无法通过全局快捷键触发）。
pub fn clear_hotkey() {
    TARGET_VK.store(0, Ordering::SeqCst);
    TARGET_MODS.store(0, Ordering::SeqCst);
    PRESS_HOLD_MODE.store(false, Ordering::SeqCst);
    PRESS_HOLD_STARTED.store(false, Ordering::SeqCst);
    TRIGGERED.store(false, Ordering::SeqCst);
}

/// 设置实时字幕专用热键（与语音输入完全独立）。
pub fn set_subtitle_hotkey(vk: u16, mods: u8) {
    SUB_TARGET_VK.store(vk, Ordering::SeqCst);
    SUB_TARGET_MODS.store(mods, Ordering::SeqCst);
    SUB_TRIGGERED.store(false, Ordering::SeqCst);
    if is_lock_key(vk) {
        force_lock_off(vk);
    }
}

/// 清除实时字幕热键（恢复未设置状态）。
pub fn clear_subtitle_hotkey() {
    SUB_TARGET_VK.store(0, Ordering::SeqCst);
    SUB_TARGET_MODS.store(0, Ordering::SeqCst);
    SUB_TRIGGERED.store(false, Ordering::SeqCst);
}

/// 前端“设置快捷键”界面开始/结束等待按键时调用。开启期间锁定键一律被钩子吞掉，
/// 只上报按了哪个键，不会真的切换大小写/NumLock/ScrollLock。
pub fn set_capturing(active: bool) {
    CAPTURING.store(active, Ordering::SeqCst);
}

fn is_lock_key(vk: u16) -> bool {
    vk == VK_CAPITAL || vk == VK_NUMLOCK || vk == VK_SCROLL
}

/// 锁定键当前是否处于开启（toggle 位）。
fn lock_is_on(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) & 0x0001) != 0 }
}

/// 模拟敲一下某键（注入事件带 LLKHF_INJECTED，会被本钩子放行）。
unsafe fn tap_key(vk: u16) {
    let mk = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [mk(KEYBD_EVENT_FLAGS(0)), mk(KEYEVENTF_KEYUP)];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

/// 如果锁定键当前是开启状态，强制关掉。
fn force_lock_off(vk: u16) {
    if lock_is_on(vk) {
        unsafe { tap_key(vk) };
        crate::dlog!("[hotkey] 已强制关闭锁定键 vk={vk:#04x}");
    }
}

fn key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

fn modifiers_match(mods: u8) -> bool {
    let ctrl = key_down(VK_CONTROL.0);
    let shift = key_down(VK_SHIFT.0);
    let alt = key_down(VK_MENU.0);
    let win = key_down(VK_LWIN.0) || key_down(VK_RWIN.0);
    ((mods & MOD_CTRL != 0) == ctrl)
        && ((mods & MOD_SHIFT != 0) == shift)
        && ((mods & MOD_ALT != 0) == alt)
        && ((mods & MOD_WIN != 0) == win)
}

fn emit_toggle() {
    if let Some(app) = APP.get() {
        let _ = app.emit("dictation-toggle", json!({}));
    }
}

fn emit_press_start(sequence: u32, delay_ms: u32) {
    std::thread::sleep(Duration::from_millis(u64::from(delay_ms)));
    if !TRIGGERED.load(Ordering::SeqCst)
        || PRESS_HOLD_SEQUENCE.load(Ordering::SeqCst) != sequence
    {
        return;
    }
    PRESS_HOLD_STARTED.store(true, Ordering::SeqCst);
    if let Some(app) = APP.get() {
        let _ = app.emit("dictation-press-start", json!({}));
    }
}

fn emit_press_end() {
    if let Some(app) = APP.get() {
        let _ = app.emit("dictation-press-end", json!({}));
    }
}

fn emit_cancel() {
    if let Some(app) = APP.get() {
        let _ = app.emit("dictation-cancel", json!({}));
    }
}

fn emit_subtitle_toggle() {
    if let Some(app) = APP.get() {
        let _ = app.emit("subtitle-toggle", json!({}));
    }
}

fn emit_capture_lock_key(vk: u16) {
    if let Some(app) = APP.get() {
        let _ = app.emit("hotkey-capture-lock-key", json!({ "vk": vk }));
    }
}

fn signal_capture_lock_key(vk: u16) {
    if let Some(lock) = CAPTURE_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(vk);
        }
    }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);

        // 忽略我们自己（enigo）注入的按键，避免粘贴/键入时误触发热键。
        if (kb.flags.0 & LLKHF_INJECTED.0) != 0 {
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let vk = kb.vkCode as u16;
        let message = wparam.0 as u32;
        let is_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let is_up = message == WM_KEYUP || message == WM_SYSKEYUP;

        // 正在设置快捷键：锁定键一律吞掉、只上报，不让它真的切换大小写等状态，
        // 避免用户拿 CapsLock 之类的锁定键绑定快捷键时意外触发/卡死锁定状态。
        if CAPTURING.load(Ordering::SeqCst) && is_lock_key(vk) {
            if is_down {
                signal_capture_lock_key(vk);
            }
            return LRESULT(1);
        }

        if vk == VK_ESCAPE {
            if is_down && DICTATION_ACTIVE.load(Ordering::SeqCst) {
                if !ESCAPE_TRIGGERED.swap(true, Ordering::SeqCst) {
                    crate::dlog!("[hotkey] 触发速记取消");
                    signal_cancel();
                }
                return LRESULT(1);
            }
            if is_up && ESCAPE_TRIGGERED.swap(false, Ordering::SeqCst) {
                return LRESULT(1);
            }
        }

        let target = TARGET_VK.load(Ordering::SeqCst);
        let mods = TARGET_MODS.load(Ordering::SeqCst);

        if target != 0 && vk == target {
            let lock = is_lock_key(target);
            let press_hold_mode = PRESS_HOLD_MODE.load(Ordering::SeqCst);
            if is_down {
                if modifiers_match(mods) {
                    // 长按只在第一次按下触发一次。
                    if !TRIGGERED.swap(true, Ordering::SeqCst) {
                        if press_hold_mode {
                            PRESS_HOLD_STARTED.store(false, Ordering::SeqCst);
                            let sequence = PRESS_HOLD_SEQUENCE.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
                            let delay_ms = PRESS_HOLD_START_MS.load(Ordering::SeqCst);
                            crate::dlog!("[hotkey] 触发长按开始候选 (vk={vk:#04x})");
                            signal_press_start(sequence, delay_ms);
                        } else {
                            crate::dlog!("[hotkey] 触发 toggle (vk={vk:#04x})");
                            signal_toggle();
                        }
                    }
                }
                if lock {
                    // 普通模式吞掉锁定键；长按模式先吞掉，若短按释放再模拟一次系统点击。
                    return LRESULT(1);
                }
            } else if is_up {
                let was_triggered = TRIGGERED.swap(false, Ordering::SeqCst);
                let press_hold_started = PRESS_HOLD_STARTED.swap(false, Ordering::SeqCst);
                if was_triggered && press_hold_mode {
                    crate::dlog!("[hotkey] 触发长按结束 (vk={vk:#04x})");
                    signal_press_end();
                }
                if lock {
                    if press_hold_mode && was_triggered && !press_hold_started {
                        unsafe { tap_key(vk) };
                    }
                    // 成对吞掉 keyup，避免下游收到孤立的 keyup。
                    return LRESULT(1);
                }
            }
        }

        let sub_target = SUB_TARGET_VK.load(Ordering::SeqCst);
        if sub_target != 0 && vk == sub_target {
            let sub_mods = SUB_TARGET_MODS.load(Ordering::SeqCst);
            let lock = is_lock_key(sub_target);
            if is_down {
                if modifiers_match(sub_mods) {
                    if !SUB_TRIGGERED.swap(true, Ordering::SeqCst) {
                        crate::dlog!("[hotkey] 触发字幕 toggle (vk={vk:#04x})");
                        signal_subtitle_toggle();
                    }
                }
                if lock {
                    return LRESULT(1);
                }
            } else if is_up {
                SUB_TRIGGERED.store(false, Ordering::SeqCst);
                if lock {
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// 把浏览器 KeyboardEvent.code 映射为 Windows 虚拟键码。
pub fn code_to_vk(code: &str) -> Option<u16> {
    // 字母 KeyA..KeyZ
    if let Some(rest) = code.strip_prefix("Key") {
        let bytes = rest.as_bytes();
        if bytes.len() == 1 && bytes[0].is_ascii_uppercase() {
            return Some(bytes[0] as u16); // 'A'(0x41)..'Z'(0x5A)
        }
    }
    // 数字 Digit0..Digit9
    if let Some(rest) = code.strip_prefix("Digit") {
        let bytes = rest.as_bytes();
        if bytes.len() == 1 && bytes[0].is_ascii_digit() {
            return Some(bytes[0] as u16); // '0'(0x30)..'9'(0x39)
        }
    }
    // 功能键 F1..F24
    if let Some(rest) = code.strip_prefix('F') {
        if let Ok(n) = rest.parse::<u16>() {
            if (1..=24).contains(&n) {
                return Some(0x70 + (n - 1)); // VK_F1 = 0x70
            }
        }
    }
    let vk = match code {
        "CapsLock" => 0x14,
        "Space" => 0x20,
        "Enter" => 0x0D,
        "Tab" => 0x09,
        "Backquote" => 0xC0,
        "Backslash" => 0xDC,
        "Minus" => 0xBD,
        "Equal" => 0xBB,
        "BracketLeft" => 0xDB,
        "BracketRight" => 0xDD,
        "Semicolon" => 0xBA,
        "Quote" => 0xDE,
        "Comma" => 0xBC,
        "Period" => 0xBE,
        "Slash" => 0xBF,
        "ArrowLeft" => 0x25,
        "ArrowUp" => 0x26,
        "ArrowRight" => 0x27,
        "ArrowDown" => 0x28,
        "Insert" => 0x2D,
        "Delete" => 0x2E,
        "Home" => 0x24,
        "End" => 0x23,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "NumLock" => 0x90,
        "ScrollLock" => 0x91,
        "Pause" => 0x13,
        "PrintScreen" => 0x2C,
        _ => return None,
    };
    Some(vk)
}

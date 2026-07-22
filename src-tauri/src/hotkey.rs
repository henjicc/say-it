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
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
    VK_SHIFT,
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
const CONTEXT_DEBUG_VK: u16 = 0x77; // F8
const CONTEXT_DEBUG_MODS: u8 = MOD_CTRL | MOD_SHIFT;
const MAX_DICTATION_BINDINGS: usize = 1 + crate::state::MAX_DICTATION_SHORTCUT_PROFILES;

#[derive(Clone, Debug)]
pub struct HotkeyBinding {
    pub vk: u16,
    pub mods: u8,
    pub profile_id: Option<String>,
    pub press_hold_mode: bool,
}

// 固定容量原子槽位由设置写入、低级钩子读取；主快捷键为空时，首槽可直接存放方案快捷键。
static TARGET_VKS: [AtomicU16; MAX_DICTATION_BINDINGS] =
    [const { AtomicU16::new(0) }; MAX_DICTATION_BINDINGS];
static TARGET_MODS: [AtomicU8; MAX_DICTATION_BINDINGS] =
    [const { AtomicU8::new(0) }; MAX_DICTATION_BINDINGS];
static TRIGGERED: [AtomicBool; MAX_DICTATION_BINDINGS] =
    [const { AtomicBool::new(false) }; MAX_DICTATION_BINDINGS];
static BINDING_PRESS_HOLD: [AtomicBool; MAX_DICTATION_BINDINGS] =
    [const { AtomicBool::new(false) }; MAX_DICTATION_BINDINGS];
static PRESS_HOLD_STARTED: [AtomicBool; MAX_DICTATION_BINDINGS] =
    [const { AtomicBool::new(false) }; MAX_DICTATION_BINDINGS];
static PRESS_HOLD_START_MS: AtomicU32 = AtomicU32::new(260);
static PRESS_HOLD_SEQUENCE: [AtomicU32; MAX_DICTATION_BINDINGS] =
    [const { AtomicU32::new(0) }; MAX_DICTATION_BINDINGS];
static HOTKEY_GENERATION: AtomicU32 = AtomicU32::new(0);
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

// 实时字幕专用的第二路目标键，与语音输入完全独立。0 表示未设置。
static SUB_TARGET_VK: AtomicU16 = AtomicU16::new(0);
static SUB_TARGET_MODS: AtomicU8 = AtomicU8::new(0);
static SUB_TRIGGERED: AtomicBool = AtomicBool::new(false);
static CONTEXT_DEBUG_ACTIVE: AtomicBool = AtomicBool::new(false);
static CONTEXT_DEBUG_TRIGGERED: AtomicBool = AtomicBool::new(false);

// 正在“设置快捷键”界面等待用户按键：此时任意锁定键（CapsLock/NumLock/ScrollLock）
// 一律吞掉、只上报按键，不让它真的切换锁定状态——避免设置过程中意外切换大小写。
static CAPTURING: AtomicBool = AtomicBool::new(false);

static APP: OnceLock<AppHandle> = OnceLock::new();
// 钩子回调 → 发送线程的信号通道。钩子回调里只做 send()（极快、非阻塞），
// 真正的 app.emit 放到独立线程，避免在低级钩子回调里耗时被系统超时卸载。
static TOGGLE_TX: OnceLock<Mutex<Sender<HotkeySignal>>> = OnceLock::new();
static PRESS_TX: OnceLock<Mutex<Sender<PressSignal>>> = OnceLock::new();
static CANCEL_TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

enum PressSignal {
    Start {
        hotkey: HotkeySignal,
        sequence: u32,
        delay_ms: u32,
        immediate: bool,
    },
    End {
        hotkey: HotkeySignal,
    },
}

#[derive(Clone, Copy)]
struct HotkeySignal {
    slot: u8,
    generation: u32,
}

static BINDING_PROFILE_IDS: OnceLock<RwLock<Vec<Option<String>>>> = OnceLock::new();
static SUB_TOGGLE_TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
static CAPTURE_TX: OnceLock<Mutex<Sender<u16>>> = OnceLock::new();
static CONTEXT_DEBUG_TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
static DICTATION_ACTIVE: AtomicBool = AtomicBool::new(false);
static ESCAPE_TRIGGERED: AtomicBool = AtomicBool::new(false);

const VK_ESCAPE: u16 = 0x1B;

/// 保存 AppHandle 并安装一次键盘钩子（带消息循环的专用线程）。
pub fn init(app: AppHandle) {
    let _ = APP.set(app);
    let _ = BINDING_PROFILE_IDS.set(RwLock::new(Vec::new()));
    if HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    // 发送线程：把钩子信号转成前端事件。
    let (tx, rx) = channel::<HotkeySignal>();
    let _ = TOGGLE_TX.set(Mutex::new(tx));
    std::thread::spawn(move || {
        while let Ok(signal) = rx.recv() {
            emit_toggle(signal);
        }
    });

    let (press_tx, press_rx) = channel::<PressSignal>();
    let _ = PRESS_TX.set(Mutex::new(press_tx));
    std::thread::spawn(move || {
        while let Ok(signal) = press_rx.recv() {
            match signal {
                PressSignal::Start {
                    hotkey,
                    sequence,
                    delay_ms,
                    immediate,
                } => emit_press_start(hotkey, sequence, delay_ms, immediate),
                PressSignal::End { hotkey } => emit_press_end(hotkey),
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

    let (context_debug_tx, context_debug_rx) = channel::<()>();
    let _ = CONTEXT_DEBUG_TX.set(Mutex::new(context_debug_tx));
    std::thread::spawn(move || {
        while context_debug_rx.recv().is_ok() {
            emit_context_debug_capture();
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
fn signal_toggle(signal: HotkeySignal) {
    if let Some(lock) = TOGGLE_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(signal);
        }
    }
}

fn signal_press_start(hotkey: HotkeySignal, sequence: u32, delay_ms: u32, immediate: bool) {
    if let Some(lock) = PRESS_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(PressSignal::Start {
                hotkey,
                sequence,
                delay_ms,
                immediate,
            });
        }
    }
}

fn signal_press_end(hotkey: HotkeySignal) {
    if let Some(lock) = PRESS_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(PressSignal::End { hotkey });
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

fn signal_context_debug_capture() {
    if let Some(lock) = CONTEXT_DEBUG_TX.get() {
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

pub fn set_context_debug_active(active: bool) {
    CONTEXT_DEBUG_ACTIVE.store(active, Ordering::SeqCst);
    CONTEXT_DEBUG_TRIGGERED.store(false, Ordering::SeqCst);
}

/// 原子替换全部听写快捷键。低级钩子只读固定槽位，不获取任何锁。
pub fn set_hotkeys(bindings: &[HotkeyBinding]) -> Result<(), String> {
    if bindings.len() > MAX_DICTATION_BINDINGS {
        return Err(format!("听写快捷键不能超过 {MAX_DICTATION_BINDINGS} 个"));
    }
    let generation = HOTKEY_GENERATION
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    for slot in 0..MAX_DICTATION_BINDINGS {
        TARGET_VKS[slot].store(0, Ordering::SeqCst);
        TARGET_MODS[slot].store(0, Ordering::SeqCst);
        BINDING_PRESS_HOLD[slot].store(false, Ordering::SeqCst);
        TRIGGERED[slot].store(false, Ordering::SeqCst);
        PRESS_HOLD_STARTED[slot].store(false, Ordering::SeqCst);
        PRESS_HOLD_SEQUENCE[slot].fetch_add(1, Ordering::SeqCst);
    }
    if let Some(ids) = BINDING_PROFILE_IDS.get() {
        *ids.write().map_err(|_| "快捷键方案锁失败".to_string())? = bindings
            .iter()
            .map(|binding| binding.profile_id.clone())
            .collect();
    }
    for (slot, binding) in bindings.iter().enumerate() {
        TARGET_MODS[slot].store(binding.mods, Ordering::SeqCst);
        BINDING_PRESS_HOLD[slot].store(binding.press_hold_mode, Ordering::SeqCst);
        TARGET_VKS[slot].store(binding.vk, Ordering::SeqCst);
        if is_lock_key(binding.vk) && !binding.press_hold_mode {
            force_lock_off(binding.vk);
        }
    }
    crate::dlog!(
        "[hotkey] 已应用 {} 个听写快捷键，generation={generation}",
        bindings.len()
    );
    Ok(())
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

fn profile_for_signal(signal: HotkeySignal) -> Option<Option<String>> {
    if HOTKEY_GENERATION.load(Ordering::SeqCst) != signal.generation {
        return None;
    }
    BINDING_PROFILE_IDS
        .get()?
        .read()
        .ok()?
        .get(signal.slot as usize)
        .cloned()
}

fn emit_toggle(signal: HotkeySignal) {
    if let (Some(app), Some(profile_id)) = (APP.get(), profile_for_signal(signal)) {
        crate::application::dictation::request_toggle_with_profile(app.clone(), profile_id);
    }
}

fn emit_press_start(signal: HotkeySignal, sequence: u32, delay_ms: u32, immediate: bool) {
    std::thread::sleep(Duration::from_millis(u64::from(delay_ms)));
    let slot = signal.slot as usize;
    if slot >= MAX_DICTATION_BINDINGS
        || HOTKEY_GENERATION.load(Ordering::SeqCst) != signal.generation
        || (!immediate
            && (!TRIGGERED[slot].load(Ordering::SeqCst)
                || PRESS_HOLD_SEQUENCE[slot].load(Ordering::SeqCst) != sequence))
    {
        return;
    }
    PRESS_HOLD_STARTED[slot].store(true, Ordering::SeqCst);
    if let (Some(app), Some(profile_id)) = (APP.get(), profile_for_signal(signal)) {
        crate::application::dictation::request_start_with_profile(app.clone(), profile_id);
    }
}

fn emit_press_end(signal: HotkeySignal) {
    if HOTKEY_GENERATION.load(Ordering::SeqCst) == signal.generation {
        let Some(app) = APP.get() else { return };
        crate::application::dictation::request_stop(app.clone());
    }
}

fn emit_cancel() {
    if let Some(app) = APP.get() {
        crate::application::dictation::request_cancel(app.clone());
    }
}

fn emit_subtitle_toggle() {
    if let Some(app) = APP.get() {
        crate::application::subtitles::request_toggle(app.clone());
    }
}

fn emit_context_debug_capture() {
    if let Some(app) = APP.get() {
        crate::active_app_context::request_debug_capture(app.clone());
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

        if CONTEXT_DEBUG_ACTIVE.load(Ordering::SeqCst) && vk == CONTEXT_DEBUG_VK {
            if is_down && modifiers_match(CONTEXT_DEBUG_MODS) {
                if !CONTEXT_DEBUG_TRIGGERED.swap(true, Ordering::SeqCst) {
                    signal_context_debug_capture();
                }
                return LRESULT(1);
            }
            if is_up && CONTEXT_DEBUG_TRIGGERED.swap(false, Ordering::SeqCst) {
                return LRESULT(1);
            }
        }

        let generation = HOTKEY_GENERATION.load(Ordering::SeqCst);
        let mut matched_lock_key = false;
        let mut matching_toggle = None;
        let mut matching_press_hold = None;
        for slot in 0..MAX_DICTATION_BINDINGS {
            let target = TARGET_VKS[slot].load(Ordering::SeqCst);
            if target == 0 || target != vk {
                continue;
            }
            matched_lock_key |= is_lock_key(target);
            if is_down && modifiers_match(TARGET_MODS[slot].load(Ordering::SeqCst)) {
                if BINDING_PRESS_HOLD[slot].load(Ordering::SeqCst) {
                    matching_press_hold = Some(slot);
                } else {
                    matching_toggle = Some(slot);
                }
            }
        }

        if is_down {
            if let Some(slot) = matching_press_hold {
                if let Some(toggle_slot) = matching_toggle {
                    // 同一组合的短按与长按共存时，短按延迟到释放再触发。
                    TRIGGERED[toggle_slot].store(true, Ordering::SeqCst);
                }
                if !TRIGGERED[slot].swap(true, Ordering::SeqCst) {
                    PRESS_HOLD_STARTED[slot].store(false, Ordering::SeqCst);
                    let sequence = PRESS_HOLD_SEQUENCE[slot]
                        .fetch_add(1, Ordering::SeqCst)
                        .wrapping_add(1);
                    let immediate = DICTATION_ACTIVE.load(Ordering::SeqCst);
                    if immediate {
                        PRESS_HOLD_STARTED[slot].store(true, Ordering::SeqCst);
                    }
                    let delay_ms = if immediate {
                        0
                    } else {
                        PRESS_HOLD_START_MS.load(Ordering::SeqCst)
                    };
                    crate::dlog!("[hotkey] 触发长按开始候选 (slot={slot}, vk={vk:#04x})");
                    signal_press_start(
                        HotkeySignal {
                            slot: slot as u8,
                            generation,
                        },
                        sequence,
                        delay_ms,
                        immediate,
                    );
                }
            } else if let Some(slot) = matching_toggle {
                if !TRIGGERED[slot].swap(true, Ordering::SeqCst) {
                    crate::dlog!("[hotkey] 触发 toggle (slot={slot}, vk={vk:#04x})");
                    signal_toggle(HotkeySignal {
                        slot: slot as u8,
                        generation,
                    });
                }
            }
        } else if is_up {
            let mut handled_toggle_slots = [false; MAX_DICTATION_BINDINGS];
            for slot in 0..MAX_DICTATION_BINDINGS {
                if TARGET_VKS[slot].load(Ordering::SeqCst) != vk
                    || !BINDING_PRESS_HOLD[slot].load(Ordering::SeqCst)
                    || !TRIGGERED[slot].swap(false, Ordering::SeqCst)
                {
                    continue;
                }
                let mods = TARGET_MODS[slot].load(Ordering::SeqCst);
                let paired_toggle = (0..MAX_DICTATION_BINDINGS).find(|candidate| {
                    TARGET_VKS[*candidate].load(Ordering::SeqCst) == vk
                        && TARGET_MODS[*candidate].load(Ordering::SeqCst) == mods
                        && !BINDING_PRESS_HOLD[*candidate].load(Ordering::SeqCst)
                        && TRIGGERED[*candidate].swap(false, Ordering::SeqCst)
                });
                if let Some(toggle_slot) = paired_toggle {
                    handled_toggle_slots[toggle_slot] = true;
                }
                let press_hold_started = PRESS_HOLD_STARTED[slot].swap(false, Ordering::SeqCst);
                if press_hold_started {
                    crate::dlog!("[hotkey] 触发长按结束 (slot={slot}, vk={vk:#04x})");
                    signal_press_end(HotkeySignal {
                        slot: slot as u8,
                        generation,
                    });
                } else if let Some(toggle_slot) = paired_toggle {
                    crate::dlog!("[hotkey] 触发配对短按 (slot={toggle_slot}, vk={vk:#04x})");
                    signal_toggle(HotkeySignal {
                        slot: toggle_slot as u8,
                        generation,
                    });
                } else if is_lock_key(vk) {
                    unsafe { tap_key(vk) };
                }
            }
            for slot in 0..MAX_DICTATION_BINDINGS {
                if TARGET_VKS[slot].load(Ordering::SeqCst) == vk
                    && !BINDING_PRESS_HOLD[slot].load(Ordering::SeqCst)
                    && !handled_toggle_slots[slot]
                {
                    TRIGGERED[slot].store(false, Ordering::SeqCst);
                }
            }
        }
        if matched_lock_key {
            // 普通模式吞掉锁定键；长按模式短按释放时已模拟一次系统点击。
            return LRESULT(1);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_bindings_replace_slots_and_advance_generation() {
        let before = HOTKEY_GENERATION.load(Ordering::SeqCst);
        set_hotkeys(&[
            HotkeyBinding {
                vk: 0x78,
                mods: MOD_CTRL,
                profile_id: None,
                press_hold_mode: false,
            },
            HotkeyBinding {
                vk: 0x79,
                mods: MOD_SHIFT,
                profile_id: Some("smart".into()),
                press_hold_mode: true,
            },
        ])
        .unwrap();
        assert_eq!(TARGET_VKS[0].load(Ordering::SeqCst), 0x78);
        assert_eq!(TARGET_VKS[1].load(Ordering::SeqCst), 0x79);
        assert_eq!(TARGET_MODS[1].load(Ordering::SeqCst), MOD_SHIFT);
        assert!(!BINDING_PRESS_HOLD[0].load(Ordering::SeqCst));
        assert!(BINDING_PRESS_HOLD[1].load(Ordering::SeqCst));
        assert_ne!(HOTKEY_GENERATION.load(Ordering::SeqCst), before);

        set_hotkeys(&[]).unwrap();
        assert_eq!(TARGET_VKS[0].load(Ordering::SeqCst), 0);
        assert_eq!(TARGET_VKS[1].load(Ordering::SeqCst), 0);
    }

    #[test]
    fn binding_count_is_bounded_for_hook_safety() {
        let bindings = (0..=MAX_DICTATION_BINDINGS)
            .map(|index| HotkeyBinding {
                vk: 0x70 + index as u16,
                mods: 0,
                profile_id: None,
                press_hold_mode: false,
            })
            .collect::<Vec<_>>();
        assert!(set_hotkeys(&bindings).is_err());
    }
}

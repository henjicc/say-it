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
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LMENU, VK_LWIN, VK_MENU,
    VK_RMENU, VK_RWIN, VK_SHIFT,
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
/// 远程控制软件（如 UU 远程）同步大小写时会经驱动连发 CapsLock 的按下与抬起，
/// 不带注入标记、扫描码与真实键盘一致，只有按住时长在 1ms 内；人手按键至少持续数十毫秒。
/// 快捷键按下后延迟这么久才生效，期间抬起的视为模拟点击：作废触发，不再补发成锁定键切换。
const MIN_HUMAN_PRESS_MS: u32 = 15;
/// 本模块自行注入的按键标记（关闭已开启的锁定键、长按模式短按补发、屏蔽修饰键抬起）。
/// 钩子只放行带此标记的锁定键，外部程序注入的同款锁定键在非长按绑定下会被拦截。
const INTERNAL_EXTRA_INFO: usize = 0x5341_5949_544B;
/// `SendInput` 切换锁定键状态异步生效，短时间内不重复关闭同一锁定键，避免再次翻开。
const FORCE_LOCK_OFF_COOLDOWN: Duration = Duration::from_millis(150);

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
// 听写进行中按下长按键：在抬起时才决定是否停止，模拟点击不应停掉正在进行的听写。
static PRESS_STOPS_ACTIVE: [AtomicBool; MAX_DICTATION_BINDINGS] =
    [const { AtomicBool::new(false) }; MAX_DICTATION_BINDINGS];
// 每次按下递增；待发出的信号携带按下时的序号，序号变了说明这次按下已作废。
static PRESS_SEQUENCE: [AtomicU32; MAX_DICTATION_BINDINGS] =
    [const { AtomicU32::new(0) }; MAX_DICTATION_BINDINGS];
// 按下事件的高精度单调毫秒时间戳，抬起时据此判断是否为模拟点击（避免 GetTickCount 15.6ms 量化误差）。
static PRESS_DOWN_TIME: [AtomicU32; MAX_DICTATION_BINDINGS] =
    [const { AtomicU32::new(0) }; MAX_DICTATION_BINDINGS];
static HOOK_CLOCK_START: OnceLock<Instant> = OnceLock::new();
static HOTKEY_GENERATION: AtomicU32 = AtomicU32::new(0);
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

// 实时字幕专用的第二路目标键，与语音输入完全独立。0 表示未设置。
static SUB_TARGET_VK: AtomicU16 = AtomicU16::new(0);
static SUB_TARGET_MODS: AtomicU8 = AtomicU8::new(0);
static SUB_TRIGGERED: AtomicBool = AtomicBool::new(false);
static SUB_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static SUB_DOWN_TIME: AtomicU32 = AtomicU32::new(0);
static CONTEXT_DEBUG_ACTIVE: AtomicBool = AtomicBool::new(false);
static CONTEXT_DEBUG_TRIGGERED: AtomicBool = AtomicBool::new(false);

// 正在“设置快捷键”界面等待用户按键：此时任意锁定键（CapsLock/NumLock/ScrollLock）
// 一律吞掉、只上报按键，不让它真的切换锁定状态——避免设置过程中意外切换大小写。
static CAPTURING: AtomicBool = AtomicBool::new(false);

static APP: OnceLock<AppHandle> = OnceLock::new();
// 钩子回调 → 发送线程的信号通道。钩子回调里只做 send()（极快、非阻塞），
// 真正的 app.emit 放到独立线程，避免在低级钩子回调里耗时被系统超时卸载。
static TOGGLE_TX: OnceLock<Mutex<Sender<(HotkeySignal, u32)>>> = OnceLock::new();
static PRESS_TX: OnceLock<Mutex<Sender<PressSignal>>> = OnceLock::new();
static CANCEL_TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

enum PressSignal {
    Start {
        hotkey: HotkeySignal,
        delay_ms: u32,
    },
    End {
        hotkey: HotkeySignal,
    },
}

#[derive(Clone, Copy)]
struct HotkeySignal {
    slot: u8,
    generation: u32,
    sequence: u32,
}

static BINDING_PROFILE_IDS: OnceLock<RwLock<Vec<Option<String>>>> = OnceLock::new();
static SUB_TOGGLE_TX: OnceLock<Mutex<Sender<u32>>> = OnceLock::new();
static CAPTURE_TX: OnceLock<Mutex<Sender<u16>>> = OnceLock::new();
static CONTEXT_DEBUG_TX: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
static LOCK_KEY_TX: OnceLock<Mutex<Sender<LockKeyCommand>>> = OnceLock::new();
static DICTATION_ACTIVE: AtomicBool = AtomicBool::new(false);
static ESCAPE_TRIGGERED: AtomicBool = AtomicBool::new(false);

const VK_ESCAPE: u16 = 0x1B;

#[derive(Clone, Copy)]
enum LockKeyCommand {
    /// 如果锁定键当前意外处于开启状态，在钩子外异步补一次内部按键将其关闭。
    ForceOff(u16),
    /// 长按模式短按放行：在钩子外异步补发一次锁定键切换，避免在低级钩子回调内重入 SendInput。
    Tap(u16),
    /// 仅用于钩子外记录被拦截的外部/模拟锁定键事件，避免在低级钩子回调内写日志耗时。
    LogBlocked(&'static str, u16),
}

fn hook_now_ms() -> u32 {
    let start = HOOK_CLOCK_START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u32
}

/// 保存 AppHandle 并安装一次键盘钩子（带消息循环的专用线程）。
pub fn init(app: AppHandle) {
    let _ = APP.set(app);
    let _ = BINDING_PROFILE_IDS.set(RwLock::new(Vec::new()));
    let _ = HOOK_CLOCK_START.get_or_init(Instant::now);
    if HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    // 发送线程：把钩子信号转成前端事件。
    let (tx, rx) = channel::<(HotkeySignal, u32)>();
    let _ = TOGGLE_TX.set(Mutex::new(tx));
    std::thread::spawn(move || {
        while let Ok((signal, delay_ms)) = rx.recv() {
            emit_toggle(signal, delay_ms);
        }
    });

    let (press_tx, press_rx) = channel::<PressSignal>();
    let _ = PRESS_TX.set(Mutex::new(press_tx));
    std::thread::spawn(move || {
        while let Ok(signal) = press_rx.recv() {
            match signal {
                PressSignal::Start { hotkey, delay_ms } => emit_press_start(hotkey, delay_ms),
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

    let (sub_tx, sub_rx) = channel::<u32>();
    let _ = SUB_TOGGLE_TX.set(Mutex::new(sub_tx));
    std::thread::spawn(move || {
        while let Ok(sequence) = sub_rx.recv() {
            emit_subtitle_toggle(sequence);
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

    let (lock_tx, lock_rx) = channel::<LockKeyCommand>();
    let _ = LOCK_KEY_TX.set(Mutex::new(lock_tx));
    std::thread::spawn(move || {
        let mut last_force_off: Option<(u16, Instant)> = None;
        while let Ok(command) = lock_rx.recv() {
            match command {
                LockKeyCommand::ForceOff(vk) => {
                    let cooled_down = last_force_off.is_none_or(|(last_vk, at)| {
                        last_vk != vk || at.elapsed() >= FORCE_LOCK_OFF_COOLDOWN
                    });
                    if cooled_down && force_lock_off(vk) {
                        last_force_off = Some((vk, Instant::now()));
                    }
                }
                LockKeyCommand::Tap(vk) => unsafe { tap_key(vk) },
                LockKeyCommand::LogBlocked(reason, vk) => {
                    crate::dlog!("[hotkey] 已拦截锁定键 ({reason}, vk={vk:#04x})");
                }
            }
        }
    });

    // 钩子线程（带消息循环）。提升线程优先级，避免编译/测试等 CPU 峰值下回调超过
    // Windows LowLevelHooksTimeout 导致单次按键穿透并意外切换大写锁定。
    std::thread::spawn(|| unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
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
fn signal_toggle(signal: HotkeySignal, delay_ms: u32) {
    if let Some(lock) = TOGGLE_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send((signal, delay_ms));
        }
    }
}

fn signal_press_start(hotkey: HotkeySignal, delay_ms: u32) {
    if let Some(lock) = PRESS_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(PressSignal::Start { hotkey, delay_ms });
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

/// 字幕热键当前是否由全局键盘钩子接管。
/// 接管了就不需要前端的焦点兜底，否则同一次按键会 toggle 两次。
pub fn subtitle_hotkey_registered() -> bool {
    SUB_TARGET_VK.load(Ordering::SeqCst) != 0
}

fn signal_subtitle_toggle(sequence: u32) {
    if let Some(lock) = SUB_TOGGLE_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(sequence);
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

fn signal_lock_key(command: LockKeyCommand) {
    if let Some(lock) = LOCK_KEY_TX.get() {
        if let Ok(tx) = lock.lock() {
            let _ = tx.send(command);
        }
    } else if let LockKeyCommand::ForceOff(vk) = command {
        #[cfg(not(test))]
        {
            force_lock_off(vk);
        }
        #[cfg(test)]
        {
            let _ = vk;
        }
    }
}

pub fn set_dictation_active(active: bool) {
    DICTATION_ACTIVE.store(active, Ordering::SeqCst);
    if !active {
        ESCAPE_TRIGGERED.store(false, Ordering::SeqCst);
    }
}

pub fn set_context_debug_active(active: bool) -> Result<(), String> {
    CONTEXT_DEBUG_ACTIVE.store(active, Ordering::SeqCst);
    CONTEXT_DEBUG_TRIGGERED.store(false, Ordering::SeqCst);
    Ok(())
}

/// 原子替换全部听写快捷键。低级钩子只读固定槽位，不获取任何锁。
pub fn set_hotkeys(bindings: &[HotkeyBinding]) -> Result<(), String> {
    if bindings.len() > MAX_DICTATION_BINDINGS {
        return Err(format!("语音输入快捷键不能超过 {MAX_DICTATION_BINDINGS} 个"));
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
        PRESS_SEQUENCE[slot].fetch_add(1, Ordering::SeqCst);
        PRESS_STOPS_ACTIVE[slot].store(false, Ordering::SeqCst);
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
            signal_lock_key(LockKeyCommand::ForceOff(binding.vk));
        }
    }
    crate::dlog!(
        "[hotkey] 已应用 {} 个听写快捷键，generation={generation}",
        bindings.len()
    );
    Ok(())
}

/// 设置实时字幕专用热键（与语音输入完全独立）。
pub fn set_subtitle_hotkey(vk: u16, mods: u8) -> Result<(), String> {
    SUB_TARGET_VK.store(vk, Ordering::SeqCst);
    SUB_TARGET_MODS.store(mods, Ordering::SeqCst);
    SUB_TRIGGERED.store(false, Ordering::SeqCst);
    SUB_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    if is_lock_key(vk) {
        signal_lock_key(LockKeyCommand::ForceOff(vk));
    }
    Ok(())
}

/// 清除实时字幕热键（恢复未设置状态）。
pub fn clear_subtitle_hotkey() {
    SUB_TARGET_VK.store(0, Ordering::SeqCst);
    SUB_TARGET_MODS.store(0, Ordering::SeqCst);
    SUB_TRIGGERED.store(false, Ordering::SeqCst);
    SUB_SEQUENCE.fetch_add(1, Ordering::SeqCst);
}

/// 前端“设置快捷键”界面开始/结束等待按键时调用。开启期间锁定键一律被钩子吞掉，
/// 只上报按了哪个键，不会真的切换大小写/NumLock/ScrollLock。
pub fn set_capturing(active: bool) {
    CAPTURING.store(active, Ordering::SeqCst);
}

fn is_lock_key(vk: u16) -> bool {
    vk == VK_CAPITAL || vk == VK_NUMLOCK || vk == VK_SCROLL
}

/// 当前锁定键是否已被非长按快捷键（或快捷键录入）接管，需要禁止其切出锁定状态。
fn has_suppressed_lock_binding(vk: u16) -> bool {
    if !is_lock_key(vk) {
        return false;
    }
    if CAPTURING.load(Ordering::SeqCst) || SUB_TARGET_VK.load(Ordering::SeqCst) == vk {
        return true;
    }
    (0..MAX_DICTATION_BINDINGS).any(|slot| {
        TARGET_VKS[slot].load(Ordering::SeqCst) == vk
            && !BINDING_PRESS_HOLD[slot].load(Ordering::SeqCst)
    })
}

/// 锁定键当前是否处于开启（toggle 位）。
fn lock_is_on(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) & 0x0001) != 0 }
}

/// 模拟敲一下某键（带 `INTERNAL_EXTRA_INFO`，会被本钩子识别为内部注入并放行）。
unsafe fn tap_key(vk: u16) {
    let mk = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INTERNAL_EXTRA_INFO,
            },
        },
    };
    let inputs = [mk(KEYBD_EVENT_FLAGS(0)), mk(KEYEVENTF_KEYUP)];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

/// 未分配的虚拟键，前台程序收到也不会做任何事。
const VK_MASK: u16 = 0xE8;

/// 系统热键会吞掉主键，前台程序只看到 Alt 或 Win 被单独按下又松开：Alt 会打开菜单栏，
/// 随后模拟的粘贴键被菜单吃掉。在修饰键还按着时补一个无意义的按键，松开就不再算单独按下。
pub(crate) fn mask_modifier_release() {
    unsafe { tap_key(VK_MASK) };
}

/// 如果锁定键当前是开启状态，强制关掉。返回是否实际发出了关闭按键。
fn force_lock_off(vk: u16) -> bool {
    if lock_is_on(vk) {
        unsafe { tap_key(vk) };
        crate::dlog!("[hotkey] 已强制关闭锁定键 vk={vk:#04x}");
        return true;
    }
    false
}

/// 按住时长短于人手可能达到的下限，说明是程序模拟的点击。`time` 为系统毫秒时间戳，会回绕。
fn is_synthetic_tap(down_time: u32, up_time: u32) -> bool {
    up_time.wrapping_sub(down_time) < MIN_HUMAN_PRESS_MS
}

fn key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// Ctrl、Shift、Alt、Win 是否有任一还按着。
pub(crate) fn modifier_held() -> bool {
    [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|vk| key_down(vk.0))
}

fn modifiers_match(mods: u8) -> bool {
    modifiers_match_for_vk(mods, 0)
}

fn modifiers_match_for_vk(mods: u8, vk: u16) -> bool {
    let ctrl = key_down(VK_CONTROL.0);
    let shift = key_down(VK_SHIFT.0);
    // 单独绑定右 Alt（VK_RMENU）时，按下它自身会让 VK_MENU 为真，只看左 Alt 才不会误判。
    let alt = if vk == VK_RMENU.0 {
        key_down(VK_LMENU.0)
    } else {
        key_down(VK_MENU.0)
    };
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

fn press_still_current(signal: HotkeySignal) -> bool {
    let slot = signal.slot as usize;
    slot < MAX_DICTATION_BINDINGS
        && HOTKEY_GENERATION.load(Ordering::SeqCst) == signal.generation
        && PRESS_SEQUENCE[slot].load(Ordering::SeqCst) == signal.sequence
}

fn emit_toggle(signal: HotkeySignal, delay_ms: u32) {
    std::thread::sleep(Duration::from_millis(u64::from(delay_ms)));
    if !press_still_current(signal) {
        return;
    }
    crate::dlog!("[hotkey] 触发 toggle (slot={})", signal.slot);
    if let (Some(app), Some(profile_id)) = (APP.get(), profile_for_signal(signal)) {
        crate::application::dictation::request_toggle_with_profile(app.clone(), profile_id);
    }
}

fn emit_press_start(signal: HotkeySignal, delay_ms: u32) {
    std::thread::sleep(Duration::from_millis(u64::from(delay_ms)));
    let slot = signal.slot as usize;
    if !press_still_current(signal) || !TRIGGERED[slot].load(Ordering::SeqCst) {
        return;
    }
    PRESS_HOLD_STARTED[slot].store(true, Ordering::SeqCst);
    crate::dlog!("[hotkey] 触发长按开始 (slot={slot})");
    if let (Some(app), Some(profile_id)) = (APP.get(), profile_for_signal(signal)) {
        crate::application::dictation::request_start_with_profile(app.clone(), profile_id);
    }
}

fn emit_press_end(signal: HotkeySignal) {
    // 结束信号只随快捷键变更作废：同一键在信号排队期间再次按下，上一次的结束仍须送达。
    if HOTKEY_GENERATION.load(Ordering::SeqCst) == signal.generation {
        crate::dlog!("[hotkey] 触发长按结束 (slot={})", signal.slot);
        let Some(app) = APP.get() else { return };
        crate::application::dictation::request_stop(app.clone());
    }
}

fn emit_cancel() {
    crate::dlog!("[hotkey] 触发速记取消");
    if let Some(app) = APP.get() {
        crate::application::dictation::request_cancel(app.clone());
    }
}

fn emit_subtitle_toggle(sequence: u32) {
    std::thread::sleep(Duration::from_millis(u64::from(MIN_HUMAN_PRESS_MS)));
    if SUB_SEQUENCE.load(Ordering::SeqCst) != sequence {
        return;
    }
    crate::dlog!("[hotkey] 触发字幕 toggle");
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

/// 判断注入按键（`LLKHF_INJECTED`）是否应被吞掉。
/// - 带 `INTERNAL_EXTRA_INFO` 的是我们自己发出的关闭/补发/掩码按键，始终放行；
/// - 其余来源注入的已接管非长按锁定键直接吞掉，避免外部程序偷偷翻开大写锁定；
/// - 非锁定键（如 enigo 模拟的粘贴/键入）放行且不触发热键。
fn should_swallow_injected_key(vk: u16, extra_info: usize) -> bool {
    extra_info != INTERNAL_EXTRA_INFO && has_suppressed_lock_binding(vk)
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode as u16;
        let message = wparam.0 as u32;
        let is_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let is_up = message == WM_KEYUP || message == WM_SYSKEYUP;

        // 注入按键不触发热键；但外部程序注入的已接管锁定键仍要拦截，防止打开大写锁定。
        if (kb.flags.0 & LLKHF_INJECTED.0) != 0 {
            if should_swallow_injected_key(vk, kb.dwExtraInfo) {
                if is_down {
                    signal_lock_key(LockKeyCommand::LogBlocked("injected", vk));
                } else if is_up {
                    signal_lock_key(LockKeyCommand::ForceOff(vk));
                }
                return LRESULT(1);
            }
            return CallNextHookEx(None, code, wparam, lparam);
        }

        // 正在设置快捷键：锁定键一律吞掉、只上报，不让它真的切换大小写等状态，
        // 避免用户拿 CapsLock 之类的锁定键绑定快捷键时意外触发/卡死锁定状态。
        if CAPTURING.load(Ordering::SeqCst) && is_lock_key(vk) {
            if is_down {
                signal_capture_lock_key(vk);
            } else if is_up {
                signal_lock_key(LockKeyCommand::ForceOff(vk));
            }
            return LRESULT(1);
        }

        if vk == VK_ESCAPE {
            if is_down && DICTATION_ACTIVE.load(Ordering::SeqCst) {
                if !ESCAPE_TRIGGERED.swap(true, Ordering::SeqCst) {
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

        let now_ms = hook_now_ms();
        let generation = HOTKEY_GENERATION.load(Ordering::SeqCst);
        let mut matched_lock_key = false;
        let mut suppressed_lock_key = false;
        let mut matching_toggle = None;
        let mut matching_press_hold = None;
        for slot in 0..MAX_DICTATION_BINDINGS {
            let target = TARGET_VKS[slot].load(Ordering::SeqCst);
            if target == 0 || target != vk {
                continue;
            }
            let press_hold = BINDING_PRESS_HOLD[slot].load(Ordering::SeqCst);
            if is_lock_key(target) {
                matched_lock_key = true;
                suppressed_lock_key |= !press_hold;
            }
            if is_down && modifiers_match_for_vk(TARGET_MODS[slot].load(Ordering::SeqCst), vk) {
                if press_hold {
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
                    PRESS_DOWN_TIME[slot].store(now_ms, Ordering::SeqCst);
                    let sequence = PRESS_SEQUENCE[slot]
                        .fetch_add(1, Ordering::SeqCst)
                        .wrapping_add(1);
                    let stops_active = DICTATION_ACTIVE.load(Ordering::SeqCst);
                    PRESS_STOPS_ACTIVE[slot].store(stops_active, Ordering::SeqCst);
                    if !stops_active {
                        signal_press_start(
                            HotkeySignal {
                                slot: slot as u8,
                                generation,
                                sequence,
                            },
                            PRESS_HOLD_START_MS.load(Ordering::SeqCst),
                        );
                    }
                }
            } else if let Some(slot) = matching_toggle {
                if !TRIGGERED[slot].swap(true, Ordering::SeqCst) {
                    PRESS_DOWN_TIME[slot].store(now_ms, Ordering::SeqCst);
                    let sequence = PRESS_SEQUENCE[slot]
                        .fetch_add(1, Ordering::SeqCst)
                        .wrapping_add(1);
                    signal_toggle(
                        HotkeySignal {
                            slot: slot as u8,
                            generation,
                            sequence,
                        },
                        MIN_HUMAN_PRESS_MS,
                    );
                }
            }
        } else if is_up {
            let mut handled_toggle_slots = [false; MAX_DICTATION_BINDINGS];
            let mut saw_synthetic_lock = false;
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
                let stops_active = PRESS_STOPS_ACTIVE[slot].swap(false, Ordering::SeqCst);
                if is_synthetic_tap(PRESS_DOWN_TIME[slot].load(Ordering::SeqCst), now_ms) {
                    PRESS_SEQUENCE[slot].fetch_add(1, Ordering::SeqCst);
                    PRESS_HOLD_STARTED[slot].store(false, Ordering::SeqCst);
                    saw_synthetic_lock |= is_lock_key(vk);
                    continue;
                }
                let press_hold_started = PRESS_HOLD_STARTED[slot].swap(false, Ordering::SeqCst);
                if stops_active || press_hold_started {
                    signal_press_end(HotkeySignal {
                        slot: slot as u8,
                        generation,
                        sequence: PRESS_SEQUENCE[slot].load(Ordering::SeqCst),
                    });
                } else if let Some(toggle_slot) = paired_toggle {
                    let sequence = PRESS_SEQUENCE[toggle_slot]
                        .fetch_add(1, Ordering::SeqCst)
                        .wrapping_add(1);
                    signal_toggle(
                        HotkeySignal {
                            slot: toggle_slot as u8,
                            generation,
                            sequence,
                        },
                        0,
                    );
                } else if is_lock_key(vk) {
                    // 纯长按模式下的真实短按：保留锁定键原有功能，异步补发避免在钩子里重入。
                    signal_lock_key(LockKeyCommand::Tap(vk));
                }
            }
            for slot in 0..MAX_DICTATION_BINDINGS {
                if TARGET_VKS[slot].load(Ordering::SeqCst) == vk
                    && !BINDING_PRESS_HOLD[slot].load(Ordering::SeqCst)
                    && !handled_toggle_slots[slot]
                    && TRIGGERED[slot].swap(false, Ordering::SeqCst)
                    && is_synthetic_tap(PRESS_DOWN_TIME[slot].load(Ordering::SeqCst), now_ms)
                {
                    PRESS_SEQUENCE[slot].fetch_add(1, Ordering::SeqCst);
                    saw_synthetic_lock |= is_lock_key(vk);
                }
            }
            if saw_synthetic_lock {
                signal_lock_key(LockKeyCommand::LogBlocked("synthetic", vk));
            }
            if suppressed_lock_key {
                // 若此前被管理员窗口或外部途径意外打开了大写锁定，释放时自动归零。
                signal_lock_key(LockKeyCommand::ForceOff(vk));
            }
        }
        if matched_lock_key {
            // 非长按模式吞掉锁定键；纯长按模式真实短按释放时已异步补发一次系统点击。
            return LRESULT(1);
        }

        let sub_target = SUB_TARGET_VK.load(Ordering::SeqCst);
        if sub_target != 0 && vk == sub_target {
            let sub_mods = SUB_TARGET_MODS.load(Ordering::SeqCst);
            let lock = is_lock_key(sub_target);
            if is_down {
                if modifiers_match(sub_mods) && !SUB_TRIGGERED.swap(true, Ordering::SeqCst) {
                    SUB_DOWN_TIME.store(now_ms, Ordering::SeqCst);
                    let sequence = SUB_SEQUENCE.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
                    signal_subtitle_toggle(sequence);
                }
                if lock {
                    return LRESULT(1);
                }
            } else if is_up {
                if SUB_TRIGGERED.swap(false, Ordering::SeqCst)
                    && is_synthetic_tap(SUB_DOWN_TIME.load(Ordering::SeqCst), now_ms)
                {
                    SUB_SEQUENCE.fetch_add(1, Ordering::SeqCst);
                    if lock {
                        signal_lock_key(LockKeyCommand::LogBlocked("subtitle-synthetic", vk));
                    }
                }
                if lock {
                    signal_lock_key(LockKeyCommand::ForceOff(vk));
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
        "AltRight" => VK_RMENU.0,
        "Space" => 0x20,
        "Enter" => 0x0D,
        "Tab" => 0x09,
        "Backspace" => 0x08,
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

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn code_to_vk_maps_onboarding_presets() {
        assert_eq!(code_to_vk("CapsLock"), Some(0x14));
        assert_eq!(code_to_vk("AltRight"), Some(VK_RMENU.0));
        assert_eq!(code_to_vk("F8"), Some(0x77));
    }

    #[test]
    fn multiple_bindings_replace_slots_and_advance_generation() {
        let _guard = TEST_LOCK.lock().unwrap();
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
    fn instant_release_is_synthetic_tap_including_timestamp_wraparound() {
        assert!(is_synthetic_tap(1000, 1001));
        assert!(!is_synthetic_tap(1000, 1060));
        assert!(is_synthetic_tap(u32::MAX - 2, 3));
    }

    #[test]
    fn binding_count_is_bounded_for_hook_safety() {
        let _guard = TEST_LOCK.lock().unwrap();
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

    #[test]
    fn external_injected_caps_lock_is_swallowed_only_when_suppressed() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_capturing(false);
        clear_subtitle_hotkey();
        // 纯长按模式允许短按切换大写，不拦截外部注入；单击模式拦截外部注入，但始终放行本模块内部注入。
        set_hotkeys(&[HotkeyBinding {
            vk: VK_CAPITAL,
            mods: 0,
            profile_id: None,
            press_hold_mode: true,
        }])
        .unwrap();
        assert!(!should_swallow_injected_key(VK_CAPITAL, 0));

        set_hotkeys(&[HotkeyBinding {
            vk: VK_CAPITAL,
            mods: 0,
            profile_id: None,
            press_hold_mode: false,
        }])
        .unwrap();
        assert!(should_swallow_injected_key(VK_CAPITAL, 0));
        assert!(!should_swallow_injected_key(VK_CAPITAL, INTERNAL_EXTRA_INFO));
        assert!(!should_swallow_injected_key(0x41, 0));

        set_hotkeys(&[]).unwrap();
        assert!(!should_swallow_injected_key(VK_CAPITAL, 0));
    }
}

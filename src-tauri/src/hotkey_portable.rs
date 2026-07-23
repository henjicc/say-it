//! macOS 等非 Windows 平台使用 Tauri 全局快捷键；macOS 的 Caps Lock 由
//! Quartz 事件过滤器单独处理，以便触发听写时吞掉锁定状态切换。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;
#[cfg(target_os = "macos")]
use tauri::Emitter;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const MOD_CTRL: u8 = 1;
pub const MOD_SHIFT: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_WIN: u8 = 8;

#[derive(Clone, Debug)]
pub struct HotkeyBinding {
    pub vk: u16,
    pub mods: u8,
    pub profile_id: Option<String>,
    pub press_hold_mode: bool,
}

#[derive(Clone, Default)]
struct RegisteredSet {
    bindings: Vec<HotkeyBinding>,
    shortcuts: Vec<String>,
}

#[derive(Default)]
struct PortablePressState {
    pressed: AtomicBool,
    started: AtomicBool,
    sequence: AtomicU32,
}

struct PortableBindingGroup {
    shortcut: String,
    toggle_profile_id: Option<Option<String>>,
    press_hold_profile_id: Option<Option<String>>,
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct CapsLockBinding {
    mods: u8,
    profile_id: Option<String>,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static DICTATION_SHORTCUTS: OnceLock<Mutex<RegisteredSet>> = OnceLock::new();
static SUBTITLE_SHORTCUT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static DICTATION_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static CAPS_LOCK_BINDING: OnceLock<Mutex<Option<CapsLockBinding>>> = OnceLock::new();
#[cfg(target_os = "macos")]
static CAPS_LOCK_TAP: OnceLock<Mutex<Option<usize>>> = OnceLock::new();
#[cfg(target_os = "macos")]
static CAPTURING: AtomicBool = AtomicBool::new(false);

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
    let _ = DICTATION_SHORTCUTS.set(Mutex::new(RegisteredSet::default()));
    let _ = SUBTITLE_SHORTCUT.set(Mutex::new(None));
    #[cfg(target_os = "macos")]
    {
        let _ = CAPS_LOCK_BINDING.set(Mutex::new(None));
        let _ = CAPS_LOCK_TAP.set(Mutex::new(None));
    }
}

pub fn set_dictation_active(active: bool) {
    DICTATION_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn set_context_debug_active(_active: bool) {
    // 当前软件上下文调试仅支持 Windows。
}

fn register_bindings(app: &AppHandle, bindings: &[HotkeyBinding]) -> Result<Vec<String>, String> {
    let mut groups = Vec::<PortableBindingGroup>::new();
    #[cfg(target_os = "macos")]
    let mut caps_lock_binding = None;
    for binding in bindings {
        #[cfg(target_os = "macos")]
        if binding.vk == 0x14 {
            if binding.press_hold_mode {
                return Err("macOS 的 Caps Lock 仅支持按一下开关模式".into());
            }
            if caps_lock_binding.is_some() {
                return Err("Caps Lock 开关快捷键重复".into());
            }
            caps_lock_binding = Some(CapsLockBinding {
                mods: binding.mods,
                profile_id: binding.profile_id.clone(),
            });
            continue;
        }
        let shortcut = shortcut_string(binding.vk, binding.mods)
            .ok_or_else(|| "当前平台不支持这个快捷键".to_string())?;
        let group = if let Some(group) = groups.iter_mut().find(|group| group.shortcut == shortcut)
        {
            group
        } else {
            groups.push(PortableBindingGroup {
                shortcut: shortcut.clone(),
                toggle_profile_id: None,
                press_hold_profile_id: None,
            });
            groups.last_mut().expect("portable shortcut group")
        };
        let target = if binding.press_hold_mode {
            &mut group.press_hold_profile_id
        } else {
            &mut group.toggle_profile_id
        };
        if target.is_some() {
            return Err(format!("快捷键 {shortcut} 的相同触发方式重复"));
        }
        *target = Some(binding.profile_id.clone());
    }

    let mut shortcuts = Vec::with_capacity(groups.len());
    for group in groups {
        let shortcut = group.shortcut;
        let toggle_profile_id = group.toggle_profile_id;
        let press_hold_profile_id = group.press_hold_profile_id;
        let press_state = Arc::new(PortablePressState::default());
        if let Err(error) =
            app.global_shortcut()
                .on_shortcut(shortcut.as_str(), move |app, _, event| match event.state {
                    ShortcutState::Pressed => {
                        if press_hold_profile_id.is_none() {
                            if let Some(profile_id) = toggle_profile_id.clone() {
                                crate::application::dictation::request_toggle_with_profile(
                                    app.clone(),
                                    profile_id,
                                );
                            }
                            return;
                        }
                        if press_state.pressed.swap(true, Ordering::SeqCst) {
                            return;
                        }
                        press_state.started.store(false, Ordering::SeqCst);
                        let sequence = press_state
                            .sequence
                            .fetch_add(1, Ordering::SeqCst)
                            .wrapping_add(1);
                        let profile_id = press_hold_profile_id
                            .clone()
                            .expect("press-hold profile exists");
                        if DICTATION_ACTIVE.load(Ordering::SeqCst) {
                            press_state.started.store(true, Ordering::SeqCst);
                            crate::application::dictation::request_start_with_profile(
                                app.clone(),
                                profile_id,
                            );
                            return;
                        }
                        let delayed_app = app.clone();
                        let delayed_state = press_state.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(260));
                            if delayed_state.pressed.load(Ordering::SeqCst)
                                && delayed_state.sequence.load(Ordering::SeqCst) == sequence
                            {
                                delayed_state.started.store(true, Ordering::SeqCst);
                                crate::application::dictation::request_start_with_profile(
                                    delayed_app,
                                    profile_id,
                                );
                            }
                        });
                    }
                    ShortcutState::Released => {
                        if press_hold_profile_id.is_none()
                            || !press_state.pressed.swap(false, Ordering::SeqCst)
                        {
                            return;
                        }
                        press_state.sequence.fetch_add(1, Ordering::SeqCst);
                        if press_state.started.swap(false, Ordering::SeqCst) {
                            crate::application::dictation::request_stop(app.clone());
                        } else if let Some(profile_id) = toggle_profile_id.clone() {
                            crate::application::dictation::request_toggle_with_profile(
                                app.clone(),
                                profile_id,
                            );
                        }
                    }
                })
        {
            unregister_shortcuts(app, &shortcuts);
            return Err(format!("注册快捷键 {shortcut} 失败：{error}"));
        }
        shortcuts.push(shortcut);
    }
    #[cfg(target_os = "macos")]
    if let Err(error) = configure_caps_lock(caps_lock_binding) {
        unregister_shortcuts(app, &shortcuts);
        return Err(error);
    }
    Ok(shortcuts)
}

fn unregister_shortcuts(app: &AppHandle, shortcuts: &[String]) {
    for shortcut in shortcuts {
        let _ = app.global_shortcut().unregister(shortcut.as_str());
    }
}

/// 事务式替换全部听写快捷键；新集合注册失败时恢复旧集合。
pub fn set_hotkeys(bindings: &[HotkeyBinding]) -> Result<(), String> {
    let app = APP
        .get()
        .ok_or_else(|| "全局快捷键尚未初始化".to_string())?;
    let storage = DICTATION_SHORTCUTS
        .get()
        .ok_or_else(|| "全局快捷键状态尚未初始化".to_string())?;
    let mut current = storage
        .lock()
        .map_err(|_| "全局快捷键状态锁失败".to_string())?;
    let previous = current.clone();
    unregister_shortcuts(app, &previous.shortcuts);
    #[cfg(target_os = "macos")]
    configure_caps_lock(None)?;
    match register_bindings(app, bindings) {
        Ok(shortcuts) => {
            *current = RegisteredSet {
                bindings: bindings.to_vec(),
                shortcuts,
            };
            Ok(())
        }
        Err(error) => {
            match register_bindings(app, &previous.bindings) {
                Ok(shortcuts) => {
                    *current = RegisteredSet {
                        shortcuts,
                        ..previous
                    }
                }
                Err(restore_error) => {
                    *current = RegisteredSet::default();
                    return Err(format!("{error}；恢复原快捷键失败：{restore_error}"));
                }
            }
            Err(error)
        }
    }
}

fn register_subtitle_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                crate::application::subtitles::request_toggle(app.clone());
            }
        })
        .map_err(|error| format!("注册跨平台字幕快捷键失败：{error}"))
}

pub fn set_subtitle_hotkey(vk: u16, mods: u8) -> Result<(), String> {
    let app = APP
        .get()
        .ok_or_else(|| "全局快捷键尚未初始化".to_string())?;
    let shortcut =
        shortcut_string(vk, mods).ok_or_else(|| "当前平台不支持这个字幕快捷键".to_string())?;
    let storage = SUBTITLE_SHORTCUT
        .get()
        .ok_or_else(|| "字幕快捷键状态尚未初始化".to_string())?;
    let mut current = storage
        .lock()
        .map_err(|_| "字幕快捷键状态锁失败".to_string())?;
    let previous = current.take();
    if let Some(old) = &previous {
        let _ = app.global_shortcut().unregister(old.as_str());
    }
    if let Err(error) = register_subtitle_shortcut(app, &shortcut) {
        if let Some(old) = previous {
            match register_subtitle_shortcut(app, &old) {
                Ok(()) => *current = Some(old),
                Err(restore_error) => {
                    return Err(format!("{error}；恢复原字幕快捷键失败：{restore_error}"));
                }
            }
        }
        return Err(error);
    }
    *current = Some(shortcut);
    Ok(())
}

pub fn clear_subtitle_hotkey() {
    unregister(SUBTITLE_SHORTCUT.get());
}

pub fn set_capturing(active: bool) {
    #[cfg(target_os = "macos")]
    {
        CAPTURING.store(active, Ordering::SeqCst);
        let _ = refresh_caps_lock_tap();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = active;
}

pub fn code_to_vk(code: &str) -> Option<u16> {
    match code.trim() {
        #[cfg(target_os = "macos")]
        "CapsLock" => Some(0x14),
        "Space" => Some(0x20),
        "Enter" => Some(0x0d),
        "Tab" => Some(0x09),
        "Escape" => Some(0x1b),
        "Backspace" => Some(0x08),
        "Backquote" => Some(0xc0),
        "Backslash" => Some(0xdc),
        "Minus" => Some(0xbd),
        "Equal" => Some(0xbb),
        "BracketLeft" => Some(0xdb),
        "BracketRight" => Some(0xdd),
        "Semicolon" => Some(0xba),
        "Quote" => Some(0xde),
        "Comma" => Some(0xbc),
        "Period" => Some(0xbe),
        "Slash" => Some(0xbf),
        "ArrowLeft" => Some(0x25),
        "ArrowUp" => Some(0x26),
        "ArrowRight" => Some(0x27),
        "ArrowDown" => Some(0x28),
        "Insert" => Some(0x2d),
        "Delete" => Some(0x2e),
        "Home" => Some(0x24),
        "End" => Some(0x23),
        "PageUp" => Some(0x21),
        "PageDown" => Some(0x22),
        "Pause" => Some(0x13),
        "PrintScreen" => Some(0x2c),
        value if value.len() == 4 && value.starts_with("Key") => {
            value.as_bytes().get(3).copied().map(u16::from)
        }
        value if value.len() == 6 && value.starts_with("Digit") => {
            value.as_bytes().get(5).copied().map(u16::from)
        }
        value if value.starts_with('F') => value[1..]
            .parse::<u16>()
            .ok()
            .filter(|value| (1..=20).contains(value))
            .map(|value| 0x70 + value - 1),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn configure_caps_lock(binding: Option<CapsLockBinding>) -> Result<(), String> {
    let storage = CAPS_LOCK_BINDING
        .get()
        .ok_or_else(|| "Caps Lock 快捷键状态尚未初始化".to_string())?;
    *storage
        .lock()
        .map_err(|_| "Caps Lock 快捷键状态锁失败".to_string())? = binding;
    if let Err(error) = refresh_caps_lock_tap() {
        if let Ok(mut current) = storage.lock() {
            *current = None;
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn refresh_caps_lock_tap() -> Result<(), String> {
    let needed = CAPTURING.load(Ordering::SeqCst)
        || CAPS_LOCK_BINDING
            .get()
            .and_then(|binding| binding.lock().ok())
            .is_some_and(|binding| binding.is_some());
    let storage = CAPS_LOCK_TAP
        .get()
        .ok_or_else(|| "Caps Lock 事件过滤器尚未初始化".to_string())?;
    let mut handle = storage
        .lock()
        .map_err(|_| "Caps Lock 事件过滤器状态锁失败".to_string())?;
    if needed && handle.is_none() {
        *handle = Some(crate::macos_native::start_caps_lock_tap(
            handle_caps_lock_event,
        )?);
    } else if !needed {
        if let Some(current) = handle.take() {
            crate::macos_native::stop_caps_lock_tap(current);
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn handle_caps_lock_event(_context: *mut std::ffi::c_void, flags: u64) -> bool {
    if CAPTURING.load(Ordering::SeqCst) {
        if let Some(app) = APP.get() {
            let _ = app.emit("hotkey-capture-lock-key", serde_json::json!({ "vk": 0x14 }));
        }
        return true;
    }
    let binding = CAPS_LOCK_BINDING
        .get()
        .and_then(|binding| binding.lock().ok())
        .and_then(|binding| binding.clone());
    if let (Some(binding), Some(app)) = (binding, APP.get()) {
        if macos_modifiers(flags) == binding.mods {
            crate::application::dictation::request_toggle_with_profile(
                app.clone(),
                binding.profile_id,
            );
        }
    }
    true
}

#[cfg(target_os = "macos")]
fn macos_modifiers(flags: u64) -> u8 {
    let mut mods = 0;
    if flags & (1 << 18) != 0 {
        mods |= MOD_CTRL;
    }
    if flags & (1 << 17) != 0 {
        mods |= MOD_SHIFT;
    }
    if flags & (1 << 19) != 0 {
        mods |= MOD_ALT;
    }
    if flags & (1 << 20) != 0 {
        mods |= MOD_WIN;
    }
    mods
}

fn shortcut_string(vk: u16, mods: u8) -> Option<String> {
    let mut parts = Vec::new();
    if mods & MOD_CTRL != 0 {
        parts.push("Control".to_string());
    }
    if mods & MOD_SHIFT != 0 {
        parts.push("Shift".to_string());
    }
    if mods & MOD_ALT != 0 {
        parts.push("Alt".to_string());
    }
    if mods & MOD_WIN != 0 {
        parts.push("CommandOrControl".to_string());
    }
    let key = match vk {
        0x20 => "Space".to_string(),
        0x0d => "Enter".to_string(),
        0x09 => "Tab".to_string(),
        0x1b => "Escape".to_string(),
        0x08 => "Backspace".to_string(),
        0xc0 => "Backquote".to_string(),
        0xdc => "Backslash".to_string(),
        0xbd => "Minus".to_string(),
        0xbb => "Equal".to_string(),
        0xdb => "BracketLeft".to_string(),
        0xdd => "BracketRight".to_string(),
        0xba => "Semicolon".to_string(),
        0xde => "Quote".to_string(),
        0xbc => "Comma".to_string(),
        0xbe => "Period".to_string(),
        0xbf => "Slash".to_string(),
        0x25 => "ArrowLeft".to_string(),
        0x26 => "ArrowUp".to_string(),
        0x27 => "ArrowRight".to_string(),
        0x28 => "ArrowDown".to_string(),
        0x2d => "Insert".to_string(),
        0x2e => "Delete".to_string(),
        0x24 => "Home".to_string(),
        0x23 => "End".to_string(),
        0x21 => "PageUp".to_string(),
        0x22 => "PageDown".to_string(),
        0x13 => "Pause".to_string(),
        0x2c => "PrintScreen".to_string(),
        0x41..=0x5a => char::from_u32(u32::from(vk))?.to_string(),
        0x30..=0x39 => char::from_u32(u32::from(vk))?.to_string(),
        0x70..=0x83 => format!("F{}", vk - 0x70 + 1),
        _ => return None,
    };
    parts.push(key);
    Some(parts.join("+"))
}

fn unregister(slot: Option<&Mutex<Option<String>>>) {
    let Some(app) = APP.get() else { return };
    let Some(slot) = slot else { return };
    if let Some(shortcut) = slot.lock().expect("shortcut lock").take() {
        let _ = app.global_shortcut().unregister(shortcut.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_key_codes_convert_to_portable_accelerators() {
        for (code, expected) in [
            ("KeyA", "A"),
            ("Digit7", "7"),
            ("Backquote", "Backquote"),
            ("ArrowLeft", "ArrowLeft"),
            ("PageDown", "PageDown"),
            ("F20", "F20"),
        ] {
            let vk = code_to_vk(code).expect("supported browser key code");
            assert_eq!(shortcut_string(vk, 0).as_deref(), Some(expected));
        }
    }

    #[test]
    fn command_shift_space_remains_available() {
        assert_eq!(
            shortcut_string(0x20, MOD_WIN | MOD_SHIFT).as_deref(),
            Some("Shift+CommandOrControl+Space")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn caps_lock_uses_native_event_tap_key_code() {
        assert_eq!(code_to_vk("CapsLock"), Some(0x14));
        assert_eq!(macos_modifiers((1 << 17) | (1 << 20)), MOD_SHIFT | MOD_WIN);
    }
}

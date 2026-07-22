//! macOS 等非 Windows 平台使用 Tauri 全局快捷键。
//! CapsLock 吞键需要 CGEventTap 与辅助功能权限，当前仍不支持绑定 CapsLock。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::AppHandle;
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
}

#[derive(Clone, Default)]
struct RegisteredSet {
    bindings: Vec<HotkeyBinding>,
    shortcuts: Vec<String>,
    press_hold_mode: bool,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static DICTATION_SHORTCUTS: OnceLock<Mutex<RegisteredSet>> = OnceLock::new();
static SUBTITLE_SHORTCUT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static DICTATION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
    let _ = DICTATION_SHORTCUTS.set(Mutex::new(RegisteredSet::default()));
    let _ = SUBTITLE_SHORTCUT.set(Mutex::new(None));
}

pub fn set_dictation_active(active: bool) {
    DICTATION_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn set_context_debug_active(_active: bool) {
    // 当前软件上下文调试仅支持 Windows。
}

fn register_bindings(
    app: &AppHandle,
    bindings: &[HotkeyBinding],
    press_hold_mode: bool,
) -> Result<Vec<String>, String> {
    let mut shortcuts = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let shortcut = shortcut_string(binding.vk, binding.mods)
            .ok_or_else(|| "当前平台不支持这个快捷键".to_string())?;
        let profile_id = binding.profile_id.clone();
        if let Err(error) =
            app.global_shortcut()
                .on_shortcut(shortcut.clone(), move |app, _, event| {
                    match (press_hold_mode, event.state) {
                        (true, ShortcutState::Pressed) => {
                            crate::application::dictation::request_start_with_profile(
                                app.clone(),
                                profile_id.clone(),
                            )
                        }
                        (true, ShortcutState::Released) => {
                            crate::application::dictation::request_stop(app.clone())
                        }
                        (false, ShortcutState::Pressed) => {
                            crate::application::dictation::request_toggle_with_profile(
                                app.clone(),
                                profile_id.clone(),
                            )
                        }
                        (false, ShortcutState::Released) => {}
                    }
                })
        {
            unregister_shortcuts(app, &shortcuts);
            return Err(format!("注册快捷键 {shortcut} 失败：{error}"));
        }
        shortcuts.push(shortcut);
    }
    Ok(shortcuts)
}

fn unregister_shortcuts(app: &AppHandle, shortcuts: &[String]) {
    for shortcut in shortcuts {
        let _ = app.global_shortcut().unregister(shortcut.clone());
    }
}

/// 事务式替换全部听写快捷键；新集合注册失败时恢复旧集合。
pub fn set_hotkeys(bindings: &[HotkeyBinding], press_hold_mode: bool) -> Result<(), String> {
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
    match register_bindings(app, bindings, press_hold_mode) {
        Ok(shortcuts) => {
            *current = RegisteredSet {
                bindings: bindings.to_vec(),
                shortcuts,
                press_hold_mode,
            };
            Ok(())
        }
        Err(error) => {
            match register_bindings(app, &previous.bindings, previous.press_hold_mode) {
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

pub fn set_subtitle_hotkey(vk: u16, mods: u8) {
    clear_subtitle_hotkey();
    let Some(app) = APP.get() else { return };
    let Some(shortcut) = shortcut_string(vk, mods) else {
        return;
    };
    let registered = app
        .global_shortcut()
        .on_shortcut(shortcut.clone(), |app, _, event| {
            if event.state == ShortcutState::Pressed {
                crate::application::subtitles::request_toggle(app.clone());
            }
        });
    if let Err(error) = registered {
        crate::dlog!("[hotkey] 注册跨平台字幕快捷键失败：{error}");
        return;
    }
    if let Some(lock) = SUBTITLE_SHORTCUT.get() {
        *lock.lock().expect("subtitle shortcut lock") = Some(shortcut);
    }
}

pub fn clear_subtitle_hotkey() {
    unregister(SUBTITLE_SHORTCUT.get());
}

pub fn set_capturing(_active: bool) {
    // 非 Windows 平台不拦截设置界面的单键输入。
}

pub fn code_to_vk(code: &str) -> Option<u16> {
    match code.trim() {
        "Space" => Some(0x20),
        "Enter" => Some(0x0d),
        "Tab" => Some(0x09),
        "Escape" => Some(0x1b),
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
        let _ = app.global_shortcut().unregister(shortcut);
    }
}

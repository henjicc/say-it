use serde::Deserialize;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;

use crate::ocr::{NormalizedRegion, OcrTextBlock};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MacWindowInfo {
    pub(crate) window_id: u32,
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) app_name: String,
    pub(crate) window_title: Option<String>,
}

pub(crate) struct MacWindowCapture {
    pub(crate) png: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[repr(C)]
struct NativeByteBuffer {
    data: *mut u8,
    length: usize,
    width: u32,
    height: u32,
}

#[derive(Deserialize)]
struct NativeOcrBlock {
    text: String,
    confidence: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

pub(crate) type CapsLockCallback = unsafe extern "C" fn(*mut c_void, u64) -> bool;
pub(crate) type EscapeCallback = unsafe extern "C" fn(*mut c_void, bool) -> bool;
pub(crate) type AudioCallback = unsafe extern "C" fn(*mut c_void, *const f32, usize);

unsafe extern "C" {
    fn sayit_macos_free_string(value: *mut c_char);
    fn sayit_macos_free_bytes(value: *mut u8);
    fn sayit_macos_frontmost_window_json(error: *mut *mut c_char) -> *mut c_char;
    fn sayit_macos_window_json(
        window_id: u32,
        process_id: u32,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn sayit_macos_running_apps_json(error: *mut *mut c_char) -> *mut c_char;
    fn sayit_macos_focused_input_security(process_id: u32, error: *mut *mut c_char) -> i32;
    fn sayit_macos_capture_window_png(
        window_id: u32,
        max_side: u32,
        output: *mut NativeByteBuffer,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_vision_ocr_png(
        bytes: *const u8,
        length: usize,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn sayit_macos_keyboard_tap_start(
        caps_lock_callback: CapsLockCallback,
        escape_callback: EscapeCallback,
        context: *mut c_void,
        monitor_caps_lock: bool,
        monitor_escape: bool,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    fn sayit_macos_keyboard_tap_stop(handle: *mut c_void);
    fn sayit_macos_system_audio_start(
        callback: AudioCallback,
        context: *mut c_void,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    fn sayit_macos_system_audio_stop(handle: *mut c_void);
    fn sayit_macos_place_indicator_window(
        ns_window: *mut c_void,
        width: f64,
        height: f64,
        anchor: i32,
        offset_y: f64,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_indicator_visible_screen_size(
        ns_window: *mut c_void,
        width: *mut f64,
        height: *mut f64,
        error: *mut *mut c_char,
    ) -> bool;
    fn sayit_macos_send_paste_shortcut(error: *mut *mut c_char) -> bool;
}

unsafe fn take_string(value: *mut c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let result = CStr::from_ptr(value).to_string_lossy().into_owned();
    sayit_macos_free_string(value);
    Some(result)
}

unsafe fn native_result(value: *mut c_char, error: *mut c_char) -> Result<String, String> {
    if let Some(value) = take_string(value) {
        if !error.is_null() {
            let _ = take_string(error);
        }
        Ok(value)
    } else {
        Err(take_string(error).unwrap_or_else(|| "macOS 原生能力调用失败".into()))
    }
}

pub(crate) fn frontmost_window() -> Result<MacWindowInfo, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_frontmost_window_json(&mut error) };
    let json = unsafe { native_result(value, error)? };
    serde_json::from_str(&json).map_err(|error| format!("解析 macOS 前台窗口失败：{error}"))
}

pub(crate) fn running_apps() -> Result<Vec<MacWindowInfo>, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_running_apps_json(&mut error) };
    let json = unsafe { native_result(value, error)? };
    serde_json::from_str(&json).map_err(|error| format!("解析 macOS 运行中应用失败：{error}"))
}

pub(crate) fn window_info(window_id: u32, process_id: u32) -> Result<MacWindowInfo, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_window_json(window_id, process_id, &mut error) };
    let json = unsafe { native_result(value, error)? };
    serde_json::from_str(&json).map_err(|error| format!("解析 macOS 窗口信息失败：{error}"))
}

pub(crate) fn focused_input_is_secure(process_id: u32) -> Result<bool, String> {
    let mut error = ptr::null_mut();
    let result = unsafe { sayit_macos_focused_input_security(process_id, &mut error) };
    match result {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(unsafe {
            take_string(error).unwrap_or_else(|| "无法确认当前输入区域安全性".into())
        }),
    }
}

pub(crate) fn capture_window(window_id: u32, max_side: u32) -> Result<MacWindowCapture, String> {
    let mut output = NativeByteBuffer {
        data: ptr::null_mut(),
        length: 0,
        width: 0,
        height: 0,
    };
    let mut error = ptr::null_mut();
    let success =
        unsafe { sayit_macos_capture_window_png(window_id, max_side, &mut output, &mut error) };
    if !success || output.data.is_null() || output.length == 0 {
        return Err(unsafe {
            take_string(error).unwrap_or_else(|| "macOS 窗口截图失败".into())
        });
    }
    let png = unsafe { std::slice::from_raw_parts(output.data, output.length).to_vec() };
    unsafe { sayit_macos_free_bytes(output.data) };
    Ok(MacWindowCapture {
        png,
        width: output.width,
        height: output.height,
    })
}

pub(crate) fn vision_ocr(png: &[u8]) -> Result<Vec<OcrTextBlock>, String> {
    let mut error = ptr::null_mut();
    let value = unsafe { sayit_macos_vision_ocr_png(png.as_ptr(), png.len(), &mut error) };
    let json = unsafe { native_result(value, error)? };
    let blocks: Vec<NativeOcrBlock> = serde_json::from_str(&json)
        .map_err(|error| format!("解析 macOS Vision OCR 结果失败：{error}"))?;
    Ok(blocks
        .into_iter()
        .filter_map(|block| {
            let text = crate::ocr::normalize_text(&block.text);
            if text.is_empty() {
                return None;
            }
            Some(OcrTextBlock {
                text,
                confidence: block.confidence.clamp(0.0, 1.0),
                bounds: NormalizedRegion {
                    left: block.left,
                    top: block.top,
                    right: block.right,
                    bottom: block.bottom,
                }
                .clamped(),
            })
        })
        .collect())
}

pub(crate) fn start_keyboard_tap(
    caps_lock_callback: CapsLockCallback,
    escape_callback: EscapeCallback,
    monitor_caps_lock: bool,
    monitor_escape: bool,
) -> Result<usize, String> {
    let mut error = ptr::null_mut();
    let handle = unsafe {
        sayit_macos_keyboard_tap_start(
            caps_lock_callback,
            escape_callback,
            ptr::null_mut(),
            monitor_caps_lock,
            monitor_escape,
            &mut error,
        )
    };
    if handle.is_null() {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "启动 macOS 键盘监听失败".into())
        })
    } else {
        Ok(handle as usize)
    }
}

pub(crate) fn stop_keyboard_tap(handle: usize) {
    if handle != 0 {
        unsafe { sayit_macos_keyboard_tap_stop(handle as *mut c_void) };
    }
}

pub(crate) unsafe fn start_system_audio(
    callback: AudioCallback,
    context: *mut c_void,
) -> Result<usize, String> {
    let mut error = ptr::null_mut();
    let handle = sayit_macos_system_audio_start(callback, context, &mut error);
    if handle.is_null() {
        Err(take_string(error).unwrap_or_else(|| "启动 macOS 系统音频采集失败".into()))
    } else {
        Ok(handle as usize)
    }
}

pub(crate) fn stop_system_audio(handle: usize) {
    if handle != 0 {
        unsafe { sayit_macos_system_audio_stop(handle as *mut c_void) };
    }
}

pub(crate) fn place_indicator_window(
    ns_window: *mut c_void,
    width: f64,
    height: f64,
    anchor: &str,
    offset_y: f64,
) -> Result<(), String> {
    let anchor = match anchor {
        "top" => 0,
        "center" => 1,
        _ => 2,
    };
    let mut error = ptr::null_mut();
    let success = unsafe {
        sayit_macos_place_indicator_window(
            ns_window,
            width,
            height,
            anchor,
            offset_y,
            &mut error,
        )
    };
    if success {
        Ok(())
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "定位 macOS 悬浮窗口失败".into())
        })
    }
}

pub(crate) fn indicator_visible_screen_size(
    ns_window: *mut c_void,
) -> Result<(f64, f64), String> {
    let mut width = 0.0;
    let mut height = 0.0;
    let mut error = ptr::null_mut();
    let success = unsafe {
        sayit_macos_indicator_visible_screen_size(
            ns_window,
            &mut width,
            &mut height,
            &mut error,
        )
    };
    if success && width > 0.0 && height > 0.0 {
        Ok((width, height))
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "读取 macOS 可用屏幕区域失败".into())
        })
    }
}

pub(crate) fn send_paste_shortcut() -> Result<(), String> {
    let mut error = ptr::null_mut();
    if unsafe { sayit_macos_send_paste_shortcut(&mut error) } {
        Ok(())
    } else {
        Err(unsafe {
            take_string(error).unwrap_or_else(|| "发送 macOS 粘贴快捷键失败".into())
        })
    }
}

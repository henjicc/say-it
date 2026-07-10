#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_dsp;
mod audio_prep;
mod commands;
mod desktop;
mod hotkey;
mod persistence;
mod prelude;
mod providers;
mod state;
mod text_align;

use prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

use commands::*;
use desktop::*;
use persistence::*;
use state::*;

static DEBUG_LOG: AtomicBool = AtomicBool::new(false);

pub fn debug_log_enabled() -> bool {
    DEBUG_LOG.load(Ordering::Relaxed)
}

#[tauri::command]
fn set_debug_log(enabled: bool) {
    DEBUG_LOG.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
fn set_hotkey_capturing(active: bool) {
    hotkey::set_capturing(active);
}

#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {{
        if $crate::debug_log_enabled() {
            eprintln!($($arg)*);
        }
    }};
}

fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows --autoplay-policy=no-user-gesture-required",
    );

    tauri::Builder::default()
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([AUTOSTART_ARG])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(RuntimeState::default())
        .setup(|app| {
            if let Some(persisted) = load_persisted_state(&app.handle())? {
                let state = app.state::<RuntimeState>();
                {
                    let mut providers = state.providers.lock().map_err(|_| {
                        std::io::Error::other(
                            "provider settings lock failed while loading persisted data",
                        )
                    })?;
                    *providers = normalize_settings(persisted.providers);
                }
                {
                    let mut dictation = state.dictation.lock().map_err(|_| {
                        std::io::Error::other("dictation lock failed while loading persisted data")
                    })?;
                    *dictation = persisted.dictation;
                }
                {
                    let mut subtitle_shortcut = state.subtitle_shortcut.lock().map_err(|_| {
                        std::io::Error::other(
                            "subtitle shortcut lock failed while loading persisted data",
                        )
                    })?;
                    *subtitle_shortcut = persisted.subtitle_shortcut;
                }
                {
                    let mut startup = state.startup.lock().map_err(|_| {
                        std::io::Error::other("startup lock failed while loading persisted data")
                    })?;
                    *startup = persisted.startup;
                }
            }

            hotkey::init(app.handle().clone());
            let dictation_settings = {
                let state = app.state::<RuntimeState>();
                let guard = state.dictation.lock().map_err(|_| {
                    std::io::Error::other("dictation lock failed while registering shortcut")
                })?;
                guard.clone()
            };
            if let Err(err) = apply_dictation_hotkey(&dictation_settings) {
                let _ = app.handle().emit(
                    "dictation-shortcut-error",
                    json!({ "message": err, "key_code": dictation_settings.key_code }),
                );
            }

            let subtitle_shortcut_settings = {
                let state = app.state::<RuntimeState>();
                let guard = state.subtitle_shortcut.lock().map_err(|_| {
                    std::io::Error::other("subtitle shortcut lock failed while registering shortcut")
                })?;
                guard.clone()
            };
            if let Err(err) = apply_subtitle_hotkey(&subtitle_shortcut_settings) {
                let _ = app.handle().emit(
                    "subtitle-shortcut-error",
                    json!({ "message": err, "key_code": subtitle_shortcut_settings.key_code }),
                );
            }

            let _ = ensure_indicator_window(&app.handle());

            let tray_menu = MenuBuilder::new(app)
                .text("show", "打开说吧！")
                .separator()
                .text("quit", "退出")
                .build()?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("说吧！")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let launched_via_autostart =
                std::env::args().any(|arg| arg == AUTOSTART_ARG);
            let silent_start = {
                let state = app.state::<RuntimeState>();
                let guard = state
                    .startup
                    .lock()
                    .map_err(|_| std::io::Error::other("startup lock failed while reading"))?;
                guard.silent_start
            };
            let start_hidden = launched_via_autostart && silent_start;

            if let Some(window) = app.get_webview_window("main") {
                remember_main_window_placement(
                    &app.handle(),
                    window.is_minimized().unwrap_or(false),
                    window.outer_position(),
                    window.inner_size(),
                );
                if start_hidden {
                    park_main_window(&app.handle());
                } else {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                match event {
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                        remember_main_window_placement(
                            &window.app_handle(),
                            window.is_minimized().unwrap_or(false),
                            window.outer_position(),
                            window.inner_size(),
                        );
                    }
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        park_main_window(&window.app_handle());
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_session_status,
            list_providers,
            get_provider_settings,
            save_provider_settings,
            set_default_provider,
            update_provider_config,
            get_provider_api_key,
            funasr_save_hotwords,
            funasr_sync_hotwords,
            funasr_clear_hotwords,
            start_asr_stream,
            asr_stream_push_chunk,
            asr_stream_push_f32_chunk,
            asr_stream_finish,
            stop_asr_stream,
            start_backend_mic,
            attach_backend_mic_to_asr,
            attach_backend_mic_raw_capture,
            pause_backend_mic,
            release_backend_mic,
            start_backend_system_audio,
            attach_backend_system_audio_to_asr,
            pause_backend_system_audio,
            release_backend_system_audio,
            process_audio_offline,
            open_api_key_page,
            open_external_link,
            get_dictation_settings,
            set_dictation_settings,
            get_subtitle_shortcut,
            set_subtitle_shortcut,
            get_startup_settings,
            set_startup_settings,
            inject_text,
            set_indicator_state,
            set_indicator_text,
            set_indicator_translation,
            set_indicator_layout,
            get_indicator_monitor_metrics,
            translate_subtitle_start,
            set_debug_log,
            set_hotkey_capturing,
            run_asr_silence_test,
            get_local_file_info,
            save_text_file,
            transcription_start,
            transcription_cancel,
            align_transcript,
            list_system_fonts,
            list_audio_devices,
            encode_mono_wav_file,
            decode_audio_file_pcm
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

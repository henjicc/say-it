#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod application;
mod audio_dsp;
mod audio_prep;
mod commands;
mod desktop;
#[cfg(windows)]
mod hotkey;
#[cfg(not(windows))]
#[path = "hotkey_portable.rs"]
mod hotkey;
mod obs_overlay;
mod persistence;
mod prelude;
mod providers;
mod state;
mod text_align;

use prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

use application::audio_lab::{
    audio_lab_audio_path, audio_lab_reprocess, audio_lab_start, audio_lab_stop,
    get_audio_lab_runtime,
};
use application::catalog::get_model_catalog;
use application::compare::{compare_cancel, compare_start, compare_stop, get_compare_runtime};
use application::contract::get_app_snapshot;
use application::dictation::{
    dictation_cancel, dictation_start, dictation_stop, dictation_toggle, get_dictation_runtime,
    preview_dictation_cue,
};
use application::llm_models::refresh_llm_models;
use application::plugin_management::{
    install_provider_plugin, list_provider_plugins, reload_provider_plugins, run_provider_plugin_action,
    set_provider_plugin_enabled, uninstall_provider_plugin,
};
use application::settings::{import_legacy_settings, update_app_settings, update_custom_cue};
use application::smart_text::preview_smart_text;
use application::subtitles::{
    apply_subtitle_obs_routing, get_subtitle_runtime, subtitle_stop, subtitle_toggle,
    sync_subtitle_presentation,
};
use application::transcription::get_transcription_runtime;
use commands::*;
use desktop::*;
use obs_overlay::*;
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

const MODEL_CALL_DEBUG_ENABLED: bool = false;

#[tauri::command]
fn debug_model_call_state(message: String) {
    if MODEL_CALL_DEBUG_ENABLED {
        eprintln!("[model-call] {message}");
    }
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

    #[cfg(windows)]
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows --autoplay-policy=no-user-gesture-required",
    );

    let builder = tauri::Builder::default();
    #[cfg(not(windows))]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());

    builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Err(error) = ensure_main_window(app) {
                eprintln!("[window] 单实例唤起主窗口失败: {error}");
            }
        }))
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
                    let mut settings = state.app_settings.lock().map_err(|_| std::io::Error::other("app settings lock failed while loading persisted data"))?;
                    *settings = persisted.app_settings.clone();
                }
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
                    let mut translation_model = state.subtitle_translation_model.lock().map_err(|_| {
                        std::io::Error::other("subtitle translation model lock failed while loading persisted data")
                    })?;
                    *translation_model = persisted.subtitle_translation_model;
                }
                {
                    let mut startup = state.startup.lock().map_err(|_| {
                        std::io::Error::other("startup lock failed while loading persisted data")
                    })?;
                    *startup = persisted.startup;
                }
                {
                    let mut obs_overlay = state.obs_overlay_settings.lock().map_err(|_| {
                        std::io::Error::other(
                            "OBS overlay settings lock failed while loading persisted data",
                        )
                    })?;
                    *obs_overlay = persisted.obs_overlay;
                }
            }

            application::plugin_management::initialize(&app.handle())?;

            let state = app.state::<RuntimeState>();
            if ensure_obs_overlay_settings(&state)? {
                save_persisted_state(&app.handle(), &state)?;
            }
            // OBS 接入是可选能力；本地端口被占用时不影响既有桌面字幕功能，状态会在前端显示。
            let _ = start_obs_overlay_server(&state);

            hotkey::init(app.handle().clone());
            application::dictation::initialize(app.handle().clone());
            application::subtitles::initialize(app.handle().clone());
            application::compare::initialize(app.handle().clone());
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
                    std::io::Error::other(
                        "subtitle shortcut lock failed while registering shortcut",
                    )
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
                    "show" => {
                        if let Err(error) = ensure_main_window(app) {
                            eprintln!("[window] 托盘打开主窗口失败: {error}");
                        }
                    }
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
                        if let Err(error) = ensure_main_window(tray.app_handle()) {
                            eprintln!("[window] 托盘点击打开主窗口失败: {error}");
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let launched_via_autostart = std::env::args().any(|arg| arg == AUTOSTART_ARG);
            let silent_start = {
                let state = app.state::<RuntimeState>();
                let guard = state
                    .startup
                    .lock()
                    .map_err(|_| std::io::Error::other("startup lock failed while reading"))?;
                guard.silent_start
            };
            let start_hidden = launched_via_autostart && silent_start;

            if app.get_webview_window("main").is_some() {
                remember_main_window_placement(&app.handle());
            }
            register_initial_main_window(&app.handle(), !start_hidden);
            if start_hidden {
                if let Err(error) = destroy_main_window(&app.handle()) {
                    eprintln!("[window] 静默启动销毁主窗口失败: {error}");
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                match event {
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                        remember_main_window_placement(&window.app_handle());
                    }
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        if let Err(error) = destroy_main_window(&window.app_handle()) {
                            eprintln!("[window] 关闭主窗口失败: {error}");
                        }
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            main_window_ready,
            get_model_catalog,
            list_provider_plugins,
            reload_provider_plugins,
            install_provider_plugin,
            set_provider_plugin_enabled,
            uninstall_provider_plugin,
            run_provider_plugin_action,
            import_legacy_settings,
            update_app_settings,
            update_custom_cue,
            get_session_status,
            list_providers,
            set_default_provider,
            update_provider_config,
            get_provider_api_key,
            add_llm_provider,
            refresh_llm_models,
            remove_llm_provider,
            preview_smart_text,
            provider_save_hotwords,
            provider_sync_hotwords,
            provider_clear_hotwords,
            start_backend_mic,
            get_backend_mic_level,
            release_backend_mic,
            start_backend_system_audio,
            get_backend_system_audio_level,
            release_backend_system_audio,
            open_api_key_page,
            open_external_link,
            get_dictation_settings,
            get_dictation_runtime,
            audio_lab_start,
            audio_lab_stop,
            audio_lab_reprocess,
            audio_lab_audio_path,
            get_audio_lab_runtime,
            compare_start,
            compare_stop,
            compare_cancel,
            get_compare_runtime,
            get_transcription_runtime,
            dictation_toggle,
            dictation_start,
            dictation_stop,
            dictation_cancel,
            preview_dictation_cue,
            set_dictation_settings,
            get_subtitle_shortcut,
            set_subtitle_shortcut,
            get_subtitle_translation_model,
            set_subtitle_translation_model,
            get_startup_settings,
            set_startup_settings,
            set_indicator_state,
            set_indicator_text,
            set_indicator_translation,
            set_indicator_layout,
            get_indicator_monitor_metrics,
            subtitle_toggle,
            subtitle_stop,
            get_subtitle_runtime,
            sync_subtitle_presentation,
            apply_subtitle_obs_routing,
            set_debug_log,
            set_hotkey_capturing,
            debug_model_call_state,
            get_local_file_info,
            save_subtitle_srt,
            transcription_start,
            transcription_cancel,
            align_transcript,
            list_system_fonts,
            list_audio_devices,
            get_obs_overlay_status,
            get_obs_connection_settings,
            get_obs_password,
            connect_obs,
            install_obs_overlay,
            uninstall_obs_overlay
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

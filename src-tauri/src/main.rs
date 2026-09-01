#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod active_app_context;
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
#[cfg(target_os = "macos")]
mod macos_native;
mod obs_overlay;
mod ocr;
mod persistence;
mod prelude;
mod providers;
mod state;
mod text_align;
#[cfg(windows)]
mod windows_native;

use prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

use application::assistant::{
    assistant_cancel, assistant_start, assistant_stop, capture_current_selection,
    close_assistant_answer, continue_assistant_answer, get_assistant_answer,
    get_default_assistant_preferences, insert_assistant_answer, preview_assistant,
    regenerate_assistant_answer, set_assistant_answer_pinned, start_assistant_follow_up_voice,
    stop_assistant_follow_up_voice,
};
use application::audio_lab::{
    audio_lab_audio_path, audio_lab_reprocess, audio_lab_start, audio_lab_stop,
    get_audio_lab_runtime,
};
use application::catalog::get_model_catalog;
use application::compare::{compare_cancel, compare_start, compare_stop, get_compare_runtime};
use application::contract::get_app_snapshot;
use application::data_root::{get_data_root_status, migrate_data_root, restart_app};
use application::diagnostics::{
    clear_diagnostic_logs, export_diagnostic_bundle, get_diagnostic_status,
    open_diagnostic_directory, set_content_diagnostics,
};
use application::dictation::{
    dictation_cancel, dictation_start, dictation_stop, dictation_toggle, dictation_use_raw_text,
    get_dictation_runtime, list_running_apps, preview_dictation_cue,
};
use application::history::{
    clear_history, clear_usage_summary, confirm_history_final_text, delete_history_entry,
    discard_history_final_text, get_usage_summary, open_history_window, query_history,
    retry_history_injection, update_history_text,
};
use application::llm_models::refresh_llm_models;
use application::performance::get_performance_metrics;
use application::plugin_management::{
    download_provider_model_pack, install_provider_plugin, list_provider_plugins,
    preview_provider_plugin, reload_provider_plugins, run_provider_plugin_action,
    set_provider_plugin_enabled, take_pending_provider_plugin_imports, uninstall_provider_plugin,
};
use application::settings::{import_legacy_settings, update_app_settings, update_custom_cue};
use application::setup::{
    complete_onboarding, get_setup_status, request_setup_permissions, run_setup_check,
};
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

#[cfg(target_os = "macos")]
const MACOS_AUTOSTART_LABEL: &str = "com.henjicc.sayit.autostart";

fn should_start_hidden(launched_via_autostart: bool, silent_start: bool) -> bool {
    launched_via_autostart && silent_start
}

#[cfg(target_os = "macos")]
fn is_macos_app_executable(path: &std::path::Path) -> bool {
    path.parent().is_some_and(|directory| {
        directory.file_name().is_some_and(|name| name == "MacOS")
            && directory.parent().is_some_and(|contents| {
                contents.file_name().is_some_and(|name| name == "Contents")
                    && contents
                        .parent()
                        .is_some_and(|bundle| bundle.extension().is_some_and(|ext| ext == "app"))
            })
    })
}

#[cfg(target_os = "macos")]
fn migrate_legacy_macos_autostart(app: &tauri::AppHandle) -> Result<(), String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("定位 macOS 应用程序失败：{error}"))?;
    // 开发态二进制不能把已安装应用的自启项迁移到 target/debug/say-it。
    if !is_macos_app_executable(&executable) {
        return Ok(());
    }
    let legacy_label = app.package_info().name.as_str();
    if legacy_label == MACOS_AUTOSTART_LABEL {
        return Ok(());
    }
    let launch_agents = app
        .path()
        .home_dir()
        .map_err(|error| format!("定位 macOS 用户目录失败：{error}"))?
        .join("Library")
        .join("LaunchAgents");
    let legacy_file = launch_agents.join(format!("{legacy_label}.plist"));
    if !legacy_file.exists() {
        return Ok(());
    }

    let current_file = launch_agents.join(format!("{MACOS_AUTOSTART_LABEL}.plist"));
    if !current_file.exists() {
        app.autolaunch()
            .enable()
            .map_err(|error| format!("迁移 macOS 开机自启项失败：{error}"))?;
    }
    fs::remove_file(&legacy_file).map_err(|error| {
        format!(
            "清理旧版 macOS 开机自启项 {} 失败：{error}",
            legacy_file.display()
        )
    })
}

pub fn debug_log_enabled() -> bool {
    DEBUG_LOG.load(Ordering::Relaxed)
}

// 上下文和提示词可能包含用户正在编辑的内容，只允许开发构建输出到终端。
// 发布构建完全移除实际日志输出，避免把这类数据写入用户机器日志。
#[cfg(debug_assertions)]
pub(crate) fn development_debug_log(component: &str, message: impl std::fmt::Display) {
    application::diagnostics::legacy_debug_log(component, &message.to_string());
}

#[cfg(not(debug_assertions))]
pub(crate) fn development_debug_log(_component: &str, _message: impl std::fmt::Display) {}

#[tauri::command]
fn set_debug_log(enabled: bool) {
    DEBUG_LOG.store(enabled, Ordering::Relaxed);
    application::diagnostics::set_verbose(enabled);
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
            let message = format!($($arg)*);
            $crate::application::diagnostics::legacy_debug_log("dlog", &message);
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
    let autostart = tauri_plugin_autostart::Builder::new().args([AUTOSTART_ARG]);
    // LaunchAgent 的 Label 和 plist 文件名应使用稳定、唯一的反向域名标识，不能跟随
    // 本地化 productName（当前为“说吧！”）变化。
    #[cfg(target_os = "macos")]
    let autostart = autostart.app_name(MACOS_AUTOSTART_LABEL);

    let app = builder
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            match application::plugin_management::queue_provider_plugin_imports(
                app,
                &args,
                std::path::Path::new(&cwd),
            ) {
                Ok(count) if count > 0 => {
                    let _ = app.emit(
                        application::plugin_management::PLUGIN_IMPORT_REQUESTED_EVENT,
                        (),
                    );
                }
                Ok(_) => {}
                Err(error) => eprintln!("[plugin-import] 接收双击导入路径失败: {error}"),
            }
            if let Err(error) = ensure_main_window(app) {
                eprintln!("[window] 单实例唤起主窗口失败: {error}");
            }
        }))
        .plugin(autostart.build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(RuntimeState::default())
        .setup(|app| {
            application::data_root::initialize(&app.handle()).map_err(std::io::Error::other)?;
            application::diagnostics::initialize(&app.handle()).map_err(std::io::Error::other)?;
            #[cfg(target_os = "macos")]
            if let Err(error) = migrate_legacy_macos_autostart(&app.handle()) {
                eprintln!("[autostart] {error}");
            }

            #[cfg(windows)]
            {
                let development_probe = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("binaries")
                    .join("context-probe-x86_64-pc-windows-msvc.exe");
                let installed_probe = std::env::current_exe()
                    .ok()
                    .and_then(|path| path.parent().map(|parent| parent.join("context-probe.exe")));
                let probe_path = installed_probe
                    .filter(|path| path.is_file())
                    .unwrap_or(development_probe);
                active_app_context::configure_native_probe_path(probe_path);
            }
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
                    let mut assistant_shortcuts = state.assistant_shortcuts.lock().map_err(|_| {
                        std::io::Error::other("assistant shortcut lock failed while loading persisted data")
                    })?;
                    *assistant_shortcuts = persisted.assistant_shortcuts;
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
                {
                    let mut floating_orb = state.floating_orb.lock().map_err(|_| {
                        std::io::Error::other("floating orb settings lock failed while loading persisted data")
                    })?;
                    *floating_orb = persisted.floating_orb;
                }
                {
                    let mut mouse_gesture = state.mouse_gesture.lock().map_err(|_| {
                        std::io::Error::other("mouse gesture settings lock failed while loading persisted data")
                    })?;
                    *mouse_gesture = persisted.mouse_gesture.normalized();
                }
            }

            let verbose_logging = app
                .state::<RuntimeState>()
                .app_settings
                .lock()
                .ok()
                .and_then(|settings| {
                    settings
                        .diagnostics_prefs
                        .get("verboseLogging")
                        .and_then(serde_json::Value::as_bool)
                })
                .unwrap_or(false);
            DEBUG_LOG.store(verbose_logging, Ordering::Relaxed);
            application::diagnostics::set_verbose(verbose_logging);

            application::history::initialize(&app.handle()).map_err(std::io::Error::other)?;
            application::plugin_management::initialize(&app.handle())?;
            let initial_args = std::env::args().collect::<Vec<_>>();
            let initial_cwd = std::env::current_dir().unwrap_or_default();
            if let Err(error) = application::plugin_management::queue_provider_plugin_imports(
                &app.handle(),
                &initial_args,
                &initial_cwd,
            ) {
                eprintln!("[plugin-import] 接收启动导入路径失败: {error}");
            }

            let state = app.state::<RuntimeState>();
            if ensure_obs_overlay_settings(&state)? {
                save_persisted_state(&app.handle(), &state)?;
            }
            // OBS 接入是可选能力；本地端口被占用时不影响既有桌面字幕功能，状态会在前端显示。
            let _ = start_obs_overlay_server(&state);

            hotkey::init(app.handle().clone());
            application::dictation::initialize(app.handle().clone());
            if let Err(error) = crate::desktop::mouse_gesture::initialize(&app.handle()) {
                eprintln!("[mouse-gesture] 启动监听失败: {error}");
            }
            crate::desktop::floating_orb::start_floating_orb_hover_watcher(app.handle().clone());
            application::subtitles::initialize(app.handle().clone());
            application::compare::initialize(app.handle().clone());
            // Tauri 会在 setup 前按平台配置预创建主窗口，这条路径不会经过
            // `ensure_main_window`，因此必须在持久化设置加载后主动应用系统材质。
            crate::desktop::floating_orb::sync_system_glass_windows(&app.handle());
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

            let assistant_shortcuts = state
                .assistant_shortcuts
                .lock()
                .map_err(|_| std::io::Error::other("assistant shortcut lock failed while registering"))?
                .clone();
            if let Err(error) = application::assistant::set_shortcuts(&app.handle(), &assistant_shortcuts) {
                eprintln!("[assistant-shortcut] {error}");
            }

            let _ = ensure_indicator_window(&app.handle());
            if let Err(error) = sync_floating_orb_window(&app.handle()) {
                eprintln!("[floating-orb] 启动悬浮球失败: {error}");
            }

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
            let start_hidden = should_start_hidden(launched_via_autostart, silent_start);

            // macOS 的普通应用即使没有窗口仍会留在 Dock。静默自启的产品语义是只驻留
            // 状态栏，因此必须在事件循环启动前隐藏 Dock 图标；用户从状态栏重新打开时，
            // `ensure_main_window` 会恢复 Dock 身份。
            #[cfg(target_os = "macos")]
            if start_hidden {
                app.set_dock_visibility(false);
            }

            if app.get_webview_window("main").is_some() {
                remember_main_window_placement(&app.handle());
            }
            register_initial_main_window(&app.handle(), !start_hidden)
                .map_err(std::io::Error::other)?;
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
            } else if window.label() == CONTEXT_DEBUG_WINDOW_LABEL {
                if matches!(event, WindowEvent::CloseRequested { .. }) {
                    let _ = hotkey::set_context_debug_active(false);
                    active_app_context::reset_debug_capture();
                }
            } else if window.label() == FLOATING_ORB_LABEL
                && matches!(event, WindowEvent::Moved(_))
            {
                schedule_remember_floating_orb_position(window.app_handle().clone());
            } else if window.label() == FLOATING_ORB_MENU_LABEL
                && matches!(event, WindowEvent::Focused(false))
            {
                let _ = hide_floating_orb_menu(window.app_handle().clone());
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            main_window_ready,
            get_model_catalog,
            list_provider_plugins,
            reload_provider_plugins,
            preview_provider_plugin,
            install_provider_plugin,
            take_pending_provider_plugin_imports,
            download_provider_model_pack,
            set_provider_plugin_enabled,
            uninstall_provider_plugin,
            run_provider_plugin_action,
            import_legacy_settings,
            get_data_root_status,
            migrate_data_root,
            restart_app,
            update_app_settings,
            update_custom_cue,
            get_session_status,
            list_providers,
            set_default_provider,
            update_provider_config,
            add_llm_provider,
            refresh_llm_models,
            remove_llm_provider,
            preview_smart_text,
            customization_sync_providers,
            customization_pull_from_provider,
            customization_clear_providers,
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
            resolve_application_identity,
            list_running_apps,
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
            dictation_use_raw_text,
            preview_dictation_cue,
            set_dictation_settings,
            get_shortcut_bindings,
            update_shortcut_binding,
            clear_shortcut_binding,
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
            set_floating_orb_enabled,
            set_floating_orb_auto_enter,
            get_floating_orb_settings,
            show_floating_orb_menu,
            hide_floating_orb_menu,
            floating_orb_open_main_window,
            set_floating_orb_appearance,
            set_mouse_gesture_settings,
            floating_orb_start_dragging,
            floating_orb_activate,
            floating_orb_stop,
            floating_orb_cancel,
            floating_orb_dismiss_submit_enter,
            floating_orb_dismiss_error,
            floating_orb_submit_enter,
            open_active_app_context_debug,
            close_active_app_context_debug,
            set_active_app_context_debug_overrides,
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
            uninstall_obs_overlay,
            query_history,
            update_history_text,
            confirm_history_final_text,
            discard_history_final_text,
            retry_history_injection,
            delete_history_entry,
            clear_history,
            get_diagnostic_status,
            set_content_diagnostics,
            clear_diagnostic_logs,
            open_diagnostic_directory,
            export_diagnostic_bundle,
            get_usage_summary,
            clear_usage_summary,
            open_history_window,
            get_setup_status,
            run_setup_check,
            request_setup_permissions,
            complete_onboarding,
            capture_current_selection,
            assistant_start,
            assistant_stop,
            assistant_cancel,
            preview_assistant,
            get_default_assistant_preferences,
            get_assistant_answer,
            insert_assistant_answer,
            regenerate_assistant_answer,
            continue_assistant_answer,
            start_assistant_follow_up_voice,
            stop_assistant_follow_up_voice,
            set_assistant_answer_pinned,
            close_assistant_answer,
            get_performance_metrics
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    #[cfg(target_os = "macos")]
    {
        let exit_code = app.run_return(|app, event| match event {
            tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } if !has_visible_windows && !should_suppress_main_reopen(app) => {
                if let Err(error) = ensure_main_window(app) {
                    eprintln!("[window] Dock 重开主窗口失败: {error}");
                }
            }
            tauri::RunEvent::Opened { urls } => {
                match application::plugin_management::queue_provider_plugin_opened_urls(app, &urls)
                {
                    Ok(count) if count > 0 => {
                        if let Err(error) = ensure_main_window(app) {
                            eprintln!("[window] 打开扩展包时唤起主窗口失败: {error}");
                        }
                        let _ = app.emit(
                            application::plugin_management::PLUGIN_IMPORT_REQUESTED_EVENT,
                            (),
                        );
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("[plugin-import] 接收 macOS 文件打开事件失败: {error}")
                    }
                }
            }
            _ => {}
        });
        active_app_context::shutdown();
        std::process::exit(exit_code);
    }
    #[cfg(not(target_os = "macos"))]
    {
        app.run(|_, _| {});
        active_app_context::shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::should_start_hidden;

    #[cfg(target_os = "macos")]
    use super::is_macos_app_executable;

    #[test]
    fn silent_start_only_hides_an_autostart_launch() {
        assert!(should_start_hidden(true, true));
        assert!(!should_start_hidden(true, false));
        assert!(!should_start_hidden(false, true));
        assert!(!should_start_hidden(false, false));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn autostart_migration_only_touches_packaged_apps() {
        assert!(is_macos_app_executable(std::path::Path::new(
            "/Applications/说吧！.app/Contents/MacOS/say-it"
        )));
        assert!(!is_macos_app_executable(std::path::Path::new(
            "/workspace/src-tauri/target/debug/say-it"
        )));
    }
}

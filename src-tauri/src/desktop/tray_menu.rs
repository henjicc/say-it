//! 托盘 / 菜单栏菜单：主窗口入口、实时字幕、悬浮球、智能处理、麦克风和常用页面。
//!
//! 勾选状态、字幕运行状态和麦克风列表在指针移到托盘图标上时重新读取，再整份重建菜单；
//! 枚举音频设备可能要几百毫秒，放在后台线程做，不卡事件循环。
use serde_json::{json, Value};
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::state::RuntimeState;

pub(crate) const TRAY_ID: &str = "main";
/// 托盘改了听写设置后通知主窗口合并，主窗口不会用旧值把它覆盖回去。
pub(crate) const DICTATION_PREFS_CHANGED_EVENT: &str = "dictation-prefs-changed-externally";

const SHOW: &str = "show";
const SUBTITLES: &str = "subtitles";
const ORB: &str = "orb";
const SMART: &str = "smart";
const MIC_PREFIX: &str = "mic:";
const VIEW_PREFIX: &str = "view:";
const QUIT: &str = "quit";

/// 菜单里直达的页面：菜单 ID 后缀是前端的页面键。
const VIEWS: [(&str, &str); 3] = [("transcription", "转写录音…"), ("history", "历史记录"), ("settings", "设置…")];

struct MenuState {
    subtitles_running: bool,
    orb_enabled: bool,
    smart_enabled: bool,
    mic: String,
    inputs: Vec<String>,
}

fn dictation_pref<T>(app: &AppHandle, read: impl FnOnce(&Value) -> T) -> T {
    let state = app.state::<RuntimeState>();
    let settings = state.app_settings.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    read(&settings.dictation_prefs)
}

fn smart_enabled(app: &AppHandle) -> bool {
    dictation_pref(app, |prefs| prefs.get("smartProcessingEnabled").and_then(Value::as_bool).unwrap_or(false))
}

fn read_state(app: &AppHandle, list_microphones: bool) -> MenuState {
    let state = app.state::<RuntimeState>();
    let mic = dictation_pref(app, |prefs| prefs.get("micDeviceId").and_then(Value::as_str).unwrap_or("").to_string());
    let inputs = match list_microphones.then(super::audio_devices::list_audio_devices).transpose() {
        Ok(list) => list.map(|list| list.inputs.into_iter().map(|device| device.name).collect()).unwrap_or_default(),
        Err(error) => {
            crate::application::diagnostics::event("warn", "tray.listMicrophonesFailed", json!({ "error": error }));
            Vec::new()
        }
    };
    MenuState {
        subtitles_running: crate::application::subtitles::is_running(&state),
        orb_enabled: state.floating_orb.lock().map(|orb| orb.enabled).unwrap_or(false),
        smart_enabled: smart_enabled(app),
        mic,
        inputs,
    }
}

fn build(app: &AppHandle, state: &MenuState) -> tauri::Result<Menu<Wry>> {
    let mut mic = SubmenuBuilder::new(app, "麦克风").item(
        &CheckMenuItemBuilder::with_id(MIC_PREFIX, "系统默认").checked(state.mic.is_empty()).build(app)?,
    );
    if !state.inputs.is_empty() {
        mic = mic.separator();
    }
    for name in &state.inputs {
        mic = mic.item(
            &CheckMenuItemBuilder::with_id(format!("{MIC_PREFIX}{name}"), name)
                .checked(*name == state.mic)
                .build(app)?,
        );
    }
    let mut menu = MenuBuilder::new(app)
        .text(SHOW, "打开说吧！")
        .separator()
        .item(&MenuItemBuilder::with_id(
            SUBTITLES,
            if state.subtitles_running { "停止实时字幕" } else { "开始实时字幕" },
        )
        .build(app)?)
        .item(&CheckMenuItemBuilder::with_id(ORB, "悬浮球").checked(state.orb_enabled).build(app)?)
        .item(&CheckMenuItemBuilder::with_id(SMART, "智能处理").checked(state.smart_enabled).build(app)?)
        .item(&mic.build()?)
        .separator();
    for (view, label) in VIEWS {
        menu = menu.text(format!("{VIEW_PREFIX}{view}"), label);
    }
    menu.separator().text(QUIT, "退出说吧！").build()
}

/// 启动时先不列麦克风（枚举设备较慢），托盘建好后由 `refresh` 在后台补上。
pub(crate) fn initial_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    build(app, &read_state(app, false))
}

/// 重新读取状态并换上新菜单。
pub(crate) fn refresh(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = read_state(&app, true);
        let result = build(&app, &state).and_then(|menu| match app.tray_by_id(TRAY_ID) {
            Some(tray) => tray.set_menu(Some(menu)),
            None => Ok(()),
        });
        if let Err(error) = result {
            eprintln!("[tray] 更新托盘菜单失败: {error}");
        }
    });
}

pub(crate) fn handle(app: &AppHandle, id: &str) {
    let result = match id {
        SHOW => super::ensure_main_window(app),
        SUBTITLES => {
            crate::application::subtitles::request_toggle(app.clone());
            Ok(())
        }
        ORB => {
            let enabled = app.state::<RuntimeState>().floating_orb.lock().map(|orb| orb.enabled).unwrap_or(false);
            super::floating_orb::set_floating_orb_enabled(app.clone(), !enabled).map(|_| ())
        }
        SMART => {
            set_dictation_pref(app, "smartProcessingEnabled", Value::Bool(!smart_enabled(app)))
        }
        QUIT => {
            app.exit(0);
            Ok(())
        }
        _ => match (id.strip_prefix(MIC_PREFIX), id.strip_prefix(VIEW_PREFIX)) {
            (Some(name), _) => set_dictation_pref(app, "micDeviceId", Value::String(name.to_string())),
            (_, Some(view)) => super::open_main_view(app, view),
            _ => Ok(()),
        },
    };
    if let Err(error) = result {
        eprintln!("[tray] 托盘菜单「{id}」失败: {error}");
        crate::application::diagnostics::event("warn", "tray.menuActionFailed", json!({ "id": id, "error": error }));
    }
    refresh(app);
}

fn set_dictation_pref(app: &AppHandle, key: &str, value: Value) -> Result<(), String> {
    let mut prefs = app
        .state::<RuntimeState>()
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败".to_string())?
        .dictation_prefs
        .clone();
    prefs
        .as_object_mut()
        .ok_or("语音输入设置不是对象")?
        .insert(key.to_string(), value.clone());
    crate::application::settings::update_app_settings(app.clone(), app.state(), "dictation".into(), prefs)?;
    app.emit(DICTATION_PREFS_CHANGED_EVENT, json!({ key: value }))
        .map_err(|error| error.to_string())
}

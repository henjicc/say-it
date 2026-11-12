#![cfg(target_os = "macos")]
//! 原生悬浮窗的 macOS 后端：用 objc2 系 crate 驱动 `sayit-overlay-core` 的模型，
//! 负责 NSPanel 窗口、CoreGraphics/CoreText 绘制与输入转发。
//!
//! 只依赖 core 与 objc2 系 crate，不依赖 tauri 或主 crate，
//! 因此能在 Windows 上用 `npm run check:mac-overlay` 做 aarch64-apple-darwin 类型检查。

pub mod cg;
pub mod ct;

pub use sayit_overlay_core as core;

mod commit;
mod glass;
mod input;
mod panel;
mod render;
mod screens;
#[cfg(test)]
mod indicator_tests;

use crate::core::spec::OverlaySpec;
use crate::ct::CtText;
use crate::render::{Host, WindowId};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Mutex, Once, OnceLock};

static START: Once = Once::new();
static LOOK: Mutex<core::look::Look> = Mutex::new(core::look::Look::MACOS_DEFAULT);

fn windows() -> &'static Mutex<HashMap<TypeId, WindowId>> {
    static WINDOWS: OnceLock<Mutex<HashMap<TypeId, WindowId>>> = OnceLock::new();
    WINDOWS.get_or_init(Default::default)
}

/// 提前启动渲染服务并开始采集屏幕信息；可重复调用。
pub fn start() {
    START.call_once(render::start);
}

/// 首次调用启动渲染服务并注册宿主；模型、CoreText 与面板都在各自线程创建。
fn register<S: OverlaySpec>() -> WindowId {
    let mut windows = windows().lock().expect("overlay registry poisoned");
    if let Some(id) = windows.get(&TypeId::of::<S>()) {
        return *id;
    }
    start();
    let id = WindowId(windows.len() as u32);
    render::install_host(
        id,
        Box::new(move |cx| {
            // 宿主在屏幕信息到位后才建，这里一定取得到焦点屏；窗口落在别的屏上时由上屏那一步改正。
            let screen = cx.screens.area_screen().expect("hosts wait for screens");
            let dpi = core::screen::dpi_for_scale(screen.scale);
            let host = Host::new(
                id,
                S::PLACEMENT,
                S::model::<CtText>(dpi, &cx.look),
                dpi,
                S::dispatch,
            );
            host.attach();
            Box::new(host)
        }),
    );
    // 持锁到登记完成：其他线程拿到编号时，注册消息已经排在它的命令之前。
    windows.insert(TypeId::of::<S>(), id);
    id
}

fn send<S: OverlaySpec>(id: WindowId, command: S::Command) {
    render::run_on_host(
        id,
        Box::new(move |host, cx| {
            host.as_any_mut()
                .downcast_mut::<Host<S::Model<CtText>>>()
                .expect("overlay host type")
                .apply(command, cx);
        }),
    );
}

/// 窗口是否已经注册。
pub fn started<S: OverlaySpec>() -> bool {
    windows()
        .lock()
        .expect("overlay registry poisoned")
        .contains_key(&TypeId::of::<S>())
}

/// 可在任意线程调用；第一条命令创建窗口。
pub fn post<S: OverlaySpec>(command: S::Command) {
    send::<S>(register::<S>(), command);
}

/// 只在窗口已注册时投递，不因为这条命令创建窗口。
pub fn post_if_started<S: OverlaySpec>(command: S::Command) {
    let id = windows()
        .lock()
        .expect("overlay registry poisoned")
        .get(&TypeId::of::<S>())
        .copied();
    if let Some(id) = id {
        send::<S>(id, command);
    }
}

/// 启动前也保留最新外观。存储与发送共用锁，防止并发更新倒序。
pub fn set_look(look: core::look::Look) {
    let mut current = LOOK.lock().expect("overlay look poisoned");
    *current = look;
    render::set_look(look);
}

pub use screens::{cursor_px, reference_size};

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
mod hosts;
mod input;
mod panel;
mod render;
mod screens;

pub use hosts::{indicator, orb, subtitle};

/// 可能在主线程或渲染线程调用，接收方必须立即返回，只投递异步任务。
pub type EventSink = Box<dyn Fn(OverlayEvent) + Send + Sync + 'static>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayEvent {
    Orb(core::orb::OrbAction),
    Subtitle(core::subtitle::SubtitleAction),
}

static SINK: std::sync::OnceLock<EventSink> = std::sync::OnceLock::new();
static START: std::sync::Once = std::sync::Once::new();
static LOOK: std::sync::Mutex<core::look::Look> =
    std::sync::Mutex::new(core::look::Look::MACOS_DEFAULT);

pub fn start(sink: EventSink) {
    SINK.get_or_init(|| sink);
    start_passive();
}

/// 无应用动作的胶囊可独立启动；交互窗口随后通过 start 绑定事件出口。
pub fn start_passive() {
    START.call_once(render::start);
}

pub fn started() -> bool {
    START.is_completed()
}

/// 启动前也保留最新外观。存储与发送共用锁，防止并发更新倒序。
pub fn set_look(look: core::look::Look) {
    let mut current = LOOK.lock().expect("overlay look poisoned");
    *current = look;
    render::set_look(look);
}

pub(crate) fn emit(event: OverlayEvent) {
    if let Some(sink) = SINK.get() {
        sink(event);
    }
}

//! macOS 原生悬浮窗启动与异步事件出口。

pub(crate) mod indicator;
pub(crate) mod orb;
pub(crate) mod subtitle;

pub(crate) fn ensure_started(app: &tauri::AppHandle) {
    let app = app.clone();
    sayit_overlay_macos::start(Box::new(move |event| {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            match event {
                sayit_overlay_macos::OverlayEvent::Orb(action) => orb::on_action(&app, action),
                sayit_overlay_macos::OverlayEvent::Subtitle(action) => {
                    subtitle::on_action(&app, action)
                }
            }
        });
    }));
}

pub(crate) fn set_look(look: sayit_overlay_core::look::Look) {
    sayit_overlay_macos::set_look(look);
}

//! Windows 原生实时字幕条。
//!
//! 复刻 indicator.css 的 subtitle-mode 与 IndicatorApp.tsx 的 useTextTrack：
//! 原文/译文两条独立文本轨道（stable 前缀 + ≤10 字符新增淡入 + 滚动/替换定位）、
//! 悬停控制条（锁定/重置/关闭）、未锁定时按住拖拽（位置不持久化，重置回到
//! 配置位）。窗口可交互（不加 WS_EX_TRANSPARENT，与原生听写胶囊相反）。
//!
//! 共用字幕模型见 overlay-core，Windows 宿主与 D2D 后端见 native_win；
//! 与原生胶囊之间的「当前谁拥有指示窗通道」路由见 indicator.rs。

#[cfg(windows)]
use super::native_win::subtitle as imp;
#[cfg(any(windows, target_os = "macos"))]
use sayit_overlay_core::subtitle::SubtitleConfig;
use std::sync::OnceLock;

/// Windows 上默认启用原生字幕条；`SAYIT_NATIVE_SUBTITLE=0` 回退 WebView。
pub(crate) fn native_subtitle_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| super::native_common::native_window_enabled("SAYIT_NATIVE_SUBTITLE"))
}

pub(crate) fn native_subtitle_attach(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        let _ = imp::APP.set(app.clone());
    }
    #[cfg(target_os = "macos")]
    super::native_mac::subtitle::attach(app);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = app;
}

/// 送字幕配置（dictation-indicator-config.subtitle 的 json 结构）。
pub(crate) fn native_subtitle_set_config(config: serde_json::Value) {
    #[cfg(windows)]
    if let Ok(config) = serde_json::from_value::<SubtitleConfig>(config) {
        imp::post(imp::Command::SetConfig(Box::new(config)));
    }
    #[cfg(target_os = "macos")]
    if let Ok(config) = serde_json::from_value::<SubtitleConfig>(config) {
        super::native_mac::subtitle::set_config(config);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = config;
}

/// 窗口几何（逻辑像素）：尺寸 + 锚点。调用方已完成取值裁剪（见 set_indicator_layout）。
pub(crate) fn native_subtitle_set_layout(width: f64, height: f64, anchor: &str, offset_y: f64) {
    #[cfg(windows)]
    imp::post(imp::Command::SetLayout {
        width: width as f32,
        height: height as f32,
        anchor: anchor.to_string(),
        offset_y: offset_y as f32,
    });
    #[cfg(target_os = "macos")]
    super::native_mac::subtitle::set_layout(width, height, anchor, offset_y);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (width, height, anchor, offset_y);
}

pub(crate) fn native_subtitle_show() {
    #[cfg(windows)]
    imp::post(imp::Command::Show);
    #[cfg(target_os = "macos")]
    super::native_mac::subtitle::show();
}

pub(crate) fn native_subtitle_hide() {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::Hide);
    #[cfg(target_os = "macos")]
    super::native_mac::subtitle::hide();
}

pub(crate) fn native_subtitle_set_text(text: String, fade: bool) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetText { text, fade });
    #[cfg(target_os = "macos")]
    super::native_mac::subtitle::set_text(text, fade);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (text, fade);
}

pub(crate) fn native_subtitle_set_translation(text: String) {
    #[cfg(windows)]
    imp::post_if_started(imp::Command::SetTranslation(text));
    #[cfg(target_os = "macos")]
    super::native_mac::subtitle::set_translation(text);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = text;
}

/// 字幕百分比字号、宽度与偏移的逻辑参考区；None 表示沿用主显示器测量。
pub(crate) fn native_subtitle_reference_size() -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    {
        super::native_mac::subtitle::reference_size()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(windows)]
pub(super) fn run_subtitle_action(action: sayit_overlay_core::subtitle::SubtitleAction) {
    match action {
        sayit_overlay_core::subtitle::SubtitleAction::Close => {
            if let Some(app) = imp::APP.get() {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::application::subtitles::subtitle_stop(app).await;
                });
            }
        }
    }
}

#[cfg(all(test, windows))]
pub(crate) use imp::snapshot_driver;

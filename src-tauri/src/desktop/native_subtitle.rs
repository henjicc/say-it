//! 原生实时字幕条的平台分发。
//!
//! 复刻 indicator.css 的 subtitle-mode 与 IndicatorApp.tsx 的 useTextTrack：
//! 原文/译文两条独立文本轨道（stable 前缀 + ≤10 字符新增淡入 + 滚动/替换定位）、
//! 悬停控制条（锁定/重置/关闭）、未锁定时按住拖拽（位置不持久化，重置回到
//! 配置位）。窗口可交互（不加 WS_EX_TRANSPARENT，与原生听写胶囊相反）。
//!
//! 共用字幕模型见 overlay-core，Windows 宿主与 D2D 后端见 native_win，macOS 宿主见 overlay-macos；
//! 与原生胶囊之间的「当前谁拥有指示窗通道」路由见 indicator.rs。

use super::native_common::{post, post_if_started, run_action};
use sayit_overlay_core::canvas::TextEngine;
use sayit_overlay_core::look::Look;
use sayit_overlay_core::spec::{OverlaySpec, Placement};
use sayit_overlay_core::subtitle::{Command, Subtitle, SubtitleAction, SubtitleConfig};
use std::sync::OnceLock;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub(crate) struct SubtitleWindow;
impl OverlaySpec for SubtitleWindow {
    const NAME: &'static str = "sayit-native-subtitle";
    const TITLE: &'static str = "说吧！实时字幕";
    const PLACEMENT: Placement = Placement::Screen;
    const CLICK_THROUGH: bool = false;
    type Command = Command;
    type Action = SubtitleAction;
    type Model<T: TextEngine + 'static> = Subtitle<T::Layout>;
    fn model<T: TextEngine + 'static>(dpi: u32, look: &Look) -> Subtitle<T::Layout> {
        Subtitle::new(dpi, look)
    }
    fn dispatch(action: SubtitleAction) {
        if super::native_test::testing(super::native_test::TestWindow::Subtitle) {
            // 测试中关闭按钮只收起字幕条，不去停字幕会话。
            let SubtitleAction::Close = action;
            super::native_test::closed_by_button(super::native_test::TestWindow::Subtitle);
            return;
        }
        run_action(move || run_subtitle_action(action));
    }
}

/// 未设置 `SAYIT_NATIVE_SUBTITLE` 时取平台默认；`0` 回退 WebView。
pub(crate) fn native_subtitle_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| super::native_common::native_window_enabled("SAYIT_NATIVE_SUBTITLE"))
}

pub(crate) fn native_subtitle_attach(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
    super::native_common::native_overlay_start();
}

/// 送字幕配置（dictation-indicator-config.subtitle 的 json 结构）。
pub(crate) fn native_subtitle_set_config(config: serde_json::Value) {
    if let Ok(config) = serde_json::from_value::<SubtitleConfig>(config) {
        post::<SubtitleWindow>(Command::SetConfig(Box::new(config)));
    }
}

/// 窗口几何（逻辑像素）：尺寸 + 锚点。调用方已完成取值裁剪（见 set_indicator_layout）。
pub(crate) fn native_subtitle_set_layout(width: f64, height: f64, anchor: &str, offset_y: f64) {
    post::<SubtitleWindow>(Command::SetLayout {
        width: width as f32,
        height: height as f32,
        anchor: anchor.to_string(),
        offset_y: offset_y as f32,
    });
}

pub(crate) fn native_subtitle_show() {
    post::<SubtitleWindow>(Command::Show);
}

/// 隐藏、文本与译文只在配置、布局或显示建好窗口之后投递。
pub(crate) fn native_subtitle_hide() {
    post_if_started::<SubtitleWindow>(Command::Hide);
}

pub(crate) fn native_subtitle_set_text(text: String, fade: bool) {
    post_if_started::<SubtitleWindow>(Command::SetText { text, fade });
}

pub(crate) fn native_subtitle_set_translation(text: String) {
    post_if_started::<SubtitleWindow>(Command::SetTranslation(text));
}

/// 字幕百分比字号、宽度与偏移的逻辑参考区；None 表示沿用主显示器测量。
pub(crate) fn native_subtitle_reference_size() -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    {
        sayit_overlay_macos::reference_size()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn run_subtitle_action(action: SubtitleAction) {
    match action {
        SubtitleAction::Close => {
            if let Some(app) = APP.get() {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::application::subtitles::subtitle_stop(app).await;
                });
            }
        }
    }
}

#[cfg(all(test, windows))]
pub(crate) use super::native_win::subtitle::snapshot_driver;

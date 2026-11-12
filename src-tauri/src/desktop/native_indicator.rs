//! Windows 原生听写悬浮指示器原型。
//!
//! 目标是验证「听写悬浮窗不占用 WebView 进程」：现有指示器是一个常驻
//! WebviewWindow，每个 WebView 渲染进程闲时约 60MB 工作集。本模块用
//! UpdateLayeredWindow + Direct2D/DirectWrite 在独立 UI 线程上复刻听写模式
//! （recording/processing/smartProcessing/fallback/hidden + 文本 + 波形），
//! error 与 subtitle 两种状态仍由原 WebView 路径承载。
//!
//! 视觉规格以 ui/src/indicator/indicator.css 的 dictation-mode 为准。
//! 状态与绘制位于 sayit-overlay-core::indicator；两个平台的宿主由 native_common 分发。

use super::native_common::{post, post_if_started, run_action};
use sayit_overlay_core::canvas::TextEngine;
use sayit_overlay_core::indicator::{Command, Indicator, IndicatorAction, IndicatorState};
use sayit_overlay_core::look::Look;
use sayit_overlay_core::spec::{OverlaySpec, Placement};
use std::sync::OnceLock;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub(crate) struct IndicatorWindow;
impl OverlaySpec for IndicatorWindow {
    const NAME: &'static str = "sayit-native-indicator";
    const TITLE: &'static str = "说吧！语音输入";
    const PLACEMENT: Placement = Placement::FollowFocus;
    const CLICK_THROUGH: bool = true;
    type Command = Command;
    type Action = IndicatorAction;
    type Model<T: TextEngine + 'static> = Indicator;
    fn model<T: TextEngine + 'static>(_: u32, look: &Look) -> Indicator {
        let model = Indicator::new(look);
        // Mac 胶囊始终点击穿透，不提供智能处理中的点击跳过。
        if cfg!(windows) {
            model.clickable()
        } else {
            model
        }
    }
    fn dispatch(action: IndicatorAction) {
        // 测试中点击只有胶囊自己的反馈，不跳过真实的智能处理。
        if !super::native_test::testing(super::native_test::TestWindow::Indicator) {
            run_action(move || run_indicator_action(action));
        }
    }
}

pub(crate) fn native_indicator_attach(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

/// 只取消令牌，不阻塞调用线程。
fn run_indicator_action(action: IndicatorAction) {
    let Some(app) = APP.get() else { return };
    match action {
        IndicatorAction::SkipSmartProcessing => {
            crate::application::dictation::skip_smart_processing(app)
        }
    }
}

/// 默认启用原生听写指示器；`SAYIT_NATIVE_INDICATOR=0` 回退 WebView。
pub(crate) fn native_dictation_indicator_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| super::native_common::native_window_enabled("SAYIT_NATIVE_INDICATOR"))
}

pub(crate) fn native_indicator_prepare() {
    post::<IndicatorWindow>(Command::Prepare);
}

/// 仅接受 recording/processing/smartProcessing/fallback；其余状态不在原生接管范围。
pub(crate) fn native_indicator_set_state(state: &str) {
    if let Some(state) = IndicatorState::from_str(state) {
        post::<IndicatorWindow>(Command::SetState(state));
    }
}

pub(crate) fn native_indicator_set_text(text: String, fade: bool) {
    post::<IndicatorWindow>(Command::SetText { text, fade });
}

pub(crate) fn native_indicator_set_waveform(level: f32, peaks: Vec<f32>) {
    post::<IndicatorWindow>(Command::SetWaveform { level, peaks });
}

pub(crate) fn native_indicator_hide() {
    post_if_started::<IndicatorWindow>(Command::Hide);
}

/// 原生错误/结果通知：显示自定义短文案的面板，几秒后自动消失。
pub(crate) fn native_indicator_notice(text: String) {
    post::<IndicatorWindow>(Command::ShowNotice(text));
}

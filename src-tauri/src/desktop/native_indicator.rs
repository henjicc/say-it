//! Windows 原生听写悬浮指示器原型。
//!
//! 目标是验证「听写悬浮窗不占用 WebView 进程」：现有指示器是一个常驻
//! WebviewWindow，每个 WebView 渲染进程闲时约 60MB 工作集。本模块用
//! UpdateLayeredWindow + Direct2D/DirectWrite 在独立 UI 线程上复刻听写模式
//! （recording/processing/smartProcessing/fallback/hidden + 文本 + 波形），
//! error 与 subtitle 两种状态仍由原 WebView 路径承载。
//!
//! 视觉规格以 ui/src/indicator/indicator.css 的 dictation-mode 为准。
//! 分层窗口/D2D 渲染目标/DIB/命令队列/UI 线程等基础设施见 native_win/；状态与绘制位于 sayit-overlay-core::indicator。

use std::sync::OnceLock;

/// Windows 上默认启用原生听写指示器；`SAYIT_NATIVE_INDICATOR=0` 回退 WebView。
pub(crate) fn native_dictation_indicator_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| super::native_common::native_window_enabled("SAYIT_NATIVE_INDICATOR"))
}

pub(crate) fn native_indicator_prepare() {
    #[cfg(windows)]
    imp::post(imp::Command::Prepare);
    #[cfg(target_os = "macos")]
    super::native_mac::indicator::prepare();
}

/// 仅接受 recording/processing/smartProcessing/fallback；其余状态不在原生接管范围。
pub(crate) fn native_indicator_set_state(state: &str) {
    #[cfg(windows)]
    if let Some(state) = imp::NativeState::from_str(state) {
        imp::post(imp::Command::SetState(state));
    }
    #[cfg(target_os = "macos")]
    super::native_mac::indicator::set_state(state);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = state;
}

pub(crate) fn native_indicator_set_text(text: String, fade: bool) {
    #[cfg(windows)]
    imp::post(imp::Command::SetText { text, fade });
    #[cfg(target_os = "macos")]
    super::native_mac::indicator::set_text(text, fade);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (text, fade);
}

pub(crate) fn native_indicator_set_waveform(level: f32, peaks: Vec<f32>) {
    #[cfg(windows)]
    imp::post(imp::Command::SetWaveform { level, peaks });
    #[cfg(target_os = "macos")]
    super::native_mac::indicator::set_waveform(level, peaks);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (level, peaks);
}

pub(crate) fn native_indicator_hide() {
    #[cfg(windows)]
    imp::post(imp::Command::Hide);
    #[cfg(target_os = "macos")]
    super::native_mac::indicator::hide();
}

/// 原生错误/结果通知：显示自定义短文案的面板，几秒后自动消失。
pub(crate) fn native_indicator_notice(text: String) {
    #[cfg(windows)]
    imp::post(imp::Command::ShowNotice(text));
    #[cfg(target_os = "macos")]
    super::native_mac::indicator::notice(text);
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = text;
}

#[cfg(windows)]
use super::native_win::indicator as imp;

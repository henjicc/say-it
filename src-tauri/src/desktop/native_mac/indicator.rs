//! macOS 听写胶囊薄桥；没有应用动作，不要求交互窗口先绑定 AppHandle。
use sayit_overlay_core::indicator::{Command, IndicatorState};

fn post(command: Command) {
    sayit_overlay_macos::start_passive();
    sayit_overlay_macos::indicator::post(command);
}

pub(crate) fn notice(text: String) {
    post(Command::ShowNotice(text));
}

pub(crate) fn hide() {
    if sayit_overlay_macos::started() {
        sayit_overlay_macos::indicator::post(Command::Hide);
    }
}

pub(crate) fn set_waveform(level: f32, peaks: Vec<f32>) {
    post(Command::SetWaveform { level, peaks });
}

pub(crate) fn set_text(text: String, fade: bool) {
    post(Command::SetText { text, fade });
}

pub(crate) fn set_state(state: &str) {
    if let Some(state) = IndicatorState::from_str(state) {
        post(Command::SetState(state));
    }
}

pub(crate) fn prepare() {
    post(Command::Prepare);
}

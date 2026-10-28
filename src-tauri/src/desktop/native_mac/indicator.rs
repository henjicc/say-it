//! macOS 听写胶囊桥接契约；宿主接入前保持无操作桩。

pub(crate) fn notice(_text: String) {}

pub(crate) fn hide() {}

pub(crate) fn set_waveform(_level: f32, _peaks: Vec<f32>) {}

pub(crate) fn set_text(_text: String, _fade: bool) {}

pub(crate) fn set_state(_state: &str) {}

pub(crate) fn prepare() {}

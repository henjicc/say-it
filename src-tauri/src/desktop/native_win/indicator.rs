use super::d2d::DWriteText;
use super::host::{self, WinWindow};
use sayit_overlay_core::geom::PxRect;
use sayit_overlay_core::indicator::Indicator;
use sayit_overlay_core::look::Look;
use sayit_overlay_core::window::OverlayModel;
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{
    WINDOW_EX_STYLE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};

pub(crate) use sayit_overlay_core::indicator::{Command, IndicatorState as NativeState};
pub(crate) struct Window;
impl WinWindow for Window {
    type Model = Indicator;
    const CLASS_NAME: PCWSTR = w!("SayItNativeDictationIndicator");
    const EX_STYLE: WINDOW_EX_STYLE = WINDOW_EX_STYLE(
        WS_EX_LAYERED.0
            | WS_EX_TOPMOST.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TRANSPARENT.0,
    );
    const THREAD_NAME: &'static str = "sayit-native-indicator";
    const TITLE: PCWSTR = w!("说吧！语音输入");
    fn create_model(_: u32) -> Indicator {
        Indicator::new(&Look::WINDOWS_LEGACY)
    }
    fn area(_: u32) -> PxRect {
        host::primary_area(true)
    }
    fn dispatch(action: <Indicator as OverlayModel<DWriteText>>::Action) {
        match action {}
    }
}
pub(crate) fn post(command: Command) {
    host::post::<Window>(command);
}

#[cfg(test)]
pub(crate) mod snapshot_driver {
    use super::{host, Window};
    pub(crate) use super::{Command, NativeState};
    use windows::Win32::Foundation::HWND;
    pub(crate) const FRAME_TIMER: usize = host::FRAME_TIMER;
    pub(crate) fn create_window() -> HWND {
        host::create_window::<Window>().expect("创建原生听写指示器窗口失败")
    }
    pub(crate) fn apply(hwnd: HWND, command: Command) {
        host::apply::<Window>(hwnd, command);
    }
    pub(crate) fn frame_timer_active(hwnd: HWND) -> bool {
        host::frame_timer_active::<Window>(hwnd)
    }
    pub(crate) fn capture(hwnd: HWND) -> Option<super::super::test_seams::Capture> {
        host::capture::<Window>(hwnd)
    }
}

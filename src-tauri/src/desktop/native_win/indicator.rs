//! 听写胶囊的快照测试驱动；窗口声明在 `desktop/native_indicator.rs`。
pub(crate) mod snapshot_driver {
    use crate::desktop::native_indicator::IndicatorWindow as Window;
    use crate::desktop::native_win::host;
    pub(crate) use sayit_overlay_core::indicator::{Command, IndicatorState as NativeState};
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
    pub(crate) fn capture(hwnd: HWND) -> Option<crate::desktop::native_win::test_seams::Capture> {
        host::capture::<Window>(hwnd)
    }
}

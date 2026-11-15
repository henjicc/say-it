//! 悬浮球设置面板的快照测试驱动；窗口声明在 `desktop/native_orb_menu.rs`。
pub(crate) mod snapshot_driver {
    use crate::desktop::native_orb_menu::OrbMenuWindow as Window;
    use crate::desktop::native_win::host;
    pub(crate) use sayit_overlay_core::orb_menu::{Command, OrbMenuView};
    use windows::Win32::Foundation::HWND;
    pub(crate) const FRAME_TIMER: usize = host::FRAME_TIMER;
    pub(crate) fn create_window() -> HWND {
        host::create_window::<Window>().expect("创建原生悬浮球设置面板失败")
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

//! 智能助手回答窗的快照测试驱动；窗口声明在 `desktop/native_answer.rs`。
pub(crate) mod snapshot_driver {
    use crate::desktop::native_answer::AnswerWindow as Window;
    use crate::desktop::native_win::host;
    pub(crate) use sayit_overlay_core::answer::{AnswerView, Command, Turn};
    use windows::Win32::Foundation::HWND;
    pub(crate) const FRAME_TIMER: usize = host::FRAME_TIMER;
    pub(crate) fn create_window() -> HWND {
        host::create_window::<Window>().expect("创建原生回答窗失败")
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

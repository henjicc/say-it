//! 实时字幕条的快照测试驱动；窗口声明在 `desktop/native_subtitle.rs`。
pub(crate) mod snapshot_driver {
    use crate::desktop::native_subtitle::SubtitleWindow as Window;
    use crate::desktop::native_win::host;
    pub(crate) use sayit_overlay_core::subtitle::Command;
    use windows::Win32::Foundation::HWND;
    pub(crate) const FRAME_TIMER: usize = host::FRAME_TIMER;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub(crate) struct Pointer {
        pub(crate) hovering_block: bool,
        pub(crate) hover_button: Option<usize>,
        pub(crate) locked: bool,
    }
    pub(crate) fn create_window() -> HWND {
        host::create_window::<Window>().expect("创建原生字幕窗口失败")
    }
    pub(crate) fn apply(hwnd: HWND, command: Command) {
        host::apply::<Window>(hwnd, command);
    }
    pub(crate) fn frame_timer_active(hwnd: HWND) -> bool {
        host::frame_timer_active::<Window>(hwnd)
    }
    pub(crate) fn pointer(hwnd: HWND) -> Pointer {
        host::with_model::<Window, _>(hwnd, |model| {
            let hit = model.pointer_hit();
            Pointer {
                hovering_block: hit.hovering_block,
                hover_button: hit.hover_button.map(|button| button.index()),
                locked: hit.locked,
            }
        })
        .unwrap_or_default()
    }
    pub(crate) fn capture(hwnd: HWND) -> Option<crate::desktop::native_win::test_seams::Capture> {
        host::capture::<Window>(hwnd)
    }
}

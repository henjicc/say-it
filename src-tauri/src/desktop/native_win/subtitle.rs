use crate::desktop::native_win::d2d::DWriteText;
use crate::desktop::native_win::host::{self, WinWindow};
use sayit_overlay_core::geom::PxRect;
use sayit_overlay_core::look::Look;
use sayit_overlay_core::subtitle::Subtitle;
use sayit_overlay_core::window::OverlayModel;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};
use windows::core::{w, PCWSTR};
use windows::Win32::Graphics::DirectWrite::IDWriteTextLayout;
use windows::Win32::UI::WindowsAndMessaging::{
    WINDOW_EX_STYLE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

pub(crate) use sayit_overlay_core::subtitle::Command;
pub(crate) static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
pub(crate) struct Window;
impl WinWindow for Window {
    type Model = Subtitle<IDWriteTextLayout>;
    const CLASS_NAME: PCWSTR = w!("SayItNativeSubtitle");
    const EX_STYLE: WINDOW_EX_STYLE = WINDOW_EX_STYLE(
        WS_EX_LAYERED.0 | WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0,
    );
    const THREAD_NAME: &'static str = "sayit-native-subtitle";
    const TITLE: PCWSTR = w!("说吧！实时字幕");
    fn create_model(dpi: u32) -> Self::Model {
        Subtitle::new(dpi, &Look::WINDOWS_LEGACY)
    }
    fn area(_: u32) -> PxRect {
        host::primary_area(false)
    }
    fn after_ops(model: &Self::Model) {
        #[cfg(test)]
        snapshot_driver::set_pointer(model.pointer_hit());
        #[cfg(not(test))]
        let _ = model;
    }
    fn dispatch(action: <Self::Model as OverlayModel<DWriteText>>::Action) {
        crate::desktop::native_subtitle::run_subtitle_action(action);
    }
}
pub(crate) fn post(command: Command) {
    // 首次配置、布局或显示后才接受文本与隐藏命令。
    host::post::<Window>(command);
    STARTED.store(true, Ordering::Relaxed);
}
pub(crate) fn post_if_started(command: Command) {
    if STARTED.load(Ordering::Relaxed) {
        host::post::<Window>(command);
    }
}

#[cfg(test)]
pub(crate) mod snapshot_driver {
    pub(crate) use super::Command;
    use super::{host, Window};
    use sayit_overlay_core::subtitle::SubtitleHit;
    use std::cell::Cell;
    use windows::Win32::Foundation::HWND;
    pub(crate) const FRAME_TIMER: usize = host::FRAME_TIMER;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub(crate) struct Pointer {
        pub(crate) hovering_block: bool,
        pub(crate) hover_button: Option<usize>,
        pub(crate) locked: bool,
    }
    thread_local! { static POINTER: Cell<Pointer> = Cell::new(Pointer::default()); }
    pub(super) fn set_pointer(hit: SubtitleHit) {
        POINTER.set(Pointer {
            hovering_block: hit.hovering_block,
            hover_button: hit.hover_button.map(|button| button.index()),
            locked: hit.locked,
        });
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
    pub(crate) fn pointer(_hwnd: HWND) -> Pointer {
        POINTER.get()
    }
    pub(crate) fn capture(hwnd: HWND) -> Option<crate::desktop::native_win::test_seams::Capture> {
        host::capture::<Window>(hwnd)
    }
}

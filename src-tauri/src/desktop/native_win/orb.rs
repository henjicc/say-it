use super::host::{self, WinWindow};
use super::OverlayThread;
use sayit_overlay_core::geom::PxRect;
use sayit_overlay_core::look::Look;
use sayit_overlay_core::orb::{Orb, OrbAction};
use std::sync::OnceLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, WINDOW_EX_STYLE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

pub(crate) use sayit_overlay_core::orb::{Command, OrbPhase};
static UI: OnceLock<Option<OverlayThread<Command>>> = OnceLock::new();
pub(crate) struct Window;
impl WinWindow for Window {
    type Model = Orb;
    const CLASS_NAME: PCWSTR = w!("SayItNativeFloatingOrb");
    const EX_STYLE: WINDOW_EX_STYLE = WINDOW_EX_STYLE(
        WS_EX_LAYERED.0 | WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0,
    );
    const THREAD_NAME: &'static str = "sayit-native-orb";
    const TITLE: PCWSTR = w!("说吧！语音输入悬浮球");
    fn create_model(dpi: u32) -> Orb {
        Orb::new(dpi, &Look::WINDOWS_LEGACY)
    }
    fn area(_: u32) -> PxRect {
        PxRect::default()
    }
    fn after_ops(model: &Orb) {
        super::super::native_orb::update_mirrors(model);
        #[cfg(test)]
        HOVERING.with(|slot| slot.set(model.hovering()));
    }
    fn dispatch(action: OrbAction) {
        super::super::native_orb::run_orb_action(action);
    }
}
pub(crate) fn started() -> bool {
    UI.get().is_some_and(Option::is_some)
}
pub(crate) fn post(command: Command) {
    if let Some(thread) = UI.get_or_init(|| {
        OverlayThread::start(
            Window::THREAD_NAME,
            host::create_window::<Window>,
            host::apply::<Window>,
        )
        .ok()
    }) {
        thread.post(command);
    }
}
pub(crate) fn post_if_started(command: Command) {
    if let Some(Some(thread)) = UI.get() {
        thread.post(command);
    }
}
pub(crate) fn cursor_over() -> bool {
    let Some((x, y, size)) = super::super::native_orb::native_orb_rect() else {
        return false;
    };
    if !super::super::native_orb::native_orb_is_visible() {
        return false;
    }
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return false;
    }
    cursor.x >= x && cursor.x < x + size && cursor.y >= y && cursor.y < y + size
}
#[cfg(test)]
thread_local! { static HOVERING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
pub(crate) mod snapshot_driver {
    use super::{host, Window};
    pub(crate) use super::{Command, OrbPhase};
    use windows::Win32::Foundation::HWND;
    pub(crate) const FRAME_TIMER: usize = host::FRAME_TIMER;
    pub(crate) fn create_window() -> HWND {
        host::create_window::<Window>().expect("创建原生悬浮球窗口失败")
    }
    pub(crate) fn apply(hwnd: HWND, command: Command) {
        host::apply::<Window>(hwnd, command);
    }
    pub(crate) fn frame_timer_active(hwnd: HWND) -> bool {
        host::frame_timer_active::<Window>(hwnd)
    }
    pub(crate) fn hovering(_: HWND) -> bool {
        super::HOVERING.with(std::cell::Cell::get)
    }
    pub(crate) fn capture(hwnd: HWND) -> Option<super::super::test_seams::Capture> {
        host::capture::<Window>(hwnd)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_seams::OffscreenGuard;
    use super::*;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, GetWindowRect, SendMessageW, WM_DPICHANGED,
    };

    struct WindowGuard(HWND);
    impl Drop for WindowGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    fn rect(hwnd: HWND) -> RECT {
        let mut rect = RECT::default();
        unsafe {
            GetWindowRect(hwnd, &mut rect).unwrap();
        }
        rect
    }
    #[test]
    fn native_orb_rect_move_and_dpi_ops_allow_synchronous_move_reentry() {
        let _offscreen = OffscreenGuard::new(96);
        let window = WindowGuard(snapshot_driver::create_window());
        snapshot_driver::apply(
            window.0,
            Command::SetRect {
                x: -80,
                y: 60,
                size: 65,
            },
        );
        let bounds = rect(window.0);
        assert_eq!(
            (bounds.left, bounds.top, bounds.right - bounds.left),
            (-80, 60, 65)
        );
        snapshot_driver::apply(window.0, Command::MoveTo { x: 100, y: 200 });
        let bounds = rect(window.0);
        assert_eq!(
            (bounds.left, bounds.top, bounds.right - bounds.left),
            (100, 200, 65)
        );
        let _new_dpi = OffscreenGuard::new(144);
        let suggested = RECT {
            left: 300,
            top: 400,
            right: 999,
            bottom: 999,
        };
        unsafe {
            SendMessageW(
                window.0,
                WM_DPICHANGED,
                WPARAM(144),
                LPARAM(&suggested as *const _ as isize),
            );
        }
        let bounds = rect(window.0);
        assert_eq!(
            (
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top
            ),
            (300, 400, 98, 98)
        );
        snapshot_driver::apply(window.0, Command::Show);
        let frame = snapshot_driver::capture(window.0).unwrap();
        assert_eq!(
            (
                frame.presented.x,
                frame.presented.y,
                frame.presented.width,
                frame.presented.height
            ),
            (300, 400, 98, 98)
        );
    }
}

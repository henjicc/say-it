//! 悬浮球设置面板的快照场景。

use super::{Driver, Session, Snapshots};
use crate::desktop::native_win::orb_menu::snapshot_driver::{self as driver, Command, OrbMenuView};
use sayit_overlay_core::geom::PxRect;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_KEYDOWN, WM_SETFOCUS};

const DRIVER: Driver = Driver {
    create_window: driver::create_window,
    frame_timer: driver::FRAME_TIMER,
    frame_timer_active: driver::frame_timer_active,
    capture: driver::capture,
};

/// 入场过渡落定后的时刻。
const SETTLE_MS: u64 = 320;
const ORB: PxRect = PxRect::new(100, 400, 56, 56);
const WORK: PxRect = PxRect::new(0, 0, 1920, 1040);
/// 第一个操作行（打开软件主界面）中间的布局坐标。
const FIRST_ACTION: (f32, f32) = (140.0, 240.0);

#[test]
fn native_snapshot_orb_menu() {
    let mut snaps = Snapshots::new("orb-menu");
    for dpi in [96, 144] {
        scenes(&mut snaps, dpi);
    }
    snaps.finish();
}

fn view() -> OrbMenuView {
    OrbMenuView {
        size_percent: 45,
        size_range: (25, 80),
        opacity: 90,
        opacity_range: (40, 100),
        auto_enter: false,
        error: None,
    }
}

fn shown(snaps: &mut Snapshots, dpi: u32, view: OrbMenuView) -> Session<'_> {
    let mut s = snaps.session(DRIVER, dpi);
    driver::apply(
        s.hwnd(),
        Command::Show {
            view,
            orb: ORB,
            work: WORK,
            focus: false,
        },
    );
    s.advance(SETTLE_MS);
    s
}

fn scenes(snaps: &mut Snapshots, dpi: u32) {
    shown(snaps, dpi, view()).shot("default");
    {
        let mut s = shown(snaps, dpi, view());
        s.mouse_move(FIRST_ACTION);
        s.advance(16);
        s.shot("hover-action");
    }
    {
        let mut s = shown(
            snaps,
            dpi,
            OrbMenuView {
                auto_enter: true,
                ..view()
            },
        );
        unsafe {
            SendMessageW(s.hwnd(), WM_SETFOCUS, WPARAM(0), LPARAM(0));
            // 第二下 Tab 落在「大小」滑块上。
            for _ in 0..2 {
                SendMessageW(s.hwnd(), WM_KEYDOWN, WPARAM(VK_TAB.0 as usize), LPARAM(0));
            }
        }
        s.advance(16);
        s.shot("keyboard-auto-enter");
    }
    shown(
        snaps,
        dpi,
        OrbMenuView {
            error: Some("请先结束当前悬浮球语音输入".into()),
            ..view()
        },
    )
    .shot("error");
}

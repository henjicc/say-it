//! 悬浮球的快照测试驱动与 Win32 回归；窗口声明在 `desktop/native_orb.rs`。
pub(crate) mod snapshot_driver {
    use crate::desktop::native_orb::OrbWindow as Window;
    use crate::desktop::native_win::host;
    pub(crate) use sayit_overlay_core::orb::{Command, OrbPhase};
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
    pub(crate) fn hovering(hwnd: HWND) -> bool {
        host::with_model::<Window, _>(hwnd, |model| model.hovering()).unwrap_or(false)
    }
    pub(crate) fn capture(hwnd: HWND) -> Option<crate::desktop::native_win::test_seams::Capture> {
        host::capture::<Window>(hwnd)
    }
}

mod tests {
    use super::snapshot_driver::{self, Command};
    use crate::desktop::native_win::d2d::DWriteText;
    use crate::desktop::native_win::test_seams::OffscreenGuard;
    use sayit_overlay_core::geom::PxRect;
    use sayit_overlay_core::look::Look;
    use sayit_overlay_core::orb::{Orb, OrbAction, OrbMirrors};
    use sayit_overlay_core::window::*;
    use std::time::Instant;
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

    fn leaked_mirrors() -> &'static OrbMirrors {
        Box::leak(Box::new(OrbMirrors::new()))
    }
    fn model_with(mirrors: &'static OrbMirrors) -> Orb {
        Orb::new(96, &Look::WINDOWS_LEGACY).with_mirrors(mirrors)
    }
    fn cx(now: Instant, text: &DWriteText) -> Cx<'_, DWriteText> {
        Cx {
            now,
            dpi: 96,
            area: PxRect::default(),
            text,
        }
    }
    #[test]
    fn native_orb_pending_requests_survive_old_frames_and_pointer_messages() {
        use sayit_overlay_core::orb::{OrbInput, OrbPointer};
        use std::time::Duration;
        let text = DWriteText::new().unwrap();
        let now = Instant::now();
        for requested_visible in [true, false] {
            let mirrors = leaked_mirrors();
            let mut model = model_with(mirrors);
            model.apply(
                Command::SetRect {
                    x: 10,
                    y: 20,
                    size: 64,
                },
                &cx(now, &text),
            );
            if !requested_visible {
                model.apply(Command::Show, &cx(now, &text));
                model.prepare_frame(&cx(now, &text));
            }
            mirrors.set_rect(10, 20, 64);
            mirrors.set_visible(!requested_visible);
            // 投递侧已写镜像，Show/Hide 和 MoveTo 尚未被 UI 消费。
            mirrors.set_visible(requested_visible);
            mirrors.move_to(200, 300);
            let mut pointer = OrbPointer::default();
            let mut clock = FrameClock::new();
            for offset in [16, 32, 200] {
                let context = cx(now + Duration::from_millis(offset), &text);
                model.on_frame(0.016, &context);
                model.prepare_frame(&context);
                clock.sync(<Orb as OverlayModel<DWriteText>>::wants_frames(&model, context.now));
                let steps = pointer.event(
                    PointerEvent::Move {
                        client: (32, 32),
                        screen: (42, 52),
                    },
                    &<Orb as OverlayModel<DWriteText>>::hit(&model),
                );
                for step in steps {
                    if let PointerStep::Model(input) = step {
                        assert_eq!(input, OrbInput::Hover(true));
                        model.input(input, &context);
                        model.prepare_frame(&context);
                    }
                }
                clock.sync(<Orb as OverlayModel<DWriteText>>::wants_frames(&model, context.now));
                assert_eq!(model.rect(), PxRect::new(10, 20, 64, 64));
                assert_eq!(model.is_visible(), !requested_visible);
                assert_eq!(mirrors.is_visible(), requested_visible);
                assert_eq!(mirrors.rect(), Some((200, 300, 64)));
            }
        }
    }
    #[test]
    fn native_orb_system_events_publish_geometry_only_and_before_drag_action() {
        let text = DWriteText::new().unwrap();
        let now = Instant::now();
        let context = cx(now, &text);
        let mirrors = leaked_mirrors();
        let mut model = model_with(mirrors);
        model.apply(
            Command::SetRect {
                x: 10,
                y: 20,
                size: 65,
            },
            &context,
        );
        mirrors.set_rect(100, 200, 80);
        mirrors.set_visible(true);
        model.window_event(WindowEvent::Moved { x: -30, y: 40 }, &context);
        assert_eq!(mirrors.rect(), Some((-30, 40, 80)));
        let ops = model.window_event(
            WindowEvent::DpiChanged {
                dpi: 144,
                suggested: PxRect::new(300, 400, 999, 999),
            },
            &context,
        );
        assert_eq!(
            ops,
            vec![
                WindowOp::SetBounds(PxRect::new(300, 400, 98, 98)),
                WindowOp::Redraw
            ]
        );
        assert_eq!(mirrors.rect(), Some((300, 400, 98)));
        mirrors.set_rect(500, 600, 70);
        assert!(model
            .window_event(
                WindowEvent::DpiChanged {
                    dpi: 144,
                    suggested: PxRect::default()
                },
                &context
            )
            .is_empty());
        assert_eq!(mirrors.rect(), Some((500, 600, 70)));
        let bounds = PxRect::new(-700, 800, 98, 98);
        let ops = model.window_event(WindowEvent::SystemDragEnded { bounds }, &context);
        assert_eq!(mirrors.rect(), Some((-700, 800, 98)));
        assert_eq!(ops, vec![WindowOp::Action(OrbAction::DragFinished)]);
        assert!(mirrors.is_visible());
    }
}

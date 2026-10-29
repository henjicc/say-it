//! 听写胶囊宿主入口。
use crate::render::{self, Host, WindowId};
use sayit_overlay_core::indicator::{Command, Indicator};
use std::sync::Once;

fn register() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        render::install_host(
            WindowId::Indicator,
            Box::new(|cx| {
                let host =
                    Host::new(
                        WindowId::Indicator,
                        Indicator::new(&cx.look),
                        96,
                        |never| match never {},
                    );
                host.attach();
                Box::new(host)
            }),
        );
    });
}

/// 可在任意线程调用；模型与 CoreText 只在渲染线程创建及使用。
pub fn post(command: Command) {
    register();
    render::run_on_host(
        WindowId::Indicator,
        Box::new(move |host, cx| {
            host.as_any_mut()
                .downcast_mut::<Host<Indicator>>()
                .expect("indicator host type")
                .apply(command, cx);
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cg::CgCanvas,
        ct::CtText,
        render::{AnyHost, RenderCx},
        screens::Screens,
    };
    use sayit_overlay_core::{
        geom::{PxRect, Rect},
        indicator::IndicatorState,
        look::{Glass, GlassMaterial, Look},
        present::MainOp,
        screen::{QRect, ScreenInfo},
        window::{Cx, OverlayModel},
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    fn render(command: Command, value: &str, scale: u32) -> Vec<u8> {
        let now = Instant::now();
        let text = CtText::default();
        let cx = Cx {
            now,
            dpi: 96 * scale,
            area: PxRect::new(0, 0, 3024, 1964),
            text: &text,
        };
        let mut model = Indicator::new(&Look::MACOS_DEFAULT);
        model.apply(command, &cx);
        model.apply(
            Command::SetText {
                text: value.into(),
                fade: false,
            },
            &cx,
        );
        model.on_frame(0.016, &cx);
        let cx = Cx {
            now: now + Duration::from_millis(320),
            ..cx
        };
        model.on_frame(0.320, &cx);
        let spec = model.prepare_frame(&cx).unwrap();
        assert_eq!(spec.alpha, 1.0);
        let mut canvas = CgCanvas::new(460 * scale, 188 * scale, scale as f64).unwrap();
        model.draw(&mut canvas, cx.now);
        canvas.pixels()
    }

    fn inside(rect: Rect, x: f32, y: f32, tolerance: f32) -> bool {
        x >= rect.left - tolerance
            && x <= rect.right + tolerance
            && y >= rect.top - tolerance
            && y <= rect.bottom + tolerance
    }

    #[test]
    fn cg_renders_all_indicator_states_inside_their_visible_panels() {
        let _guard = crate::cg::cg_test_lock();
        for scale in [1, 2] {
            for (command, panel) in [
                (
                    Command::SetState(IndicatorState::Recording),
                    Rect::new(146.0, 124.0, 314.0, 164.0),
                ),
                (
                    Command::SetState(IndicatorState::Processing),
                    Rect::new(146.0, 124.0, 314.0, 164.0),
                ),
                (
                    Command::SetState(IndicatorState::SmartProcessing),
                    Rect::new(146.0, 124.0, 314.0, 164.0),
                ),
                (
                    Command::SetState(IndicatorState::Fallback),
                    Rect::new(32.0, 108.0, 428.0, 164.0),
                ),
                (
                    Command::ShowNotice("这是一个通知".into()),
                    Rect::new(32.0, 108.0, 428.0, 164.0),
                ),
            ] {
                let pixels = render(command.clone(), "", scale);
                let mut ink = 0;
                let mut visible = 0;
                for (index, pixel) in pixels.chunks_exact(4).enumerate() {
                    if pixel[3] == 0 {
                        continue;
                    }
                    let x = (index % (460 * scale) as usize) as f32 / scale as f32;
                    let y = (index / (460 * scale) as usize) as f32 / scale as f32;
                    assert!(inside(panel, x, y, 1.0), "{command:?} leaked at {x},{y}");
                    visible += 1;
                    ink += usize::from(pixel[..3].iter().copied().max().unwrap() > 100);
                }
                assert!(
                    visible > 1000 && ink > 10,
                    "{command:?} did not render its face"
                );
                // 通知/兜底的左侧面板区域不属于普通胶囊，必须确实出现。
                let alpha = pixels[((140 * scale * 460 * scale + 50 * scale) * 4 + 3) as usize];
                assert_eq!(
                    alpha > 0,
                    matches!(
                        command,
                        Command::ShowNotice(_) | Command::SetState(IndicatorState::Fallback)
                    )
                );
            }
        }
    }

    #[test]
    fn cg_clips_overflowing_text_to_the_text_box_and_keeps_the_tail_visible() {
        let _guard = crate::cg::cg_test_lock();
        for scale in [1, 2] {
            let command = Command::SetState(IndicatorState::Processing);
            let empty = render(command.clone(), " ", scale);
            let long = render(
                command,
                &format!("{}最后一行可见文本", "溢出测试文本\n".repeat(30)),
                scale,
            );
            let clip = Rect::new(44.0, 37.5, 416.0, 105.0);
            let mut changed = 0;
            let mut tail = 0;
            for (index, (a, b)) in empty.chunks_exact(4).zip(long.chunks_exact(4)).enumerate() {
                if a == b {
                    continue;
                }
                let x = (index % (460 * scale) as usize) as f32 / scale as f32;
                let y = (index / (460 * scale) as usize) as f32 / scale as f32;
                assert!(inside(clip, x, y, 1.0), "overflow ink at {x},{y}");
                changed += 1;
                if y > 90.0 {
                    tail += 1;
                }
            }
            assert!(changed > 100 && tail > 10);
        }
    }

    #[test]
    fn host_reflows_dpi_position_and_glass_for_the_focused_screen() {
        let _guard = crate::cg::cg_test_lock();
        let screen = ScreenInfo {
            id: 2,
            frame: QRect {
                x: -1512.0,
                y: 0.0,
                width: 1512.0,
                height: 982.0,
            },
            visible: QRect {
                x: -1442.0,
                y: 25.0,
                width: 1442.0,
                height: 957.0,
            },
            scale: 2.0,
        };
        let look = Look {
            glass: Some(Glass {
                material: GlassMaterial::UnderWindow,
                tint_percent: 20,
                border_percent: 30,
            }),
            ..Look::MACOS_DEFAULT
        };
        let mut cx = RenderCx {
            now: Instant::now(),
            text: CtText::default(),
            look,
            screens: Arc::new(Screens {
                list: vec![screen],
                main: Some(2),
                primary_height: 982.0,
            }),
        };
        let mut host = Host::new(
            WindowId::Indicator,
            Indicator::new(&look),
            96,
            |never| match never {},
        );
        host.apply(Command::SetState(IndicatorState::Recording), &cx);
        let ops = host.slot.take_for_test();
        let frame = ops
            .iter()
            .find_map(|op| {
                if let MainOp::Present(frame) = op {
                    Some(frame)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(frame.screen, Some(screen));
        assert_eq!(frame.dpi, 192);
        assert_eq!(frame.bounds, PxRect::new(-1972, 1632, 920, 376));
        assert!(frame.mask.is_some());
        let mut next = screen;
        next.scale = 1.0;
        next.visible = QRect {
            x: -1512.0,
            y: 25.0,
            width: 1512.0,
            height: 890.0,
        };
        cx.screens = Arc::new(Screens {
            list: vec![next],
            main: Some(2),
            primary_height: 982.0,
        });
        host.redraw(&cx);
        let ops = host.slot.take_for_test();
        let MainOp::Present(frame) = &ops[0] else {
            panic!("missing reflow frame")
        };
        assert_eq!(frame.screen, Some(next));
        assert_eq!(frame.dpi, 96);
        assert_eq!(frame.bounds, PxRect::new(-986, 749, 460, 188));
    }
}

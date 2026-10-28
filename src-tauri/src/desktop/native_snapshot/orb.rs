//! 悬浮球的快照场景。

use super::{Driver, Session, Snapshots};
use crate::desktop::native_orb::snapshot_driver::{self as driver, Command, OrbPhase};

const DRIVER: Driver = Driver {
    create_window: driver::create_window,
    frame_timer: driver::FRAME_TIMER,
    frame_timer_active: driver::frame_timer_active,
    capture: driver::capture,
};

/// （DPI，物理边长）：逻辑尺寸 64 在各档缩放下的取整结果，奇数缩放覆盖取整差异。
const SIZES: [(u32, i32); 4] = [(96, 64), (144, 96), (120, 80), (168, 112)];
/// 入场过渡 240ms 与样式过渡 150ms 都落定后的时刻。
const SETTLE_MS: u64 = 320;
const PEAKS: [f32; 6] = [0.1, 0.4, 0.8, 0.2, 0.6, 0.05];
/// spinner 每帧固定转 6°，15 帧取一档角度。
const SPINNER_STEP_MS: u64 = 15 * 16;

#[test]
fn native_snapshot_orb() {
    let mut snaps = Snapshots::new("orb");
    for (dpi, size) in SIZES {
        phases(&mut snaps, dpi, size);
        interactions(&mut snaps, dpi, size);
    }
    snaps.finish();
}

fn phases(snaps: &mut Snapshots, dpi: u32, size: i32) {
    for (case, phase, can_submit) in [
        ("idle", OrbPhase::Idle, false),
        ("armed", OrbPhase::Armed, false),
        ("moving", OrbPhase::Moving, false),
        ("busy", OrbPhase::Busy, false),
        ("submitting", OrbPhase::Submitting, false),
        ("success", OrbPhase::Success, false),
        ("success-submit", OrbPhase::Success, true),
        ("submitted", OrbPhase::Submitted, false),
        ("fallback", OrbPhase::Fallback, false),
        ("error", OrbPhase::Error, false),
        ("cancelled", OrbPhase::Cancelled, false),
    ] {
        let mut s = shown(snaps, dpi, size, phase, can_submit);
        s.shot(case);
    }
    {
        // CSS 里 positioning 整体 opacity 0。
        let mut s = shown(snaps, dpi, size, OrbPhase::Positioning, false);
        s.shot_transparent("positioning");
    }
    {
        let mut s = shown(snaps, dpi, size, OrbPhase::Processing, false);
        s.shot("processing-f0");
        s.advance(SPINNER_STEP_MS);
        s.shot("processing-f15");
        s.advance(SPINNER_STEP_MS);
        s.shot("processing-f30");
    }
}

fn interactions(snaps: &mut Snapshots, dpi: u32, size: i32) {
    {
        let mut s = shown(snaps, dpi, size, OrbPhase::Idle, false);
        hover(&mut s, size);
        s.advance(75);
        hovered_shot(&mut s, "idle-hover-75ms");
        s.advance(125);
        hovered_shot(&mut s, "idle-hover-200ms");
    }
    {
        let mut s = configured(snaps, dpi, size, 1.0);
        send(
            &s,
            Command::SetState {
                phase: OrbPhase::Recording,
                transient: false,
                can_submit: false,
            },
        );
        send(
            &s,
            Command::SetWaveform {
                level: 0.3,
                peaks: PEAKS.to_vec(),
            },
        );
        send(&s, Command::Show);
        s.advance(400);
        s.shot("recording");
        // 悬停时波形区域从 54% 放大到 59.4%。
        hover(&mut s, size);
        s.advance(200);
        hovered_shot(&mut s, "recording-hover");
    }
    {
        // 悬停时对勾与角标换成整幅回车箭头。
        let mut s = shown(snaps, dpi, size, OrbPhase::Success, true);
        hover(&mut s, size);
        s.advance(200);
        hovered_shot(&mut s, "success-submit-hover");
    }
    {
        let mut s = configured(snaps, dpi, size, 0.6);
        set_phase(&s, OrbPhase::Idle, false);
        send(&s, Command::Show);
        s.advance(SETTLE_MS);
        s.shot("opacity-60");
    }
    {
        let mut s = configured(snaps, dpi, size, 1.0);
        set_phase(&s, OrbPhase::Idle, false);
        send(&s, Command::Show);
        s.advance(120);
        s.shot("enter-120ms");
    }
    {
        let mut s = shown(snaps, dpi, size, OrbPhase::Idle, false);
        send(&s, Command::Hide);
        s.advance(70);
        s.shot("exit-70ms");
    }
}

/// 与 native_orb_configure 相同的建窗顺序：先定矩形，再定整体不透明度。
fn configured(snaps: &mut Snapshots, dpi: u32, size: i32, opacity: f32) -> Session<'_> {
    let s = snaps.session(DRIVER, dpi);
    send(
        &s,
        Command::SetRect {
            x: 100,
            y: 100,
            size,
        },
    );
    send(&s, Command::SetOpacity(opacity));
    s
}

fn shown(
    snaps: &mut Snapshots,
    dpi: u32,
    size: i32,
    phase: OrbPhase,
    can_submit: bool,
) -> Session<'_> {
    let mut s = configured(snaps, dpi, size, 1.0);
    set_phase(&s, phase, can_submit);
    send(&s, Command::Show);
    s.advance(SETTLE_MS);
    s
}

fn hover(s: &mut Session, size: i32) {
    let logical = size as f32 * 96.0 / s.dpi() as f32;
    s.mouse_move((logical / 2.0, logical / 2.0));
    assert!(driver::hovering(s.hwnd()), "指针移入后悬浮球应处于悬停态");
}

/// 悬停态只在 WM_MOUSELEAVE 未被派发时成立（见 `Session::mouse_move`），截取前再确认一次。
fn hovered_shot(s: &mut Session, case: &str) {
    assert!(driver::hovering(s.hwnd()), "{case}：截取前悬停态已丢失");
    s.shot(case);
}

fn set_phase(s: &Session, phase: OrbPhase, can_submit: bool) {
    send(
        s,
        Command::SetState {
            phase,
            transient: false,
            can_submit,
        },
    );
}

fn send(s: &Session, command: Command) {
    driver::apply(s.hwnd(), command);
}

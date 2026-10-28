//! 听写胶囊的快照场景。

use super::{Driver, Session, Snapshots};
use crate::desktop::native_indicator::snapshot_driver::{self as driver, Command, NativeState};

const DRIVER: Driver = Driver {
    create_window: driver::create_window,
    frame_timer: driver::FRAME_TIMER,
    frame_timer_active: driver::frame_timer_active,
    capture: driver::capture,
};

/// 入场过渡 240ms、点色过渡 150ms、文本淡入 180ms 都落定后的时刻。
const SETTLE_MS: u64 = 320;
const PEAKS: [f32; 6] = [0.1, 0.4, 0.8, 0.2, 0.6, 0.05];
const SHORT_TEXT: &str = "今天下午三点开会";
const APPENDED_TEXT: &str = "今天下午三点开会，记得带电脑";
/// 超过 4 行，触发滚动累积。
const LONG_TEXT: &str = "我们今天主要讨论三件事情：第一是下个季度的产品规划，第二是招聘进度和面试安排，第三是办公室搬迁的具体时间。请大家提前准备好各自负责部分的材料，会后由我统一整理成会议纪要发到群里，有问题随时在群里提出来，我们下周一再碰一次进度。";
/// 通知面板里折成两行。
const NOTICE_LONG: &str =
    "识别服务暂时不可用，录音已保留，请检查网络连接后到历史记录里重新识别这段内容";
const NOTICE_SHORT: &str = "识别失败，请重试";

#[test]
fn native_snapshot_indicator() {
    let mut snaps = Snapshots::new("indicator");
    for dpi in [96, 144] {
        states(&mut snaps, dpi);
    }
    // 波形柱宽按物理像素取整，奇数缩放比例单独覆盖。
    for dpi in [96, 120, 144, 168] {
        let mut s = snaps.session(DRIVER, dpi);
        send(&s, Command::SetState(NativeState::Recording));
        send(
            &s,
            Command::SetWaveform {
                level: 0.3,
                peaks: PEAKS.to_vec(),
            },
        );
        s.advance(400);
        s.shot("recording-waveform-400ms");
    }
    snaps.finish();
}

fn states(snaps: &mut Snapshots, dpi: u32) {
    {
        let mut s = snaps.session(DRIVER, dpi);
        send(&s, Command::SetState(NativeState::Recording));
        s.advance(48);
        s.shot("recording-enter-48ms");
    }
    {
        let mut s = settled(snaps, dpi, NativeState::Recording);
        set_text(&s, SHORT_TEXT);
        s.advance(SETTLE_MS);
        s.shot("body-single-line");
        set_text(&s, APPENDED_TEXT);
        s.advance(90);
        s.shot("body-append-fade-90ms");
    }
    {
        let mut s = settled(snaps, dpi, NativeState::Recording);
        set_text(&s, LONG_TEXT);
        s.advance(32);
        s.shot("body-overflow-scroll");
    }
    {
        let mut s = settled(snaps, dpi, NativeState::Recording);
        send(&s, Command::SetState(NativeState::Processing));
        s.advance(75);
        s.shot("processing-dot-75ms");
        s.advance(325);
        s.shot("processing-dot-400ms");
    }
    {
        let mut s = settled(snaps, dpi, NativeState::SmartProcessing);
        s.shot("smart-processing");
    }
    {
        let mut s = settled(snaps, dpi, NativeState::Fallback);
        s.shot("fallback");
    }
    {
        let mut s = snaps.session(DRIVER, dpi);
        send(&s, Command::ShowNotice(NOTICE_LONG.into()));
        s.advance(SETTLE_MS);
        s.shot("notice-two-lines");
        s.fire_timer(driver::NOTICE_TIMER);
        s.advance(70);
        s.shot("notice-expired-exit-70ms");
    }
    {
        let mut s = settled(snaps, dpi, NativeState::Recording);
        set_text(&s, SHORT_TEXT);
        s.advance(SETTLE_MS);
        send(&s, Command::Hide);
        s.advance(64);
        s.shot("hide-exit-64ms");
    }
    {
        // Prepare 只重置内容，不改可见性：通知面板应原样保留。
        let mut s = snaps.session(DRIVER, dpi);
        send(&s, Command::ShowNotice(NOTICE_SHORT.into()));
        s.advance(SETTLE_MS);
        send(&s, Command::Prepare);
        s.advance(32);
        s.shot("notice-then-prepare");
    }
}

fn settled(snaps: &mut Snapshots, dpi: u32, state: NativeState) -> Session<'_> {
    let mut s = snaps.session(DRIVER, dpi);
    send(&s, Command::SetState(state));
    s.advance(SETTLE_MS);
    s
}

fn set_text(s: &Session, text: &str) {
    send(
        s,
        Command::SetText {
            text: text.into(),
            fade: false,
        },
    );
}

fn send(s: &Session, command: Command) {
    driver::apply(s.hwnd(), command);
}

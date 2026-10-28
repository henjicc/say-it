//! 实时字幕条的快照场景。

use super::{Driver, Session, Snapshots};
use crate::desktop::native_subtitle::snapshot_driver::{self as driver, Command, Pointer};
use crate::desktop::native_subtitle::SubtitleConfig;

const DRIVER: Driver = Driver {
    create_window: driver::create_window,
    frame_timer: driver::FRAME_TIMER,
    frame_timer_active: driver::frame_timer_active,
    capture: driver::capture,
};

/// 入场过渡 240ms、淡入 180ms、位移 120ms 都落定后的时刻。
const SETTLE_MS: u64 = 320;
const SHORT_TEXT: &str = "今天的会议现在开始";
/// 默认 28px 字号、834 DIP 可视宽下恰好两行；再追加 `APPEND_TEXT` 溢出到第三行。
const TWO_LINES: &str = "我们今天主要讨论下个季度的产品规划、招聘进度和办公室搬迁的具体时间安排，请大家会后把各自负责的材料整理好";
const APPEND_TEXT: &str = "，周一统一发到群里";
const OVERFLOW_TEXT: &str = "我们今天主要讨论下个季度的产品规划、招聘进度和办公室搬迁的具体时间安排，请大家会后把各自负责的材料整理好，周一统一发到群里，有问题随时提出来";
const REPLACE_LONG: &str =
    "单句替换模式下这一句话很长很长，超出可视宽度之后应该贴右显示最新的内容而不是停在开头";
const SWAP_TEXT: &str = "下一句字幕整段换新";
const TRANSLATION: &str = "The meeting starts now, please take your seats.";
/// 纯拉丁文本：换成 Segoe UI 后不走字体回退，换行位置随字体变化。
const LATIN_TEXT: &str =
    "Live captions keep the newest words visible while the speaker keeps talking about the plan";

/// 880×134 单语布局下原文块（top 28，高 106）内、控制条按钮之外的一点。
const BLOCK_POINT: (f32, f32) = (440.0, 90.0);
/// 控制条三个按钮的中心：right 8、gap 8、22px 按钮、top 5。
const BUTTON_CENTERS: [(f32, f32); 3] = [(801.0, 44.0), (831.0, 44.0), (861.0, 44.0)];
/// 指针停在 `BLOCK_POINT` 时应有的状态。
const OVER_BLOCK: Pointer = Pointer {
    hovering_block: true,
    hover_button: None,
    locked: false,
};

#[test]
fn native_snapshot_subtitle() {
    let mut snaps = Snapshots::new("subtitle");
    for dpi in [96, 144] {
        scroll(&mut snaps, dpi);
        replace(&mut snaps, dpi);
        bilingual(&mut snaps, dpi);
        animations(&mut snaps, dpi);
        controls(&mut snaps, dpi);
        styles(&mut snaps, dpi);
    }
    snaps.finish();
}

fn scroll(snaps: &mut Snapshots, dpi: u32) {
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), SHORT_TEXT);
        s.shot("scroll-short");
    }
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), OVERFLOW_TEXT);
        s.shot("scroll-overflow");
    }
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), TWO_LINES);
        set_text(&s, &format!("{TWO_LINES}{APPEND_TEXT}"), false);
        s.advance(60);
        s.shot("scroll-motion-60ms");
    }
}

fn replace(snaps: &mut Snapshots, dpi: u32) {
    let config = SubtitleConfig {
        display_mode: "replace".into(),
        line_count: 1,
        ..SubtitleConfig::default()
    };
    {
        let mut s = opened(snaps, dpi, config.clone(), SHORT_TEXT);
        s.shot("replace-short");
    }
    {
        let mut s = opened(snaps, dpi, config.clone(), REPLACE_LONG);
        s.shot("replace-overflow");
    }
    {
        let mut s = opened(snaps, dpi, config, "今天的会议");
        set_text(&s, SHORT_TEXT, false);
        s.advance(60);
        s.shot("replace-motion-60ms");
    }
}

fn bilingual(snaps: &mut Snapshots, dpi: u32) {
    for (case, order) in [
        ("bilingual-translation-first", "translationFirst"),
        ("bilingual-source-first", "sourceFirst"),
    ] {
        let config = SubtitleConfig {
            window_height: 214.0,
            translation_enabled: true,
            translation_order: order.into(),
            ..SubtitleConfig::default()
        };
        let mut s = opened(snaps, dpi, config, SHORT_TEXT);
        send(&s, Command::SetTranslation(TRANSLATION.into()));
        s.advance(SETTLE_MS);
        s.shot(case);
    }
}

fn animations(snaps: &mut Snapshots, dpi: u32) {
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), "今天的会议");
        set_text(&s, SHORT_TEXT, false);
        s.advance(90);
        s.shot("fresh-fade-90ms");
    }
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), SHORT_TEXT);
        set_text(&s, SWAP_TEXT, true);
        s.advance(100);
        s.shot("swap-blur-100ms");
        s.advance(140);
        s.shot("swap-blur-240ms");
    }
    {
        let mut s = shown(snaps, dpi, SubtitleConfig::default());
        set_text(&s, SHORT_TEXT, false);
        s.advance(120);
        s.shot("enter-120ms");
    }
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), SHORT_TEXT);
        send(&s, Command::Hide);
        s.advance(70);
        s.shot("exit-70ms");
    }
}

fn controls(snaps: &mut Snapshots, dpi: u32) {
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), SHORT_TEXT);
        s.mouse_move(BLOCK_POINT);
        s.advance(60);
        pointer_shot(&mut s, "controls-fade-60ms", OVER_BLOCK);
        s.advance(60);
        pointer_shot(&mut s, "controls-fade-120ms", OVER_BLOCK);
        for (index, center) in BUTTON_CENTERS.into_iter().enumerate() {
            s.mouse_move(center);
            let expected = Pointer {
                hover_button: Some(index),
                ..OVER_BLOCK
            };
            pointer_shot(&mut s, &format!("hover-button-{index}"), expected);
        }
    }
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), SHORT_TEXT);
        s.mouse_move(BLOCK_POINT);
        s.advance(SETTLE_MS);
        s.click(BUTTON_CENTERS[0]);
        assert!(
            driver::pointer(s.hwnd()).locked,
            "点击锁定按钮后应进入锁定态"
        );
        // 移开按钮，看锁定态本身的高亮。
        s.mouse_move(BLOCK_POINT);
        let expected = Pointer {
            locked: true,
            ..OVER_BLOCK
        };
        pointer_shot(&mut s, "locked", expected);
    }
}

fn styles(snaps: &mut Snapshots, dpi: u32) {
    {
        let config = SubtitleConfig {
            text_color: "#ffd400".into(),
            background_color: "rgba(30,30,30,0.5)".into(),
            rounded: 0.0,
            font_size: 36.0,
            ..SubtitleConfig::default()
        };
        let mut s = opened(snaps, dpi, config, SHORT_TEXT);
        s.shot("custom-style");
    }
    {
        let mut s = opened(snaps, dpi, SubtitleConfig::default(), LATIN_TEXT);
        let config = SubtitleConfig {
            font_family: "Segoe UI".into(),
            ..SubtitleConfig::default()
        };
        send(&s, Command::SetConfig(Box::new(config)));
        s.advance(SETTLE_MS);
        s.shot("font-segoe-ui");
    }
}

/// 与 sync_presentation 相同的顺序：先送窗口几何，再送配置，然后显示。
fn shown(snaps: &mut Snapshots, dpi: u32, config: SubtitleConfig) -> Session<'_> {
    let s = snaps.session(DRIVER, dpi);
    send(
        &s,
        Command::SetLayout {
            width: config.window_width as f32,
            height: config.window_height as f32,
            anchor: config.anchor.clone(),
            offset_y: config.offset_y as f32,
        },
    );
    send(&s, Command::SetConfig(Box::new(config)));
    send(&s, Command::Show);
    s
}

fn opened<'a>(
    snaps: &'a mut Snapshots,
    dpi: u32,
    config: SubtitleConfig,
    text: &str,
) -> Session<'a> {
    let mut s = shown(snaps, dpi, config);
    set_text(&s, text, false);
    s.advance(SETTLE_MS);
    s
}

/// 坐标是按布局常量写死的，截取前确认确实命中了预期的块/按钮；悬停态还依赖
/// WM_MOUSELEAVE 不被派发（见 `Session::mouse_move`），失准或丢失时直接失败。
fn pointer_shot(s: &mut Session, case: &str, expected: Pointer) {
    assert_eq!(
        driver::pointer(s.hwnd()),
        expected,
        "{case}：指针状态与场景预期不符"
    );
    s.shot(case);
}

fn set_text(s: &Session, text: &str, fade: bool) {
    send(
        s,
        Command::SetText {
            text: text.into(),
            fade,
        },
    );
}

fn send(s: &Session, command: Command) {
    driver::apply(s.hwnd(), command);
}

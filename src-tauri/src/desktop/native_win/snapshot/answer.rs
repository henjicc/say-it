//! 智能助手回答窗的快照场景。

use super::{Driver, Session, Snapshots};
use crate::desktop::native_win::answer::snapshot_driver::{
    self as driver, AnswerView, Command, Turn,
};
use sayit_overlay_core::geom::PxRect;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_CHAR, WM_SETFOCUS};

const DRIVER: Driver = Driver {
    create_window: driver::create_window,
    frame_timer: driver::FRAME_TIMER,
    frame_timer_active: driver::frame_timer_active,
    capture: driver::capture,
};

/// 入场过渡落定后的时刻。
const SETTLE_MS: u64 = 320;
const ANCHOR: PxRect = PxRect::new(200, 160, 300, 24);
const WORK: PxRect = PxRect::new(0, 0, 1920, 1040);
const SHORT: &str = "使用边界框作为 MiniMax H3 图像的放置位置。\n这个周末试试看，把你做出来的东西分享出来。";
const RICH: &str = "比如要生成「今天天气很好」：\n\n1. 小模型一口气猜出「今天 / 天气 / 很 / 好」四个词\n2. 大模型跑**一次**，发现前三个对、第四个该是「不错」\n   - 猜错的位置从这里重新生成\n3. 保留前三个，从第四个接着生成，参数 `draft_tokens = 4`\n\n```text\ndraft:  今天 天气 很 好\nverify: ✓    ✓    ✓  ✗ → 不错\n```\n\n| 模型 | 作用 |\n|---|---|\n| 小模型 | 先猜，便宜又快 |\n| 大模型 | 一次核对多个字 |\n\n> 猜对得越多，提速越明显。详见[这篇文章](https://example.com)。";

#[test]
fn native_snapshot_answer() {
    let mut snaps = Snapshots::new("answer");
    for dpi in [96, 144] {
        scenes(&mut snaps, dpi);
    }
    snaps.finish();
}

fn view(text: &str) -> AnswerView {
    AnswerView {
        label: "翻译".into(),
        text: text.into(),
        can_follow_up: true,
        hotkey: Some("右 Alt".into()),
        ..AnswerView::default()
    }
}

fn thread() -> AnswerView {
    AnswerView {
        label: "问答 · 第 2 轮".into(),
        earlier: vec![Turn {
            question: "解释一下这段".into(),
            answer: "投机解码：小模型先猜几个字，大模型一次核对。".into(),
        }],
        question: Some("能举个具体的例子吗？".into()),
        ..view(RICH)
    }
}

fn shown(snaps: &mut Snapshots, dpi: u32, view: AnswerView) -> Session<'_> {
    let mut s = snaps.session(DRIVER, dpi);
    send(
        &s,
        Command::Show {
            view,
            anchor: ANCHOR,
            work: WORK,
            focus: false,
        },
    );
    s.advance(SETTLE_MS);
    s
}

fn scenes(snaps: &mut Snapshots, dpi: u32) {
    shown(snaps, dpi, view(SHORT)).shot("short");
    shown(snaps, dpi, thread()).shot("thread-rich");
    {
        let mut s = shown(
            snaps,
            dpi,
            AnswerView {
                reasoning: "用户想知道投机解码的具体例子，先举一个短句……".into(),
                text: String::new(),
                streaming: true,
                ..thread()
            },
        );
        s.shot("streaming-thinking");
    }
    {
        let mut s = shown(snaps, dpi, thread());
        send(
            &s,
            Command::Voice {
                active: true,
                text: "那这个小模型一般选多大的比较合适".into(),
                hotkey: true,
            },
        );
        for level in [0.05, 0.2, 0.4, 0.3, 0.1, 0.35] {
            send(&s, Command::Level(level));
        }
        s.advance(16);
        s.shot("voice");
    }
    {
        let mut s = shown(
            snaps,
            dpi,
            AnswerView {
                label: "问答".into(),
                error: Some("大语言模型请求失败：网络连接超时".into()),
                ..view("")
            },
        );
        s.shot("error");
    }
    {
        let mut s = shown(snaps, dpi, view(SHORT));
        unsafe {
            SendMessageW(s.hwnd(), WM_SETFOCUS, WPARAM(0), LPARAM(0));
            for unit in "再口语一点".encode_utf16() {
                SendMessageW(s.hwnd(), WM_CHAR, WPARAM(unit as usize), LPARAM(0));
            }
        }
        s.advance(16);
        s.shot("typing");
    }
}

fn send(s: &Session, command: Command) {
    driver::apply(s.hwnd(), command);
}

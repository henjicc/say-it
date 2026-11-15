//! 智能助手回答窗：没有标题栏的深色卡片，贴着选区或指针出现，高度随内容伸缩，
//! 超过上限后正文滚动。正文是可选中的 Markdown；底部是追问输入框，语音追问时换成语音条。
//!
//! 窗口状态都由后端快照（`AnswerView`）决定，这里只保留纯界面状态：滚动、选区、
//! 输入框草稿、折叠开关与悬停。
use crate::anim::Transition;
use crate::canvas::{
    Canvas, FontFamily, ParagraphAlign, SvgIcon, TextAlign, TextEngine, TextStyle,
    UniformLineSpacing,
};
use crate::geom::{Color, Point, PxRect, Rect, Shape, Stroke, Transform};
use crate::indicator::wave_scale;
use crate::look::{Look, Profile};
use crate::ui::gesture::ClickCounter;
use crate::ui::rich_text::{RichText, RichTheme};
use crate::ui::scroll::Scroll;
use crate::ui::text_field::{FieldColors, FieldEvent, TextField};
use crate::ui::{paint, HitKind, HitMap, UiInput, UiPointer};
use crate::window::*;
use std::time::Instant;

const WIDTH: f32 = 520.0;
const MIN_HEIGHT: f32 = 132.0;
const MAX_HEIGHT: f32 = 560.0;
const RADIUS: f32 = 12.0;
const HEADER: f32 = 40.0;
const PAD_X: f32 = 20.0;
const BUTTON: f32 = 28.0;
const ICON: f32 = 16.0;
const COMPOSER_PAD: f32 = 12.0;
const FIELD_PAD_X: f32 = 12.0;
const FIELD_PAD_Y: f32 = 8.0;
const FIELD_LINES: u32 = 4;
const STRIP_HEIGHT: f32 = 92.0;
const ROW: f32 = 32.0;
const GAP: f32 = 12.0;
/// 窗口放不下需要挪开时，离工作区边缘的距离（DIP）。
const EDGE: f32 = 12.0;
const WAVE_BARS: usize = 19;
pub const MAX_PROMPT_CHARS: usize = 8000;

const ID_DRAG: u16 = 0;
const ID_PIN: u16 = 1;
const ID_CLOSE: u16 = 2;
const ID_BODY: u16 = 3;
const ID_EARLIER: u16 = 4;
const ID_REASONING: u16 = 5;
const ID_REGENERATE: u16 = 6;
const ID_COPY: u16 = 7;
const ID_INSERT: u16 = 8;
const ID_FIELD: u16 = 9;
const ID_SEND: u16 = 10;
const ID_STRIP: u16 = 11;

// Lucide 图标，24×24 视框。
const PIN_ICON: SvgIcon = SvgIcon {
    key: "answer-pin",
    d: "M12 17v5M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z",
};
const CLOSE_ICON: SvgIcon = SvgIcon {
    key: "answer-close",
    d: "M18 6L6 18M6 6l12 12",
};
const COPY_ICON: SvgIcon = SvgIcon {
    key: "answer-copy",
    d: "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2zM4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
};
const CHECK_ICON: SvgIcon = SvgIcon {
    key: "answer-check",
    d: "M20 6L9 17l-5-5",
};
const REGENERATE_ICON: SvgIcon = SvgIcon {
    key: "answer-regenerate",
    d: "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8M21 3v5h-5M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16M8 16H3v5",
};
const MIC_ICON: SvgIcon = SvgIcon {
    key: "answer-mic",
    d: "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3M19 10v2a7 7 0 0 1-14 0v-2M12 19v3",
};
const SEND_ICON: SvgIcon = SvgIcon {
    key: "answer-send",
    d: "M5 12l7-7 7 7M12 19V5",
};
const CHEVRON_RIGHT: SvgIcon = SvgIcon {
    key: "answer-chevron-right",
    d: "M9 18l6-6-6-6",
};
const CHEVRON_DOWN: SvgIcon = SvgIcon {
    key: "answer-chevron-down",
    d: "M6 9l6 6 6-6",
};
const INSERT_ICON: SvgIcon = SvgIcon {
    key: "answer-insert",
    d: "M20 4v7a4 4 0 0 1-4 4H4M9 10l-5 5 5 5",
};

/// 之前的一轮问答。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

/// 后端给的回答快照。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnswerView {
    /// 左上角的任务名，如「翻译 · 英 → 中」。
    pub label: String,
    /// 本轮之前的问答，最早的在前。
    pub earlier: Vec<Turn>,
    /// 本轮的追问；第一轮为 None。
    pub question: Option<String>,
    pub text: String,
    pub reasoning: String,
    pub error: Option<String>,
    pub streaming: bool,
    pub can_insert: bool,
    pub pinned: bool,
    /// 能否继续追问（问答类回答才有对话上下文）；不能时不显示输入框。
    pub can_follow_up: bool,
    /// 追问快捷键的显示名；None = 未设置，输入框不提示按键。
    pub hotkey: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// 显示新回答。pointer 为鼠标位置，窗口左上角尽量与它对齐；work 为所在屏的工作区
    /// （都是 DesktopPx）；focus 时取得键盘焦点。
    Show {
        view: AnswerView,
        pointer: (i32, i32),
        work: PxRect,
        focus: bool,
    },
    /// 同一回答的更新（流式输出、置顶变化、出错）。
    Update(AnswerView),
    /// 语音追问开始、结束与实时识别的文字；hotkey 表示是按住快捷键开始的。
    Voice {
        active: bool,
        text: String,
        hotkey: bool,
    },
    /// 语音追问的音量（RMS，0..1）。
    Level(f32),
    /// 剪贴板文字，回应 `AnswerAction::Paste`。
    Paste(String),
    /// 追问没发出去时把草稿放回输入框。
    RestoreDraft(String),
    Hide,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnswerAction {
    Close,
    SetPinned(bool),
    /// 写入剪贴板。
    Copy(String),
    Regenerate,
    Insert,
    Send(String),
    StartVoice,
    /// 结束语音并发送。
    StopVoice,
    /// 放弃这次语音追问。
    CancelVoice,
    OpenLink(String),
    /// 请求剪贴板文字，结果用 `Command::Paste` 送回。
    Paste,
    /// 键盘焦点得失；后端据此决定失焦关闭和快捷键是否用于追问。
    Focused(bool),
}

/// 正文里的一个可点区域与它所在的内容坐标。
#[derive(Clone, Copy, Default)]
struct ContentRect {
    rect: Rect,
    shown: bool,
}

/// 正文在内容坐标中的排布（相对正文左上角，滚动前）。
struct Content<L> {
    earlier_row: ContentRect,
    /// 展开后每轮：问题气泡（矩形 + 排版）、回答顶边。
    earlier: Vec<(Rect, Option<L>, f32)>,
    question: Option<(Rect, Option<L>)>,
    reasoning_row: ContentRect,
    reasoning_text: Option<(L, f32)>,
    answer_top: f32,
    status: Option<(L, f32, bool)>,
    regenerate: ContentRect,
    copy: ContentRect,
    insert: ContentRect,
    height: f32,
}

impl<L> Default for Content<L> {
    fn default() -> Self {
        Self {
            earlier_row: ContentRect::default(),
            earlier: Vec::new(),
            question: None,
            reasoning_row: ContentRect::default(),
            reasoning_text: None,
            answer_top: 0.0,
            status: None,
            regenerate: ContentRect::default(),
            copy: ContentRect::default(),
            insert: ContentRect::default(),
            height: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    Body,
    Field,
}

pub struct Answer<L> {
    look: Look,
    dpi: u32,
    visible: bool,
    transition: Transition,
    view: AnswerView,
    rich: RichText<L>,
    earlier_rich: Vec<RichText<L>>,
    field: TextField<L>,
    scroll: Scroll,
    content: Content<L>,
    /// 窗口高度与正文、输入区的位置（DIP）。
    height: f32,
    body: Rect,
    composer: Rect,
    field_text: Rect,
    /// 希望的窗口左上角（DesktopPx）：出现时是鼠标位置，拖动后是拖到的位置。
    /// 实际位置在工作区里放不下时才挪开，内容变多时先向下长，碰到工作区底边再整体上移。
    x: i32,
    y: i32,
    work: PxRect,
    earlier_open: bool,
    reasoning_open: bool,
    voice: Option<(String, bool)>,
    levels: [f32; WAVE_BARS],
    focused: bool,
    area: Area,
    clicks: ClickCounter,
    hover: Option<u16>,
    pressed: Option<u16>,
    copied: bool,
    /// 输入框光标在窗口里的位置，供输入法候选窗定位。
    ime_caret: Option<Rect>,
}

fn style(size: f32, line: f32, weight: u16) -> TextStyle<'static> {
    TextStyle {
        family: FontFamily::SystemUi,
        size,
        weight,
        align: TextAlign::Leading,
        paragraph: ParagraphAlign::Near,
        wrap: true,
        line_spacing: Some(UniformLineSpacing {
            height: line,
            baseline: line * 0.8,
        }),
    }
}
const BODY_SIZE: f32 = 14.0;
const BODY_LINE: f32 = 23.0;
const SMALL_SIZE: f32 = 13.0;
const SMALL_LINE: f32 = 20.0;
const CAPTION_SIZE: f32 = 12.0;
const CAPTION_LINE: f32 = 18.0;

/// 深色工作台配色（与主窗口令牌一致）。
mod palette {
    pub const PANEL: (f32, f32, f32) = (38.0 / 255.0, 38.0 / 255.0, 38.0 / 255.0);
    pub const FG: (f32, f32, f32) = (236.0 / 255.0, 237.0 / 255.0, 239.0 / 255.0);
    pub const MUTED: (f32, f32, f32) = (180.0 / 255.0, 183.0 / 255.0, 189.0 / 255.0);
    pub const SUBTLE: (f32, f32, f32) = (152.0 / 255.0, 156.0 / 255.0, 164.0 / 255.0);
    pub const FAINT: (f32, f32, f32) = (112.0 / 255.0, 116.0 / 255.0, 124.0 / 255.0);
    pub const SURFACE: (f32, f32, f32) = (51.0 / 255.0, 51.0 / 255.0, 51.0 / 255.0);
    pub const WELL: (f32, f32, f32) = (26.0 / 255.0, 26.0 / 255.0, 26.0 / 255.0);
    pub const ACCENT: (f32, f32, f32) = (39.0 / 255.0, 103.0 / 255.0, 234.0 / 255.0);
    pub const ACCENT_TEXT: (f32, f32, f32) = (107.0 / 255.0, 152.0 / 255.0, 241.0 / 255.0);
    pub const ERR: (f32, f32, f32) = (229.0 / 255.0, 114.0 / 255.0, 108.0 / 255.0);
    pub const WHITE: (f32, f32, f32) = (1.0, 1.0, 1.0);
}

fn center(rect: Rect) -> Point {
    Point::new(
        (rect.left + rect.right) / 2.0,
        (rect.top + rect.bottom) / 2.0,
    )
}

fn moved(rect: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(rect.left + dx, rect.top + dy, rect.right + dx, rect.bottom + dy)
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let r = Rect::new(
        a.left.max(b.left),
        a.top.max(b.top),
        a.right.min(b.right),
        a.bottom.min(b.bottom),
    );
    (r.width() > 0.0 && r.height() > 0.0).then_some(r)
}

impl<L: Clone> Answer<L> {
    pub fn new(dpi: u32, look: &Look) -> Self {
        Self {
            look: *look,
            dpi: dpi.max(1),
            visible: false,
            transition: Transition::new(false),
            view: AnswerView::default(),
            rich: RichText::default(),
            earlier_rich: Vec::new(),
            field: TextField::new(MAX_PROMPT_CHARS),
            scroll: Scroll::default(),
            content: Content::default(),
            height: MIN_HEIGHT,
            body: Rect::ZERO,
            composer: Rect::ZERO,
            field_text: Rect::ZERO,
            x: 0,
            y: 0,
            work: PxRect::default(),
            earlier_open: false,
            reasoning_open: false,
            voice: None,
            levels: [0.0; WAVE_BARS],
            focused: false,
            area: Area::Field,
            clicks: ClickCounter::default(),
            hover: None,
            pressed: None,
            copied: false,
            ime_caret: None,
        }
    }
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }
    /// 画刷颜色一律 straight alpha：这是新窗口，不沿用胶囊与悬浮球在 Windows 上的 rgb×a 兼容规则，
    /// 否则半透明的线和底色会被再乘一次透明度而几乎看不见。
    fn color(&self, rgb: (f32, f32, f32), alpha: f32) -> Color {
        Color::new(rgb.0, rgb.1, rgb.2, alpha)
    }
    fn bounds(&self) -> PxRect {
        let scale = self.scale();
        let width = (WIDTH * scale).round() as i32;
        let height = (self.height * scale).round() as i32;
        let edge = (EDGE * scale).round() as i32;
        let work = self.work;
        let clamp = |value: i32, low: i32, high: i32| value.min(high).max(low);
        let (x, y) = if work.width > 0 && work.height > 0 {
            (
                clamp(self.x, work.x + edge, work.right() - width - edge),
                clamp(self.y, work.y + edge, work.bottom() - height - edge),
            )
        } else {
            (self.x, self.y)
        };
        PxRect::new(x, y, width, height)
    }
    /// 左上角对齐鼠标；工作区里放不下时由 `bounds` 挪开。
    fn place(&mut self, pointer: (i32, i32), work: PxRect) {
        self.work = work;
        (self.x, self.y) = pointer;
    }

    fn rich_theme(&self, dim: f32) -> RichTheme {
        RichTheme {
            size: BODY_SIZE,
            line_height: BODY_LINE,
            text: self.color(palette::FG, dim),
            muted: self.color(palette::MUTED, dim),
            link: self.color(palette::ACCENT_TEXT, dim),
            code_fill: self.color(palette::WELL, dim),
            line: self.color(palette::WHITE, 0.11 * dim),
            quote_bar: self.color(palette::ACCENT, dim),
            header_fill: self.color(palette::WHITE, 0.05 * dim),
            selection: self.color(palette::ACCENT, 0.38),
        }
    }
    fn placeholder(&self) -> String {
        match (&self.view.hotkey, self.focused) {
            (Some(key), true) => format!("输入追问，或按住 {key} 说"),
            _ => "输入追问".into(),
        }
    }
    fn has_answer_actions(&self) -> bool {
        !self.view.streaming && !self.view.text.is_empty()
    }

    /// 按当前快照重排正文与输入区，更新窗口高度；返回窗口矩形是否变了。
    fn relayout<T: TextEngine<Layout = L>>(&mut self, engine: &T) -> bool {
        let before = self.bounds();
        let inner = WIDTH - PAD_X * 2.0;
        let mut content = Content::default();
        let mut y = 0.0;
        let bubble = |engine: &T, text: &str, y: f32| -> (Rect, Option<L>) {
            let max = inner * 0.8 - 24.0;
            let layout = engine.layout(text, &style(SMALL_SIZE, SMALL_LINE, 400), max, f32::MAX);
            let metrics = layout.as_ref().map(|l| engine.metrics(l));
            let (w, h) = metrics.map_or((0.0, SMALL_LINE), |m| (m.width, m.height));
            let rect = Rect::new(inner - w - 24.0, y, inner, y + h + 12.0);
            (rect, layout)
        };
        if !self.view.earlier.is_empty() {
            content.earlier_row = ContentRect {
                rect: Rect::new(0.0, y, inner, y + ROW),
                shown: true,
            };
            y += ROW + GAP;
            if self.earlier_open {
                self.earlier_rich.resize_with(self.view.earlier.len(), RichText::default);
                let theme = self.rich_theme(1.0);
                for (turn, rich) in self.view.earlier.iter().zip(&mut self.earlier_rich) {
                    let (rect, layout) = bubble(engine, &turn.question, y);
                    y = rect.bottom + GAP;
                    rich.set_markdown(&turn.answer);
                    rich.layout(engine, inner, &theme);
                    content.earlier.push((rect, layout, y));
                    y += rich.height() + GAP * 1.5;
                }
            }
        }
        if let Some(question) = &self.view.question {
            let (rect, layout) = bubble(engine, question, y);
            y = rect.bottom + GAP;
            content.question = Some((rect, layout));
        }
        if !self.view.reasoning.is_empty() {
            content.reasoning_row = ContentRect {
                rect: Rect::new(0.0, y, inner, y + 24.0),
                shown: true,
            };
            y += 24.0 + 6.0;
            if self.reasoning_open {
                if let Some(layout) = engine.layout(
                    &self.view.reasoning,
                    &style(CAPTION_SIZE, CAPTION_LINE, 400),
                    inner,
                    f32::MAX,
                ) {
                    let h = engine.metrics(&layout).height;
                    content.reasoning_text = Some((layout, y));
                    y += h + GAP;
                }
            }
        }
        content.answer_top = y;
        self.rich.set_markdown(&self.view.text);
        self.rich.layout(engine, inner, &self.rich_theme(1.0));
        y += self.rich.height();
        let status = match (&self.view.error, self.view.text.is_empty()) {
            (Some(error), _) => Some((error.clone(), true)),
            (None, true) if self.view.streaming => Some(("正在生成回答…".into(), false)),
            (None, true) => Some(("正在等待回答…".into(), false)),
            (None, false) => None,
        };
        if let Some((text, error)) = status {
            if !self.view.text.is_empty() {
                y += GAP;
            }
            if let Some(layout) =
                engine.layout(&text, &style(SMALL_SIZE, SMALL_LINE, 400), inner, f32::MAX)
            {
                let h = engine.metrics(&layout).height;
                content.status = Some((layout, y, error));
                y += h;
            }
        }
        if self.has_answer_actions() {
            y += 6.0;
            let mut x = 0.0;
            if self.view.can_insert {
                content.insert = ContentRect {
                    rect: Rect::new(x, y, x + 68.0, y + BUTTON),
                    shown: true,
                };
                x += 68.0 + 4.0;
            }
            content.copy = ContentRect {
                rect: Rect::new(x, y, x + BUTTON, y + BUTTON),
                shown: true,
            };
            x += BUTTON + 2.0;
            content.regenerate = ContentRect {
                rect: Rect::new(x, y, x + BUTTON, y + BUTTON),
                shown: true,
            };
            y += BUTTON;
        }
        content.height = y + GAP;
        self.content = content;

        let composer_h = if self.voice.is_some() {
            STRIP_HEIGHT + COMPOSER_PAD * 2.0
        } else if !self.view.can_follow_up {
            0.0
        } else {
            let text_w = WIDTH - COMPOSER_PAD * 2.0 - FIELD_PAD_X - 4.0 - BUTTON - 4.0;
            let field_h = self.field.layout(
                engine,
                &style(SMALL_SIZE, SMALL_LINE, 400),
                text_w,
                FIELD_LINES,
            );
            field_h + FIELD_PAD_Y * 2.0 + COMPOSER_PAD * 2.0
        };
        let wanted = HEADER + self.content.height + composer_h;
        self.height = wanted.clamp(MIN_HEIGHT, MAX_HEIGHT);
        self.body = Rect::new(0.0, HEADER, WIDTH, self.height - composer_h);
        self.composer = Rect::new(
            COMPOSER_PAD,
            self.height - composer_h + COMPOSER_PAD,
            WIDTH - COMPOSER_PAD,
            self.height - COMPOSER_PAD,
        );
        self.field_text = Rect::new(
            self.composer.left + FIELD_PAD_X,
            self.composer.top + FIELD_PAD_Y,
            self.composer.right - 4.0 - BUTTON - 4.0,
            self.composer.bottom - FIELD_PAD_Y,
        );
        let stick = self.view.streaming;
        self.scroll
            .set_extent(self.content.height, self.body.height(), stick);
        before != self.bounds()
    }

    /// 内容坐标的矩形在窗口里的位置（已滚动），不在可视区时为 None。
    fn on_screen(&self, rect: ContentRect) -> Option<Rect> {
        if !rect.shown {
            return None;
        }
        let r = moved(rect.rect, PAD_X, self.body.top - self.scroll.offset());
        intersect(r, self.body)
    }
    fn answer_point(&self, point: Point) -> Point {
        Point::new(
            point.x - PAD_X,
            point.y - self.body.top + self.scroll.offset() - self.content.answer_top,
        )
    }
    fn send_shown(&self) -> bool {
        !self.field.is_empty()
    }
    fn can_send(&self) -> bool {
        self.view.can_follow_up && !self.view.streaming && !self.field.text().trim().is_empty()
    }
    fn send(&mut self, ops: &mut Ops<AnswerAction>) {
        if !self.can_send() {
            return;
        }
        let prompt = self.field.text().trim().to_owned();
        self.field.set_text("");
        ops.push(WindowOp::Action(AnswerAction::Send(prompt)));
    }
    fn refresh<T: TextEngine<Layout = L>>(&mut self, engine: &T, ops: &mut Ops<AnswerAction>) {
        if self.relayout(engine) {
            ops.push(WindowOp::SetBounds(self.bounds()));
        }
        self.ime_caret = (self.voice.is_none() && self.area == Area::Field)
            .then(|| self.field.caret_rect(engine))
            .flatten()
            .map(|caret| moved(caret, self.field_text.left, self.field_text.top));
        ops.push(WindowOp::Redraw);
    }
    fn click(&mut self, id: u16, ops: &mut Ops<AnswerAction>) {
        match id {
            ID_PIN => ops.push(WindowOp::Action(AnswerAction::SetPinned(!self.view.pinned))),
            ID_CLOSE => ops.push(WindowOp::Action(AnswerAction::Close)),
            ID_EARLIER => self.earlier_open = !self.earlier_open,
            ID_REASONING => self.reasoning_open = !self.reasoning_open,
            ID_REGENERATE => ops.push(WindowOp::Action(AnswerAction::Regenerate)),
            ID_COPY => {
                self.copied = true;
                ops.push(WindowOp::Action(AnswerAction::Copy(self.view.text.clone())));
            }
            ID_INSERT => ops.push(WindowOp::Action(AnswerAction::Insert)),
            ID_SEND if self.send_shown() => self.send(ops),
            ID_SEND if !self.view.streaming => {
                ops.push(WindowOp::Action(AnswerAction::StartVoice))
            }
            ID_STRIP => ops.push(WindowOp::Action(AnswerAction::StopVoice)),
            _ => {}
        }
    }

    fn draw_icon<C: Canvas>(&self, canvas: &mut C, icon: &SvgIcon, rect: Rect, color: Color) {
        let c = center(rect);
        canvas.save();
        canvas.concat_transform(Transform {
            scale: ICON / 24.0,
            dx: c.x - ICON / 2.0,
            dy: c.y - ICON / 2.0,
        });
        canvas.stroke_icon(icon, color, Stroke::round(2.0));
        canvas.restore();
    }
    fn icon_button<C: Canvas>(
        &self,
        canvas: &mut C,
        id: u16,
        icon: &SvgIcon,
        rect: Rect,
        active: bool,
    ) {
        let backdrop = match (self.pressed == Some(id), self.hover == Some(id)) {
            (true, _) => Some(0.14),
            (false, true) => Some(0.08),
            _ => None,
        };
        if let Some(alpha) = backdrop {
            canvas.fill_rounded_rect(rect, 6.0, self.color(palette::WHITE, alpha));
        }
        let color = if active {
            self.color(palette::ACCENT_TEXT, 1.0)
        } else if self.hover == Some(id) {
            self.color(palette::FG, 1.0)
        } else {
            self.color(palette::SUBTLE, 1.0)
        };
        self.draw_icon(canvas, icon, rect, color);
    }
    fn draw_body<C: Canvas>(&self, canvas: &mut C)
    where
        C::Text: TextEngine<Layout = L>,
    {
        let dim = if self.voice.is_some() { 0.45 } else { 1.0 };
        let theme = self.rich_theme(dim);
        let origin = Point::new(PAD_X, self.body.top - self.scroll.offset());
        let at = |r: Rect| moved(r, origin.x, origin.y);
        let visible = (
            self.scroll.offset(),
            self.scroll.offset() + self.body.height(),
        );
        canvas.save();
        canvas.clip_rect(self.body);
        let content = &self.content;
        if content.earlier_row.shown {
            let r = at(content.earlier_row.rect);
            let hovered = self.hover == Some(ID_EARLIER);
            canvas.stroke_rounded_rect(
                Rect::new(r.left + 0.5, r.top + 0.5, r.right - 0.5, r.bottom - 0.5),
                8.0,
                self.color(palette::WHITE, if hovered { 0.18 } else { 0.11 } * dim),
                Stroke::plain(1.0),
            );
            let chevron = if self.earlier_open {
                &CHEVRON_DOWN
            } else {
                &CHEVRON_RIGHT
            };
            let icon = Rect::new(r.left + 6.0, r.top, r.left + 30.0, r.bottom);
            canvas.save();
            canvas.concat_transform(Transform {
                scale: 14.0 / 24.0,
                dx: center(icon).x - 7.0,
                dy: center(icon).y - 7.0,
            });
            canvas.stroke_icon(chevron, self.color(palette::SUBTLE, dim), Stroke::round(2.0));
            canvas.restore();
            let summary = self
                .view
                .earlier
                .first()
                .map(|turn| turn.answer.lines().next().unwrap_or_default().trim_start_matches(['#', ' ']))
                .unwrap_or_default();
            let label = format!("前 {} 轮 · {summary}", self.view.earlier.len());
            paint::label(
                canvas,
                &label,
                &paint::label_style(CAPTION_SIZE, 400),
                Rect::new(r.left + 30.0, r.top, r.right - 10.0, r.bottom),
                self.color(palette::SUBTLE, dim),
            );
        }
        let draw_bubble = |canvas: &mut C, rect: Rect, layout: &Option<L>| {
            let r = at(rect);
            canvas.fill_rounded_rect(r, 8.0, self.color(palette::SURFACE, dim));
            if let Some(layout) = layout {
                canvas.draw_text(
                    layout,
                    Point::new(r.left + 12.0, r.top + 6.0),
                    self.color(palette::FG, dim),
                    None,
                );
            }
        };
        for ((rect, layout, answer_top), rich) in content.earlier.iter().zip(&self.earlier_rich) {
            draw_bubble(canvas, *rect, layout);
            rich.draw(
                canvas,
                Point::new(origin.x, origin.y + answer_top),
                (visible.0 - answer_top, visible.1 - answer_top),
                &self.rich_theme(0.7 * dim),
            );
        }
        if let Some((rect, layout)) = &content.question {
            draw_bubble(canvas, *rect, layout);
        }
        if content.reasoning_row.shown {
            let r = at(content.reasoning_row.rect);
            let chevron = if self.reasoning_open {
                &CHEVRON_DOWN
            } else {
                &CHEVRON_RIGHT
            };
            canvas.save();
            canvas.concat_transform(Transform {
                scale: 14.0 / 24.0,
                dx: r.left,
                dy: center(r).y - 7.0,
            });
            canvas.stroke_icon(chevron, self.color(palette::SUBTLE, dim), Stroke::round(2.0));
            canvas.restore();
            let label = if self.view.streaming && self.view.text.is_empty() {
                "思考中…"
            } else {
                "思考过程"
            };
            let color = if self.hover == Some(ID_REASONING) {
                palette::MUTED
            } else {
                palette::SUBTLE
            };
            paint::label(
                canvas,
                label,
                &paint::label_style(CAPTION_SIZE, 400),
                Rect::new(r.left + 20.0, r.top, r.right, r.bottom),
                self.color(color, dim),
            );
        }
        if let Some((layout, top)) = &content.reasoning_text {
            canvas.draw_text(
                layout,
                Point::new(origin.x, origin.y + top),
                self.color(palette::SUBTLE, dim),
                None,
            );
        }
        let answer_top = content.answer_top;
        self.rich.draw(
            canvas,
            Point::new(origin.x, origin.y + answer_top),
            (visible.0 - answer_top, visible.1 - answer_top),
            &theme,
        );
        if let Some((layout, top, error)) = &content.status {
            let rgb = if *error { palette::ERR } else { palette::SUBTLE };
            canvas.draw_text(
                layout,
                Point::new(origin.x, origin.y + top),
                self.color(rgb, dim),
                None,
            );
        }
        if content.insert.shown {
            let r = at(content.insert.rect);
            let fill = if self.pressed == Some(ID_INSERT) {
                (35.0 / 255.0, 93.0 / 255.0, 211.0 / 255.0)
            } else if self.hover == Some(ID_INSERT) {
                (52.0 / 255.0, 112.0 / 255.0, 235.0 / 255.0)
            } else {
                palette::ACCENT
            };
            canvas.fill_rounded_rect(r, 6.0, self.color(fill, dim));
            let icon = Rect::new(r.left + 6.0, r.top, r.left + 26.0, r.bottom);
            canvas.save();
            canvas.concat_transform(Transform {
                scale: 15.0 / 24.0,
                dx: center(icon).x - 7.5,
                dy: center(icon).y - 7.5,
            });
            canvas.stroke_icon(&INSERT_ICON, self.color(palette::WHITE, dim), Stroke::round(2.0));
            canvas.restore();
            paint::label(
                canvas,
                "插入",
                &paint::label_style(SMALL_SIZE, 400),
                Rect::new(r.left + 28.0, r.top, r.right, r.bottom),
                self.color(palette::WHITE, dim),
            );
        }
        if content.copy.shown {
            let icon = if self.copied { &CHECK_ICON } else { &COPY_ICON };
            self.icon_button(canvas, ID_COPY, icon, at(content.copy.rect), self.copied);
        }
        if content.regenerate.shown {
            self.icon_button(
                canvas,
                ID_REGENERATE,
                &REGENERATE_ICON,
                at(content.regenerate.rect),
                false,
            );
        }
        canvas.restore();
        self.scroll
            .draw(canvas, self.body, self.color(palette::WHITE, 0.18));
    }
    fn draw_composer<C: Canvas>(&self, canvas: &mut C)
    where
        C::Text: TextEngine<Layout = L>,
    {
        let r = self.composer;
        if let Some((text, hotkey)) = &self.voice {
            let hovered = self.hover == Some(ID_STRIP);
            canvas.fill_rounded_rect(r, 10.0, self.color(palette::ACCENT, 0.14));
            canvas.stroke_rounded_rect(
                Rect::new(r.left + 0.5, r.top + 0.5, r.right - 0.5, r.bottom - 0.5),
                10.0,
                self.color(palette::ACCENT, if hovered { 0.8 } else { 0.55 }),
                Stroke::plain(1.0),
            );
            let wave_top = r.top + 12.0;
            let bar = 3.0;
            let gap = 3.0;
            let total = WAVE_BARS as f32 * (bar + gap) - gap;
            let mut x = center(r).x - total / 2.0;
            let half = (WAVE_BARS - 1) / 2;
            for i in 0..WAVE_BARS {
                // 最新的音量在正中，越旧越往两边，左右对称；两端再压低一点。
                let steps = i.abs_diff(half);
                let level = self.levels[WAVE_BARS - 1 - steps * 2];
                let distance = steps as f32 / half as f32;
                let h = (4.0 + 24.0 * level * (1.0 - distance * 0.45)).clamp(4.0, 28.0);
                let y = wave_top + (28.0 - h) / 2.0;
                canvas.fill_rounded_rect(
                    Rect::new(x, y, x + bar, y + h),
                    1.5,
                    self.color(palette::ACCENT_TEXT, 1.0),
                );
                x += bar + gap;
            }
            let shown = if text.is_empty() { "请说…" } else { text.as_str() };
            let mut line = style(15.0, 24.0, 400);
            line.align = TextAlign::Center;
            line.wrap = false;
            if let Some(layout) = canvas.text().layout(shown, &line, r.width() - 24.0, 24.0) {
                canvas.draw_text(
                    &layout,
                    Point::new(r.left + 12.0, r.top + 44.0),
                    self.color(if text.is_empty() { palette::SUBTLE } else { palette::FG }, 1.0),
                    None,
                );
            }
            let hint = match (&self.view.hotkey, hotkey) {
                (Some(key), true) => format!("松开 {key} 发送 · Esc 取消"),
                _ => "说完点这里发送 · Esc 取消".into(),
            };
            let mut caption = paint::label_style(CAPTION_SIZE, 400);
            caption.align = TextAlign::Center;
            paint::label(
                canvas,
                &hint,
                &caption,
                Rect::new(r.left, r.bottom - 26.0, r.right, r.bottom - 8.0),
                self.color(palette::SUBTLE, 1.0),
            );
            return;
        }
        if !self.view.can_follow_up {
            return;
        }
        canvas.fill_rounded_rect(r, 8.0, self.color(palette::WELL, 1.0));
        if self.focused && self.area == Area::Field {
            canvas.stroke_rounded_rect(
                Rect::new(r.left + 0.5, r.top + 0.5, r.right - 0.5, r.bottom - 0.5),
                8.0,
                self.color(palette::ACCENT, 0.55),
                Stroke::plain(1.0),
            );
        }
        self.field.draw(
            canvas,
            self.field_text,
            &self.placeholder(),
            &style(SMALL_SIZE, SMALL_LINE, 400),
            &FieldColors {
                text: self.color(palette::FG, 1.0),
                placeholder: self.color(palette::FAINT, 1.0),
                caret: self.color(palette::FG, 1.0),
                selection: self.color(palette::ACCENT, 0.38),
            },
            self.focused && self.area == Area::Field,
        );
        let button = self.send_button();
        if self.send_shown() {
            let fill = if self.can_send() {
                palette::ACCENT
            } else {
                palette::SURFACE
            };
            canvas.fill_rounded_rect(button, 6.0, self.color(fill, 1.0));
            let color = if self.can_send() {
                palette::WHITE
            } else {
                palette::FAINT
            };
            self.draw_icon(canvas, &SEND_ICON, button, self.color(color, 1.0));
        } else {
            self.icon_button(canvas, ID_SEND, &MIC_ICON, button, false);
        }
    }
    fn send_button(&self) -> Rect {
        let c = self.composer;
        let size = BUTTON;
        let bottom = c.bottom - (36.0 - size) / 2.0;
        Rect::new(c.right - 4.0 - size, bottom - size, c.right - 4.0, bottom)
    }
}

impl<T: TextEngine> OverlayModel<T> for Answer<T::Layout>
where
    T::Layout: Clone,
{
    type Command = Command;
    type Action = AnswerAction;
    type Pointer = UiPointer<AnswerAction>;

    fn apply(&mut self, command: Command, cx: &Cx<'_, T>) -> Ops<AnswerAction> {
        if cx.dpi != 0 {
            self.dpi = cx.dpi;
        }
        let mut ops = Vec::new();
        match command {
            Command::Show {
                view,
                pointer,
                work,
                focus,
            } => {
                let first = !self.visible;
                self.view = view;
                self.copied = false;
                if first {
                    self.earlier_open = false;
                    self.reasoning_open = false;
                    self.voice = None;
                    self.rich.clear_selection();
                    self.scroll = Scroll::default();
                    self.area = Area::Field;
                }
                let mut sink = Vec::new();
                self.refresh(cx.text, &mut sink);
                if first {
                    self.place(pointer, work);
                }
                self.visible = true;
                self.transition.show();
                ops.extend([
                    WindowOp::SetBounds(self.bounds()),
                    WindowOp::SetInteractive(true),
                    WindowOp::Redraw,
                    WindowOp::RaiseTopmost,
                ]);
                if focus {
                    ops.push(WindowOp::Focus);
                }
                return ops;
            }
            Command::Update(view) => {
                if view.text != self.view.text {
                    self.copied = false;
                }
                self.view = view;
            }
            Command::Voice {
                active,
                text,
                hotkey,
            } => {
                self.voice = active.then_some((text, hotkey));
                if !active {
                    self.levels = [0.0; WAVE_BARS];
                }
            }
            Command::Level(level) => {
                self.levels.rotate_left(1);
                self.levels[WAVE_BARS - 1] = wave_scale(level);
                // 波形只改画面，不改尺寸。
                return vec![WindowOp::Redraw];
            }
            Command::Paste(text) => {
                self.field.insert(&text);
                self.area = Area::Field;
            }
            Command::RestoreDraft(text) => {
                if self.field.is_empty() {
                    self.field.set_text(&text);
                }
            }
            Command::Hide => {
                self.voice = None;
                self.hover = None;
                self.pressed = None;
                return if self.transition.hide() {
                    vec![WindowOp::SetInteractive(false)]
                } else {
                    self.visible = false;
                    vec![WindowOp::Hide]
                };
            }
        }
        if self.visible {
            self.refresh(cx.text, &mut ops);
        }
        ops
    }

    fn input(&mut self, input: UiInput, cx: &Cx<'_, T>) -> Ops<AnswerAction> {
        let mut ops = Vec::new();
        match input {
            UiInput::Hover(id) => self.hover = id,
            UiInput::Press(id) => self.pressed = id,
            UiInput::Click(id) => self.click(id, &mut ops),
            UiInput::RightClick(_) => return ops,
            UiInput::PointPress { id, point } => {
                let clicks = self.clicks.press(cx.now, point);
                match id {
                    ID_BODY => {
                        self.area = Area::Body;
                        self.rich.press(cx.text, self.answer_point(point), clicks);
                    }
                    ID_FIELD => {
                        self.area = Area::Field;
                        self.rich.clear_selection();
                        let local = Point::new(
                            point.x - self.field_text.left,
                            point.y - self.field_text.top,
                        );
                        match clicks {
                            1 => self.field.point(cx.text, local, false),
                            2 => self.field.select_word_at(cx.text, local),
                            _ => self.field.select_all(),
                        }
                    }
                    _ => {}
                }
            }
            UiInput::PointDrag { id, point } => match id {
                ID_BODY => {
                    // 拖到可视区外时跟着滚动。
                    if point.y < self.body.top {
                        self.scroll.scroll_by(point.y - self.body.top);
                    } else if point.y > self.body.bottom {
                        self.scroll.scroll_by(point.y - self.body.bottom);
                    }
                    self.rich.drag(cx.text, self.answer_point(point));
                }
                ID_FIELD => {
                    let local = Point::new(
                        point.x - self.field_text.left,
                        point.y - self.field_text.top,
                    );
                    self.field.point(cx.text, local, true);
                }
                _ => return ops,
            },
            UiInput::PointRelease { id, point } => {
                if id == ID_BODY {
                    if let Some(url) = self.rich.release(cx.text, self.answer_point(point)) {
                        ops.push(WindowOp::Action(AnswerAction::OpenLink(url)));
                    }
                }
            }
        }
        self.refresh(cx.text, &mut ops);
        ops
    }

    fn key(&mut self, input: KeyInput, cx: &Cx<'_, T>) -> Ops<AnswerAction> {
        let mut ops = Vec::new();
        if let KeyInput::Down { key, modifiers } = &input {
            let body_selected = self.area == Area::Body && self.rich.has_selection();
            match key {
                Key::Escape if self.voice.is_some() => {
                    ops.push(WindowOp::Action(AnswerAction::CancelVoice));
                    return ops;
                }
                Key::Escape => {
                    ops.push(WindowOp::Action(AnswerAction::Close));
                    return ops;
                }
                Key::Letter('C') if modifiers.command && body_selected => {
                    if let Some(text) = self.rich.selected_text() {
                        ops.push(WindowOp::Action(AnswerAction::Copy(text)));
                    }
                    return ops;
                }
                Key::Letter('A') if modifiers.command && self.area == Area::Body => {
                    self.rich.select_all();
                    self.refresh(cx.text, &mut ops);
                    return ops;
                }
                Key::PageUp | Key::PageDown => {
                    let page = self.body.height() - BODY_LINE;
                    self.scroll
                        .scroll_by(if *key == Key::PageUp { -page } else { page });
                    ops.push(WindowOp::Redraw);
                    return ops;
                }
                _ => {}
            }
        }
        if self.voice.is_some() || !self.view.can_follow_up {
            return ops;
        }
        if !matches!(input, KeyInput::Down { .. }) {
            // 打字回到输入框。
            self.area = Area::Field;
            self.rich.clear_selection();
        }
        match self.field.key(cx.text, input) {
            FieldEvent::None | FieldEvent::Unhandled(_) => return ops,
            FieldEvent::Changed => {}
            FieldEvent::Submit => self.send(&mut ops),
            FieldEvent::Copy(text) => ops.push(WindowOp::Action(AnswerAction::Copy(text))),
            FieldEvent::Paste => ops.push(WindowOp::Action(AnswerAction::Paste)),
        }
        self.refresh(cx.text, &mut ops);
        ops
    }

    fn wheel(&mut self, _: Point, lines: f32, _: &Cx<'_, T>) -> Ops<AnswerAction> {
        if self.scroll.scroll_by(-lines * BODY_LINE) {
            vec![WindowOp::Redraw]
        } else {
            Vec::new()
        }
    }

    fn window_event(&mut self, event: WindowEvent, cx: &Cx<'_, T>) -> Ops<AnswerAction> {
        match event {
            // 自己改位置也会收到移动事件；只有拖动结束才改变放置方式。
            WindowEvent::Moved { .. } => Vec::new(),
            WindowEvent::SystemDragEnded { bounds } => {
                self.x = bounds.x;
                self.y = bounds.y;
                Vec::new()
            }
            WindowEvent::DpiChanged { dpi, suggested } => {
                if dpi == 0 || dpi == self.dpi {
                    return Vec::new();
                }
                self.dpi = dpi;
                self.x = suggested.x;
                self.y = suggested.y;
                self.relayout(cx.text);
                vec![WindowOp::SetBounds(self.bounds()), WindowOp::Redraw]
            }
            WindowEvent::Focus(focused) => {
                self.focused = focused;
                vec![
                    WindowOp::Redraw,
                    WindowOp::Action(AnswerAction::Focused(focused)),
                ]
            }
            WindowEvent::AreaChanged => Vec::new(),
        }
    }
    fn set_look(&mut self, look: &Look) -> Ops<AnswerAction> {
        self.look = *look;
        vec![WindowOp::Redraw]
    }
    fn on_frame(&mut self, _: f32, cx: &Cx<'_, T>) -> Ops<AnswerAction> {
        let (_, finished) = self.transition.tick(cx.now);
        let mut ops = Vec::new();
        if finished && self.transition.is_fully_hidden() {
            self.visible = false;
            ops.push(WindowOp::Hide);
        }
        ops.push(WindowOp::Redraw);
        ops
    }
    fn wants_frames(&self, _: Instant) -> bool {
        self.transition.is_animating()
    }
    fn prepare_frame(&mut self, cx: &Cx<'_, T>) -> Option<FrameSpec> {
        if cx.dpi != 0 {
            self.dpi = cx.dpi;
        }
        self.visible.then(|| FrameSpec {
            bounds: self.bounds(),
            dpi: self.dpi,
            alpha: self.transition.visual(),
        })
    }
    fn draw<C: Canvas<Text = T>>(&self, canvas: &mut C, _: Instant) {
        let panel = Rect::new(0.0, 0.0, WIDTH, self.height);
        canvas.fill_rounded_rect(panel, RADIUS, self.color(palette::PANEL, 1.0));
        canvas.stroke_rounded_rect(
            Rect::new(0.5, 0.5, WIDTH - 0.5, self.height - 0.5),
            RADIUS - 0.5,
            self.color(palette::WHITE, 0.18),
            Stroke::plain(1.0),
        );
        paint::label(
            canvas,
            &self.view.label,
            &paint::label_style(CAPTION_SIZE, 400),
            Rect::new(16.0, 0.0, WIDTH - 80.0, HEADER),
            self.color(palette::ACCENT_TEXT, 1.0),
        );
        let (pin, close) = header_buttons();
        self.icon_button(canvas, ID_PIN, &PIN_ICON, pin, self.view.pinned);
        self.icon_button(canvas, ID_CLOSE, &CLOSE_ICON, close, false);
        self.draw_body(canvas);
        self.draw_composer(canvas);
    }
    fn hit(&self) -> HitMap {
        let mut map = HitMap::new(self.dpi);
        if !self.visible {
            return map;
        }
        map.push(ID_DRAG, Rect::new(0.0, 0.0, WIDTH, HEADER), HitKind::Drag);
        let (pin, close) = header_buttons();
        map.push(ID_PIN, pin, HitKind::Button);
        map.push(ID_CLOSE, close, HitKind::Button);
        map.push(ID_BODY, self.body, HitKind::Text);
        let content = &self.content;
        for (id, rect) in [
            (ID_EARLIER, content.earlier_row),
            (ID_REASONING, content.reasoning_row),
            (ID_INSERT, content.insert),
            (ID_COPY, content.copy),
            (ID_REGENERATE, content.regenerate),
        ] {
            if let Some(rect) = self.on_screen(rect) {
                map.push(id, rect, HitKind::Button);
            }
        }
        if self.voice.is_some() {
            map.push(ID_STRIP, self.composer, HitKind::Button);
        } else if self.view.can_follow_up {
            map.push(ID_FIELD, self.composer, HitKind::Text);
            map.push(ID_SEND, self.send_button(), HitKind::Button);
        }
        map.set_hover(self.hover);
        map.set_caret(self.ime_caret.filter(|_| self.focused));
        map
    }
    fn glass(&self) -> Vec<Shape> {
        if self.look.profile != Profile::Css || self.look.glass.is_none() || !self.visible {
            return Vec::new();
        }
        vec![Shape::RoundedRect {
            rect: Rect::new(0.0, 0.0, WIDTH, self.height),
            radius: RADIUS,
        }]
    }
}

fn header_buttons() -> (Rect, Rect) {
    let top = (HEADER - BUTTON) / 2.0;
    let close = Rect::new(WIDTH - 6.0 - BUTTON, top, WIDTH - 6.0, top + BUTTON);
    let pin = moved(close, -BUTTON - 2.0, 0.0);
    (pin, close)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeLayout, FakeText};
    use std::rc::Rc;

    const WORK: PxRect = PxRect::new(0, 0, 1920, 1040);

    fn cx(text: &FakeText) -> Cx<'_, FakeText> {
        Cx {
            now: Instant::now(),
            dpi: 96,
            area: PxRect::default(),
            text,
        }
    }
    fn view(text: &str) -> AnswerView {
        AnswerView {
            label: "解释".into(),
            text: text.into(),
            hotkey: Some("右 Alt".into()),
            can_follow_up: true,
            ..AnswerView::default()
        }
    }
    fn shown(text: &str, pointer: (i32, i32)) -> (Answer<Rc<FakeLayout>>, Ops<AnswerAction>) {
        let mut model = Answer::new(96, &Look::WINDOWS_LEGACY);
        let ops = model.apply(
            Command::Show {
                view: view(text),
                pointer,
                work: WORK,
                focus: true,
            },
            &cx(&FakeText),
        );
        (model, ops)
    }
    fn bounds(ops: &Ops<AnswerAction>) -> Option<PxRect> {
        ops.iter().rev().find_map(|op| match op {
            WindowOp::SetBounds(b) => Some(*b),
            _ => None,
        })
    }
    fn actions(ops: Ops<AnswerAction>) -> Vec<AnswerAction> {
        ops.into_iter()
            .filter_map(|op| match op {
                WindowOp::Action(a) => Some(a),
                _ => None,
            })
            .collect()
    }
    fn key(model: &mut Answer<Rc<FakeLayout>>, input: KeyInput) -> Vec<AnswerAction> {
        actions(model.key(input, &cx(&FakeText)))
    }
    fn down(key: Key, command: bool) -> KeyInput {
        KeyInput::Down {
            key,
            modifiers: Modifiers {
                command,
                ..Modifiers::default()
            },
        }
    }

    #[test]
    fn top_left_follows_the_pointer_takes_focus_and_grows_downwards_up_to_a_limit() {
        let (mut model, ops) = shown("短", (300, 200));
        let first = bounds(&ops).unwrap();
        assert_eq!((first.x, first.y, first.width), (300, 200, 520));
        assert!(ops.contains(&WindowOp::Focus));
        let ops = model.apply(Command::Update(view(&"很长的回答。".repeat(400))), &cx(&FakeText));
        let grown = bounds(&ops).unwrap();
        assert_eq!((grown.y, grown.height), (200, MAX_HEIGHT as i32));
        assert!(model.scroll.overflows());
    }

    #[test]
    fn near_the_edges_it_moves_only_as_far_as_needed_to_stay_on_screen() {
        let (mut model, ops) = shown("短", (1800, 900));
        let first = bounds(&ops).unwrap();
        assert_eq!(first.x + first.width, 1920 - 12);
        assert_eq!(first.y, 900.min(1040 - 12 - first.height));
        let grown = bounds(&model.apply(
            Command::Update(view(&"回答。".repeat(200))),
            &cx(&FakeText),
        ))
        .unwrap();
        assert_eq!(grown.y + grown.height, 1040 - 12);
        assert!(grown.height > first.height);
    }

    #[test]
    fn enter_sends_the_trimmed_prompt_and_clears_the_field_but_not_while_streaming() {
        let (mut model, _) = shown("答", (0, 0));
        key(&mut model, KeyInput::Text(" 再详细点 ".into()));
        assert_eq!(
            key(&mut model, down(Key::Enter, false)),
            vec![AnswerAction::Send("再详细点".into())]
        );
        assert!(model.field.is_empty());
        let mut streaming = view("");
        let mut no_context = view("答");
        no_context.can_follow_up = false;
        model.apply(Command::Update(no_context), &cx(&FakeText));
        key(&mut model, KeyInput::Text("译".into()));
        assert!(model.field.is_empty());
        streaming.streaming = true;
        model.apply(Command::Update(streaming), &cx(&FakeText));
        key(&mut model, KeyInput::Text("下一个".into()));
        assert!(key(&mut model, down(Key::Enter, false)).is_empty());
        assert_eq!(model.field.text(), "下一个");
    }

    #[test]
    fn escape_cancels_voice_first_then_closes() {
        let (mut model, _) = shown("答", (0, 0));
        model.apply(
            Command::Voice {
                active: true,
                text: String::new(),
                hotkey: true,
            },
            &cx(&FakeText),
        );
        assert_eq!(key(&mut model, down(Key::Escape, false)), vec![AnswerAction::CancelVoice]);
        model.apply(
            Command::Voice {
                active: false,
                text: String::new(),
                hotkey: true,
            },
            &cx(&FakeText),
        );
        assert_eq!(key(&mut model, down(Key::Escape, false)), vec![AnswerAction::Close]);
    }

    #[test]
    fn copy_shortcut_takes_the_answer_selection_after_selecting_in_the_body() {
        let (mut model, _) = shown("可以复制的回答", (0, 0));
        let body = Point::new(PAD_X + 1.0, HEADER + 5.0);
        model.input(UiInput::PointPress { id: ID_BODY, point: body }, &cx(&FakeText));
        key(&mut model, down(Key::Letter('A'), true));
        assert_eq!(
            key(&mut model, down(Key::Letter('C'), true)),
            vec![AnswerAction::Copy("可以复制的回答".into())]
        );
    }

    #[test]
    fn focus_changes_are_reported_and_the_ime_caret_is_published_only_while_focused() {
        let (mut model, _) = shown("答", (0, 0));
        assert_eq!(
            actions(model.window_event(WindowEvent::Focus(true), &cx(&FakeText))),
            vec![AnswerAction::Focused(true)]
        );
        key(&mut model, KeyInput::Text("问".into()));
        let caret = OverlayModel::<FakeText>::hit(&model).caret().unwrap();
        assert!(model.field_text.contains(Point::new(caret.left, caret.top + 1.0)));
        model.window_event(WindowEvent::Focus(false), &cx(&FakeText));
        assert_eq!(OverlayModel::<FakeText>::hit(&model).caret(), None);
    }
}

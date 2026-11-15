//! 输入框：多行纯文字编辑，最多长到若干行，之后在框内滚动。
//!
//! 只管编辑状态、排版与绘制；剪贴板读写由宿主完成：复制、剪切通过返回值交出文字，
//! 粘贴返回 `FieldEvent::Paste`，宿主读到剪贴板后调 `insert`。索引一律为 UTF-16。
use crate::canvas::{Canvas, TextEngine, TextRun, TextStyle};
use crate::geom::{Color, Point, Rect};
use crate::window::{Key, KeyInput, Modifiers};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldEvent {
    None,
    /// 文字或光标变了，需要重画。
    Changed,
    /// 回车提交（Shift+回车换行）。
    Submit,
    /// 复制或剪切出的文字，交给剪贴板。
    Copy(String),
    /// 请求粘贴：宿主读取剪贴板后调用 `insert`。
    Paste,
    /// 输入框不处理的键（Esc、PageUp 等），交给窗口。
    Unhandled(Key),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldColors {
    pub text: Color,
    pub placeholder: Color,
    pub caret: Color,
    pub selection: Color,
}

pub struct TextField<L> {
    text: String,
    caret: u32,
    anchor: u32,
    /// 输入法组字中的文字与组字串内的光标。
    compose: Option<(String, u32)>,
    max_length: usize,
    layout: Option<(L, String, f32)>,
    scroll: f32,
}

impl<L: Clone> TextField<L> {
    pub fn new(max_length: usize) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            anchor: 0,
            compose: None,
            max_length,
            layout: None,
            scroll: 0.0,
        }
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.compose.is_none()
    }
    pub fn is_composing(&self) -> bool {
        self.compose.is_some()
    }
    pub fn set_text(&mut self, text: &str) {
        self.text = truncate_chars(text, self.max_length);
        self.caret = utf16_len(&self.text);
        self.anchor = self.caret;
        self.compose = None;
    }
    fn selection(&self) -> (u32, u32) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }
    fn selected(&self) -> Option<String> {
        let (start, end) = self.selection();
        (start < end).then(|| slice(&self.text, start, end))
    }
    /// 用 text 替换选区（粘贴、键入、输入法上屏），超出长度上限的部分丢弃。
    pub fn insert(&mut self, text: &str) {
        let (start, end) = self.selection();
        let kept = self.text.chars().count() - slice(&self.text, start, end).chars().count();
        let text = truncate_chars(text, self.max_length.saturating_sub(kept));
        self.text = format!(
            "{}{}{}",
            slice(&self.text, 0, start),
            text,
            slice(&self.text, end, u32::MAX)
        );
        self.caret = start + utf16_len(&text);
        self.anchor = self.caret;
    }
    fn move_to(&mut self, index: u32, extend: bool) {
        self.caret = index.min(utf16_len(&self.text));
        if !extend {
            self.anchor = self.caret;
        }
    }
    fn delete(&mut self, forward: bool) {
        let (start, end) = self.selection();
        if start == end {
            let target = if forward {
                next_boundary(&self.text, self.caret)
            } else {
                previous_boundary(&self.text, self.caret)
            };
            self.anchor = target;
        }
        self.insert("");
    }
    /// 处理键盘与输入法输入；engine 用于上下移动时按行定位。
    pub fn key<T: TextEngine<Layout = L>>(&mut self, engine: &T, input: KeyInput) -> FieldEvent {
        match input {
            KeyInput::Text(text) => {
                self.compose = None;
                self.insert(&text);
                FieldEvent::Changed
            }
            KeyInput::Compose { text, caret } => {
                self.compose = (!text.is_empty()).then(|| {
                    let caret = caret.min(utf16_len(&text));
                    (text, caret)
                });
                FieldEvent::Changed
            }
            // 组字期间的按键由输入法处理。
            KeyInput::Down { .. } if self.compose.is_some() => FieldEvent::None,
            KeyInput::Down { key, modifiers } => self.key_down(engine, key, modifiers),
        }
    }
    fn key_down<T: TextEngine<Layout = L>>(
        &mut self,
        engine: &T,
        key: Key,
        modifiers: Modifiers,
    ) -> FieldEvent {
        let extend = modifiers.shift;
        match key {
            Key::Enter if modifiers.shift => self.insert("\n"),
            Key::Enter => return FieldEvent::Submit,
            Key::Backspace => self.delete(false),
            Key::Delete => self.delete(true),
            // 有选区时不带 Shift 的左右键只收起选区，落在选区对应的一端。
            Key::Left | Key::Right => {
                let (start, end) = self.selection();
                let target = match (key == Key::Left, extend || start == end) {
                    (true, true) => previous_boundary(&self.text, self.caret),
                    (false, true) => next_boundary(&self.text, self.caret),
                    (true, false) => start,
                    (false, false) => end,
                };
                self.move_to(target, extend);
            }
            Key::Home => self.move_to(0, extend),
            Key::End => self.move_to(u32::MAX, extend),
            Key::Up | Key::Down => {
                let Some((layout, _, _)) = &self.layout else {
                    return FieldEvent::None;
                };
                let caret = engine.caret_rect(layout, self.caret);
                let line = caret.bottom - caret.top;
                let y = if key == Key::Up {
                    caret.top - line / 2.0
                } else {
                    caret.bottom + line / 2.0
                };
                if y < 0.0 {
                    self.move_to(0, extend);
                } else {
                    let index = engine.hit_test(layout, Point::new(caret.left, y));
                    self.move_to(index, extend);
                }
            }
            Key::Letter('A') if modifiers.command => {
                self.anchor = 0;
                self.caret = utf16_len(&self.text);
            }
            Key::Letter('C') if modifiers.command => {
                return self.selected().map_or(FieldEvent::None, FieldEvent::Copy);
            }
            Key::Letter('X') if modifiers.command => {
                let Some(text) = self.selected() else {
                    return FieldEvent::None;
                };
                self.insert("");
                return FieldEvent::Copy(text);
            }
            Key::Letter('V') if modifiers.command => return FieldEvent::Paste,
            other => return FieldEvent::Unhandled(other),
        }
        FieldEvent::Changed
    }

    /// 显示用的文字：组字串插在光标处。返回文字与组字区间。
    fn display(&self) -> (String, Option<(u32, u32)>) {
        match &self.compose {
            None => (self.text.clone(), None),
            Some((compose, _)) => {
                let (start, end) = self.selection();
                (
                    format!(
                        "{}{}{}",
                        slice(&self.text, 0, start),
                        compose,
                        slice(&self.text, end, u32::MAX)
                    ),
                    Some((start, utf16_len(compose))),
                )
            }
        }
    }
    fn display_caret(&self) -> u32 {
        match &self.compose {
            Some((_, caret)) => self.selection().0 + caret,
            None => self.caret,
        }
    }
    /// 排版并返回框内文字区的高度：不少于一行，最多 max_lines 行。
    pub fn layout<T: TextEngine<Layout = L>>(
        &mut self,
        engine: &T,
        style: &TextStyle<'_>,
        width: f32,
        max_lines: u32,
    ) -> f32 {
        let line = style.line_spacing.map_or(style.size * 1.5, |s| s.height);
        let (display, compose) = self.display();
        let reuse = self
            .layout
            .as_ref()
            .is_some_and(|(_, text, w)| *text == display && (w - width).abs() < 0.01);
        if !reuse {
            let runs: Vec<TextRun> = compose
                .map(|(start, len)| TextRun {
                    utf16_start: start,
                    utf16_len: len,
                    underline: true,
                    ..TextRun::default()
                })
                .into_iter()
                .collect();
            self.layout = engine
                .layout_rich(&display, style, &runs, width, f32::MAX)
                .map(|layout| (layout, display, width));
        }
        let Some((layout, _, _)) = &self.layout else {
            return line;
        };
        let content = engine.metrics(layout).height.max(line);
        let height = content.min(line * max_lines as f32);
        // 光标始终留在可视区内。
        let caret = engine.caret_rect(layout, self.display_caret());
        if caret.top < self.scroll {
            self.scroll = caret.top;
        } else if caret.bottom > self.scroll + height {
            self.scroll = caret.bottom - height;
        }
        self.scroll = self.scroll.clamp(0.0, (content - height).max(0.0));
        height
    }
    /// 光标矩形（相对文字区左上角），供输入法候选窗定位。
    pub fn caret_rect<T: TextEngine<Layout = L>>(&self, engine: &T) -> Option<Rect> {
        let (layout, _, _) = self.layout.as_ref()?;
        let caret = engine.caret_rect(layout, self.display_caret());
        Some(Rect::new(
            caret.left,
            caret.top - self.scroll,
            caret.right + 1.0,
            caret.bottom - self.scroll,
        ))
    }
    /// 文字区内的点击位置放下光标；extend 时扩展选区（拖动）。
    pub fn point<T: TextEngine<Layout = L>>(&mut self, engine: &T, point: Point, extend: bool) {
        if self.compose.is_some() {
            return;
        }
        if let Some((layout, _, _)) = &self.layout {
            let index = engine.hit_test(layout, Point::new(point.x, point.y + self.scroll));
            self.move_to(index, extend);
        }
    }
    pub fn select_word_at<T: TextEngine<Layout = L>>(&mut self, engine: &T, point: Point) {
        self.point(engine, point, false);
        let (start, end) = super::rich_text::word_bounds(&self.text, self.caret);
        self.anchor = start;
        self.caret = end;
    }
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = utf16_len(&self.text);
    }
    /// 在 rect（文字区）内绘制；focused 时画光标与选区。
    pub fn draw<C: Canvas>(
        &self,
        canvas: &mut C,
        rect: Rect,
        placeholder: &str,
        style: &TextStyle<'_>,
        colors: &FieldColors,
        focused: bool,
    ) where
        C::Text: TextEngine<Layout = L>,
    {
        canvas.save();
        canvas.clip_rect(rect);
        let origin = Point::new(rect.left, rect.top - self.scroll);
        if self.is_empty() {
            if let Some(layout) = canvas
                .text()
                .layout(placeholder, style, rect.width(), rect.height())
            {
                canvas.draw_text(&layout, Point::new(rect.left, rect.top), colors.placeholder, None);
            }
        } else if let Some((layout, _, _)) = &self.layout {
            let (start, end) = self.selection();
            if focused && start < end && self.compose.is_none() {
                for r in canvas.text().range_rects(layout, start, end - start) {
                    canvas.fill_rounded_rect(
                        Rect::new(
                            r.left + origin.x,
                            r.top + origin.y,
                            r.right + origin.x,
                            r.bottom + origin.y,
                        ),
                        2.0,
                        colors.selection,
                    );
                }
            }
            canvas.draw_text(layout, origin, colors.text, None);
        }
        if focused {
            let caret = self
                .layout
                .as_ref()
                .filter(|_| !self.is_empty())
                .map(|(layout, _, _)| canvas.text().caret_rect(layout, self.display_caret()))
                .unwrap_or_else(|| {
                    let line = style.line_spacing.map_or(style.size * 1.5, |s| s.height);
                    Rect::new(0.0, 0.0, 0.0, line)
                });
            canvas.fill_rounded_rect(
                Rect::new(
                    origin.x + caret.left,
                    origin.y + caret.top + 2.0,
                    origin.x + caret.left + 1.5,
                    origin.y + caret.bottom - 2.0,
                ),
                0.0,
                colors.caret,
            );
        }
        canvas.restore();
    }
}

fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

fn slice(text: &str, start: u32, end: u32) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = (end as usize).min(units.len());
    let start = (start as usize).min(end);
    String::from_utf16_lossy(&units[start..end])
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// 字符边界：代理对不能从中间切开。
fn previous_boundary(text: &str, index: u32) -> u32 {
    let mut at = 0;
    let mut previous = 0;
    for c in text.chars() {
        if at >= index {
            break;
        }
        previous = at;
        at += c.len_utf16() as u32;
    }
    previous
}

fn next_boundary(text: &str, index: u32) -> u32 {
    let mut at = 0;
    for c in text.chars() {
        at += c.len_utf16() as u32;
        if at > index {
            return at;
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{FontFamily, ParagraphAlign, TextAlign, UniformLineSpacing};
    use crate::testing::{FakeLayout, FakeText};
    use std::rc::Rc;

    const STYLE: TextStyle<'static> = TextStyle {
        family: FontFamily::SystemUi,
        size: 10.0,
        weight: 400,
        align: TextAlign::Leading,
        paragraph: ParagraphAlign::Near,
        wrap: true,
        line_spacing: Some(UniformLineSpacing {
            height: 20.0,
            baseline: 16.0,
        }),
    };
    const NONE: Modifiers = Modifiers {
        command: false,
        shift: false,
        alt: false,
    };

    fn field() -> TextField<Rc<FakeLayout>> {
        TextField::new(10)
    }
    fn down(key: Key, modifiers: Modifiers) -> KeyInput {
        KeyInput::Down { key, modifiers }
    }

    #[test]
    fn typing_editing_and_surrogate_pairs() {
        let mut f = field();
        f.key(&FakeText, KeyInput::Text("ab😀c".into()));
        f.key(&FakeText, down(Key::Left, NONE));
        f.key(&FakeText, down(Key::Backspace, NONE));
        assert_eq!(f.text(), "abc");
        f.key(&FakeText, down(Key::Home, NONE));
        f.key(&FakeText, down(Key::End, Modifiers { shift: true, ..NONE }));
        f.key(&FakeText, KeyInput::Text("新".into()));
        assert_eq!(f.text(), "新");
    }

    #[test]
    fn enter_submits_and_shift_enter_breaks_the_line() {
        let mut f = field();
        f.key(&FakeText, KeyInput::Text("a".into()));
        f.key(&FakeText, down(Key::Enter, Modifiers { shift: true, ..NONE }));
        assert_eq!(f.text(), "a\n");
        assert_eq!(f.key(&FakeText, down(Key::Enter, NONE)), FieldEvent::Submit);
    }

    #[test]
    fn clipboard_shortcuts_hand_text_to_the_host() {
        let cmd = Modifiers {
            command: true,
            ..NONE
        };
        let mut f = field();
        f.set_text("hello");
        f.key(&FakeText, down(Key::Letter('A'), cmd));
        assert_eq!(
            f.key(&FakeText, down(Key::Letter('X'), cmd)),
            FieldEvent::Copy("hello".into())
        );
        assert!(f.is_empty());
        assert_eq!(f.key(&FakeText, down(Key::Letter('V'), cmd)), FieldEvent::Paste);
        f.insert("一二三四五六七八九十十一");
        assert_eq!(f.text(), "一二三四五六七八九十");
    }

    #[test]
    fn composition_shows_inline_and_blocks_editing_keys_until_committed() {
        let mut f = field();
        f.set_text("ab");
        f.key(&FakeText, down(Key::Left, NONE));
        f.key(
            &FakeText,
            KeyInput::Compose {
                text: "ni".into(),
                caret: 2,
            },
        );
        assert_eq!(f.display().0, "anib");
        assert_eq!(f.display_caret(), 3);
        assert_eq!(f.key(&FakeText, down(Key::Backspace, NONE)), FieldEvent::None);
        f.key(&FakeText, KeyInput::Text("你".into()));
        assert_eq!(f.text(), "a你b");
        assert!(!f.is_composing());
    }

    #[test]
    fn grows_to_max_lines_and_scrolls_to_keep_the_caret_visible() {
        let mut f = TextField::new(100);
        f.set_text("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        // 字宽 6：60 宽一行 10 字，51 字共 6 行，最多显示 4 行。
        assert_eq!(f.layout(&FakeText, &STYLE, 60.0, 4), 80.0);
        assert_eq!(f.caret_rect(&FakeText).map(|r| r.bottom), Some(80.0));
        f.key(&FakeText, down(Key::Home, NONE));
        f.layout(&FakeText, &STYLE, 60.0, 4);
        assert_eq!(f.caret_rect(&FakeText).map(|r| r.top), Some(0.0));
    }
}

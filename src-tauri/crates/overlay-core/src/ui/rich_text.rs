//! 富文本视图：把 Markdown 排成一列可选中、可点链接的块。
//!
//! 只负责内容坐标里的排版、命中、选区和绘制；滚动、裁剪和放在窗口哪里由使用方决定
//! （坐标都相对内容左上角）。每块的排版结果按块内容缓存，流式回答只重排变了的块。
use crate::canvas::{
    Canvas, FontFamily, ParagraphAlign, TextAlign, TextEngine, TextRun, TextStyle, TextTint,
    UniformLineSpacing,
};
use crate::geom::{Color, Point, Rect, Stroke};
use crate::markdown::{self, Block, BlockKind, Inline};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RichTheme {
    /// 正文字号与行高。
    pub size: f32,
    pub line_height: f32,
    pub text: Color,
    /// 列表符号、引用文字。
    pub muted: Color,
    pub link: Color,
    /// 行内代码与代码块的底色。
    pub code_fill: Color,
    /// 分隔线、表格线、代码块边框。
    pub line: Color,
    pub quote_bar: Color,
    pub header_fill: Color,
    pub selection: Color,
}

const LIST_INDENT: f32 = 22.0;
const QUOTE_INDENT: f32 = 14.0;
const MARKER_GAP: f32 = 6.0;
const CODE_PAD_X: f32 = 12.0;
const CODE_PAD_Y: f32 = 10.0;
const CELL_PAD_X: f32 = 8.0;
const CELL_PAD_Y: f32 = 6.0;
const MIN_COLUMN: f32 = 56.0;
const BASELINE_RATIO: f32 = 0.8;

/// 文档里的位置：第 block 块第 piece 段可选文字中的 UTF-16 索引。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPos {
    pub block: usize,
    pub piece: usize,
    pub index: u32,
}

/// 可选中的一段文字（段落、代码块、表格单元格）。坐标相对所在块的左上角。
struct Piece<L> {
    layout: L,
    origin: Point,
    size: (f32, f32),
    text: String,
    /// 复制时放在这一段前面的分隔符（块首段为换行，同行单元格为制表符）。
    joint: &'static str,
    tints: Vec<TextTint>,
    links: Vec<(u32, u32, String)>,
    /// 行内代码的底色矩形。
    code_rects: Vec<Rect>,
}

#[derive(Clone, Copy)]
enum Paint {
    Muted,
    CodeFill,
    Line,
    Quote,
    Header,
}

enum Decor<L> {
    Fill(Rect, f32, Paint),
    Stroke(Rect, f32, Paint),
    Text(L, Point, Paint),
}

struct Laid<L> {
    block: Block,
    width: f32,
    /// 与上一块的间距；块的 top 已包含它。
    gap: f32,
    top: f32,
    height: f32,
    pieces: Vec<Piece<L>>,
    decor: Vec<Decor<L>>,
}

pub struct RichText<L> {
    source: String,
    blocks: Vec<Block>,
    laid: Vec<Laid<L>>,
    width: f32,
    dirty: bool,
    anchor: Option<TextPos>,
    focus: Option<TextPos>,
    /// 按下后是否拖动过：没拖动的单击才算点链接。
    dragged: bool,
}

impl<L: Clone> Default for RichText<L> {
    fn default() -> Self {
        Self {
            source: String::new(),
            blocks: Vec::new(),
            laid: Vec::new(),
            width: 0.0,
            dirty: false,
            anchor: None,
            focus: None,
            dragged: false,
        }
    }
}

fn body_style(theme: &RichTheme, heading: Option<u8>) -> TextStyle<'static> {
    let (size, line) = match heading {
        Some(1) => (theme.size + 4.0, theme.line_height + 4.0),
        Some(2) => (theme.size + 2.0, theme.line_height + 2.0),
        Some(_) => (theme.size + 1.0, theme.line_height),
        None => (theme.size, theme.line_height),
    };
    TextStyle {
        family: FontFamily::SystemUi,
        size,
        weight: if heading.is_some() { 600 } else { 400 },
        align: TextAlign::Leading,
        paragraph: ParagraphAlign::Near,
        wrap: true,
        line_spacing: Some(UniformLineSpacing {
            height: line,
            baseline: line * BASELINE_RATIO,
        }),
    }
}

fn code_style(theme: &RichTheme) -> TextStyle<'static> {
    let size = theme.size - 1.0;
    let line = theme.line_height - 3.0;
    TextStyle {
        family: FontFamily::Monospace,
        size,
        weight: 400,
        align: TextAlign::Leading,
        paragraph: ParagraphAlign::Near,
        wrap: true,
        line_spacing: Some(UniformLineSpacing {
            height: line,
            baseline: line * BASELINE_RATIO,
        }),
    }
}

fn runs(inline: &Inline) -> Vec<TextRun> {
    inline
        .spans
        .iter()
        .map(|span| TextRun {
            utf16_start: span.utf16_start,
            utf16_len: span.utf16_len,
            bold: span.style.bold,
            italic: span.style.italic,
            mono: span.style.code,
            underline: span.link.is_some(),
            strikethrough: span.style.strike,
        })
        .collect()
}

fn offset(rect: Rect, by: Point) -> Rect {
    Rect::new(
        rect.left + by.x,
        rect.top + by.y,
        rect.right + by.x,
        rect.bottom + by.y,
    )
}

/// 一段带行内样式文字排成的可选段。
fn inline_piece<T: TextEngine>(
    engine: &T,
    inline: &Inline,
    style: &TextStyle<'_>,
    theme: &RichTheme,
    origin: Point,
    width: f32,
    joint: &'static str,
) -> Option<Piece<T::Layout>> {
    let layout = engine.layout_rich(&inline.text, style, &runs(inline), width, f32::MAX)?;
    let metrics = engine.metrics(&layout);
    let mut tints = Vec::new();
    let mut links = Vec::new();
    let mut code_rects = Vec::new();
    for span in &inline.spans {
        if let Some(url) = &span.link {
            tints.push(TextTint {
                utf16_start: span.utf16_start,
                utf16_len: span.utf16_len,
                color: theme.link,
            });
            links.push((span.utf16_start, span.utf16_len, url.clone()));
        }
        if span.style.code {
            code_rects.extend(
                engine
                    .range_rects(&layout, span.utf16_start, span.utf16_len)
                    .into_iter()
                    .map(|r| {
                        offset(
                            Rect::new(r.left - 3.0, r.top + 1.0, r.right + 3.0, r.bottom - 1.0),
                            origin,
                        )
                    }),
            );
        }
    }
    Some(Piece {
        layout,
        origin,
        size: (width, metrics.height),
        text: inline.text.clone(),
        joint,
        tints,
        links,
        code_rects,
    })
}

/// 块与上一块的间距：标题前多留，列表项之间紧凑。
fn gap_before(previous: Option<&Block>, block: &Block) -> f32 {
    let Some(previous) = previous else { return 0.0 };
    let is_item = |b: &Block| {
        matches!(
            &b.kind,
            BlockKind::Text {
                marker: Some(_),
                ..
            }
        )
    };
    match &block.kind {
        BlockKind::Text {
            heading: Some(_), ..
        } => 14.0,
        _ if is_item(block) && previous.indent > 0 => 4.0,
        _ if block.indent > 0 && previous.indent >= block.indent => 4.0,
        _ => 10.0,
    }
}

fn lay_block<T: TextEngine>(
    engine: &T,
    block: &Block,
    width: f32,
    theme: &RichTheme,
) -> (f32, Vec<Piece<T::Layout>>, Vec<Decor<T::Layout>>) {
    let left = f32::from(block.quote) * QUOTE_INDENT + f32::from(block.indent) * LIST_INDENT;
    let avail = (width - left).max(1.0);
    let mut pieces = Vec::new();
    let mut decor = Vec::new();
    let height = match &block.kind {
        BlockKind::Text {
            inline,
            heading,
            marker,
        } => {
            let style = body_style(theme, *heading);
            if let Some(marker) = marker {
                if let Some(layout) = engine.layout(marker, &style, LIST_INDENT, f32::MAX) {
                    let w = engine.metrics(&layout).width;
                    decor.push(Decor::Text(
                        layout,
                        Point::new(left - MARKER_GAP - w, 0.0),
                        Paint::Muted,
                    ));
                }
            }
            match inline_piece(
                engine,
                inline,
                &style,
                theme,
                Point::new(left, 0.0),
                avail,
                "\n",
            ) {
                Some(piece) => {
                    let h = piece.size.1;
                    pieces.push(piece);
                    h
                }
                None => 0.0,
            }
        }
        BlockKind::Code { text, .. } => {
            let style = code_style(theme);
            let inner = (avail - CODE_PAD_X * 2.0).max(1.0);
            let piece = inline_piece(
                engine,
                &Inline::plain(text),
                &style,
                theme,
                Point::new(left + CODE_PAD_X, CODE_PAD_Y),
                inner,
                "\n",
            );
            let text_h = piece.as_ref().map_or(0.0, |p| p.size.1);
            let h = text_h + CODE_PAD_Y * 2.0;
            let rect = Rect::new(left, 0.0, width, h);
            decor.push(Decor::Fill(rect, 8.0, Paint::CodeFill));
            decor.push(Decor::Stroke(rect, 8.0, Paint::Line));
            pieces.extend(piece);
            h
        }
        BlockKind::Rule => {
            decor.push(Decor::Fill(
                Rect::new(left, 8.0, width, 9.0),
                0.0,
                Paint::Line,
            ));
            17.0
        }
        BlockKind::Table { header, rows } => lay_table(
            engine,
            header,
            rows,
            left,
            avail,
            theme,
            &mut pieces,
            &mut decor,
        ),
    };
    for level in 0..block.quote {
        let x = f32::from(level) * QUOTE_INDENT + 2.0;
        decor.push(Decor::Fill(
            Rect::new(x, 0.0, x + 2.0, height),
            1.0,
            Paint::Quote,
        ));
    }
    (height, pieces, decor)
}

// 表格排版就是 lay_block 的一个分支，参数都是它手头的排版上下文，不值得为此另包一个结构。
#[allow(clippy::too_many_arguments)]
fn lay_table<T: TextEngine>(
    engine: &T,
    header: &[Inline],
    rows: &[Vec<Inline>],
    left: f32,
    avail: f32,
    theme: &RichTheme,
    pieces: &mut Vec<Piece<T::Layout>>,
    decor: &mut Vec<Decor<T::Layout>>,
) -> f32 {
    let all: Vec<&[Inline]> = std::iter::once(header)
        .chain(rows.iter().map(Vec::as_slice))
        .collect();
    let columns = all.iter().map(|row| row.len()).max().unwrap_or(0);
    if columns == 0 {
        return 0.0;
    }
    let style = body_style(theme, None);
    let mut natural = vec![MIN_COLUMN; columns];
    for row in &all {
        for (column, cell) in row.iter().enumerate() {
            let mut one_line = style;
            one_line.wrap = false;
            if let Some(layout) =
                engine.layout_rich(&cell.text, &one_line, &runs(cell), f32::MAX, f32::MAX)
            {
                let w = engine.metrics(&layout).width + CELL_PAD_X * 2.0 + 1.0;
                natural[column] = natural[column].max(w);
            }
        }
    }
    let total: f32 = natural.iter().sum();
    let widths: Vec<f32> = if total <= avail {
        natural
    } else {
        natural
            .iter()
            .map(|w| (w / total * avail).max(MIN_COLUMN.min(avail / columns as f32)))
            .collect()
    };
    let table_width: f32 = widths.iter().sum();
    let mut y = 0.0;
    for (row_index, row) in all.iter().enumerate() {
        let mut x = left;
        let mut row_h: f32 = theme.line_height;
        let start = pieces.len();
        for column in 0..columns {
            let w = widths[column];
            if let Some(cell) = row.get(column) {
                let mut cell_style = style;
                if row_index == 0 {
                    cell_style.weight = 600;
                }
                let joint = if column == 0 { "\n" } else { "\t" };
                if let Some(piece) = inline_piece(
                    engine,
                    cell,
                    &cell_style,
                    theme,
                    Point::new(x + CELL_PAD_X, y + CELL_PAD_Y),
                    (w - CELL_PAD_X * 2.0).max(1.0),
                    joint,
                ) {
                    row_h = row_h.max(piece.size.1);
                    pieces.push(piece);
                }
            }
            x += w;
        }
        for piece in &mut pieces[start..] {
            piece.size.1 = row_h;
        }
        let row_bottom = y + row_h + CELL_PAD_Y * 2.0;
        if row_index == 0 {
            decor.push(Decor::Fill(
                Rect::new(left, y, left + table_width, row_bottom),
                0.0,
                Paint::Header,
            ));
        }
        if row_index > 0 {
            decor.push(Decor::Fill(
                Rect::new(left, y, left + table_width, y + 1.0),
                0.0,
                Paint::Line,
            ));
        }
        y = row_bottom;
    }
    let mut x = left;
    for w in &widths[..columns - 1] {
        x += w;
        decor.push(Decor::Fill(Rect::new(x, 0.0, x + 1.0, y), 0.0, Paint::Line));
    }
    decor.push(Decor::Stroke(
        Rect::new(left, 0.0, left + table_width, y),
        6.0,
        Paint::Line,
    ));
    y
}

/// 双击选词的边界：连续的字母数字、连续的汉字各算一个词，其余字符单独成词。
pub(crate) fn word_bounds(text: &str, index: u32) -> (u32, u32) {
    #[derive(PartialEq)]
    enum Class {
        Word,
        Cjk,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() && c.is_ascii() || c == '_' {
            Class::Word
        } else if c.is_alphabetic() {
            Class::Cjk
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    };
    let chars: Vec<(u32, char)> = {
        let mut at = 0;
        text.chars()
            .map(|c| {
                let item = (at, c);
                at += c.len_utf16() as u32;
                item
            })
            .collect()
    };
    let Some(hit) = chars.iter().rposition(|&(at, _)| at <= index) else {
        return (0, 0);
    };
    let kind = class(chars[hit].1);
    if kind == Class::Other {
        let end = chars[hit].0 + chars[hit].1.len_utf16() as u32;
        return (chars[hit].0, end);
    }
    let mut first = hit;
    while first > 0 && class(chars[first - 1].1) == kind {
        first -= 1;
    }
    let mut last = hit;
    while last + 1 < chars.len() && class(chars[last + 1].1) == kind {
        last += 1;
    }
    (
        chars[first].0,
        chars[last].0 + chars[last].1.len_utf16() as u32,
    )
}

fn utf16_slice(text: &str, start: u32, end: u32) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = (end as usize).min(units.len());
    let start = (start as usize).min(end);
    String::from_utf16_lossy(&units[start..end])
}

impl<L: Clone> RichText<L> {
    pub fn source(&self) -> &str {
        &self.source
    }
    /// 换内容；与当前相同时不做任何事。选区超出新内容时收紧到末尾。
    pub fn set_markdown(&mut self, text: &str) {
        if text == self.source {
            return;
        }
        self.source = text.to_owned();
        self.blocks = markdown::parse(text);
        self.dirty = true;
        if self.blocks.is_empty() {
            self.anchor = None;
            self.focus = None;
        }
    }
    /// 按宽度排版；内容和宽度都没变时不做任何事。
    pub fn layout<T: TextEngine<Layout = L>>(&mut self, engine: &T, width: f32, theme: &RichTheme) {
        if !self.dirty && (width - self.width).abs() < 0.01 {
            return;
        }
        let mut previous: Vec<Option<Laid<L>>> = std::mem::take(&mut self.laid)
            .into_iter()
            .map(Some)
            .collect();
        let mut top = 0.0;
        for (i, block) in self.blocks.iter().enumerate() {
            let gap = gap_before(i.checked_sub(1).map(|p| &self.blocks[p]), block);
            top += gap;
            let reused = previous
                .get_mut(i)
                .and_then(Option::take)
                .filter(|old| old.block == *block && (old.width - width).abs() < 0.01);
            let mut laid = match reused {
                Some(old) => old,
                None => {
                    let (height, pieces, decor) = lay_block(engine, block, width, theme);
                    Laid {
                        block: block.clone(),
                        width,
                        gap,
                        top,
                        height,
                        pieces,
                        decor,
                    }
                }
            };
            laid.gap = gap;
            laid.top = top;
            top += laid.height;
            self.laid.push(laid);
        }
        self.width = width;
        self.dirty = false;
        self.clamp_selection();
    }
    pub fn height(&self) -> f32 {
        self.laid.last().map_or(0.0, |laid| laid.top + laid.height)
    }
    fn piece(&self, pos: TextPos) -> Option<&Piece<L>> {
        self.laid.get(pos.block)?.pieces.get(pos.piece)
    }
    fn clamp_selection(&mut self) {
        let clamp = |pos: Option<TextPos>, laid: &[Laid<L>]| -> Option<TextPos> {
            let pos = pos?;
            let last_block = laid.iter().rposition(|l| !l.pieces.is_empty())?;
            let block = pos.block.min(last_block);
            let pieces = &laid[block].pieces;
            if pieces.is_empty() {
                return Some(TextPos {
                    block,
                    piece: 0,
                    index: 0,
                });
            }
            let piece = pos.piece.min(pieces.len() - 1);
            let len = markdown::utf16_len(&pieces[piece].text);
            Some(TextPos {
                block,
                piece,
                index: pos.index.min(len),
            })
        };
        self.anchor = clamp(self.anchor, &self.laid);
        self.focus = clamp(self.focus, &self.laid);
    }

    /// 内容坐标处最近的文字位置；没有任何可选文字时为 None。
    pub fn pos_at<T: TextEngine<Layout = L>>(&self, engine: &T, point: Point) -> Option<TextPos> {
        let block = self
            .laid
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.pieces.is_empty())
            .min_by(|(_, a), (_, b)| {
                let distance = |l: &Laid<L>| {
                    if point.y < l.top {
                        l.top - point.y
                    } else if point.y >= l.top + l.height {
                        point.y - (l.top + l.height) + 0.001
                    } else {
                        0.0
                    }
                };
                distance(a).total_cmp(&distance(b))
            })
            .map(|(i, _)| i)?;
        let laid = &self.laid[block];
        let local = Point::new(point.x, point.y - laid.top);
        let piece = laid
            .pieces
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let distance = |p: &Piece<L>| {
                    let dy = if local.y < p.origin.y {
                        p.origin.y - local.y
                    } else if local.y >= p.origin.y + p.size.1 {
                        local.y - p.origin.y - p.size.1
                    } else {
                        0.0
                    };
                    let dx = if local.x < p.origin.x {
                        p.origin.x - local.x
                    } else if local.x >= p.origin.x + p.size.0 {
                        local.x - p.origin.x - p.size.0
                    } else {
                        0.0
                    };
                    dy * 1000.0 + dx
                };
                distance(a).total_cmp(&distance(b))
            })
            .map(|(i, _)| i)?;
        let p = &laid.pieces[piece];
        let index = engine.hit_test(
            &p.layout,
            Point::new(local.x - p.origin.x, local.y - p.origin.y),
        );
        Some(TextPos {
            block,
            piece,
            index,
        })
    }

    /// 按下：clicks = 1 放下光标，2 选词，3 选整段。
    pub fn press<T: TextEngine<Layout = L>>(&mut self, engine: &T, point: Point, clicks: u8) {
        self.dragged = false;
        let Some(pos) = self.pos_at(engine, point) else {
            self.anchor = None;
            self.focus = None;
            return;
        };
        let text = self.piece(pos).map(|p| p.text.as_str()).unwrap_or_default();
        let (start, end) = match clicks {
            2 => word_bounds(text, pos.index),
            3 => (0, markdown::utf16_len(text)),
            _ => (pos.index, pos.index),
        };
        self.anchor = Some(TextPos {
            index: start,
            ..pos
        });
        self.focus = Some(TextPos { index: end, ..pos });
    }
    pub fn drag<T: TextEngine<Layout = L>>(&mut self, engine: &T, point: Point) {
        if self.anchor.is_none() {
            return;
        }
        if let Some(pos) = self.pos_at(engine, point) {
            if Some(pos) != self.focus {
                self.dragged = true;
            }
            self.focus = Some(pos);
        }
    }
    /// 抬起；没拖动、没选中文字的单击落在链接上时返回链接地址。
    pub fn release<T: TextEngine<Layout = L>>(
        &mut self,
        engine: &T,
        point: Point,
    ) -> Option<String> {
        if self.dragged || self.has_selection() {
            return None;
        }
        let pos = self.pos_at(engine, point)?;
        let piece = self.piece(pos)?;
        let laid = &self.laid[pos.block];
        let local = Point::new(
            point.x - piece.origin.x,
            point.y - laid.top - piece.origin.y,
        );
        if local.x < 0.0 || local.y < 0.0 || local.y > piece.size.1 {
            return None;
        }
        // 命中测试给的是最近的字符边界；点在某字后半时边界在它之后，要退回到这个字。
        let boundary = pos.index;
        let char_index =
            if boundary > 0 && engine.caret_rect(&piece.layout, boundary).left > local.x {
                boundary - 1
            } else {
                boundary
            };
        piece
            .links
            .iter()
            .find(|(start, len, _)| char_index >= *start && char_index < start + len)
            .map(|(_, _, url)| url.clone())
    }
    pub fn has_selection(&self) -> bool {
        matches!((self.anchor, self.focus), (Some(a), Some(b)) if a != b)
    }
    pub fn clear_selection(&mut self) {
        self.anchor = None;
        self.focus = None;
    }
    pub fn select_all(&mut self) {
        let Some(last_block) = self.laid.iter().rposition(|l| !l.pieces.is_empty()) else {
            return;
        };
        let first_block = self
            .laid
            .iter()
            .position(|l| !l.pieces.is_empty())
            .unwrap_or(0);
        let last = &self.laid[last_block];
        let piece = last.pieces.len() - 1;
        self.anchor = Some(TextPos {
            block: first_block,
            piece: 0,
            index: 0,
        });
        self.focus = Some(TextPos {
            block: last_block,
            piece,
            index: markdown::utf16_len(&last.pieces[piece].text),
        });
    }
    fn ordered(&self) -> Option<(TextPos, TextPos)> {
        let (a, b) = (self.anchor?, self.focus?);
        (a != b).then(|| if a <= b { (a, b) } else { (b, a) })
    }
    /// 选中的纯文字；块之间换行，同一行的表格单元格之间用制表符。
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.ordered()?;
        let mut out = String::new();
        for (b, laid) in self
            .laid
            .iter()
            .enumerate()
            .take(end.block + 1)
            .skip(start.block)
        {
            for (p, piece) in laid.pieces.iter().enumerate() {
                let here = TextPos {
                    block: b,
                    piece: p,
                    index: 0,
                };
                if (b, p) < (start.block, start.piece) || (b, p) > (end.block, end.piece) {
                    continue;
                }
                let from = if (b, p) == (start.block, start.piece) {
                    start.index
                } else {
                    0
                };
                let to = if (b, p) == (end.block, end.piece) {
                    end.index
                } else {
                    markdown::utf16_len(&piece.text)
                };
                if here.block != start.block || p != start.piece {
                    out.push_str(piece.joint);
                }
                out.push_str(&utf16_slice(&piece.text, from, to));
            }
        }
        Some(out)
    }
    /// 一块内容的纵向范围（内容坐标），供滚动到选区末端。
    pub fn pos_rect<T: TextEngine<Layout = L>>(&self, engine: &T, pos: TextPos) -> Option<Rect> {
        let piece = self.piece(pos)?;
        let top = self.laid[pos.block].top;
        let caret = engine.caret_rect(&piece.layout, pos.index);
        Some(offset(
            caret,
            Point::new(piece.origin.x, top + piece.origin.y),
        ))
    }
    pub fn focus(&self) -> Option<TextPos> {
        self.focus
    }

    /// 画在 origin（内容左上角在画布上的位置）；只画与 visible（内容坐标的纵向区间）相交的块。
    pub fn draw<C: Canvas>(
        &self,
        canvas: &mut C,
        origin: Point,
        visible: (f32, f32),
        theme: &RichTheme,
    ) where
        C::Text: TextEngine<Layout = L>,
    {
        let selection = self.ordered();
        for (b, laid) in self.laid.iter().enumerate() {
            if laid.top + laid.height < visible.0 || laid.top > visible.1 {
                continue;
            }
            let at = Point::new(origin.x, origin.y + laid.top);
            for decor in &laid.decor {
                match decor {
                    Decor::Fill(rect, radius, paint) => canvas.fill_rounded_rect(
                        offset(*rect, at),
                        *radius,
                        paint_color(theme, *paint),
                    ),
                    Decor::Stroke(rect, radius, paint) => {
                        let r = offset(*rect, at);
                        let inner =
                            Rect::new(r.left + 0.5, r.top + 0.5, r.right - 0.5, r.bottom - 0.5);
                        canvas.stroke_rounded_rect(
                            inner,
                            *radius,
                            paint_color(theme, *paint),
                            Stroke::plain(1.0),
                        );
                    }
                    Decor::Text(layout, point, paint) => canvas.draw_text(
                        layout,
                        Point::new(at.x + point.x, at.y + point.y),
                        paint_color(theme, *paint),
                        None,
                    ),
                }
            }
            for (p, piece) in laid.pieces.iter().enumerate() {
                let piece_at = Point::new(at.x + piece.origin.x, at.y + piece.origin.y);
                for rect in &piece.code_rects {
                    canvas.fill_rounded_rect(offset(*rect, at), 4.0, theme.code_fill);
                }
                if let Some((start, end)) = selection {
                    let key = (b, p);
                    if key >= (start.block, start.piece) && key <= (end.block, end.piece) {
                        let from = if key == (start.block, start.piece) {
                            start.index
                        } else {
                            0
                        };
                        let to = if key == (end.block, end.piece) {
                            end.index
                        } else {
                            markdown::utf16_len(&piece.text)
                        };
                        if to > from {
                            for rect in canvas.text().range_rects(&piece.layout, from, to - from) {
                                canvas.fill_rounded_rect(
                                    offset(rect, piece_at),
                                    2.0,
                                    theme.selection,
                                );
                            }
                        }
                    }
                }
                let color = if laid.block.quote > 0 {
                    theme.muted
                } else {
                    theme.text
                };
                canvas.draw_text_tinted(&piece.layout, piece_at, color, &piece.tints);
            }
        }
    }
}

fn paint_color(theme: &RichTheme, paint: Paint) -> Color {
    match paint {
        Paint::Muted => theme.muted,
        Paint::CodeFill => theme.code_fill,
        Paint::Line => theme.line,
        Paint::Quote => theme.quote_bar,
        Paint::Header => theme.header_fill,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Draw, FakeText, RecordingCanvas};
    use std::rc::Rc;

    const THEME: RichTheme = RichTheme {
        size: 10.0,
        line_height: 20.0,
        text: Color::new(1.0, 1.0, 1.0, 1.0),
        muted: Color::new(0.6, 0.6, 0.6, 1.0),
        link: Color::new(0.4, 0.6, 1.0, 1.0),
        code_fill: Color::new(0.1, 0.1, 0.1, 1.0),
        line: Color::new(1.0, 1.0, 1.0, 0.1),
        quote_bar: Color::new(0.2, 0.4, 0.9, 1.0),
        header_fill: Color::new(0.2, 0.2, 0.2, 1.0),
        selection: Color::new(0.2, 0.4, 0.9, 0.3),
    };

    fn laid(markdown: &str, width: f32) -> RichText<Rc<crate::testing::FakeLayout>> {
        let mut rich = RichText::default();
        rich.set_markdown(markdown);
        rich.layout(&FakeText, width, &THEME);
        rich
    }

    #[test]
    fn blocks_stack_with_gaps_and_wrap_to_width() {
        // 字宽 6：60 宽一行 10 字。
        let rich = laid("aaaaaaaaaaaaaaa\n\nbb", 60.0);
        assert_eq!(rich.laid[0].height, 40.0);
        assert_eq!(rich.laid[1].top, 50.0);
        assert_eq!(rich.height(), 70.0);
    }

    #[test]
    fn unchanged_blocks_keep_their_layout_while_streaming() {
        let mut rich = laid("第一段\n\n第二", 200.0);
        let first = Rc::as_ptr(&rich.laid[0].pieces[0].layout);
        rich.set_markdown("第一段\n\n第二段继续");
        rich.layout(&FakeText, 200.0, &THEME);
        assert_eq!(Rc::as_ptr(&rich.laid[0].pieces[0].layout), first);
        assert_eq!(rich.laid[1].pieces[0].text, "第二段继续");
    }

    #[test]
    fn drag_selection_spans_blocks_and_copies_with_line_breaks() {
        let mut rich = laid("abcdef\n\nghijkl", 200.0);
        rich.press(&FakeText, Point::new(12.0, 5.0), 1);
        rich.drag(&FakeText, Point::new(18.0, 35.0));
        assert_eq!(rich.selected_text().as_deref(), Some("cdef\nghi"));
        assert_eq!(rich.release(&FakeText, Point::new(18.0, 35.0)), None);
    }

    #[test]
    fn double_and_triple_click_select_word_and_block() {
        let mut rich = laid("hello world 你好世界", 400.0);
        rich.press(&FakeText, Point::new(40.0, 5.0), 2);
        assert_eq!(rich.selected_text().as_deref(), Some("world"));
        rich.press(&FakeText, Point::new(80.0, 5.0), 2);
        assert_eq!(rich.selected_text().as_deref(), Some("你好世界"));
        rich.press(&FakeText, Point::new(40.0, 5.0), 3);
        assert_eq!(
            rich.selected_text().as_deref(),
            Some("hello world 你好世界")
        );
    }

    #[test]
    fn clicking_a_link_without_dragging_returns_its_url() {
        let mut rich = laid("见[文档](https://a.b)吧", 400.0);
        rich.press(&FakeText, Point::new(9.0, 5.0), 1);
        assert_eq!(
            rich.release(&FakeText, Point::new(9.0, 5.0)).as_deref(),
            Some("https://a.b")
        );
        rich.press(&FakeText, Point::new(1.0, 5.0), 1);
        assert_eq!(rich.release(&FakeText, Point::new(1.0, 5.0)), None);
    }

    #[test]
    fn table_cells_copy_with_tabs_and_rows_with_line_breaks() {
        let mut rich = laid("| a | b |\n|---|---|\n| c | d |", 400.0);
        rich.select_all();
        assert_eq!(rich.selected_text().as_deref(), Some("a\tb\nc\td"));
    }

    #[test]
    fn list_markers_and_links_are_drawn_in_their_colors() {
        let rich = laid("- 见[文档](https://a.b)", 400.0);
        let mut canvas = RecordingCanvas::new(96);
        rich.draw(&mut canvas, Point::new(10.0, 20.0), (0.0, 100.0), &THEME);
        assert!(canvas.calls.iter().any(
            |c| matches!(c, Draw::Text(t, _, color, None) if t == "•" && *color == THEME.muted)
        ));
        assert!(canvas.calls.iter().any(|c| matches!(
            c,
            Draw::Tinted(t, p, _, tints) if t == "见文档" && *p == Point::new(32.0, 20.0)
                && tints[0].color == THEME.link && tints[0].utf16_start == 1
        )));
    }
}

//! Markdown 解析成按块排列的文档：段落、标题、列表项、引用、代码块、分隔线、表格。
//!
//! 只描述「有什么」，不涉及字体、颜色和尺寸，排版与绘制在 `ui::rich_text`。嵌套结构
//! 压平成一列块：每块记下列表缩进层数和引用层数，排版时据此留出缩进、画引用竖线。
//! 段内的单个换行照样保留：翻译、改写的结果要照原文分行。
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// 一段文字里某个区间的行内样式；区间为 UTF-16。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub utf16_start: u32,
    pub utf16_len: u32,
    pub style: InlineStyle,
    /// 链接地址；None = 不是链接。
    pub link: Option<String>,
}

/// 带行内样式的一段文字；只登记非默认样式的区间，相邻同样式区间合并。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<Span>,
}

impl Inline {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.into(),
            spans: Vec::new(),
        }
    }
    fn push(&mut self, text: &str, style: InlineStyle, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let start = utf16_len(&self.text);
        let len = utf16_len(text);
        self.text.push_str(text);
        if style == InlineStyle::default() && link.is_none() {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if last.utf16_start + last.utf16_len == start
                && last.style == style
                && last.link.as_deref() == link
            {
                last.utf16_len += len;
                return;
            }
        }
        self.spans.push(Span {
            utf16_start: start,
            utf16_len: len,
            style,
            link: link.map(str::to_owned),
        });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// 段落、标题或列表项里的一段文字。heading 为 1–6；marker 只出现在列表项的第一段，
    /// 是要画在缩进左侧的项目符号（「•」「1.」「☐」「☑」）。
    Text {
        inline: Inline,
        heading: Option<u8>,
        marker: Option<String>,
    },
    Code {
        text: String,
        lang: String,
    },
    Rule,
    Table {
        header: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// 所在列表的嵌套层数（0 = 不在列表里）。
    pub indent: u8,
    /// 所在引用的嵌套层数。
    pub quote: u8,
}

pub fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

struct ListLevel {
    /// 有序列表的下一个序号；None = 无序列表。
    next: Option<u64>,
}

#[derive(Default)]
struct TableBuilder {
    header: Vec<Inline>,
    rows: Vec<Vec<Inline>>,
    in_head: bool,
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    current: Option<Inline>,
    heading: Option<u8>,
    marker: Option<String>,
    lists: Vec<ListLevel>,
    quote: u8,
    bold: u8,
    italic: u8,
    strike: u8,
    link: Option<String>,
    code: Option<(String, String)>,
    table: Option<TableBuilder>,
}

impl Builder {
    fn style(&self, code: bool) -> InlineStyle {
        InlineStyle {
            bold: self.bold > 0 || self.heading.is_some(),
            italic: self.italic > 0,
            code,
            strike: self.strike > 0,
        }
    }
    fn indent(&self) -> u8 {
        self.lists.len().min(u8::MAX as usize) as u8
    }
    fn text(&mut self, text: &str, code: bool) {
        let style = self.style(code);
        let link = self.link.clone();
        if let Some((_, buffer)) = &mut self.code {
            buffer.push_str(text);
            return;
        }
        if let Some(table) = &mut self.table {
            let row = if table.in_head {
                &mut table.header
            } else {
                match table.rows.last_mut() {
                    Some(row) => row,
                    None => return,
                }
            };
            if let Some(cell) = row.last_mut() {
                cell.push(text, style, link.as_deref());
            }
            return;
        }
        self.current
            .get_or_insert_with(Inline::default)
            .push(text, style, link.as_deref());
    }
    fn flush(&mut self) {
        let inline = self.current.take();
        if inline.is_none() && self.marker.is_none() {
            return;
        }
        let mut inline = inline.unwrap_or_default();
        // 段尾的换行不占一行；段内换行要保留。
        let trimmed = inline.text.trim_end_matches('\n').len();
        inline.text.truncate(trimmed);
        let len = utf16_len(&inline.text);
        inline.spans.retain_mut(|span| {
            span.utf16_len = span.utf16_len.min(len.saturating_sub(span.utf16_start));
            span.utf16_len > 0
        });
        self.blocks.push(Block {
            kind: BlockKind::Text {
                inline,
                heading: self.heading,
                marker: self.marker.take(),
            },
            indent: self.indent(),
            quote: self.quote,
        });
    }
    fn push(&mut self, kind: BlockKind) {
        self.blocks.push(Block {
            kind,
            indent: self.indent(),
            quote: self.quote,
        });
    }
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                self.text(&text, false)
            }
            Event::Code(text) => self.text(&text, true),
            Event::InlineMath(text) | Event::DisplayMath(text) => self.text(&text, true),
            Event::SoftBreak | Event::HardBreak => self.text("\n", false),
            Event::Rule => {
                self.flush();
                self.push(BlockKind::Rule);
            }
            Event::TaskListMarker(checked) => {
                self.marker = Some(if checked { "☑" } else { "☐" }.into());
            }
            Event::FootnoteReference(label) => self.text(&format!("[{label}]"), false),
        }
    }
    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level as u8);
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.quote = self.quote.saturating_add(1);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(ListLevel { next: start });
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(ListLevel { next: Some(n) }) => {
                        let marker = format!("{n}.");
                        *n += 1;
                        marker
                    }
                    _ => "•".into(),
                };
                self.marker = Some(marker);
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(TableBuilder::default());
            }
            Tag::TableHead => {
                if let Some(table) = &mut self.table {
                    table.in_head = true;
                }
            }
            Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.rows.push(Vec::new());
                }
            }
            Tag::TableCell => {
                if let Some(table) = &mut self.table {
                    let row = if table.in_head {
                        &mut table.header
                    } else {
                        match table.rows.last_mut() {
                            Some(row) => row,
                            None => return,
                        }
                    };
                    row.push(Inline::default());
                }
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.link = Some(dest_url.to_string())
            }
            _ => {}
        }
    }
    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some((lang, mut text)) = self.code.take() {
                    let trimmed = text.trim_end_matches('\n').len();
                    text.truncate(trimmed);
                    self.push(BlockKind::Code { text, lang });
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    table.in_head = false;
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.push(BlockKind::Table {
                        header: table.header,
                        rows: table.rows,
                    });
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link | TagEnd::Image => self.link = None,
            _ => {}
        }
    }
}

/// 解析整段 Markdown。流式回答每次增量都整段重解析：回答只有几千字，解析是微秒级，
/// 排版缓存按块内容复用（见 `ui::rich_text`）。
pub fn parse(text: &str) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut builder = Builder::default();
    for event in Parser::new_ext(text, options) {
        builder.event(event);
    }
    builder.flush();
    builder.blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_block(block: &Block) -> (&str, Option<u8>, Option<&str>) {
        match &block.kind {
            BlockKind::Text {
                inline,
                heading,
                marker,
            } => (inline.text.as_str(), *heading, marker.as_deref()),
            other => panic!("expected text block, got {other:?}"),
        }
    }

    #[test]
    fn paragraphs_keep_single_line_breaks() {
        let blocks = parse("使用边界框作为放置位置。\n这个周末试试看。\n\n第二段");
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            text_block(&blocks[0]).0,
            "使用边界框作为放置位置。\n这个周末试试看。"
        );
        assert_eq!(text_block(&blocks[1]).0, "第二段");
    }

    #[test]
    fn inline_styles_become_merged_utf16_spans() {
        let blocks = parse("这是**投机**解码，参数 `draft = 4`，见[文档](https://a.b)");
        let BlockKind::Text { inline, .. } = &blocks[0].kind else {
            panic!()
        };
        assert_eq!(inline.text, "这是投机解码，参数 draft = 4，见文档");
        assert_eq!(
            inline.spans,
            vec![
                Span {
                    utf16_start: 2,
                    utf16_len: 2,
                    style: InlineStyle {
                        bold: true,
                        ..Default::default()
                    },
                    link: None
                },
                Span {
                    utf16_start: 10,
                    utf16_len: 9,
                    style: InlineStyle {
                        code: true,
                        ..Default::default()
                    },
                    link: None
                },
                Span {
                    utf16_start: 21,
                    utf16_len: 2,
                    style: InlineStyle::default(),
                    link: Some("https://a.b".into())
                },
            ]
        );
    }

    #[test]
    fn nested_lists_flatten_with_markers_and_indent() {
        let blocks = parse("1. 第一\n   - 子项\n2. 第二\n\n- [x] 完成\n- [ ] 待办");
        let summary: Vec<_> = blocks
            .iter()
            .map(|b| (text_block(b).0, text_block(b).2, b.indent))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("第一", Some("1."), 1),
                ("子项", Some("•"), 2),
                ("第二", Some("2."), 1),
                ("完成", Some("☑"), 1),
                ("待办", Some("☐"), 1),
            ]
        );
    }

    #[test]
    fn headings_quotes_code_rules_and_tables() {
        let blocks = parse(
            "## 标题\n\n> 引用\n\n```rust\nlet a = 1;\n```\n\n---\n\n| 名称 | 值 |\n|---|---|\n| a | **1** |",
        );
        assert_eq!(text_block(&blocks[0]), ("标题", Some(2), None));
        assert_eq!((text_block(&blocks[1]).0, blocks[1].quote), ("引用", 1));
        assert_eq!(
            blocks[2].kind,
            BlockKind::Code {
                text: "let a = 1;".into(),
                lang: "rust".into()
            }
        );
        assert_eq!(blocks[3].kind, BlockKind::Rule);
        let BlockKind::Table { header, rows } = &blocks[4].kind else {
            panic!()
        };
        assert_eq!(
            header.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
            ["名称", "值"]
        );
        assert_eq!(rows[0][1].text, "1");
        assert!(rows[0][1].spans[0].style.bold);
    }

    #[test]
    fn unfinished_stream_still_renders_what_has_arrived() {
        let blocks = parse("开头\n\n```py\nprint(1)");
        assert_eq!(text_block(&blocks[0]).0, "开头");
        assert_eq!(
            blocks[1].kind,
            BlockKind::Code {
                text: "print(1)".into(),
                lang: "py".into()
            }
        );
    }
}

//! 事件流 → 可见片段 + 隐藏区间。
//!
//! 核心是差集：`隐藏 = 块区间 − 所有可见片段区间`。
//!
//! M0 探针（`.workbuddy/lab/inline-probe`）证实了三个必须按此处理的事实：
//!
//! 1. pulldown-cmark 的 `End` 事件区间覆盖**整个容器**而非闭合定界符，
//!    所以闭合标记无法从 End 事件直接取得，只能靠差集。
//! 2. `Event::Code` 的区间**包含反引号**，必须按定界符长度内缩，
//!    否则 `` `x` `` 差集为空、反引号不会被隐藏。
//! 3. `SoftBreak` / `HardBreak` 若一并隐藏，段落内换行会消失、排版塌成一行。

use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::block::{
    Block, BlockId, BlockKind, ColumnAlign, ListInfo, ListItem, MarkerPair, OutlineItem,
    ParseResult, Span, SpanKind, Style, TableCell, TableGrid, TableRow,
};
use crate::highlight::highlight;
use crate::split::split_blocks;

/// Markdown 解析入口。所有区间均为 byte offset（与 `into_offset_iter` 一致）。
#[derive(Debug, Clone)]
pub struct MarkdownParser {
    options: Options,
}

impl MarkdownParser {
    pub fn new(options: Options) -> Self {
        Self { options }
    }

    /// CommonMark + 表格 / 脚注 / 删除线 / 任务列表 / 数学公式。
    /// 不启用 RAW_HTML：原始 HTML 退化为普通文本，杜绝脚本注入。
    pub fn gfm() -> Self {
        Self::new(
            Options::ENABLE_TABLES
                | Options::ENABLE_FOOTNOTES
                | Options::ENABLE_STRIKETHROUGH
                | Options::ENABLE_TASKLISTS
                | Options::ENABLE_MATH,
        )
    }

    pub fn options(&self) -> Options {
        self.options
    }

    pub fn parse(&self, src: &str) -> ParseResult {
        parse(src, self.options)
    }
}

impl Default for MarkdownParser {
    fn default() -> Self {
        Self::gfm()
    }
}

#[derive(Default)]
struct Acc {
    spans: Vec<Span>,
    covered: Vec<Range<usize>>,
    /// 需要成对删除的容器区间（Strong / Emphasis / Strikethrough / Link）
    containers: Vec<Range<usize>>,
    kind: Option<BlockKind>,
    /// 表格构建状态。 pulldown 的表格事件不直接给出「内容区间」，
    /// 必须在单元格结束时用该格累积的可见片段反推。
    table: Option<TableBuild>,
    /// 该块的列表项（按出现顺序）。Tag::Item 事件给出准确嵌套深度。
    list_items: Vec<ListItem>,
}

#[derive(Default)]
struct TableBuild {
    alignment: Vec<ColumnAlign>,
    rows: Vec<BuildRow>,
}

struct BuildRow {
    head: bool,
    cells: Vec<BuildCell>,
}

struct BuildCell {
    /// 单元格竖线之间的源码区间（含填充空格）
    byte: Range<usize>,
    /// 进入该格时 spans 的长度，用来切出属于它的可见片段
    span_start: usize,
}

fn column_align(a: Alignment) -> ColumnAlign {
    match a {
        Alignment::Left => ColumnAlign::Left,
        Alignment::Center => ColumnAlign::Center,
        Alignment::Right => ColumnAlign::Right,
        _ => ColumnAlign::None,
    }
}

/// 把事件流累积出的表格状态定型。每格的 `content` 取该格可见片段的并集，
/// 空单元格留一个空区间，由 `TableGrid::caret_byte` 按源码填充处理。
fn finish_table(build: TableBuild, spans: &[Span]) -> TableGrid {
    let rows = build
        .rows
        .into_iter()
        .map(|row| {
            let cells = row
                .cells
                .into_iter()
                .map(|cell| {
                    let owned: Vec<&Span> = spans[cell.span_start..]
                        .iter()
                        .take_while(|s| s.byte.start < cell.byte.end)
                        .collect();
                    let content = match (owned.first(), owned.last()) {
                        (Some(first), Some(last)) => first.byte.start..last.byte.end,
                        _ => cell.byte.start..cell.byte.start,
                    };
                    TableCell {
                        byte: cell.byte,
                        content,
                    }
                })
                .collect();
            TableRow {
                head: row.head,
                cells,
            }
        })
        .collect();
    TableGrid {
        alignment: build.alignment,
        rows,
    }
}

/// 把容器区间翻译成「标记对」。
///
/// `hidden` 是差集结果，容器首尾各自落下一段折叠区间；两头一配就是一对。
/// 行内代码没有容器事件（`Event::Code` 是单点事件），只能靠片段边缘反推。
fn collect_markers(
    hidden: &[Range<usize>],
    containers: &[Range<usize>],
    spans: &[Span],
) -> Vec<MarkerPair> {
    let mut pairs: Vec<MarkerPair> = Vec::new();
    let mut remember = |open: &Range<usize>, close: &Range<usize>| {
        if open.end > close.start {
            return;
        }
        // 嵌套同形标记（`***a***`）会产生重复的容器区间
        if pairs
            .iter()
            .any(|p| p.open.start == open.start && p.close.end == close.end)
        {
            return;
        }
        pairs.push(MarkerPair {
            open: open.clone(),
            close: close.clone(),
        });
    };

    for container in containers {
        let open = hidden
            .iter()
            .find(|h| h.start == container.start && h.end <= container.end);
        let close = hidden
            .iter()
            .rev()
            .find(|h| h.end == container.end && h.start >= container.start);
        if let (Some(open), Some(close)) = (open, close) {
            remember(open, close);
        }
    }

    // 行内代码与数学公式都没有容器事件（都是单点事件），只能靠片段边缘反推；
    // 配对后，退格落在 `$` 上会整对抽掉定界符、留下公式源码。
    for span in spans
        .iter()
        .filter(|s| matches!(s.kind, SpanKind::Code | SpanKind::Math { .. }))
    {
        let open = hidden.iter().find(|h| h.end == span.byte.start);
        let close = hidden.iter().find(|h| h.start == span.byte.end);
        if let (Some(open), Some(close)) = (open, close) {
            remember(open, close);
        }
    }

    pairs.sort_by_key(|p| p.open.start);
    pairs
}

pub fn parse(src: &str, options: Options) -> ParseResult {
    let ranges = split_blocks(src);
    let mut accs: Vec<Acc> = (0..ranges.len()).map(|_| Acc::default()).collect();

    let mut style = Style::default();
    let mut stack: Vec<Style> = Vec::new();
    // 当前打开的 List 容器（true = 有序）。松散列表会被空行切成多块，
    // 后半段块的首个事件是 `Item`，只能靠这个栈知道自己仍在列表里、且是有序还是无序。
    let mut list_stack: Vec<bool> = Vec::new();
    // (起始 byte, alt, url, 所属块)
    let mut image: Option<(usize, String, String, usize)> = None;

    for (event, range) in Parser::new_ext(src, options).into_offset_iter() {
        // 图片整体原子化：内部事件不外泄，避免 alt 文本与 url 被当成可见内容。
        if image.is_some() {
            match event {
                Event::Text(t) => {
                    if let Some(entry) = image.as_mut() {
                        entry.1.push_str(&t);
                    }
                }
                Event::End(TagEnd::Image) => {
                    if let Some((start, alt, url, owner)) = image.take() {
                        let mut span_style = style.clone();
                        span_style.link = Some(url);
                        accs[owner].spans.push(Span {
                            kind: SpanKind::Image,
                            byte: start..range.end,
                            text: alt,
                            style: span_style,
                        });
                        accs[owner].covered.push(start..range.end);
                        style = stack.pop().unwrap_or_default();
                    }
                }
                _ => {}
            }
            continue;
        }

        match event {
            Event::Start(tag) => {
                // 一个列表容器会被空行切成多块：松散的列表项之间本来就隔着空行。
                // 后半段块的首个事件是 `Item` 而不是 `List`，走 `classify` 会退化成段落，
                // 序号与项目符号一起消失（用户看到的是「列表变成了普通文字」）。
                // 这里按「当前打开的 List 容器」判类型，只对首事件是 `Item` 的块生效。
                let from_item =
                    matches!(tag, Tag::Item).then(|| list_stack.last().copied().unwrap_or(false));
                if let Some(owner) = owner_of(&ranges, range.start)
                    && accs[owner].kind.is_none()
                {
                    accs[owner].kind = Some(match from_item {
                        Some(ordered) => BlockKind::List { ordered },
                        None => classify(&tag),
                    });
                }
                if let Tag::List(start) = &tag {
                    list_stack.push(start.is_some());
                }
                // 记录该项的嵌套深度。此刻 list_stack 已含它所属的所有 List：
                // 外层项 len=1 → depth 0，二级项 len=2 → depth 1，依此类推。
                // Item 事件的 range.start 是该项行首的标记 byte，渲染层据此把
                // 每个 Break 映射到「它后面的那一项」，从而还原层级。
                if matches!(tag, Tag::Item)
                    && let Some(owner) = owner_of(&ranges, range.start)
                {
                    // 分隔这一项与上一项的换行：从行首往回找最近的换行，且必须
                    // 落在本块内。块首第一项找不到 → None。项内的 Shift+Enter
                    // 不产生 Item 事件，因此绝不会成为任何项的 lead_break。
                    let block = &ranges[owner];
                    let lead_break = src
                        .get(..range.start)
                        .and_then(|head| head.rfind('\n'))
                        .filter(|&nl| nl >= block.start && nl < block.end);
                    accs[owner].list_items.push(ListItem {
                        byte: range.start,
                        depth: list_stack.len().saturating_sub(1),
                        ordered: list_stack.last().copied().unwrap_or(false),
                        lead_break,
                    });
                }
                // 表格：pulldown 的事件流里表格是一串 Table/TableHead/TableRow/TableCell
                // 容器事件，单元格区间直接取事件 range（竖线之间的原文，含填充空格）。
                if let Some(owner) = owner_of(&ranges, range.start) {
                    let span_start = accs[owner].spans.len();
                    match &tag {
                        Tag::Table(alignments) => {
                            accs[owner].table = Some(TableBuild {
                                alignment: alignments.iter().copied().map(column_align).collect(),
                                rows: Vec::new(),
                            });
                        }
                        Tag::TableHead => {
                            if let Some(t) = accs[owner].table.as_mut() {
                                t.rows.push(BuildRow {
                                    head: true,
                                    cells: Vec::new(),
                                });
                            }
                        }
                        Tag::TableRow => {
                            if let Some(t) = accs[owner].table.as_mut() {
                                t.rows.push(BuildRow {
                                    head: false,
                                    cells: Vec::new(),
                                });
                            }
                        }
                        Tag::TableCell => {
                            if let Some(t) = accs[owner].table.as_mut()
                                && let Some(row) = t.rows.last_mut()
                            {
                                row.cells.push(BuildCell {
                                    byte: range.clone(),
                                    span_start,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                // 列表项与表格行之间的换行必须显式保留：切块按空行进行，
                // 这些换行落在块内、又不在任何片段里，会被差集当成标记折叠掉，
                // 结果整段列表塌成一行。这里补一个硬换行片段占住它。
                if matches!(tag, Tag::Item | Tag::TableRow | Tag::TableHead) {
                    close_previous_line(src, &ranges, &mut accs, range.start);
                }
                stack.push(style.clone());
                match &tag {
                    Tag::Strong => style.strong = true,
                    Tag::Emphasis => style.emphasis = true,
                    Tag::Strikethrough => style.strikethrough = true,
                    Tag::Heading { level, .. } => style.heading = Some(level_num(*level)),
                    Tag::Link { dest_url, .. } => style.link = Some(dest_url.to_string()),
                    Tag::Image { dest_url, .. } => {
                        let owner = owner_of(&ranges, range.start).unwrap_or(0);
                        image = Some((range.start, String::new(), dest_url.to_string(), owner));
                    }
                    _ => {}
                }
            }
            Event::End(tag) => {
                // End 的区间覆盖整个容器（硬约束 4），正好拿来给首尾标记配对
                if matches!(
                    tag,
                    TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link
                ) && let Some(owner) = owner_of(&ranges, range.start)
                {
                    accs[owner].containers.push(range.clone());
                }
                if matches!(tag, TagEnd::List(_)) {
                    list_stack.pop();
                }
                style = stack.pop().unwrap_or_default();
            }
            Event::Text(text) => {
                if let Some(owner) = owner_of(&ranges, range.start) {
                    if !text.is_empty() {
                        // 处于 Link 容器内时标记为 Link 片段，渲染层需要拿到 url 才能绑定点击
                        let kind = if style.link.is_some() {
                            SpanKind::Link
                        } else {
                            SpanKind::Text
                        };
                        accs[owner].spans.push(Span {
                            kind,
                            byte: range.clone(),
                            text: text.to_string(),
                            style: style.clone(),
                        });
                    }
                    accs[owner].covered.push(range);
                }
            }
            Event::Code(text) => {
                if let Some(owner) = owner_of(&ranges, range.start) {
                    let inner = code_inner(src, &range);
                    let mut span_style = style.clone();
                    span_style.code = true;
                    accs[owner].spans.push(Span {
                        kind: SpanKind::Code,
                        byte: inner.clone(),
                        text: text.to_string(),
                        style: span_style,
                    });
                    accs[owner].covered.push(inner);
                }
            }
            // 原始 HTML 降级为文本：pulldown 不会执行它，此处也只当纯文本投影。
            Event::Html(raw) | Event::InlineHtml(raw) => {
                if let Some(owner) = owner_of(&ranges, range.start) {
                    if !raw.is_empty() {
                        accs[owner].spans.push(Span {
                            kind: SpanKind::Text,
                            byte: range.clone(),
                            text: raw.to_string(),
                            style: style.clone(),
                        });
                    }
                    accs[owner].covered.push(range);
                }
            }
            other @ (Event::SoftBreak | Event::HardBreak) => {
                if let Some(owner) = owner_of(&ranges, range.start) {
                    let hard = matches!(other, Event::HardBreak);
                    accs[owner].spans.push(Span {
                        kind: SpanKind::Break { hard },
                        byte: range.clone(),
                        text: "\n".to_owned(),
                        style: style.clone(),
                    });
                    accs[owner].covered.push(range);
                }
            }
            Event::Rule => {
                if let Some(owner) = owner_of(&ranges, range.start) {
                    accs[owner].spans.push(Span {
                        kind: SpanKind::Rule,
                        byte: range.clone(),
                        text: String::new(),
                        style: style.clone(),
                    });
                    accs[owner].covered.push(range);
                }
            }
            Event::TaskListMarker(checked) => {
                if let Some(owner) = owner_of(&ranges, range.start) {
                    accs[owner].spans.push(Span {
                        kind: SpanKind::TaskMarker { checked },
                        byte: range.clone(),
                        text: String::new(),
                        style: style.clone(),
                    });
                    accs[owner].covered.push(range);
                }
            }
            Event::FootnoteReference(label) => {
                if let Some(owner) = owner_of(&ranges, range.start) {
                    accs[owner].spans.push(Span {
                        kind: SpanKind::FootnoteReference,
                        byte: range.clone(),
                        text: format!("[^{label}]"),
                        style: style.clone(),
                    });
                    accs[owner].covered.push(range);
                }
            }
            Event::InlineMath(text) => {
                push_math(src, &ranges, &mut accs, range, false, &text, &style)
            }
            Event::DisplayMath(text) => {
                push_math(src, &ranges, &mut accs, range, true, &text, &style)
            }
        }
    }

    let mut blocks = Vec::with_capacity(ranges.len());
    let mut outline = Vec::new();

    for (index, acc) in accs.into_iter().enumerate() {
        let Acc {
            spans,
            covered,
            containers,
            kind,
            table,
            list_items,
        } = acc;
        let byte = ranges[index].clone();
        let kind = kind.unwrap_or_else(|| {
            if spans.first().is_some_and(|s| s.kind == SpanKind::Rule) {
                BlockKind::Rule
            } else {
                BlockKind::Paragraph
            }
        });
        let hidden = subtract(byte.clone(), &merge(covered));
        let markers = collect_markers(&hidden, &containers, &spans);
        let table = table.map(|build| finish_table(build, &spans));
        let list = matches!(kind, BlockKind::List { .. })
            .then(|| ListInfo { items: list_items })
            .filter(|info| !info.items.is_empty());
        // 代码块在投影完成后做一次词法着色。区间取可见片段的并集，围栏本身
        // 落在 `hidden` 里，不会被当成代码。
        let tokens = match &kind {
            BlockKind::CodeBlock { info } => {
                let content = spans
                    .iter()
                    .filter(|s| s.byte.start < s.byte.end)
                    .map(|s| s.byte.clone())
                    .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end));
                content
                    .map(|range| highlight(info.as_deref(), src, range))
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let block = Block {
            id: BlockId(index),
            kind,
            byte,
            spans,
            hidden,
            markers,
            table,
            list,
            tokens,
        };
        let block = if heading_is_bare(src, &block) {
            demote_to_paragraph(src, block)
        } else {
            block
        };
        if let BlockKind::Heading(level) = &block.kind {
            outline.push(OutlineItem {
                level: *level,
                title: block.plain_text(),
                byte_offset: block.byte.start,
                block_id: block.id,
            });
        }
        blocks.push(block);
    }

    ParseResult { blocks, outline }
}

/// 标题是否「还没成形」：`#` 之后既没有空格、也没有内容。
///
/// pulldown 按 CommonMark 把行尾的裸 `#` 也当成合法的 ATX 标题（内容为空），
/// 于是逐字输入时会出现两次跳变：敲 `#` 整行先变成一级标题（`#` 被折叠、字号
/// 变大），接着输入文字又掉回段落（`#` 重新可见、字号缩回）。
///
/// 这里把「只有 `#`、后面没有空格」的块降级成段落：标记始终可见，用户敲下空格
/// 的那一刻才升格为标题。既消灭了抖动，也与「ATX 标题必须有空格」一致。
fn heading_is_bare(src: &str, block: &Block) -> bool {
    if !matches!(block.kind, BlockKind::Heading(_)) {
        return false;
    }
    if !block.plain_text().is_empty() {
        return false;
    }
    // 内容为空时，`#` 之后只可能什么都没有（`#a` 本来就不是标题）
    let raw = src.get(block.byte.clone()).unwrap_or("");
    !raw.trim_start()
        .trim_start_matches('#')
        .chars()
        .next()
        .is_some_and(|c| c == ' ' || c == '\t')
}

/// 把未成形的标题还原成一段普通文本：`#` 原样显示、不再折叠、不进大纲。
fn demote_to_paragraph(src: &str, block: Block) -> Block {
    let mut spans = Vec::new();
    if let Some(text) = src.get(block.byte.clone()).filter(|t| !t.is_empty()) {
        spans.push(Span {
            kind: SpanKind::Text,
            byte: block.byte.clone(),
            text: text.to_owned(),
            style: Style::default(),
        });
    }
    Block {
        id: block.id,
        kind: BlockKind::Paragraph,
        byte: block.byte,
        spans,
        hidden: Vec::new(),
        markers: Vec::new(),
        table: None,
        list: None,
        tokens: Vec::new(),
    }
}

/// 在 `start` 之前找最近的换行，把它登记为可见的硬换行。
///
/// 只处理块内、尚未被任何片段覆盖的换行；块首或跨块的情况直接跳过。
fn close_previous_line(src: &str, ranges: &[Range<usize>], accs: &mut [Acc], start: usize) {
    let Some(owner) = owner_of(ranges, start) else {
        return;
    };
    let Some(nl) = src.get(..start).and_then(|head| head.rfind('\n')) else {
        return;
    };
    let block = &ranges[owner];
    if nl < block.start || nl + 1 > block.end {
        return;
    }
    if accs[owner]
        .covered
        .iter()
        .any(|r| r.start <= nl && nl < r.end)
    {
        return;
    }
    accs[owner].spans.push(Span {
        kind: SpanKind::Break { hard: true },
        byte: nl..nl + 1,
        text: "\n".to_owned(),
        style: Style::default(),
    });
    accs[owner].covered.push(nl..nl + 1);
}

fn code_inner(src: &str, range: &Range<usize>) -> Range<usize> {
    let delims = src
        .get(range.clone())
        .map(|s| s.bytes().take_while(|b| *b == b'`').count())
        .unwrap_or(0);
    if delims > 0 && range.len() >= delims * 2 {
        (range.start + delims)..(range.end - delims)
    } else {
        range.clone()
    }
}

/// 去掉数学事件的 `$` / `$$` 定界符，只留公式源码。
///
/// 与 `code_inner` 同理：pulldown 的事件区间含定界符，不内缩的话 `$` 会被当成
/// 可见文字、差集里反而没有折叠标记。这里只砍固定长度的定界符，不数连续 `$`，
/// 这样 `$$ $x$ $$` 这类内容本身就以 `$` 开头/结尾的公式也能正确内缩。
fn math_inner(src: &str, range: &Range<usize>, display: bool) -> Range<usize> {
    let want = if display { 2 } else { 1 };
    let Some(raw) = src.get(range.clone()) else {
        return range.clone();
    };
    let bytes = raw.as_bytes();
    let has_open = bytes.iter().take(want).all(|b| *b == b'$') && bytes.len() >= want;
    let has_close = bytes.iter().rev().take(want).all(|b| *b == b'$') && bytes.len() >= want;
    if has_open && has_close && bytes.len() >= want * 2 {
        (range.start + want)..(range.end - want)
    } else {
        range.clone()
    }
}

/// 把一个数学事件落成可见片段 + 折叠定界符。
fn push_math(
    src: &str,
    ranges: &[Range<usize>],
    accs: &mut [Acc],
    range: Range<usize>,
    display: bool,
    text: &str,
    style: &Style,
) {
    if let Some(owner) = owner_of(ranges, range.start) {
        let inner = math_inner(src, &range, display);
        accs[owner].spans.push(Span {
            kind: SpanKind::Math { display },
            byte: inner.clone(),
            text: text.to_owned(),
            style: style.clone(),
        });
        accs[owner].covered.push(inner);
    }
}

fn level_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn classify(tag: &Tag<'_>) -> BlockKind {
    match tag {
        Tag::Heading { level, .. } => BlockKind::Heading(level_num(*level)),
        Tag::Paragraph => BlockKind::Paragraph,
        Tag::BlockQuote(_) => BlockKind::BlockQuote,
        Tag::List(start) => BlockKind::List {
            ordered: start.is_some(),
        },
        Tag::CodeBlock(kind) => BlockKind::CodeBlock {
            info: match kind {
                CodeBlockKind::Fenced(info) => {
                    let trimmed = info.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                }
                CodeBlockKind::Indented => None,
            },
        },
        Tag::Table(_) => BlockKind::Table,
        Tag::HtmlBlock => BlockKind::Html,
        _ => BlockKind::Paragraph,
    }
}

fn owner_of(ranges: &[Range<usize>], byte: usize) -> Option<usize> {
    let index = ranges.partition_point(|r| r.end <= byte);
    ranges.get(index).filter(|r| r.start <= byte).map(|_| index)
}

pub(crate) fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut out: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match out.last_mut() {
            Some(last) if range.start <= last.end => {
                if range.end > last.end {
                    last.end = range.end;
                }
            }
            _ => out.push(range),
        }
    }
    out
}

pub(crate) fn subtract(total: Range<usize>, cuts: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut cursor = total.start;
    for cut in cuts {
        let start = cut.start.max(total.start);
        let end = cut.end.min(total.end);
        if start >= end {
            continue;
        }
        if start > cursor {
            out.push(cursor..start);
        }
        if end > cursor {
            cursor = end;
        }
        if cursor >= total.end {
            break;
        }
    }
    if cursor < total.end {
        out.push(cursor..total.end);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::item_marker;

    fn hidden_text(src: &str) -> Vec<String> {
        parse(src, MarkdownParser::gfm().options())
            .blocks
            .iter()
            .flat_map(|b| b.hidden.iter().map(|r| src[r.clone()].to_owned()))
            .collect()
    }

    fn first_block(src: &str) -> Block {
        parse(src, MarkdownParser::gfm().options())
            .blocks
            .into_iter()
            .next()
            .expect("至少一个块")
    }

    const TABLE: &str = "| 左 | 中 | 右 |\n| :-- | :-: | --: |\n| a | **b** | c |\n| d | e | f |\n";

    #[test]
    fn table_is_one_block_with_grid() {
        let parsed = parse(TABLE, MarkdownParser::gfm().options());
        assert_eq!(parsed.blocks.len(), 1);
        let block = &parsed.blocks[0];
        assert_eq!(block.kind, BlockKind::Table);
        let grid = block.table.as_ref().expect("表格应带结构化投影");
        assert_eq!(grid.rows.len(), 3, "一行表头 + 两行正文");
        assert!(grid.rows[0].head);
        assert!(!grid.rows[1].head);
        assert!(grid.rows.iter().all(|r| r.cells.len() == 3));
    }

    #[test]
    fn table_cell_ranges_come_from_events() {
        let block = first_block(TABLE);
        let grid = block.table.as_ref().unwrap();
        // 单元格 byte 含两侧填充空格；content 只含可见文本
        assert_eq!(grid.rows[0].cells[0].byte, 1..6);
        assert_eq!(grid.rows[0].cells[0].content, 2..5);
        assert_eq!(grid.rows[0].cells[2].content, 14..17);
        // `**b**` 的可见内容只有 b，两个星号仍留给差集去折叠
        assert_eq!(grid.rows[1].cells[1].byte, 45..52);
        assert_eq!(grid.rows[1].cells[1].content, 48..49);
    }

    #[test]
    fn table_alignment_is_preserved() {
        let grid = first_block(TABLE).table.unwrap();
        assert_eq!(
            grid.alignment,
            vec![ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right]
        );
    }

    #[test]
    fn table_locate_and_caret_byte() {
        let grid = first_block(TABLE).table.unwrap();
        assert_eq!(grid.locate(3), Some((0, 0)));
        assert_eq!(grid.locate(48), Some((1, 1)));
        assert_eq!(grid.locate(0), None, "竖线不属于任何单元格");
        assert_eq!(grid.locate(42), Some((1, 0)));
        assert_eq!(grid.caret_byte(TABLE, 1, 1), Some(48));
    }

    #[test]
    fn table_empty_cell_caret_skips_padding() {
        let src = "| a |  | c |\n| --- | --- | --- |\n| d | e | f |\n";
        let grid = first_block(src).table.unwrap();
        let cell = &grid.rows[0].cells[1];
        assert_eq!(cell.content, cell.byte.start..cell.byte.start);
        // 左侧两个空格被跳过，落点仍在格内
        assert_eq!(grid.caret_byte(src, 0, 1), Some(cell.byte.end - 1));
    }

    #[test]
    fn table_cell_navigation_wraps_rows() {
        let grid = first_block(TABLE).table.unwrap();
        assert_eq!(grid.next_cell(0, 2), Some((1, 0)));
        assert_eq!(grid.prev_cell(1, 0), Some((0, 2)));
        assert_eq!(grid.next_cell(2, 2), None);
        assert_eq!(grid.prev_cell(0, 0), None);
    }

    #[test]
    fn paragraph_has_no_table_grid() {
        assert!(first_block("普通段落").table.is_none());
    }

    #[test]
    fn strong_markers_are_paired() {
        // 配对是「退格整对删除」的唯一依据，配错了会把标记写坏
        let block = first_block("**粗**");
        assert_eq!(block.markers.len(), 1);
        assert_eq!(block.markers[0].open, 0..2);
        assert_eq!(block.markers[0].close, 5..7);
    }

    #[test]
    fn heading_prefix_has_no_pair() {
        // 标题前缀是单边的，整段删掉即可，不能去配一个不存在的尾巴
        let block = first_block("# 标题");
        assert!(block.markers.is_empty());
        assert_eq!(block.hidden, vec![0..2]);
    }

    #[test]
    fn nested_emphasis_pairs_stay_distinct() {
        let block = first_block("**a *b* c**");
        assert_eq!(block.markers.len(), 2, "外层加粗与内层斜体各一对");
        // 外层必须包住内层，否则退格会拆错那一对
        let outer = &block.markers[0];
        let inner = &block.markers[1];
        assert!(outer.open.start < inner.open.start);
        assert!(outer.close.end > inner.close.end);
    }

    #[test]
    fn inline_code_backticks_are_paired() {
        // 行内代码没有容器事件，只能靠片段边缘反推
        let block = first_block("前`x`后");
        assert_eq!(block.markers.len(), 1);
        assert_eq!(block.markers[0].open, 3..4);
        assert_eq!(block.markers[0].close, 5..6);
    }

    #[test]
    fn link_brackets_are_paired() {
        let block = first_block("[文字](https://a.b)");
        assert_eq!(block.markers.len(), 1);
        assert_eq!(&block.markers[0].open, &(0..1));
        assert_eq!(block.markers[0].close.end, block.byte.end);
    }

    #[test]
    fn strong_markers_are_hidden() {
        assert_eq!(hidden_text("**加粗**"), vec!["**", "**"]);
    }

    #[test]
    fn heading_marker_is_hidden() {
        assert_eq!(hidden_text("## 标题"), vec!["## "]);
    }

    #[test]
    fn bare_hashes_are_not_a_heading() {
        // 逐字输入 `#` 时不能先跳成标题再掉回段落：标记要一直看得见
        for src in ["#", "##", "######", "  #"] {
            let block = first_block(src);
            assert_eq!(block.kind, BlockKind::Paragraph, "{src} 被当成了标题");
            assert!(block.hidden.is_empty(), "{src} 的标记被折叠了");
            assert_eq!(block.plain_text(), src.trim(), "{src} 的字符丢了");
        }
    }

    #[test]
    fn space_turns_hashes_into_a_heading() {
        // 敲下空格才算成形，之后输入内容不会再变回段落
        for src in ["# ", "## ", "# 标题", "## 标题"] {
            assert!(
                matches!(first_block(src).kind, BlockKind::Heading(_)),
                "{src} 应当是标题"
            );
        }
    }

    #[test]
    fn bare_hashes_stay_out_of_outline() {
        let parsed = parse("#\n\n# 甲\n", MarkdownParser::gfm().options());
        assert_eq!(parsed.outline.len(), 1);
        assert_eq!(parsed.outline[0].title, "甲");
    }

    #[test]
    fn code_span_hides_backticks() {
        // 探针发现的坑：Code 事件区间包含反引号，必须内缩
        assert_eq!(hidden_text("`x`"), vec!["`", "`"]);
        assert_eq!(hidden_text("``a`b``"), vec!["``", "``"]);
    }

    #[test]
    fn link_hides_brackets_and_url() {
        assert_eq!(
            hidden_text("[文字](https://x.com)"),
            vec!["[", "](https://x.com)"]
        );
    }

    #[test]
    fn image_is_atomic() {
        let block = first_block("![图](a.png)");
        assert_eq!(block.spans.len(), 1);
        assert_eq!(block.spans[0].kind, SpanKind::Image);
        assert_eq!(block.spans[0].text, "图");
        assert_eq!(block.spans[0].style.link.as_deref(), Some("a.png"));
        assert!(
            block.hidden.is_empty(),
            "整段被图片节点覆盖，不应再有隐藏区间"
        );
    }

    #[test]
    fn list_marker_is_hidden() {
        assert_eq!(hidden_text("- 甲"), vec!["- "]);
        assert_eq!(hidden_text("1. 甲"), vec!["1. "]);
    }

    #[test]
    fn soft_break_is_not_hidden() {
        // 探针发现的坑：隐藏换行会让段落塌成一行
        let block = first_block("第一行\n第二行");
        assert!(block.hidden.is_empty());
        assert!(
            block
                .spans
                .iter()
                .any(|s| matches!(s.kind, SpanKind::Break { hard: false }))
        );
    }

    #[test]
    fn table_pipes_are_hidden() {
        let src = "| A | B |\n|---|---|\n| 1 | 2 |";
        let block = first_block(src);
        assert_eq!(block.kind, BlockKind::Table);
        let hidden: String = block.hidden.iter().map(|r| &src[r.clone()]).collect();
        assert!(hidden.contains("|"));
        assert!(hidden.contains("---"));
        assert!(!hidden.contains('A'), "单元格文本不应被隐藏");
    }

    #[test]
    fn fenced_code_keeps_body_and_hides_fence() {
        let src = "```rust\nlet a = 1;\n```";
        let block = first_block(src);
        assert_eq!(
            block.kind,
            BlockKind::CodeBlock {
                info: Some("rust".into())
            }
        );
        let visible: String = block.spans.iter().map(|s| s.text.as_str()).collect();
        assert!(visible.contains("let a = 1;"));
        let hidden: String = block.hidden.iter().map(|r| &src[r.clone()]).collect();
        assert!(hidden.contains("```"));
    }

    #[test]
    fn code_block_tokens_cover_only_the_body() {
        let src = "```rust\nlet a = 1; // 说明\n```";
        let block = first_block(src);
        assert!(!block.tokens.is_empty(), "代码块没有产出着色片段");
        assert!(
            block
                .tokens
                .iter()
                .any(|t| t.kind == crate::highlight::TokenKind::Keyword),
            "缺少关键字着色: {:?}",
            block.tokens
        );
        let body_start = src.find('\n').unwrap() + 1;
        let body_end = src.rfind("```").unwrap();
        for token in &block.tokens {
            assert!(
                body_start <= token.byte.start && token.byte.end <= body_end,
                "着色片段越过围栏: {:?}",
                token.byte
            );
        }
    }

    #[test]
    fn non_code_blocks_have_no_tokens() {
        let src = "# 标题\n\n普通段落\n";
        let parsed = parse(src, MarkdownParser::gfm().options());
        assert!(parsed.blocks.iter().all(|b| b.tokens.is_empty()));
    }

    #[test]
    fn reference_links_resolve_across_blocks() {
        // 块级独立解析会丢掉这个链接，必须整篇解析
        let src = "[甲][r]\n\n正文\n\n[r]: https://a.example";
        let parsed = parse(src, MarkdownParser::gfm().options());
        let has_link = parsed
            .blocks
            .iter()
            .flat_map(|b| &b.spans)
            .any(|s| s.kind == SpanKind::Link);
        assert!(has_link, "引用式链接未被解析，说明走了块级独立解析");
    }

    #[test]
    fn hidden_never_overlaps_visible_spans() {
        let src = "# 标题\n\n**粗** 与 `code` 和 [链接](https://x.com)\n\n- [ ] 待办\n";
        let parsed = parse(src, MarkdownParser::gfm().options());
        for block in &parsed.blocks {
            for hidden in &block.hidden {
                for span in &block.spans {
                    assert!(
                        hidden.end <= span.byte.start || hidden.start >= span.byte.end,
                        "隐藏区间 {hidden:?} 与片段 {:?} 重叠",
                        span.byte
                    );
                }
            }
        }
    }

    #[test]
    fn task_marker_becomes_widget() {
        let block = first_block("- [ ] 待办");
        assert!(
            block
                .spans
                .iter()
                .any(|s| matches!(s.kind, SpanKind::TaskMarker { checked: false }))
        );
    }

    #[test]
    fn raw_html_is_not_emitted_verbatim() {
        let src = "点击 <script>alert(1)</script> 这里\n";
        let parsed = parse(src, MarkdownParser::gfm().options());
        let html = parsed.to_html(src);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn list_split_by_blank_line_is_still_a_list() {
        // 松散列表的项之间隔着空行，切块会把一个 List 容器拆成多块。
        // 后半段块的首个事件是 `Item` 而不是 `List`，曾经因此退化成段落——
        // 序号与项目符号一起消失，用户看到的是「列表变成了普通文字」。
        let src = "1. 甲\n2. 乙\n\n3. 丙\n4. 丁\n";
        let parsed = parse(src, MarkdownParser::gfm().options());
        assert_eq!(parsed.blocks.len(), 2);
        assert_eq!(parsed.blocks[0].kind, BlockKind::List { ordered: true });
        assert_eq!(
            parsed.blocks[1].kind,
            BlockKind::List { ordered: true },
            "空行之后的列表块仍应是有序列表"
        );
    }

    #[test]
    fn unordered_list_after_blank_line_keeps_its_kind() {
        let src = "- 甲\n- 乙\n\n- 丙\n- 丁\n";
        let parsed = parse(src, MarkdownParser::gfm().options());
        assert_eq!(parsed.blocks.len(), 2);
        for block in &parsed.blocks {
            assert_eq!(block.kind, BlockKind::List { ordered: false });
        }
    }

    #[test]
    fn ordered_and_unordered_lists_are_distinguished() {
        let ordered = first_block("1. 甲");
        let unordered = first_block("- 甲");
        assert_eq!(ordered.kind, BlockKind::List { ordered: true });
        assert_eq!(unordered.kind, BlockKind::List { ordered: false });
    }

    #[test]
    fn list_html_gets_one_li_per_item() {
        // 没有 <li> 时浏览器只肯给一个序号，整份列表看起来只有一项
        let src = "- 甲\n- 乙\n\n3. 丙\n4. 丁\n";
        let html = parse(src, MarkdownParser::gfm().options()).to_html(src);
        assert!(html.contains("<ul><li>甲</li><li>乙</li></ul>"), "{html}");
        assert!(
            html.contains("<ol start=\"3\"><li>丙</li><li>丁</li></ol>"),
            "{html}"
        );
    }

    #[test]
    fn nested_list_keeps_indent_levels_in_both_views() {
        // 三级嵌套曾被拍平：阅读视图靠 line_starts_item 猜项边界，四空格
        // 缩进的三级项被误判成正文、挤进二级的 <li>；投影层也只存了一个
        // ordered 布尔值，深度信息全丢。现在两级视图都要带出层级。
        let src = "- 一级\n  - 二级\n    - 三级\n";
        let parsed = parse(src, MarkdownParser::gfm().options());
        let block = &parsed.blocks[0];

        // 投影层：三个项，深度 0/1/2
        let list = block.list.as_ref().expect("列表块应有 ListInfo");
        assert_eq!(list.items.len(), 3);
        assert_eq!(
            list.items.iter().map(|i| i.depth).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "每项的嵌套深度应来自 pulldown 的 Item 事件"
        );

        // 阅读视图：三个 li，层级以 data-depth / margin-left 表达
        let html = parsed.to_html(src);
        assert_eq!(html.matches("<li").count(), 3, "{html}");
        assert!(html.contains("data-depth=\"1\""), "{html}");
        assert!(html.contains("data-depth=\"2\""), "{html}");
        assert!(html.contains("margin-left:2em"), "{html}");
        assert!(html.contains("margin-left:4em"), "{html}");
    }

    #[test]
    fn nested_list_editor_li_keeps_depth_without_touching_stops() {
        // 编辑视图的 li 必须带 data-depth，且字符停靠点 data-b/data-a 序列
        // 与拍平版本完全一致——否则光标定位、选区、点击都会错位。
        use crate::{DocumentLayout, preview::block_frames};

        let src = "- 一级\n  - 二级\n    - 三级\n";
        let parsed = parse(src, MarkdownParser::gfm().options());
        let doc = DocumentLayout::build(src, &parsed);
        let html = &block_frames(src, &parsed, &doc)[0].html;

        assert_eq!(html.matches("<div class=\"li\"").count(), 3, "{html}");
        assert!(html.contains("data-depth=\"1\""), "{html}");
        assert!(html.contains("data-depth=\"2\""), "{html}");

        // 所有 data-b 停靠点编号连续 0..=10（含被折叠标记的 mk 停靠点与块尾），
        // 与拍平版本完全一致——层级只加在 li 容器上，没碰任何停靠点。
        let before: Vec<usize> = html
            .split("data-b=\"")
            .skip(1)
            .filter_map(|s| s.split('"').next()?.parse().ok())
            .collect();
        assert_eq!(before, (0..=10).collect::<Vec<_>>(), "{html}");
    }

    #[test]
    fn item_marker_reads_the_source_verbatim() {
        assert_eq!(item_marker("1. 甲", 0).as_deref(), Some("1."));
        assert_eq!(item_marker("12) 甲", 0).as_deref(), Some("12)"));
        assert_eq!(item_marker("- 甲", 0), None);
        assert_eq!(item_marker("1.x 不是列表", 0), None);
        assert_eq!(item_marker("正文 1. 甲", 0), None);
        // 行首三个以内空格缩进仍算列表项，四格就不再是了
        assert_eq!(item_marker("    1. 甲", 0), None);
        assert_eq!(item_marker("   1. 甲", 0).as_deref(), Some("1."));
    }

    #[test]
    fn hard_break_inside_an_item_does_not_split_it() {
        // 项内 Shift+Enter 也是硬换行，但它后面那行不是新的列表项
        let src = "- 甲\\\n  续行\n- 乙\n";
        let html = parse(src, MarkdownParser::gfm().options()).to_html(src);
        assert_eq!(html.matches("<li>").count(), 2, "{html}");
    }

    #[test]
    fn outline_comes_from_heading_blocks() {
        let parsed = parse("# 甲\n\n## 乙\n", MarkdownParser::gfm().options());
        assert_eq!(parsed.outline.len(), 2);
        assert_eq!(parsed.outline[0].level, 1);
        assert_eq!(parsed.outline[1].title, "乙");
    }

    #[test]
    fn chinese_byte_ranges_stay_on_char_boundary() {
        let src = "# 中文标题\n\n第二段内容\n";
        let parsed = parse(src, MarkdownParser::gfm().options());
        for block in &parsed.blocks {
            assert!(src.is_char_boundary(block.byte.start));
            assert!(src.is_char_boundary(block.byte.end));
            for span in &block.spans {
                assert!(
                    src.is_char_boundary(span.byte.start),
                    "span 起点不在字符边界"
                );
                assert!(src.is_char_boundary(span.byte.end), "span 终点不在字符边界");
            }
            for hidden in &block.hidden {
                assert!(src.is_char_boundary(hidden.start));
                assert!(src.is_char_boundary(hidden.end));
            }
        }
    }

    #[test]
    fn list_items_keep_their_own_line() {
        // 切块按空行进行，列表项之间的换行落在块内，必须显式保留
        let block = first_block("- 甲\n- 乙\n");
        let breaks = block
            .spans
            .iter()
            .filter(|s| matches!(s.kind, SpanKind::Break { hard: true }))
            .count();
        assert_eq!(breaks, 1, "两个列表项之间应有一个硬换行");
    }

    #[test]
    fn table_rows_keep_their_own_line() {
        // 分隔行 `|---|---|` 整段折叠，只留表头与数据行之间的那一个换行
        let block = first_block("| A | B |\n|---|---|\n| 1 | 2 |");
        let breaks = block
            .spans
            .iter()
            .filter(|s| matches!(s.kind, SpanKind::Break { hard: true }))
            .count();
        assert_eq!(breaks, 1, "表头与数据行之间应有 1 个换行");
    }

    #[test]
    fn inline_math_folds_delimiters_and_keeps_source() {
        let src = "公式 $E=mc^2$ 结束";
        let block = first_block(src);
        let math: Vec<_> = block
            .spans
            .iter()
            .filter(|s| matches!(s.kind, SpanKind::Math { .. }))
            .collect();
        assert_eq!(math.len(), 1);
        assert_eq!(math[0].text, "E=mc^2");
        assert_eq!(&src[math[0].byte.clone()], "E=mc^2");
        assert!(matches!(math[0].kind, SpanKind::Math { display: false }));
        // 定界符进差集，且被登记为一对，退格时能整对抽掉
        assert!(block.hidden.iter().any(|r| &src[r.clone()] == "$"));
        assert_eq!(block.markers.len(), 1);
    }

    #[test]
    fn display_math_folds_double_dollars() {
        let src = "$$\n\\int_0^1 x\\,dx\n$$";
        let block = first_block(src);
        let math: Vec<_> = block
            .spans
            .iter()
            .filter(|s| matches!(s.kind, SpanKind::Math { .. }))
            .collect();
        assert_eq!(math.len(), 1);
        assert!(matches!(math[0].kind, SpanKind::Math { display: true }));
        assert!(math[0].text.contains("\\int_0^1"));
        assert_eq!(&src[math[0].byte.clone()], math[0].text);
        assert!(block.markers.len() >= 1);
    }

    #[test]
    fn math_inner_trims_exactly_the_declared_delimiters() {
        // 即使内容两端也是 `$`，也只砍固定层数的定界符，不多吃内容的 `$`
        let src = "$$ $x$ $$";
        let inner = math_inner(src, &(0..src.len()), true);
        assert_eq!(&src[inner], " $x$ ");
    }

    #[test]
    fn merge_and_subtract_are_sane() {
        let merged = merge(vec![3..5, 1..4, 9..10]);
        assert_eq!(merged, vec![1..5, 9..10]);
        assert_eq!(subtract(0..10, &merged), vec![0..1, 5..9]);
        assert_eq!(subtract(0..10, &[]), vec![0..10]);
    }
}

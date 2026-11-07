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

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::block::{
    Block, BlockId, BlockKind, MarkerPair, OutlineItem, ParseResult, Span, SpanKind, Style,
};
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

    /// CommonMark + 表格 / 脚注 / 删除线 / 任务列表。
    /// 不启用 ENABLE_RAW_HTML：原始 HTML 退化为普通文本，杜绝脚本注入。
    pub fn gfm() -> Self {
        Self::new(
            Options::ENABLE_TABLES
                | Options::ENABLE_FOOTNOTES
                | Options::ENABLE_STRIKETHROUGH
                | Options::ENABLE_TASKLISTS,
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

    for span in spans.iter().filter(|s| s.kind == SpanKind::Code) {
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
                if let Some(owner) = owner_of(&ranges, range.start) {
                    if accs[owner].kind.is_none() {
                        accs[owner].kind = Some(classify(&tag));
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
                ) {
                    if let Some(owner) = owner_of(&ranges, range.start) {
                        accs[owner].containers.push(range.clone());
                    }
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
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
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
        let block = Block {
            id: BlockId(index),
            kind,
            byte,
            spans,
            hidden,
            markers,
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

fn code_inner(src: &str, range: &Range<usize>) -> Range<usize> {    let delims = src
        .get(range.clone())
        .map(|s| s.bytes().take_while(|b| *b == b'`').count())
        .unwrap_or(0);
    if delims > 0 && range.len() >= delims * 2 {
        (range.start + delims)..(range.end - delims)
    } else {
        range.clone()
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
    ranges
        .get(index)
        .filter(|r| r.start <= byte)
        .map(|_| index)
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
        assert!(block.hidden.is_empty(), "整段被图片节点覆盖，不应再有隐藏区间");
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
        let parsed = parse("点击 <script>alert(1)</script> 这里\n", MarkdownParser::gfm().options());
        let html = parsed.to_html();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
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
                assert!(src.is_char_boundary(span.byte.start), "span 起点不在字符边界");
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
    fn merge_and_subtract_are_sane() {
        let merged = merge(vec![3..5, 1..4, 9..10]);
        assert_eq!(merged, vec![1..5, 9..10]);
        assert_eq!(subtract(0..10, &merged), vec![0..1, 5..9]);
        assert_eq!(subtract(0..10, &[]), vec![0..10]);
    }
}

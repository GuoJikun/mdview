//! 视觉布局：把「byte + 折叠区间」进一步压成「视觉列 + 停靠点」。
//!
//! 编辑器不能直接用 byte 定位光标：`**加粗**` 的视觉宽度是 2，byte 宽度是 8，
//! 而且 byte 0..2 与 8..10 在视觉上根本不存在。所以本层产出**停靠点表**：
//!
//! ```text
//! 源码   ** 加 粗 **
//! byte   0  2  5  8 10
//! 视觉   0  0  1  2  2
//! ```
//!
//! 关键性质：**同一个视觉列可以有多个停靠点**。视觉 0 对应 byte 0 与 byte 2，
//! 前者在标记里、后者在文字前。光标右移时从 byte 0 走到 byte 2，视觉上不动，
//! 于是标记自然被「跨过」；反之若把两者合并成一个位置，光标就会卡在标记里，
//! 打字时把 `**` 拆散。
//!
//! 本层只产出「第几列」这个有序标量，不做像素测量——字体、缩放、换行都由
//! 前端 DOM 负责，内核不感知。

use std::ops::Range;

use crate::block::{align, Block, BlockId, ParseResult, SpanKind};
use crate::sourcemap::SourceMap;

/// 光标可停靠的一个位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stop {
    /// 原始源码字节偏移
    pub byte: usize,
    /// 块内视觉列。两个 byte 不同的停靠点可以共享同一列（中间隔着被折叠的标记）。
    pub visual: usize,
}

/// 由 byte 反查停靠点时的偏好方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gravity {
    /// 贴住左侧可见文本。落在折叠区间里时回到区间**左端**。
    /// 点击定位用它：点「加」的左边应该落在文字前，而不是 `**` 前。
    Upstream,
    /// 越过折叠区间，落到区间**右端**。键盘右移用它，标记才跨得过去。
    Downstream,
}

/// 单个块的视觉布局。
#[derive(Debug, Clone)]
pub struct Layout {
    block: BlockId,
    byte: Range<usize>,
    stops: Vec<Stop>,
    visual_width: usize,
}

impl Layout {
    pub fn build(src: &str, block: &Block) -> Self {
        let mut segs: Vec<Seg<'_>> = Vec::with_capacity(block.spans.len() + block.hidden.len());
        for span in &block.spans {
            segs.push(match &span.kind {
                // 原子控件：整体占一列，内部不可进入
                SpanKind::Image
                | SpanKind::Rule
                | SpanKind::TaskMarker { .. }
                | SpanKind::Break { .. } => Seg::Atom(span.byte.clone()),
                _ => Seg::Visible {
                    byte: span.byte.clone(),
                    text: span.text.as_str(),
                },
            });
        }
        for range in &block.hidden {
            segs.push(Seg::Collapsed(range.clone()));
        }
        // 稳定排序：同起点时保持「先可见、后折叠」的插入顺序
        segs.sort_by_key(|seg| (seg.start(), seg.end()));

        let mut stops: Vec<Stop> = Vec::new();
        let mut visual = 0usize;
        let mut cursor = block.byte.start;

        for seg in &segs {
            let end = seg.end();
            if end <= cursor {
                continue;
            }
            let start = seg.start().max(cursor);
            if start > cursor {
                // 既不在片段、也不在折叠区间里：说明投影层漏了覆盖。
                // 按原文可见呈现，宁可多显示，也不能吞字。
                let slice = src.get(cursor..start).unwrap_or("");
                emit_visible(&mut stops, &mut visual, cursor, slice, slice);
            }
            match seg {
                Seg::Collapsed(_) => {
                    // 零宽：前后两个停靠点共享同一视觉列，这正是「多停靠点」的来源
                    push_stop(&mut stops, start, visual);
                    push_stop(&mut stops, end, visual);
                }
                Seg::Atom(_) => {
                    push_stop(&mut stops, start, visual);
                    visual += 1;
                    push_stop(&mut stops, end, visual);
                }
                Seg::Visible { byte, text } => {
                    let (slice, text) = if start > byte.start {
                        let skip = src
                            .get(byte.start..start)
                            .map(|s| s.chars().count())
                            .unwrap_or(0);
                        let at = char_index(text, skip);
                        (&src[start..byte.end], &text[at..])
                    } else {
                        (src.get(byte.clone()).unwrap_or(""), *text)
                    };
                    emit_visible(&mut stops, &mut visual, start, slice, text);
                }
            }
            cursor = end;
        }

        if cursor < block.byte.end {
            let slice = src.get(cursor..block.byte.end).unwrap_or("");
            emit_visible(&mut stops, &mut visual, cursor, slice, slice);
        }
        if stops.is_empty() {
            stops.push(Stop {
                byte: block.byte.start,
                visual: 0,
            });
        }

        Self {
            block: block.id,
            byte: block.byte.clone(),
            stops,
            visual_width: visual,
        }
    }

    pub fn block(&self) -> BlockId {
        self.block
    }

    pub fn byte_range(&self) -> &Range<usize> {
        &self.byte
    }

    /// 停靠点数量，即块内光标可停的位置数。
    pub fn len(&self) -> usize {
        self.stops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }

    /// 块内视觉列总数（不含块间距）。
    pub fn width(&self) -> usize {
        self.visual_width
    }

    pub fn stops(&self) -> &[Stop] {
        &self.stops
    }

    pub fn stop(&self, index: usize) -> Option<Stop> {
        self.stops.get(index).copied()
    }

    pub fn byte(&self, index: usize) -> Option<usize> {
        self.stops.get(index).map(|s| s.byte)
    }

    pub fn visual(&self, index: usize) -> Option<usize> {
        self.stops.get(index).map(|s| s.visual)
    }

    /// 视觉列 → 停靠点序号。
    ///
    /// 同一列有多个停靠点时按 `gravity` 二选一：`Upstream` 取最左（贴住左词末尾），
    /// 但列 0 左侧没有可见文本，只能取最右，否则光标会停在 `**` 里面。
    pub fn stop_at_visual(&self, visual: usize, gravity: Gravity) -> usize {
        let target = visual.min(self.visual_width);
        let lo = self.stops.partition_point(|s| s.visual < target);
        let hi = self.stops.partition_point(|s| s.visual <= target);
        if lo >= hi {
            return self.stops.len().saturating_sub(1);
        }
        let has_text_on_left = lo > 0;
        match gravity {
            Gravity::Downstream => hi - 1,
            Gravity::Upstream if has_text_on_left => lo,
            Gravity::Upstream => hi - 1,
        }
    }

    /// byte → 停靠点序号。落在字符或折叠区间内部时按 `gravity` 吸附。
    pub fn stop_at_byte(&self, byte: usize, gravity: Gravity) -> usize {
        let index = self.stops.partition_point(|s| s.byte <= byte);
        if index == 0 {
            return 0;
        }
        let left = index - 1;
        let inside_collapsed = self
            .stops
            .get(index)
            .is_some_and(|next| next.visual == self.stops[left].visual);
        if !inside_collapsed {
            // 落在可见字符内部：回到字符左边界
            return left;
        }
        match gravity {
            // 块首的连续标记左侧没有可见文本，贴左会退到 `**` 之前；
            // 此时必须越过标记，否则光标落在块外。
            Gravity::Upstream if left > 0 => left,
            Gravity::Upstream => index,
            Gravity::Downstream => index,
        }
    }
}

enum Seg<'a> {
    Visible { byte: Range<usize>, text: &'a str },
    /// 视觉宽 1，内部不可进入
    Atom(Range<usize>),
    /// 视觉宽 0，被折叠的标记
    Collapsed(Range<usize>),
}

impl Seg<'_> {
    fn start(&self) -> usize {
        match self {
            Seg::Visible { byte, .. } | Seg::Atom(byte) | Seg::Collapsed(byte) => byte.start,
        }
    }

    fn end(&self) -> usize {
        match self {
            Seg::Visible { byte, .. } | Seg::Atom(byte) | Seg::Collapsed(byte) => byte.end,
        }
    }
}

fn push_stop(stops: &mut Vec<Stop>, byte: usize, visual: usize) {
    if let Some(last) = stops.last() {
        if byte < last.byte {
            return;
        }
        if last.byte == byte && last.visual == visual {
            return;
        }
    }
    stops.push(Stop { byte, visual });
}

/// 逐字符产出停靠点。转义与实体会让源码比文本长，所以边界一律走 [`align`] 反查，
/// 不能用 `byte_start + char_index` 直接算。
fn emit_visible(
    stops: &mut Vec<Stop>,
    visual: &mut usize,
    byte_start: usize,
    src_slice: &str,
    text: &str,
) {
    push_stop(stops, byte_start, *visual);
    for i in 1..=text.chars().count() {
        let rel = align(src_slice, text, i);
        *visual += 1;
        push_stop(stops, byte_start + rel, *visual);
    }
}

fn char_index(s: &str, chars: usize) -> usize {
    s.char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(s.len())
}

/// 全文档视觉布局：各块布局串联，给出全局停靠点索引与全局视觉列。
///
/// 全局停靠点索引 = 块起始偏移 + 块内序号，是个可以直接 `+1` 的连续整数，
/// 跨块移动不需要特殊分支。块间空行不产生停靠点——视觉上它只是块间距。
#[derive(Debug, Clone, Default)]
pub struct DocumentLayout {
    blocks: Vec<Layout>,
    /// base[i] = 块 i 首个停靠点的全局序号
    base: Vec<usize>,
    /// prefix[i] = 块 0..i 的视觉宽度之和
    prefix: Vec<usize>,
    total: usize,
    width: usize,
}

impl DocumentLayout {
    pub fn build(src: &str, parsed: &ParseResult) -> Self {
        let mut blocks = Vec::with_capacity(parsed.blocks.len());
        let mut base = Vec::with_capacity(parsed.blocks.len());
        let mut prefix = Vec::with_capacity(parsed.blocks.len());
        let mut total = 0usize;
        let mut width = 0usize;
        for block in &parsed.blocks {
            let layout = Layout::build(src, block);
            base.push(total);
            prefix.push(width);
            total += layout.len();
            width += layout.width();
            blocks.push(layout);
        }
        Self {
            blocks,
            base,
            prefix,
            total,
            width,
        }
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// 全局停靠点总数。
    pub fn stops(&self) -> usize {
        self.total
    }

    /// 全文档视觉列总数，不含块间距。
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn layout(&self, id: BlockId) -> Option<&Layout> {
        self.blocks.get(id.0)
    }

    pub fn block(&self, id: BlockId) -> Option<&BlockId> {
        self.blocks.get(id.0).map(|l| &l.block)
    }

    /// 全局停靠点 → (块, 块内序号)
    pub fn locate(&self, global: usize) -> Option<(BlockId, usize)> {
        if global >= self.total {
            return None;
        }
        let index = self.base.partition_point(|b| *b <= global) - 1;
        Some((BlockId(index), global - self.base[index]))
    }

    pub fn global(&self, id: BlockId, local: usize) -> Option<usize> {
        let layout = self.blocks.get(id.0)?;
        (local < layout.len()).then(|| self.base[id.0] + local)
    }

    pub fn byte(&self, global: usize) -> Option<usize> {
        let (id, local) = self.locate(global)?;
        self.blocks[id.0].byte(local)
    }

    /// 全局停靠点 → 全局视觉列（不含块间距）。
    pub fn visual(&self, global: usize) -> Option<usize> {
        let (id, local) = self.locate(global)?;
        Some(self.prefix[id.0] + self.blocks[id.0].visual(local)?)
    }

    pub fn stop_at_byte(&self, byte: usize, gravity: Gravity) -> Option<usize> {
        if self.blocks.is_empty() {
            return None;
        }
        let index = self.blocks.partition_point(|b| b.byte.end < byte);
        let index = index.min(self.blocks.len() - 1);
        let local = self.blocks[index].stop_at_byte(byte, gravity);
        Some(self.base[index] + local)
    }

    /// 与 [`Self::stop_at_byte`] 配套的块内查询，供选区高亮按块切分。
    pub fn source_map(&self) -> SourceMap {
        let ranges: Vec<_> = self
            .blocks
            .iter()
            .map(|l| (l.byte.clone(), l.block))
            .collect();
        SourceMap::from_entries(ranges)
    }
}

/// 选区：一对全局停靠点索引。`anchor` 是不动的一端，`head` 是拖动的一端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    /// 有序区间（起止），不含方向。
    pub fn normalized(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// 选区对应的源码 byte 区间。跨块时**包含块间空行**，否则复制粘贴会把两段粘连。
    pub fn byte_range(&self, doc: &DocumentLayout) -> Option<Range<usize>> {
        let (lo, hi) = self.normalized();
        Some(doc.byte(lo)?..doc.byte(hi)?)
    }

    /// 选区覆盖到的块，按文档顺序。
    pub fn blocks(&self, doc: &DocumentLayout) -> Vec<BlockId> {
        let (lo, hi) = self.normalized();
        let Some((first, _)) = doc.locate(lo) else {
            return Vec::new();
        };
        let Some((last, _)) = doc.locate(hi) else {
            return Vec::new();
        };
        (first.0..=last.0).map(BlockId).collect()
    }

    /// 某个块内被选中的 byte 区间，供渲染层按片段切高亮。落在选区外返回 `None`。
    pub fn block_slice(&self, doc: &DocumentLayout, id: BlockId) -> Option<Range<usize>> {
        let layout = doc.layout(id)?;
        let (lo, hi) = self.normalized();
        let first = doc.global(id, 0)?;
        let last = doc.global(id, layout.len() - 1)?;
        if hi < first || lo > last {
            return None;
        }
        let start = if lo <= first {
            layout.byte_range().start
        } else {
            doc.byte(lo)?
        };
        let end = if hi >= last {
            layout.byte_range().end
        } else {
            doc.byte(hi)?
        };
        (start < end).then_some(start..end)
    }
}

/// 光标：块 + 块内停靠点序号。
///
/// 不直接用全局索引，是因为块内序号在重解析后仍能对回同一个块；
/// 跨版本持久化请用 [`Self::to_byte`] 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub block: BlockId,
    pub stop: usize,
}

impl Caret {
    pub fn from_byte(doc: &DocumentLayout, byte: usize) -> Option<Self> {
        let global = doc.stop_at_byte(byte, Gravity::Downstream)?;
        let (block, stop) = doc.locate(global)?;
        Some(Self { block, stop })
    }

    pub fn at(doc: &DocumentLayout, block: BlockId, stop: usize) -> Option<Self> {
        let layout = doc.layout(block)?;
        (stop < layout.len()).then_some(Self { block, stop })
    }

    pub fn to_byte(&self, doc: &DocumentLayout) -> Option<usize> {
        doc.layout(self.block)?.byte(self.stop)
    }

    pub fn to_global(&self, doc: &DocumentLayout) -> Option<usize> {
        doc.global(self.block, self.stop)
    }

    pub fn visual(&self, doc: &DocumentLayout) -> Option<usize> {
        self.to_global(doc).and_then(|g| doc.visual(g))
    }

    /// 右移一个停靠点。跨过折叠标记时视觉列不变，这正是标记不会被停进去的原因。
    pub fn next(&self, doc: &DocumentLayout) -> Option<Self> {
        let global = self.to_global(doc)?;
        let (block, stop) = doc.locate(global + 1)?;
        Some(Self { block, stop })
    }

    pub fn prev(&self, doc: &DocumentLayout) -> Option<Self> {
        let global = self.to_global(doc)?;
        let (block, stop) = doc.locate(global.checked_sub(1)?)?;
        Some(Self { block, stop })
    }

    pub fn home(&self, doc: &DocumentLayout) -> Option<Self> {
        let layout = doc.layout(self.block)?;
        let visual = layout.visual(self.stop)?;
        let stop = layout.stop_at_visual(0, Gravity::Downstream);
        // 已经在行首时退回块首，避免 Home 反复无效
        let stop = if visual == 0 { 0 } else { stop };
        Self::at(doc, self.block, stop)
    }

    pub fn end(&self, doc: &DocumentLayout) -> Option<Self> {
        let layout = doc.layout(self.block)?;
        Self::at(doc, self.block, layout.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::MarkdownParser;

    fn build(src: &str) -> (ParseResult, DocumentLayout) {
        let parsed = MarkdownParser::gfm().parse(src);
        let layout = DocumentLayout::build(src, &parsed);
        (parsed, layout)
    }

    fn first(src: &str) -> Layout {
        let (parsed, _) = build(src);
        Layout::build(src, &parsed.blocks[0])
    }

    fn pairs(layout: &Layout) -> Vec<(usize, usize)> {
        layout.stops().iter().map(|s| (s.byte, s.visual)).collect()
    }

    #[test]
    fn plain_text_has_one_stop_per_char() {
        let layout = first("中文");
        assert_eq!(pairs(&layout), vec![(0, 0), (3, 1), (6, 2)]);
        assert_eq!(layout.width(), 2);
    }

    #[test]
    fn strong_markers_collapse_to_zero_width() {
        let layout = first("**加粗**");
        assert_eq!(
            pairs(&layout),
            vec![(0, 0), (2, 0), (5, 1), (8, 2), (10, 2)]
        );
        assert_eq!(layout.width(), 2);
    }

    #[test]
    fn upstream_gravity_sticks_to_visible_text() {
        let layout = first("**加粗**");
        // 列 0 左侧没有文字，只能跳到文字前
        assert_eq!(layout.stop_at_visual(0, Gravity::Upstream), 1);
        assert_eq!(layout.byte(1), Some(2));
        // 列 2 左侧有文字，停在「粗」之后而非闭合标记之后
        assert_eq!(layout.byte(layout.stop_at_visual(2, Gravity::Upstream)), Some(8));
    }

    #[test]
    fn downstream_gravity_crosses_markers() {
        let layout = first("**加粗**");
        assert_eq!(layout.byte(layout.stop_at_visual(0, Gravity::Downstream)), Some(2));
        assert_eq!(layout.byte(layout.stop_at_visual(2, Gravity::Downstream)), Some(10));
    }

    #[test]
    fn marker_inside_line_shares_one_visual_column() {
        // 视觉上是「甲乙」，中间的 `](u)` 被折叠，于是列 1 有两个停靠点
        let layout = first("[甲](u)乙");
        let at_one: Vec<usize> = layout
            .stops()
            .iter()
            .filter(|s| s.visual == 1)
            .map(|s| s.byte)
            .collect();
        assert_eq!(at_one, vec![4, 8]);
        assert_eq!(layout.width(), 2);
    }

    #[test]
    fn caret_moves_one_stop_at_a_time_across_markers() {
        let (_, doc) = build("**a**");
        let mut caret = Caret::at(&doc, BlockId(0), 0).unwrap();
        let mut path = vec![caret.to_byte(&doc).unwrap()];
        while let Some(next) = caret.next(&doc) {
            caret = next;
            path.push(caret.to_byte(&doc).unwrap());
        }
        // 视觉列依次是 0 0 1 1：前两站与后两站视觉不动，标记被跨过而非停进去
        assert_eq!(path, vec![0, 2, 3, 5]);
    }

    #[test]
    fn image_takes_exactly_one_column() {
        let layout = first("![图](a.png)");
        assert_eq!(layout.width(), 1);
        assert_eq!(pairs(&layout), vec![(0, 0), (13, 1)]);
    }

    #[test]
    fn soft_break_is_a_visible_newline() {
        let layout = first("第一行\n第二行");
        assert_eq!(layout.width(), 7);
        // 换行本身占一列，前后字符边界都保留
        assert!(layout.stops().iter().any(|s| s.byte == "第一行".len()));
    }

    #[test]
    fn escape_keeps_byte_offsets_aligned() {
        // pulldown 把反斜杠留在前一个片段之外，这里折叠成 hidden，
        // 于是 `\` 前后共享视觉列 1
        let layout = first(r"a\*b");
        assert_eq!(pairs(&layout), vec![(0, 0), (1, 1), (2, 1), (3, 2), (4, 3)]);
        assert_eq!(layout.width(), 3);
    }

    #[test]
    fn entity_is_one_column_but_many_bytes() {
        // 源码 `&amp;x` 是 6 字节，投影 `&x` 只有 2 列，边界必须走 align 反查
        let layout = first("&amp;x");
        assert_eq!(pairs(&layout), vec![(0, 0), (5, 1), (6, 2)]);
        assert_eq!(layout.width(), 2);
    }

    #[test]
    fn every_stop_is_on_char_boundary() {
        let src = "# 标题\n\n**粗**、`code`、[链接](https://x.com)、![图](a.png)\n\n- [x] 待办\n\n```rust\nlet a = 1;\n```\n";
        let (parsed, doc) = build(src);
        for block in &parsed.blocks {
            let layout = doc.layout(block.id).unwrap();
            for stop in layout.stops() {
                assert!(
                    src.is_char_boundary(stop.byte),
                    "停靠点 {} 不在字符边界",
                    stop.byte
                );
            }
        }
    }

    #[test]
    fn document_stops_are_monotonic() {
        let src = "# 甲\n\n乙**丙**\n\n- 丁\n";
        let (_, doc) = build(src);
        let mut last = 0usize;
        for g in 0..doc.stops() {
            let byte = doc.byte(g).unwrap();
            assert!(byte >= last, "全局停靠点 byte 回退了");
            last = byte;
        }
        assert!(doc.width() > 0);
    }

    #[test]
    fn byte_to_caret_snaps_out_of_marker() {
        let (_, doc) = build("**加粗**");
        let caret = Caret::from_byte(&doc, 1).unwrap();
        assert_eq!(caret.to_byte(&doc), Some(2));
    }

    #[test]
    fn cross_block_selection_includes_blank_line() {
        let src = "abc\n\ndef\n";
        let (_, doc) = build(src);
        let start = doc.stop_at_byte(2, Gravity::Upstream).unwrap();
        let end = doc.stop_at_byte(6, Gravity::Downstream).unwrap();
        let selection = Selection::new(start, end);
        let range = selection.byte_range(&doc).unwrap();
        assert_eq!(&src[range.clone()], "c\n\nd");
        assert_eq!(selection.blocks(&doc), vec![BlockId(0), BlockId(1)]);
        assert_eq!(
            selection.block_slice(&doc, BlockId(0)),
            Some(2..3),
            "块 0 只高亮到尾部"
        );
        assert_eq!(selection.block_slice(&doc, BlockId(1)), Some(5..6));
    }

    #[test]
    fn reversed_selection_normalizes() {
        let src = "abcdef";
        let (_, doc) = build(src);
        let selection = Selection::new(4, 1);
        assert_eq!(&src[selection.byte_range(&doc).unwrap()], "bcd");
    }

    #[test]
    fn home_and_end_stay_inside_block() {
        let src = "第一段\n\n第二段\n";
        let (_, doc) = build(src);
        let caret = Caret::at(&doc, BlockId(1), 2).unwrap();
        assert_eq!(caret.home(&doc).unwrap().to_byte(&doc), Some(11));
        assert_eq!(caret.end(&doc).unwrap().to_byte(&doc), Some(20));
    }

    #[test]
    fn empty_document_has_no_stops() {
        let (_, doc) = build("");
        assert_eq!(doc.stops(), 0);
        assert!(Caret::from_byte(&doc, 0).is_none());
    }
}

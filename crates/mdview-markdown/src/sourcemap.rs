use std::ops::Range;

use crate::block::{Block, BlockId, Span};

/// byte ↔ 块 / 片段 的双向索引。
///
/// 生产实现还会补一张 node → layout rect 表，形成 byte ↔ node ↔ rect 三向映射；
/// 同步滚动按锚点块插值，不能用滚动百分比换算。
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    entries: Vec<(Range<usize>, BlockId)>,
}

impl SourceMap {
    pub fn build(blocks: &[Block]) -> Self {
        let mut entries: Vec<(Range<usize>, BlockId)> =
            blocks.iter().map(|b| (b.byte.clone(), b.id)).collect();
        entries.sort_by_key(|(range, _)| range.start);
        Self { entries }
    }

    /// 由 DOM 的 data-byte-start + 点击位置得到 byte offset，再反查所属块。
    pub fn block_at_byte(&self, byte: usize) -> Option<BlockId> {
        let index = self.entries.partition_point(|(range, _)| range.end <= byte);
        self.entries
            .get(index)
            .filter(|(range, _)| range.start <= byte && byte < range.end)
            .map(|(_, id)| *id)
    }

    pub fn span_of(&self, id: BlockId) -> Option<Range<usize>> {
        self.entries
            .iter()
            .find(|(_, block_id)| *block_id == id)
            .map(|(range, _)| range.clone())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 编辑器光标的中间表示：块 + 片段 + 片段内字符偏移。
///
/// 不直接用 byte 存光标，是因为编辑后 byte 会整体平移；
/// 而（块, 片段, 字符）在块内是稳定的，重解析后可以重新锚定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub block: BlockId,
    pub span: usize,
    pub char_in_span: usize,
}

impl Caret {
    /// 由绝对 byte 构造。落在隐藏标记里时吸附到右侧可见位置。
    pub fn from_byte(blocks: &[Block], src: &str, byte: usize) -> Option<Self> {
        let block = blocks.iter().find(|b| b.byte.start <= byte && byte <= b.byte.end)?;
        let snapped = block.snap_visible(byte);
        let span = block
            .span_at(snapped)
            .or_else(|| block.spans.len().checked_sub(1))?;
        let target = &block.spans[span];
        let rel = snapped.clamp(target.byte.start, target.byte.end) - target.byte.start;
        let char_in_span = src
            .get(target.byte.start..target.byte.start + rel)
            .map(|s| s.chars().count())
            .unwrap_or(0);
        Some(Self {
            block: block.id,
            span,
            char_in_span,
        })
    }

    /// 还原为绝对 byte。
    pub fn to_byte(&self, blocks: &[Block], src: &str) -> Option<usize> {
        let block = blocks.get(self.block.0)?;
        let span: &Span = block.spans.get(self.span)?;
        let slice = src.get(span.byte.clone())?;
        let rel = crate::block::align(slice, &span.text, self.char_in_span);
        Some(span.byte.start + rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::MarkdownParser;

    #[test]
    fn lookup_returns_containing_block() {
        let src = "# 标题\n\n第二段内容\n";
        let parsed = MarkdownParser::gfm().parse(src);
        let map = parsed.source_map();
        assert_eq!(map.len(), 2);
        assert_eq!(map.block_at_byte(0), Some(parsed.blocks[0].id));
        assert_eq!(
            map.block_at_byte(parsed.blocks[1].byte.start),
            Some(parsed.blocks[1].id)
        );
    }

    #[test]
    fn gap_between_blocks_maps_to_none() {
        let src = "aaa\n\n\n\nbbb\n";
        let parsed = MarkdownParser::gfm().parse(src);
        let map = parsed.source_map();
        let gap = parsed.blocks[0].byte.end + 1;
        assert!(map.block_at_byte(gap).is_none());
    }

    #[test]
    fn caret_roundtrips_over_chinese() {
        let src = "# 标题\n\n中文段落 abc\n";
        let parsed = MarkdownParser::gfm().parse(src);
        let block = &parsed.blocks[1];
        // 指向 "中" 之后
        let target = block.byte.start + "中".len();
        let caret = Caret::from_byte(&parsed.blocks, src, target).unwrap();
        assert_eq!(caret.to_byte(&parsed.blocks, src), Some(target));
    }

    #[test]
    fn caret_snaps_out_of_hidden_marker() {
        let src = "**加粗**";
        let parsed = MarkdownParser::gfm().parse(src);
        // byte 1 落在开头的 "**" 里
        let caret = Caret::from_byte(&parsed.blocks, src, 1).unwrap();
        assert_eq!(caret.to_byte(&parsed.blocks, src), Some(2));
    }
}

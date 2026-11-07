use std::ops::Range;

use crate::block::{Block, BlockId};

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
        let entries: Vec<_> = blocks.iter().map(|b| (b.byte.clone(), b.id)).collect();
        Self::from_entries(entries)
    }

    /// 由已排好序的 (块区间, 块 id) 建索引。视觉布局层直接复用自己那份区间，
    /// 免得为了拿 SourceMap 再把整篇 block 传一遍。
    pub fn from_entries(mut entries: Vec<(Range<usize>, BlockId)>) -> Self {
        entries.retain(|(range, _)| range.start <= range.end);
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

#[cfg(test)]
mod tests {
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
}

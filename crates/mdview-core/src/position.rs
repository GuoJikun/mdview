use std::ops::Range;

use crate::transaction::Transaction;

/// 插入点与光标重合时的归属方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bias {
    Left,
    Right,
}

/// 锚点：编辑发生后仍可解析的光标位置（char index）。
/// 生产实现会用 CRDT/位置标识符持久化，骨架中用「随 Transaction 平移」表达同一语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub offset: usize,
    pub bias: Bias,
}

impl Anchor {
    pub fn new(offset: usize, bias: Bias) -> Self {
        Self { offset, bias }
    }

    pub fn shift(&self, tx: &Transaction) -> Self {
        let removed_len = tx.removed.chars().count();
        let inserted_len = tx.insert.chars().count();
        let start = tx.span.start;
        let end = tx.span.end;

        let offset = if start == end {
            // 纯插入：左偏光标停在插入内容之前，右偏光标随文本右移。
            if self.offset < start || (self.offset == start && self.bias == Bias::Left) {
                self.offset
            } else {
                self.offset + inserted_len
            }
        } else if self.offset <= start {
            self.offset
        } else if self.offset >= end {
            self.offset - removed_len + inserted_len
        } else {
            match self.bias {
                Bias::Left => start,
                Bias::Right => start + inserted_len,
            }
        };

        Self { offset, bias: self.bias }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Anchor,
    pub head: Anchor,
}

impl Selection {
    pub fn caret(offset: usize) -> Self {
        let anchor = Anchor::new(offset, Bias::Right);
        Self { anchor, head: anchor }
    }

    pub fn range(&self) -> Range<usize> {
        let a = self.anchor.offset;
        let h = self.head.offset;
        a.min(h)..a.max(h)
    }

    pub fn shift(&self, tx: &Transaction) -> Self {
        Self {
            anchor: self.anchor.shift(tx),
            head: self.head.shift(tx),
        }
    }
}

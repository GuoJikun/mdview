use std::ops::Range;

use crate::document::Revision;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid char range {start}..{end}, document length is {len}")]
    InvalidRange { start: usize, end: usize, len: usize },
    #[error("empty transaction")]
    NoOp,
}

/// 一次可逆的文本变更。`span` 与所有长度均为 char index。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub span: Range<usize>,
    pub insert: String,
    pub removed: String,
    pub revision_before: Revision,
    pub revision_after: Revision,
}

impl Transaction {
    /// 构造逆操作：把插入的内容删掉，把删掉的内容放回。
    pub fn inverse(&self) -> (Range<usize>, String) {
        let inserted_len = self.insert.chars().count();
        (self.span.start..self.span.start + inserted_len, self.removed.clone())
    }
}

/// 撤销/重做栈。存的是 Transaction 而非文档快照，长文档下内存占用与变更量成正比。
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
}

impl History {
    pub(crate) fn push_undo(&mut self, tx: Transaction) {
        self.undo.push(tx);
        self.redo.clear();
    }

    pub(crate) fn push_undo_no_clear(&mut self, tx: Transaction) {
        self.undo.push(tx);
    }

    pub(crate) fn push_redo(&mut self, tx: Transaction) {
        self.redo.push(tx);
    }

    pub(crate) fn pop_undo(&mut self) -> Option<Transaction> {
        self.undo.pop()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<Transaction> {
        self.redo.pop()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
}

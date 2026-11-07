use std::ops::Range;

use ropey::Rope;

use crate::transaction::{CoreError, History, Transaction};

/// 单调递增的文档版本号。异步解析结果必须携带发起时的 Revision，
/// 回传时不一致即丢弃，避免慢解析覆盖新内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Revision(pub u64);

#[derive(Debug)]
pub struct Document {
    text: Rope,
    revision: Revision,
    history: History,
}

impl Document {
    pub fn new(text: impl AsRef<str>) -> Self {
        let mut rope = Rope::new();
        rope.insert(0, text.as_ref());
        Self {
            text: rope,
            revision: Revision(0),
            history: History::default(),
        }
    }

    pub fn text(&self) -> String {
        self.text.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    pub fn len_bytes(&self) -> usize {
        self.text.len_bytes()
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Markdown 层（pulldown-cmark）使用 byte offset，进入 core 前必须换算。
    pub fn byte_to_char(&self, byte: usize) -> usize {
        self.text.byte_to_char(byte.min(self.text.len_bytes()))
    }

    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        self.text.char_to_byte(char_idx.min(self.text.len_chars()))
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        self.text.slice(range).to_string()
    }

    /// 按 **byte** 区间取文本，非字符边界自动向左取整。
    /// 供「光标前一行源码前缀」这类 byte 语义的查询使用。
    pub fn slice_bytes(&self, range: Range<usize>) -> String {
        let len = self.text.len_bytes();
        let start = self.byte_to_char(range.start.min(len));
        let end = self.byte_to_char(range.end.min(len));
        if start >= end {
            return String::new();
        }
        self.text.slice(start..end).to_string()
    }

    pub fn can_undo(&self) -> bool {
        self.history.undo_len() > 0
    }

    pub fn can_redo(&self) -> bool {
        self.history.redo_len() > 0
    }

    fn apply_raw(&mut self, span: Range<usize>, insert: &str) -> Result<Transaction, CoreError> {
        let len = self.text.len_chars();
        if span.start > span.end || span.end > len {
            return Err(CoreError::InvalidRange { start: span.start, end: span.end, len });
        }

        let removed = self.slice(span.clone());
        self.text.remove(span.clone());
        if !insert.is_empty() {
            self.text.insert(span.start, insert);
        }
        self.revision.0 += 1;

        Ok(Transaction {
            span,
            insert: insert.to_owned(),
            removed,
            revision_before: Revision(self.revision.0 - 1),
            revision_after: self.revision,
        })
    }

    /// 应用一次编辑并入撤销栈。生产实现会在此处按时间窗 / 词边界合并连续输入。
    pub fn edit(&mut self, span: Range<usize>, insert: &str) -> Result<Transaction, CoreError> {
        if span.start == span.end && insert.is_empty() {
            return Err(CoreError::NoOp);
        }
        let tx = self.apply_raw(span, insert)?;
        self.history.push_undo(tx.clone());
        Ok(tx)
    }

    pub fn undo(&mut self) -> Option<Transaction> {
        let tx = self.history.pop_undo()?;
        let (span, text) = tx.inverse();
        let inverse = self.apply_raw(span, &text).ok()?;
        self.history.push_redo(inverse);
        Some(tx)
    }

    pub fn redo(&mut self) -> Option<Transaction> {
        let tx = self.history.pop_redo()?;
        let (span, text) = tx.inverse();
        let inverse = self.apply_raw(span, &text).ok()?;
        self.history.push_undo_no_clear(inverse);
        Some(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::{Anchor, Bias};

    #[test]
    fn edit_undo_redo_roundtrip() {
        let mut doc = Document::new("你好 world");
        let tx = doc.edit(0..0, "> ").unwrap();
        assert_eq!(doc.text(), "> 你好 world");
        assert_eq!(tx.revision_after, Revision(1));

        doc.undo().unwrap();
        assert_eq!(doc.text(), "你好 world");
        assert!(doc.can_redo());

        doc.redo().unwrap();
        assert_eq!(doc.text(), "> 你好 world");
        assert_eq!(doc.revision(), Revision(3));
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut doc = Document::new("abc");
        doc.edit(0..0, "X").unwrap();
        doc.undo().unwrap();
        assert!(doc.can_redo());
        doc.edit(3..3, "Y").unwrap();
        assert!(!doc.can_redo());
    }

    #[test]
    fn chinese_offsets_are_char_based() {
        let doc = Document::new("中文abc");
        assert_eq!(doc.len_chars(), 5);
        assert_eq!(doc.len_bytes(), 9);
        assert_eq!(doc.slice(0..2), "中文");
        assert_eq!(doc.byte_to_char(6), 2);
        assert_eq!(doc.char_to_byte(2), 6);
    }

    #[test]
    fn anchor_follows_edit() {
        let mut doc = Document::new("abc");
        let tx = doc.edit(0..0, "> ").unwrap();
        assert_eq!(Anchor::new(3, Bias::Right).shift(&tx).offset, 5);
        assert_eq!(Anchor::new(0, Bias::Left).shift(&tx).offset, 0);

        let replace = doc.edit(2..3, "Z").unwrap();
        assert_eq!(Anchor::new(5, Bias::Right).shift(&replace).offset, 5);
    }

    #[test]
    fn invalid_range_is_rejected() {
        let mut doc = Document::new("中文");
        assert!(matches!(
            doc.edit(0..9, "x"),
            Err(CoreError::InvalidRange { .. })
        ));
    }
}

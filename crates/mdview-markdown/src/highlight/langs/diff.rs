//! diff / patch 扫描器：按行判定文件头、hunk 头、新增行与删除行。
//!
//! 补丁的语义完全由行首字符决定，用通用词法器反而会把 `+ let x = 1;` 当代码
//! 扫一遍、丢掉增删语义，所以单独实现。整行染色（而不是只染 `+` / `-`）是为了
//! 让背景色像 GitHub 一样铺满整行。

use super::super::lexer::{Sink, for_each_line};
use super::super::{Token, TokenKind};

/// 补丁的元信息行前缀（只染到冒号/前缀为止，路径保持原色）。
const META_PREFIXES: &[&str] = &[
    "Binary files ",
    "copy from ",
    "copy to ",
    "deleted file mode ",
    "dissimilarity index ",
    "index ",
    "new file mode ",
    "new mode ",
    "old mode ",
    "rename from ",
    "rename to ",
    "similarity index ",
];

pub(crate) fn diff_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    for_each_line(code, |start, line| {
        if line.is_empty() {
            return;
        }
        match line.as_bytes()[0] {
            // `+++ b/path` / `--- a/path` 是文件头，不是增删行；统一按 `Tag` 染。
            b'+' if line.starts_with("+++") => {
                sink.push(start, start + line.len(), TokenKind::Tag);
            }
            b'-' if line.starts_with("---") => {
                sink.push(start, start + line.len(), TokenKind::Tag);
            }
            b'+' => sink.push(start, start + line.len(), TokenKind::Inserted),
            b'-' => sink.push(start, start + line.len(), TokenKind::Deleted),
            // `@@ -1,3 +1,4 @@` hunk 头。
            b'@' if line.starts_with("@@") => {
                sink.push(start, start + line.len(), TokenKind::Annotation);
            }
            // `diff --git a/x b/x` 分节头。
            b'd' if line.starts_with("diff ") => {
                sink.push(start, start + line.len(), TokenKind::Macro);
            }
            // `\ No newline at end of file`。
            b'\\' => sink.push(start, start + line.len(), TokenKind::Comment),
            _ => {
                if let Some(len) = META_PREFIXES
                    .iter()
                    .find(|p| line.starts_with(**p))
                    .map(|p| p.trim_end().len())
                {
                    sink.push(start, start + len, TokenKind::Comment);
                }
            }
        }
    });
    sink.finish()
}

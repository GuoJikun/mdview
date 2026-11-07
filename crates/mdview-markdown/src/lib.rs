//! 解析层：把 Markdown 源码投影成「可见片段 + 隐藏区间」。
//!
//! 设计前提是**源码为唯一真值**：渲染不做 Markdown → 富文本 → Markdown 的往返，
//! 而是把同一份字节流投影成视图。因此每个块除了正文，还带上：
//!
//! - `spans`：可见片段，每段携带原始 byte 区间与投影文本；
//! - `hidden`：被折叠的标记区间，由「块区间 − 片段区间」差集得出。
//!
//! 本层所有 `span` / `byte` 一律为 **byte offset**（与 pulldown-cmark 的
//! `into_offset_iter` 一致）。进入 mdview-core 或编辑器光标前必须换算为 char
//! index——`&str[非字符边界]` 会直接 panic，中文文档的坐标错位都出在这里。

mod block;
mod layout;
mod project;
mod sourcemap;
mod split;

pub use block::{
    align, Block, BlockId, BlockKind, OutlineItem, ParseResult, Span, SpanKind, Style,
};
pub use layout::{Caret, DocumentLayout, Gravity, Layout, Selection, Stop};
pub use project::{parse, MarkdownParser};
pub use sourcemap::SourceMap;
pub use split::split_blocks;

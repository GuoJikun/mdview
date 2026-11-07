//! 领域层：文档模型、可逆变更、位置锚点。
//!
//! 约束：本 crate 不得依赖任何 GUI 框架、文件系统或时钟。
//! 所有 `Range<usize>` / `offset` 在 core 内一律为 **char index**（ropey 语义）。
//! Markdown 解析器给出的 byte offset 必须在边界处换算后再进入本层。

mod document;
mod position;
mod transaction;

pub use document::{Document, Revision};
pub use position::{Anchor, Bias, Selection};
pub use transaction::{CoreError, History, Transaction};

//! 应用层：单向数据流（State / Message / update / Effect）+ 运行日志。
//!
//! 本层不含任何 GUI 类型。所有 I/O 与渲染经 ports trait 注入，
//! 因此状态机可以用内存实现做单元测试，UI 替换不影响本层。

mod ports;
mod state;

pub use mdview_support::{logging, init_logging, init_logging_default, AppError, LogConfig};
pub use ports::{
    CollectingSink, FileStore, Fingerprint, InMemoryFileStore, RenderFrame, RenderSink,
    StdFileStore, Watcher,
};
pub use state::{AppState, Effect, Message, ViewMode};

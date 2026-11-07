//! 运行期基础设施：日志与错误类型。
//!
//! 独立成 crate 的理由是层级：core 是领域，markdown 是解析，app 是状态机，
//! 这三层都可能需要写日志、都需要表达失败，但不该彼此依赖。
//! 把日志与错误下沉到最底层的 support，三层都能用，且不引入反向依赖。

pub mod error;
pub mod logging;

pub use error::AppError;
pub use logging::{LogConfig, init as init_logging, init_default as init_logging_default};

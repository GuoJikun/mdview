use std::path::PathBuf;

/// 基础设施层的统一失败类型：文件读写、路径解析、日志初始化。
///
/// 领域层的 `CoreError`（非法区间等）留在 `mdview-core`，不向这里合并——
/// 两者的消费者不同，混在一起会逼着纯领域代码依赖 IO 概念。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("path not found: {0}")]
    NotFound(PathBuf),
    #[error("log init failed: {0}")]
    Log(String),
}

impl AppError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, AppError::NotFound(_))
    }

    pub fn not_found(path: impl Into<PathBuf>) -> Self {
        AppError::NotFound(path.into())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_is_recognisable() {
        let err = AppError::not_found("a.md");
        assert!(err.is_not_found());
        assert_eq!(err.to_string(), "path not found: a.md");
    }

    #[test]
    fn io_error_converts() {
        let err: AppError = std::io::Error::other("boom").into();
        assert!(!err.is_not_found());
        assert!(err.to_string().contains("boom"));
    }
}

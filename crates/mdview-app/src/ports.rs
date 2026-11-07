use mdview_support::AppError;

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// 外部改动检测用的轻量指纹。生产实现应改用内容 hash + mtime 组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub u64);

pub trait FileStore {
    fn load(&self, path: &Path) -> Result<String, AppError>;
    fn save(&self, path: &Path, content: &str) -> Result<(), AppError>;
    fn fingerprint(&self, path: &Path) -> Result<Fingerprint, AppError>;
}

pub struct StdFileStore;

impl FileStore for StdFileStore {
    fn load(&self, path: &Path) -> Result<String, AppError> {
        Ok(fs::read_to_string(path)?)
    }

    fn save(&self, path: &Path, content: &str) -> Result<(), AppError> {
        // 原子写：先写临时文件再 rename。
        // 注意：Windows 的 std::fs::rename 无法覆盖已存在目标，
        // 生产实现应调用 MoveFileExW(MOVEFILE_REPLACE_EXISTING) 或 ReplaceFileW，
        // 避免此处 remove + rename 之间的窗口期。
        let tmp = path.with_extension("mdview.tmp");
        fs::write(&tmp, content)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    fn fingerprint(&self, path: &Path) -> Result<Fingerprint, AppError> {
        let meta = fs::metadata(path)?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut hasher = DefaultHasher::new();
        meta.len().hash(&mut hasher);
        modified.hash(&mut hasher);
        Ok(Fingerprint(hasher.finish()))
    }
}

/// 单元测试替身：用 RefCell 让 save 在 &self 下仍可写入。
#[derive(Default)]
pub struct InMemoryFileStore {
    files: RefCell<HashMap<PathBuf, String>>,
}

impl InMemoryFileStore {
    pub fn insert(&mut self, path: impl Into<PathBuf>, content: impl Into<String>) {
        self.files.get_mut().insert(path.into(), content.into());
    }
}

impl FileStore for InMemoryFileStore {
    fn load(&self, path: &Path) -> Result<String, AppError> {
        self.files
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| AppError::NotFound(path.to_path_buf()))
    }

    fn save(&self, path: &Path, content: &str) -> Result<(), AppError> {
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), content.to_owned());
        Ok(())
    }

    fn fingerprint(&self, path: &Path) -> Result<Fingerprint, AppError> {
        let files = self.files.borrow();
        let content = files
            .get(path)
            .ok_or_else(|| AppError::NotFound(path.to_path_buf()))?;
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        Ok(Fingerprint(hasher.finish()))
    }
}

/// UI 侧的渲染出口。Tauri 适配层把它转成 IPC 消息，egui 适配层直接绘制。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderFrame {
    pub revision: u64,
    pub html: String,
    pub blocks: usize,
    pub mode: &'static str,
}

pub trait RenderSink {
    fn render(&mut self, frame: &RenderFrame);
}

#[derive(Default)]
pub struct CollectingSink {
    pub frames: Vec<RenderFrame>,
}

impl RenderSink for CollectingSink {
    fn render(&mut self, frame: &RenderFrame) {
        self.frames.push(frame.clone());
    }
}

/// 生产实现由 notify crate 驱动，轮询/回调得到被外部修改的文件列表。
pub trait Watcher {
    fn poll(&mut self) -> Vec<PathBuf>;
}

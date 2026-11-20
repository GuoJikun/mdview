use mdview_support::AppError;

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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

#[derive(Clone, Copy, Default)]
pub struct StdFileStore;

impl FileStore for StdFileStore {
    fn load(&self, path: &Path) -> Result<String, AppError> {
        Ok(fs::read_to_string(path)?)
    }

    fn save(&self, path: &Path, content: &str) -> Result<(), AppError> {
        // 原子写：同目录临时文件 → 落盘 → 原子替换。
        // 临时文件与目标同目录，保证 rename/replace 在同一卷内，是原子操作。
        // 任一步失败都清理临时文件，且不掩盖原始错误。
        let tmp = path.with_extension("mdview.tmp");
        fs::write(&tmp, content)?;
        let replaced = atomic_replace(&tmp, path);
        if replaced.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        replaced
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

/// 把临时文件原子地替换到目标路径。
///
/// 关键：不能是「remove 目标 → rename 临时」这种两步操作——中间窗口期里
/// 目标文件不存在，读者会看到文件凭空消失。必须是单步、内核保证原子的替换。
#[cfg(windows)]
fn atomic_replace(tmp: &Path, target: &Path) -> Result<(), AppError> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "MoveFileExW"]
        fn move_file_ex_w(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    // 路径转 UTF-16 并补 NUL 结尾（Win32 W 系列 API 要求）
    let from: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: 两个指针都指向以 NUL 结尾、调用期间有效的 UTF-16 缓冲区；
    // flags 是有效常量组合。MOVEFILE_REPLACE_EXISTING 让内核原子覆盖目标。
    let ok = unsafe {
        move_file_ex_w(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(AppError::Io(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(tmp: &Path, target: &Path) -> Result<(), AppError> {
    // POSIX rename 语义：目标存在时原子替换，不存在时原子创建
    fs::rename(tmp, target).map_err(AppError::Io)
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

/// 由 notify 驱动的文件监听器：回调线程把变更路径压进队列，`poll()` 取走。
///
/// 观察策略：watch **父目录**而非文件本身。编辑器保存常走「写临时文件 →
/// rename 覆盖」，直接 watch 单文件会在 rename 的瞬间丢失监听目标；watch 目录
/// 再按路径过滤才不会漏。回调里只保留 Modify / Create / Remove，忽略 Access
/// （纯读不该触发冲突检查），并在 poll 时去重（一次保存常发多个事件）。
pub struct NotifyWatcher {
    _backend: notify::RecommendedWatcher,
    events: Arc<Mutex<VecDeque<PathBuf>>>,
    watched: PathBuf,
}

impl NotifyWatcher {
    /// 开始监听 `path` 所在目录，只关心 `path` 本身的变化。
    pub fn new(path: &Path) -> Result<Self, AppError> {
        let watched = path.to_path_buf();
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let sink = Arc::clone(&events);
        let target = watched.clone();

        use notify::Watcher as _;
        let mut backend = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                // 只有会改变内容的事件才值得上报；Access 一律忽略
                if !matches!(
                    event.kind,
                    notify::EventKind::Modify(_)
                        | notify::EventKind::Create(_)
                        | notify::EventKind::Remove(_)
                ) {
                    return;
                }
                if let Ok(mut q) = sink.lock() {
                    for p in event.paths {
                        if p == target {
                            q.push_back(p);
                        }
                    }
                }
            }
        })
        .map_err(|err| AppError::Log(format!("监听启动失败: {err}")))?;

        backend
            .watch(dir, notify::RecursiveMode::NonRecursive)
            .map_err(|err| AppError::Log(format!("监听目录失败 {}: {err}", dir.display())))?;

        Ok(Self {
            _backend: backend,
            events,
            watched,
        })
    }

    /// 构造一个只做去重、不真正监听的监听器，供单元测试验证过滤逻辑。
    #[cfg(test)]
    fn from_queue(events: VecDeque<PathBuf>, watched: PathBuf) -> (Self, Arc<Mutex<VecDeque<PathBuf>>>) {
        // 测试里没有真实后端，用一个永不触发的空壳占位；注意不要调用 poll 依赖后端的部分
        let shared = Arc::new(Mutex::new(events));
        (
            Self {
                _backend: notify::recommended_watcher(|_| {}).expect("空监听器构造失败"),
                events: Arc::clone(&shared),
                watched,
            },
            shared,
        )
    }

    pub fn path(&self) -> &Path {
        &self.watched
    }
}

/// 取走外部修改的路径列表（同步轮询语义，供状态机在合适时机调用）。
pub trait Watcher {
    fn poll(&mut self) -> Vec<PathBuf>;
}

impl Watcher for NotifyWatcher {
    fn poll(&mut self) -> Vec<PathBuf> {
        let Ok(mut q) = self.events.lock() else {
            return Vec::new();
        };
        let mut seen = Vec::new();
        while let Some(path) = q.pop_front() {
            if !seen.contains(&path) {
                seen.push(path);
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试用独立目录，避免并行跑测试时临时文件名相撞。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("mdview_{tag}_{nanos}_{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn save_overwrites_an_existing_file() {
        // 这是旧实现的核心缺陷：remove + rename 之间有窗口期，且 Windows
        // 上 std::fs::rename 遇到已存在目标会直接失败。新实现必须能稳定覆盖。
        let dir = TempDir::new("overwrite");
        let path = dir.file("note.md");
        fs::write(&path, "旧内容").unwrap();

        StdFileStore.save(&path, "新内容").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "新内容");
    }

    #[test]
    fn save_creates_a_new_file() {
        let dir = TempDir::new("create");
        let path = dir.file("fresh.md");

        StdFileStore.save(&path, "首次写入").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "首次写入");
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = TempDir::new("notmp");
        let path = dir.file("clean.md");
        StdFileStore.save(&path, "甲").unwrap();
        StdFileStore.save(&path, "乙").unwrap();

        let tmp = path.with_extension("mdview.tmp");
        assert!(!tmp.exists(), "替换成功后临时文件必须被消费掉");
        assert_eq!(fs::read_to_string(&path).unwrap(), "乙");
    }

    #[test]
    fn fingerprint_changes_when_content_changes() {
        let dir = TempDir::new("fp");
        let path = dir.file("fp.md");
        StdFileStore.save(&path, "甲").unwrap();
        let before = StdFileStore.fingerprint(&path).unwrap();

        StdFileStore.save(&path, "乙").unwrap();
        let after = StdFileStore.fingerprint(&path).unwrap();

        assert_ne!(before, after, "内容变了指纹必须变，外部冲突检测才有效");
    }

    #[test]
    fn watcher_poll_deduplicates_paths() {
        // 验证 poll 的取走 + 去重语义，不依赖真实的 OS 事件时序
        let path = PathBuf::from("note.md");
        let q = VecDeque::from(vec![
            path.clone(),
            path.clone(),
            PathBuf::from("other.md"),
            path.clone(),
        ]);
        let (mut watcher, _shared) = NotifyWatcher::from_queue(q, path.clone());

        let got = watcher.poll();
        assert_eq!(got, vec![path.clone(), PathBuf::from("other.md")]);
        // poll 是「取走」语义：再 poll 应当为空
        assert!(watcher.poll().is_empty());
    }

    #[test]
    fn watcher_detects_external_modification() {
        // 端到端：真实起监听 → 外部改文件 → poll 应上报该路径。
        // 用轮询等待而不是固定 sleep，避免慢机器上事件还没送到就断言。
        let dir = TempDir::new("watch");
        let path = dir.file("watched.md");
        fs::write(&path, "原始").unwrap();

        let mut watcher = match NotifyWatcher::new(&path) {
            Ok(w) => w,
            Err(err) => {
                // 某些 CI / 沙箱环境不允许文件监听，跳过而非误报失败
                eprintln!("跳过文件监听测试：{err}");
                return;
            }
        };
        assert_eq!(watcher.path(), path);

        // 外部修改（模拟另一个进程保存）
        fs::write(&path, "被外部改了").unwrap();

        let mut detected = false;
        for _ in 0..50 {
            if !watcher.poll().is_empty() {
                detected = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(detected, "外部修改应当被 Watcher 捕获");
    }
}

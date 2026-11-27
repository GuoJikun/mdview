use mdview_support::{
    AppError, TextEncoding,
    text::{self, decode as decode_text, encode as encode_text},
};

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

/// 真实磁盘上的文件存储：读入时判定编码，保存时按同一编码回写。
///
/// **为什么要记住编码**：内核内存态永远是 UTF-8 的 `String`，但用户的文件可能
/// 是 GBK。若保存时一律写 UTF-8，用户的文件会被悄悄改编码——diff、版本历史、
/// 别的编辑器里看全是改动。所以编码这份记忆挂在存储上：按**路径**记，因为
/// 编码属于文件，而不是属于某次编辑。
///
/// 这份表最多几条，用 `Arc<Mutex<..>>` 只是为了 `Clone` 能在多个 `Session`
/// 之间共享同一份记忆（同一份文件在多个窗口里打开时判定结果一致）。
#[derive(Clone, Default)]
pub struct StdFileStore {
    encodings: Arc<Mutex<HashMap<PathBuf, TextEncoding>>>,
}

impl StdFileStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 某个路径上次读入时的编码。没读过（新建 / 另存为的新路径）返回 `None`。
    pub fn encoding_of(&self, path: &Path) -> Option<TextEncoding> {
        self.encodings
            .lock()
            .ok()
            .and_then(|map| map.get(path).copied())
    }

    /// 记下编码（`restore` 这类「内容来自内存」的路径也要补记，否则恢复出来的
    /// GBK 文档会在下一次保存时变成 UTF-8）。
    pub fn remember_encoding(&self, path: &Path, encoding: TextEncoding) {
        if let Ok(mut map) = self.encodings.lock() {
            map.insert(path.to_path_buf(), encoding);
        }
    }

    /// 从磁盘再判定一次编码并记下（已经记过就不重读）。
    ///
    /// 给崩溃恢复用：副本里是 UTF-8 文本，而原文件可能是 GBK。读一次原文件的
    /// 字节、只做编码判定（不拿它的内容），代价是一份文件大小的顺序读。
    pub fn remember_from_disk(&self, path: &Path) {
        if self.encoding_of(path).is_some() {
            return;
        }
        if let Ok(bytes) = fs::read(path) {
            self.remember_encoding(path, text::detect(&bytes));
        }
    }
}

impl FileStore for StdFileStore {
    fn load(&self, path: &Path) -> Result<String, AppError> {
        let bytes = fs::read(path)?;
        // 先严格 UTF-8、读不成再退 GB18030；BOM 在这里一并剥掉——留着它会让
        // 首个 `# ` 不再被识别成标题、字数多算一个、保存时被当作内容写回。
        let (text, encoding) = decode_text(&bytes);
        self.remember_encoding(path, encoding);
        Ok(text)
    }

    fn save(&self, path: &Path, content: &str) -> Result<(), AppError> {
        // 按读入时的编码回写；没读过的新文件就是 UTF-8。
        let encoding = self.encoding_of(path).unwrap_or(TextEncoding::Utf8);
        // 原子写：同目录临时文件 → 落盘 → 原子替换。
        // 临时文件与目标同目录，保证 rename/replace 在同一卷内，是原子操作。
        // 任一步失败都清理临时文件，且不掩盖原始错误。
        let tmp = path.with_extension("mdview.tmp");
        fs::write(&tmp, encode_text(content, encoding))?;
        let replaced = atomic_replace(&tmp, path);
        if replaced.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        replaced
    }

    fn fingerprint(&self, path: &Path) -> Result<Fingerprint, AppError> {
        // 文件不存在要报 NotFound，不能混进 Io：上层据此区分「被删了，
        // 允许写回重建」与「读不了，状态未知，保守按冲突处理」。
        let meta = fs::metadata(path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(path)
            } else {
                AppError::Io(err)
            }
        })?;
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

    /// 模拟外部删除：之后 `load` / `fingerprint` 都返回 `NotFound`。
    pub fn remove(&mut self, path: impl AsRef<Path>) {
        self.files.get_mut().remove(path.as_ref());
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
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
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
    fn from_queue(
        events: VecDeque<PathBuf>,
        watched: PathBuf,
    ) -> (Self, Arc<Mutex<VecDeque<PathBuf>>>) {
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
            let dir =
                std::env::temp_dir().join(format!("mdview_{tag}_{nanos}_{}", std::process::id()));
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
    fn gbk_file_is_written_back_as_gbk() {
        // 「标」的 GBK 字节是 B1 EA。读进来是 UTF-8 字符，存回去必须还是这两个字节。
        let dir = TempDir::new("gbk");
        let path = dir.file("旧稿.md");
        fs::write(&path, [0x23u8, 0x20, 0xB1, 0xEA]).unwrap(); // "# 标"

        let store = StdFileStore::new();
        assert_eq!(store.load(&path).unwrap(), "# 标");
        assert_eq!(store.encoding_of(&path), Some(TextEncoding::Gb18030));

        store.save(&path, "# 标题").unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            encode_text("# 标题", TextEncoding::Gb18030),
            "保存要按读入时的编码回写"
        );
        assert!(
            std::str::from_utf8(&fs::read(&path).unwrap()).is_err(),
            "回写后仍是 GBK 字节，不该被悄悄改成 UTF-8"
        );
    }

    #[test]
    fn a_file_that_was_never_read_is_written_as_utf8() {
        // 另存为的新路径没有读入记录，按 UTF-8 写是唯一合理的默认。
        let dir = TempDir::new("freshenc");
        let path = dir.file("新稿.md");
        let store = StdFileStore::new();
        assert_eq!(store.encoding_of(&path), None);

        store.save(&path, "中文").unwrap();
        assert_eq!(fs::read(&path).unwrap(), "中文".as_bytes());
    }

    #[test]
    fn remember_from_disk_picks_up_the_file_encoding() {
        // 崩溃恢复走这条：内容来自内存副本，编码得现读一次原文件才知道。
        let dir = TempDir::new("recoverenc");
        let path = dir.file("旧稿.md");
        fs::write(&path, [0xB1u8, 0xEA]).unwrap(); // "标"

        let store = StdFileStore::new();
        assert_eq!(store.encoding_of(&path), None);
        store.remember_from_disk(&path);
        assert_eq!(store.encoding_of(&path), Some(TextEncoding::Gb18030));

        // 已经记过就不重读：文件删了也不会把已有记忆抹掉
        fs::remove_file(&path).unwrap();
        store.remember_from_disk(&path);
        assert_eq!(store.encoding_of(&path), Some(TextEncoding::Gb18030));
    }

    #[test]
    fn save_overwrites_an_existing_file() {
        // 这是旧实现的核心缺陷：remove + rename 之间有窗口期，且 Windows
        // 上 std::fs::rename 遇到已存在目标会直接失败。新实现必须能稳定覆盖。
        let dir = TempDir::new("overwrite");
        let path = dir.file("note.md");
        fs::write(&path, "旧内容").unwrap();

        StdFileStore::new().save(&path, "新内容").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "新内容");
    }

    #[test]
    fn save_creates_a_new_file() {
        let dir = TempDir::new("create");
        let path = dir.file("fresh.md");

        StdFileStore::new().save(&path, "首次写入").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "首次写入");
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = TempDir::new("notmp");
        let path = dir.file("clean.md");
        StdFileStore::new().save(&path, "甲").unwrap();
        StdFileStore::new().save(&path, "乙").unwrap();

        let tmp = path.with_extension("mdview.tmp");
        assert!(!tmp.exists(), "替换成功后临时文件必须被消费掉");
        assert_eq!(fs::read_to_string(&path).unwrap(), "乙");
    }

    #[test]
    fn fingerprint_changes_when_content_changes() {
        let dir = TempDir::new("fp");
        let path = dir.file("fp.md");
        StdFileStore::new().save(&path, "甲").unwrap();
        let before = StdFileStore::new().fingerprint(&path).unwrap();

        StdFileStore::new().save(&path, "乙").unwrap();
        let after = StdFileStore::new().fingerprint(&path).unwrap();

        assert_ne!(before, after, "内容变了指纹必须变，外部冲突检测才有效");
    }

    #[test]
    fn load_strips_a_utf8_bom() {
        // BOM 解码后是正文首字符，会让首个 `# ` 认不出来，保存时还被写回去。
        let dir = TempDir::new("bom");
        let path = dir.file("bom.md");
        fs::write(&path, "\u{feff}# 标题\n").unwrap();
        assert_eq!(StdFileStore::new().load(&path).unwrap(), "# 标题\n");
    }

    #[test]
    fn fingerprint_reports_a_missing_file_as_not_found() {
        // 上层靠 NotFound 区分「文件被删，可以写回重建」和「读不了，按冲突处理」。
        let dir = TempDir::new("fpmiss");
        let err = StdFileStore::new()
            .fingerprint(&dir.file("gone.md"))
            .expect_err("文件不存在应当报错");
        assert!(err.is_not_found(), "实际错误：{err}");
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

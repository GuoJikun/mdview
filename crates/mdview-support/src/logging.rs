//! 运行日志（mdview-support）：落盘到应用同级目录的 `logs/`，
//! error 单独一个文件，其余按大小滚动。
//!
//! 落盘策略：
//! - `logs/mdview.log` + `mdview.1.log` … `mdview.4.log`：非 error 级别，5 个文件 × 1 MB
//! - `logs/error.log` + `error.1.log` … `error.4.log`：仅 error 级别，同样 5 个文件 × 1 MB
//!
//! error 是**独占**的：一条 error 不会同时写进 mdview.log，避免两张表里重复计数。
//!
//! 写入是同步的（不缓冲、不起后台线程）：日志量级远低于磁盘顺序写上限，
//! 换来的是进程崩溃时最后一条日志不丢。

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use log::{Level, LevelFilter, Log, Metadata, Record};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024;
pub const DEFAULT_MAX_FILES: usize = 5;

const APP_STEM: &str = "mdview";
const ERROR_STEM: &str = "error";
const EXT: &str = "log";

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub dir: PathBuf,
    pub max_bytes: u64,
    /// 含当前文件在内的文件总数
    pub max_files: usize,
    pub level: LevelFilter,
}

impl LogConfig {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            max_bytes: DEFAULT_MAX_BYTES,
            max_files: DEFAULT_MAX_FILES,
            level: if cfg!(debug_assertions) {
                LevelFilter::Debug
            } else {
                LevelFilter::Info
            },
        }
    }
}

/// 应用同级目录下的 `logs`。
pub fn default_log_dir() -> PathBuf {
    match std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        Some(dir) => dir.join("logs"),
        None => PathBuf::from("logs"),
    }
}

/// 同级目录不可写时的兜底位置（装在 Program Files 之类只读目录下会遇到）。
fn fallback_log_dir() -> PathBuf {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(base).join("mdview").join("logs");
    }
    std::env::temp_dir().join("mdview").join("logs")
}

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

/// 安装日志器。重复调用是幂等的（第二次直接返回）。
pub fn init(config: LogConfig) -> std::io::Result<()> {
    let logger = FileLogger::new(&config)?;
    if LOGGER.set(logger).is_err() {
        return Ok(());
    }
    log::set_logger(LOGGER.get().expect("刚写入的日志器必然存在"))
        .map_err(|_| std::io::Error::other("全局日志器已被外部占用"))?;
    log::set_max_level(config.level);
    Ok(())
}

/// 按默认约定初始化，返回实际使用的目录。
pub fn init_default() -> std::io::Result<PathBuf> {
    let primary = default_log_dir();
    let dir = match fs::create_dir_all(&primary) {
        Ok(()) => primary,
        Err(_) => {
            let fallback = fallback_log_dir();
            fs::create_dir_all(&fallback)?;
            fallback
        }
    };
    init(LogConfig::new(dir.clone()))?;
    Ok(dir)
}

struct FileLogger {
    app: Mutex<RotatingWriter>,
    error: Mutex<RotatingWriter>,
    /// 本实例的门槛。不用全局 max_level 判断，否则未调用 init 时一律被丢弃。
    level: LevelFilter,
}

impl FileLogger {
    fn new(config: &LogConfig) -> std::io::Result<Self> {
        Ok(Self {
            app: Mutex::new(RotatingWriter::open(
                &config.dir,
                APP_STEM,
                config.max_bytes,
                config.max_files,
            )?),
            error: Mutex::new(RotatingWriter::open(
                &config.dir,
                ERROR_STEM,
                config.max_bytes,
                config.max_files,
            )?),
            level: config.level,
        })
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(record);
        let sink = if record.level() == Level::Error {
            &self.error
        } else {
            &self.app
        };
        if let Ok(mut writer) = sink.lock() {
            let _ = writer.write_line(&line);
        }
    }

    fn flush(&self) {
        for sink in [&self.app, &self.error] {
            if let Ok(mut writer) = sink.lock() {
                let _ = writer.flush();
            }
        }
    }
}

fn format_line(record: &Record<'_>) -> String {
    let message = record
        .args()
        .to_string()
        .replace('\r', "")
        .replace('\n', "\\n");
    let location = match (record.file(), record.line()) {
        (Some(file), Some(line)) => format!(" ({file}:{line})"),
        (Some(file), None) => format!(" ({file})"),
        _ => String::new(),
    };
    format!(
        "{} {:<5} [{}] {}{}\n",
        timestamp(),
        record.level(),
        record.target(),
        message,
        location
    )
}

fn timestamp() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    now.format(&Rfc3339).unwrap_or_else(|_| "unknown-time".to_owned())
}

/// 按大小滚动的单文件写入器。文件名形如 `mdview.log` / `mdview.1.log`。
struct RotatingWriter {
    dir: PathBuf,
    stem: &'static str,
    max_bytes: u64,
    max_files: usize,
    file: File,
    size: u64,
}

impl RotatingWriter {
    fn open(dir: &Path, stem: &'static str, max_bytes: u64, max_files: usize) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{stem}.{EXT}"));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            dir: dir.to_path_buf(),
            stem,
            max_bytes,
            max_files: max_files.max(1),
            file,
            size,
        })
    }

    fn base_path(&self) -> PathBuf {
        self.dir.join(format!("{}.{EXT}", self.stem))
    }

    fn archive_path(&self, index: usize) -> PathBuf {
        self.dir.join(format!("{}.{index}.{EXT}", self.stem))
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let bytes = line.as_bytes();
        // 单条超过上限时不再切分，直接整条写入，否则会无限滚动
        if self.size > 0 && self.size + bytes.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        self.file.write_all(bytes)?;
        self.size += bytes.len() as u64;
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        let base = self.base_path();
        if self.max_files >= 2 {
            // 先删最旧的，再从后往前错位，最后把当前文件挪到 .1
            let _ = fs::remove_file(self.archive_path(self.max_files - 1));
            for index in (1..self.max_files - 1).rev() {
                let from = self.archive_path(index);
                if from.exists() {
                    let to = self.archive_path(index + 1);
                    let _ = fs::remove_file(&to);
                    fs::rename(&from, &to)?;
                }
            }
            if base.exists() {
                let first = self.archive_path(1);
                let _ = fs::remove_file(&first);
                fs::rename(&base, &first)?;
            }
        } else {
            let _ = fs::remove_file(&base);
        }
        self.file = OpenOptions::new().create(true).append(true).open(&base)?;
        self.size = 0;
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Record;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("mdview-log-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(logger: &FileLogger, level: Level, message: &str) {
        // Record 借用 format_args 的临时量，必须在同一个语句内消费掉
        logger.log(
            &Record::builder()
                .args(format_args!("{message}"))
                .level(level)
                .target("mdview::test")
                .build(),
        );
    }

    #[test]
    fn writes_info_to_app_file_and_error_to_separate_file() {
        let dir = temp_dir("split");
        let logger = FileLogger::new(&LogConfig::new(&dir)).unwrap();

        write(&logger, Level::Info, "普通消息");
        write(&logger, Level::Error, "错误消息");

        let app = fs::read_to_string(dir.join("mdview.log")).unwrap();
        let err = fs::read_to_string(dir.join("error.log")).unwrap();
        assert!(app.contains("普通消息"));
        assert!(!app.contains("错误消息"), "error 不应重复写入 mdview.log");
        assert!(err.contains("错误消息"));
        assert!(!err.contains("普通消息"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rotates_when_exceeding_max_bytes() {
        let dir = temp_dir("rotate");
        let mut config = LogConfig::new(&dir);
        config.max_bytes = 256;
        config.max_files = 5;
        let logger = FileLogger::new(&config).unwrap();

        for i in 0..200 {
            write(&logger, Level::Info, &format!("第 {i} 条用于触发滚动的填充日志"));
        }

        // error.log 是空文件，不属于本次滚动统计
        let mut files: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("mdview."))
            .collect();
        files.sort();
        assert_eq!(
            files,
            vec![
                "mdview.1.log",
                "mdview.2.log",
                "mdview.3.log",
                "mdview.4.log",
                "mdview.log"
            ],
            "恰好 5 个文件，文件名按序"
        );

        for name in &files {
            let size = fs::metadata(dir.join(name)).unwrap().len();
            assert!(size <= 256 + 128, "{name} 超过上限：{size}");
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn max_files_one_keeps_single_file() {
        let dir = temp_dir("single");
        let mut config = LogConfig::new(&dir);
        config.max_bytes = 64;
        config.max_files = 1;
        let logger = FileLogger::new(&config).unwrap();
        for i in 0..20 {
            write(&logger, Level::Info, &format!("第 {i} 条填充日志内容"));
        }
        let count = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("mdview."))
            .count();
        assert_eq!(count, 1, "max_files=1 时只保留当前文件");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn multiline_message_stays_on_one_line() {
        let dir = temp_dir("multiline");
        let logger = FileLogger::new(&LogConfig::new(&dir)).unwrap();
        write(&logger, Level::Warn, "第一行\n第二行");
        let text = fs::read_to_string(dir.join("mdview.log")).unwrap();
        assert_eq!(text.lines().count(), 1, "多行消息必须压成单行，否则日志解析会错位");
        assert!(text.contains("第一行\\n第二行"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn appends_to_existing_file_on_reopen() {
        let dir = temp_dir("append");
        {
            let logger = FileLogger::new(&LogConfig::new(&dir)).unwrap();
            write(&logger, Level::Info, "第一条");
        }
        {
            let logger = FileLogger::new(&LogConfig::new(&dir)).unwrap();
            write(&logger, Level::Info, "第二条");
        }
        let text = fs::read_to_string(dir.join("mdview.log")).unwrap();
        assert!(text.contains("第一条") && text.contains("第二条"), "重启后不能清空历史日志");
        fs::remove_dir_all(&dir).ok();
    }
}

//! 崩溃副本：脏文档的定时快照，用来兜住「强杀 / 断电 / 崩溃」这类不经过
//! 正常保存流程的退出。
//!
//! 原则三条：
//! 1. **只写副本，绝不动原文件**——自动写原文件是另一件事（见偏好里的自动保存），
//!    混在一起会让「我没按保存，文件怎么变了」变成无法解释的行为。
//! 2. **不经过 watcher**：副本放在配置目录，不在文档目录，`notify` 看不到它，
//!    不会误触发「文件已被外部修改」。
//! 3. **保存成功后自清理**：正常保存会删掉副本，所以「副本存在」本身
//!    就等于「上次退出时还有没保存的内容」。
//!
//! 目录结构：`<config>/backup/<id>.md` + `<id>.json`。`id` 由文档路径哈希而来；
//! 未命名文档用 `untitled-<窗口标签>`，同一窗口的未命名内容下次启动还能对上。

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// 副本的元数据。`disk_mtime` 用来判断「副本是不是已经过时」：
/// 备份之后文件又被别处改过，就不该拿旧内容盖上去。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BackupMeta {
    /// 存的是哪份文档（未命名时为 None）。
    pub path: Option<String>,
    /// 窗口标签，仅未命名文档用得上。
    pub label: String,
    /// 写入副本的时间（Unix 秒）。
    pub saved_at: u64,
    /// 备份时原文件的修改时间（Unix 秒）。文件不存在时为 None。
    pub disk_mtime: Option<u64>,
}

fn backup_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_config_dir().ok()?.join("backup");
    if let Err(err) = fs::create_dir_all(&dir) {
        log::warn!("备份目录创建失败 {}: {err}", dir.display());
        return None;
    }
    Some(dir)
}

/// 文档标识：路径的 FNV-1a 十六进制，未命名文档退回窗口标签。
///
/// 不用 `DefaultHasher`：它的算法不保证跨版本稳定，升级一次 Rust 就可能
/// 换个值，用户的崩溃副本会集体失联。
fn doc_id(label: &str, path: Option<&Path>) -> String {
    let Some(path) = path else {
        return format!("untitled-{}", sanitize(label));
    };
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 窗口标签要进文件名，去掉路径分隔符与空白。
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

fn unix_secs(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

fn disk_mtime(path: Option<&Path>) -> Option<u64> {
    let meta = fs::metadata(path?).ok()?;
    unix_secs(meta.modified().ok()?)
}

fn paths(dir: &Path, label: &str, path: Option<&Path>) -> (PathBuf, PathBuf) {
    let id = doc_id(label, path);
    (dir.join(format!("{id}.md")), dir.join(format!("{id}.json")))
}

/// 写一份副本。先写临时文件再改名：写到一半被杀掉时，旧副本还在，
/// 不会留下半截正文被下次启动当成完整内容恢复。
fn save_in(dir: &Path, label: &str, path: Option<&Path>, text: &str) -> Result<(), String> {
    let (body_path, meta_path) = paths(dir, label, path);
    let meta = BackupMeta {
        path: path.map(|p| p.display().to_string()),
        label: label.to_owned(),
        saved_at: unix_secs(SystemTime::now()).unwrap_or_default(),
        disk_mtime: disk_mtime(path),
    };
    let json = serde_json::to_string_pretty(&meta).map_err(|err| err.to_string())?;

    let tmp = body_path.with_extension("md.tmp");
    fs::write(&tmp, text).map_err(|err| format!("写入副本失败：{err}"))?;
    fs::rename(&tmp, &body_path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        format!("替换副本失败：{err}")
    })?;
    fs::write(&meta_path, json).map_err(|err| format!("写入副本信息失败：{err}"))
}

/// 读副本。返回 (元数据, 正文)。文件不全或解析失败一律当作没有副本。
fn load_in(dir: &Path, label: &str, path: Option<&Path>) -> Option<(BackupMeta, String)> {
    let (body_path, meta_path) = paths(dir, label, path);
    let meta: BackupMeta = serde_json::from_str(&fs::read_to_string(meta_path).ok()?).ok()?;
    let text = fs::read_to_string(body_path).ok()?;
    Some((meta, text))
}

fn discard_in(dir: &Path, label: &str, path: Option<&Path>) {
    let (body_path, meta_path) = paths(dir, label, path);
    let _ = fs::remove_file(body_path);
    let _ = fs::remove_file(meta_path);
}

/// 写一份副本（应用配置目录下）。
pub(crate) fn save(
    app: &AppHandle,
    label: &str,
    path: Option<&Path>,
    text: &str,
) -> Result<(), String> {
    let dir = backup_dir(app).ok_or("备份目录不可用")?;
    save_in(&dir, label, path, text)
}

/// 删掉副本（正常保存后、或用户选择丢弃时）。
pub(crate) fn discard(app: &AppHandle, label: &str, path: Option<&Path>) {
    let Some(dir) = backup_dir(app) else {
        return;
    };
    discard_in(&dir, label, path);
}

/// 副本是否还有效。
///
/// 只比「备份时记下的磁盘修改时间」与「现在的磁盘修改时间」：磁盘变新了说明
/// 用户在别处保存过，这份副本只会把旧内容盖回去。两种情况例外——
/// 备份时文件还不存在（新建未保存）、或现在文件被删了——副本都值得恢复。
fn is_current(recorded: Option<u64>, current: Option<u64>) -> bool {
    match (recorded, current) {
        (Some(recorded), Some(current)) => current <= recorded,
        _ => true,
    }
}

/// 待恢复的副本：存在、有内容，且不比磁盘旧。
pub(crate) fn pending(
    app: &AppHandle,
    label: &str,
    path: Option<&Path>,
) -> Option<(BackupMeta, String)> {
    pending_in(&backup_dir(app)?, label, path)
}

fn pending_in(dir: &Path, label: &str, path: Option<&Path>) -> Option<(BackupMeta, String)> {
    let (meta, text) = load_in(dir, label, path)?;
    if text.is_empty() {
        return None;
    }
    if !is_current(meta.disk_mtime, disk_mtime(path)) {
        log::info!("副本已过时（文件在备份后被改过），跳过");
        return None;
    }
    Some((meta, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同一路径必须给出同一个 id，否则重启后找不到自己的副本。
    #[test]
    fn doc_id_is_stable_and_distinguishes_paths() {
        let a = doc_id("main", Some(Path::new("C:/docs/a.md")));
        let b = doc_id("main", Some(Path::new("C:/docs/a.md")));
        let c = doc_id("main", Some(Path::new("C:/docs/b.md")));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
    }

    /// 未命名文档按窗口标签区分，且标签里的非法字符不会跑进文件名。
    #[test]
    fn untitled_backups_are_keyed_by_window_label() {
        assert_eq!(doc_id("main", None), "untitled-main");
        assert_eq!(doc_id("a/b:c", None), "untitled-a-b-c");
    }

    /// 副本的有效性只看磁盘有没有在备份之后变新。
    #[test]
    fn backup_is_stale_once_the_file_changes() {
        assert!(is_current(Some(100), Some(100)), "磁盘没变，副本有效");
        assert!(is_current(Some(100), Some(90)), "磁盘更旧，副本有效");
        assert!(!is_current(Some(100), Some(101)), "磁盘变新，副本过时");
        assert!(is_current(None, Some(101)), "备份时文件还不存在，照样恢复");
        assert!(is_current(Some(100), None), "文件被删了，副本更该恢复");
    }

    /// 完整走一遍：写副本 → 读回来 → 判定可恢复 → 保存后自清理。
    ///
    /// 这是「崩溃后能救回正文」的唯一可自动化部分：真正的强杀没法在测试里演，
    /// 但副本的落盘、读取与有效性判定可以逐条钉住。
    #[test]
    fn backup_round_trip_and_cleanup() {
        let dir = std::env::temp_dir().join(format!("mdview-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let doc = dir.join("稿.md");
        std::fs::write(&doc, "磁盘上的旧内容").unwrap();

        save_in(&dir, "main", Some(&doc), "崩溃前的一千字").expect("写副本");
        let (meta, text) = load_in(&dir, "main", Some(&doc)).expect("读副本");
        assert_eq!(text, "崩溃前的一千字");
        assert_eq!(meta.path.as_deref(), Some(doc.to_string_lossy().as_ref()));
        assert!(
            pending_in(&dir, "main", Some(&doc)).is_some(),
            "磁盘没变，应可恢复"
        );

        // 文件在备份之后被别处改过：副本过时，不能拿旧内容盖回去
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&doc, "别处保存过的新内容").unwrap();
        assert!(
            pending_in(&dir, "main", Some(&doc)).is_none(),
            "磁盘变新后副本必须失效"
        );

        // 正常保存会自清理：副本存在就等于「上次没保存」
        discard_in(&dir, "main", Some(&doc));
        assert!(load_in(&dir, "main", Some(&doc)).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 未命名文档也要能备份与恢复——它连原文件都没有，是崩溃副本唯一兜底的场景。
    #[test]
    fn untitled_documents_get_backups_too() {
        let dir = std::env::temp_dir().join(format!("mdview-backup-u-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        save_in(&dir, "main", None, "还没起名字的一段").expect("写副本");
        let (meta, text) = pending_in(&dir, "main", None).expect("应可恢复");
        assert_eq!(text, "还没起名字的一段");
        assert!(meta.path.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

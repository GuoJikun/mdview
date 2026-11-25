//! 最近打开文件的持久化列表。
//!
//! 只存路径，不存内容；上限 [`MAX`] 条，最近的排在最前。放在应用配置目录下的
//! `recent.json`，与窗口几何分开（那份由 window-state 插件管，格式不归我们）。
//!
//! 这里刻意不依赖 `Session`：记录「用户打开过什么」是宿主的职责，内核只管当前
//! 这一份文档。文件读写失败一律静默降级——最近列表坏掉不该影响打开文件本身。

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use tauri::{AppHandle, Manager};

/// 最多保留多少条。够回访常用文档，又不至于把菜单撑成一屏。
pub(crate) const MAX: usize = 10;

/// 读改写要串行化：打开文件跑在工作线程、另存为走在主线程，两个窗口几乎
/// 同时保存时，无锁的 read-modify-write 会互相覆盖，丢掉中间那一条。
static STORE_LOCK: Mutex<()> = Mutex::new(());

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_config_dir().ok()?;
    if let Err(err) = fs::create_dir_all(&dir) {
        log::warn!("最近文件目录创建失败 {}: {err}", dir.display());
        return None;
    }
    Some(dir.join("recent.json"))
}

fn read_from(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_else(|err| {
        log::warn!("最近文件列表解析失败 {}: {err}", path.display());
        Vec::new()
    })
}

fn write_to(path: &Path, items: &[String]) {
    let Ok(text) = serde_json::to_string_pretty(items) else {
        return;
    };
    // 同目录临时文件 + 原子替换。直接 fs::write 会先截断目标，写到一半
    // 崩溃就留下半截 JSON，下次解析失败被当成空表，整份历史随之丢失。
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    if let Err(err) = fs::write(&tmp, text) {
        log::warn!("最近文件列表写入失败 {}: {err}", tmp.display());
        let _ = fs::remove_file(&tmp);
        return;
    }
    if let Err(err) = fs::rename(&tmp, path) {
        log::warn!("最近文件列表替换失败 {}: {err}", path.display());
        let _ = fs::remove_file(&tmp);
    }
}

/// 把一条路径插到最前，去重并截断。纯函数，单测直接覆盖。
pub(crate) fn push_unique(items: &mut Vec<String>, path: String) {
    if path.trim().is_empty() {
        return;
    }
    items.retain(|old| old != &path);
    items.insert(0, path);
    items.truncate(MAX);
}

/// 当前列表，最近的在前。读不到就是空表。
pub fn list(app: &AppHandle) -> Vec<String> {
    store_path(app)
        .map(|path| read_from(&path))
        .unwrap_or_default()
}

/// 记住一次打开 / 另存为。重复路径会提到最前，不会出现两次。
pub fn remember(app: &AppHandle, path: &Path) {
    let Some(store) = store_path(app) else {
        return;
    };
    // 锁中毒也继续用（取值），否则一次 panic 会让最近列表永久不再更新。
    let _guard = STORE_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let mut items = read_from(&store);
    push_unique(&mut items, path.display().to_string());
    write_to(&store, &items);
}

/// 清空列表（菜单里的「清除记录」）。
pub fn clear(app: &AppHandle) {
    let Some(store) = store_path(app) else {
        return;
    };
    let _guard = STORE_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    write_to(&store, &[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_path_goes_first() {
        let mut items = Vec::new();
        push_unique(&mut items, "b.md".into());
        push_unique(&mut items, "a.md".into());
        assert_eq!(items, vec!["a.md", "b.md"]);
    }

    #[test]
    fn reopening_moves_to_front_without_duplicating() {
        let mut items = vec!["a.md".into(), "b.md".into(), "c.md".into()];
        push_unique(&mut items, "c.md".into());
        assert_eq!(items, vec!["c.md", "a.md", "b.md"]);
    }

    #[test]
    fn list_is_capped() {
        let mut items = Vec::new();
        for i in 0..(MAX + 5) {
            push_unique(&mut items, format!("{i}.md"));
        }
        assert_eq!(items.len(), MAX);
        assert_eq!(items[0], format!("{}.md", MAX + 4));
    }

    #[test]
    fn blank_paths_are_ignored() {
        let mut items = vec!["a.md".into()];
        push_unique(&mut items, "   ".into());
        push_unique(&mut items, String::new());
        assert_eq!(items, vec!["a.md"]);
    }

    #[test]
    fn atomic_write_round_trips_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!(
            "mdview-recent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("recent.json");

        let items = vec!["a.md".to_string(), "b.md".to_string()];
        write_to(&path, &items);

        assert_eq!(read_from(&path), items);
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        assert!(!tmp.exists(), "替换成功后临时文件必须被消费掉");
        let _ = fs::remove_dir_all(&dir);
    }
}

//! 工作区目录遍历（文件侧栏的后端）。
//!
//! 只做一件事：把某个目录下的「Markdown 文件 + 子目录」列出来。**一次只列一层**，
//! 展开某个子目录时再问一次——递归扫一遍整棵树的代价不可控（用户的笔记目录可能
//! 挂在网络盘上、可能塞了几万个文件），而一次 `read_dir` 总是很快。
//!
//! 过滤规则是白名单式的：目录一律收（哪怕眼下没有 md，用户随时可能往里加），
//! 文件只收 `.md` / `.markdown` / `.txt`。隐藏项与几个「一打开就是几万文件」的
//! 工程目录（`node_modules` / `target` / `dist` / `build` 等）直接跳过。

use std::path::{Path, PathBuf};

/// 单层最多列出多少项。超出的部分不列，并在 `FolderListing.truncated` 里说明——
/// 真实的 `read_dir` 上万条不慢，慢的是把它们全塞进 DOM。
pub const MAX_ENTRIES: usize = 1000;

/// 目录里不值得展开的名字。判的是目录名本身，不是路径。
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "vendor",
    "Library",
    "AppData",
];

/// 侧栏里的一行。`dir` 为真时前端画成可展开的目录。
///
/// 路径一律给**绝对路径字符串**：前端不需要知道它是怎么拼出来的，打开时原样
/// 交回 `open_file` 即可。Windows 上转 `to_string_lossy` 会保留反斜杠，前后端
/// 之间只是当标识用，不参与任何拼接。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub dir: bool,
}

/// 一次目录查询的结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct FolderListing {
    pub path: String,
    pub name: String,
    pub entries: Vec<Entry>,
    /// 是否因为超过 [`MAX_ENTRIES`] 而截断。
    pub truncated: bool,
}

/// 这个扩展名算不算「可编辑的文本」。与 `open_file` 的过滤器保持同一份白名单。
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt"))
}

/// 列出 `dir` 的直接子项：子目录（不含隐藏/工程目录）+ Markdown 文件。
///
/// 排序是「目录在前、文件在后，各自按名字不区分大小写」——这是资源管理器与
/// VSCode 的共同习惯，用户不用重新学。
pub fn read_dir(dir: &Path) -> Result<FolderListing, String> {
    let meta = std::fs::metadata(dir).map_err(|err| format!("读不到目录：{err}"))?;
    if !meta.is_dir() {
        return Err("这个路径不是目录".to_string());
    }
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.display().to_string());

    let iter = std::fs::read_dir(dir).map_err(|err| format!("读不到目录：{err}"))?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for item in iter {
        // 单个子项读失败（权限、刚好被删）就跳过，不该让整次展开失败
        let Ok(item) = item else { continue };
        let path = item.path();
        let Some(file_name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        let Ok(ft) = item.file_type() else { continue };
        // 符号链接按目标判定：指向目录的链接也该能展开
        let is_dir = if ft.is_symlink() {
            std::fs::metadata(&path)
                .map(|m| m.is_dir())
                .unwrap_or(false)
        } else {
            ft.is_dir()
        };
        // 隐藏的东西一律不列（目录与文件同理）：`.git` / `.obsidian` 这类是
        // 工具自己的元数据，`.隐藏.md` 也不像是要编辑的正文。
        if file_name.starts_with('.') {
            continue;
        }
        if is_dir {
            if SKIP_DIRS.contains(&file_name.as_str()) {
                continue;
            }
        } else if !is_markdown(&path) {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(Entry {
            name: file_name,
            path: path.to_string_lossy().to_string(),
            dir: is_dir,
        });
    }
    sort_entries(&mut entries);

    Ok(FolderListing {
        path: dir.to_string_lossy().to_string(),
        name,
        entries,
        truncated,
    })
}

/// 目录在前、文件在后，各自按名字不区分大小写排序（稳定：同名时保持原序）。
fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// 把路径规范成绝对路径。
///
/// 用 `std::path::absolute` 而不是 `canonicalize`：后者在 Windows 上会加上
/// `\\?\` 前缀（`\\?\E:\笔记`），路径会原样显示在侧栏提示与标题栏里；它还会
/// 解析符号链接与 8.3 短名，让「用户看到的路径」与「实际存的路径」对不上。
pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdview-ws-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录失败");
        dir
    }

    #[test]
    fn lists_markdown_files_and_subdirectories_only() {
        let dir = temp_dir("list");
        std::fs::create_dir_all(dir.join("子目录")).expect("建子目录失败");
        std::fs::create_dir_all(dir.join("node_modules")).expect("建 node_modules 失败");
        std::fs::create_dir_all(dir.join(".git")).expect("建隐藏目录失败");
        std::fs::write(dir.join("甲.md"), "# 甲").expect("写 md 失败");
        std::fs::write(dir.join("乙.MARKDOWN"), "# 乙").expect("写 md 失败");
        std::fs::write(dir.join("丙.txt"), "丙").expect("写 txt 失败");
        std::fs::write(dir.join("图.png"), b"png").expect("写 png 失败");
        std::fs::write(dir.join(".隐藏.md"), "藏").expect("写隐藏文件失败");

        let listing = read_dir(&dir).expect("列目录失败");
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["子目录", "丙.txt", "乙.MARKDOWN", "甲.md"],
            "目录在前、文件在后；png / 隐藏项 / node_modules / .git 都不该出现"
        );
        assert!(listing.entries[0].dir);
        assert!(!listing.entries[1].dir);
        assert_eq!(listing.name, dir.file_name().unwrap().to_string_lossy());
        assert!(!listing.truncated);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_directory_yields_nothing_rather_than_failing() {
        let dir = temp_dir("empty");
        let listing = read_dir(&dir).expect("列空目录失败");
        assert!(listing.entries.is_empty());
        assert!(!listing.truncated);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_a_file_and_a_missing_path() {
        let dir = temp_dir("reject");
        let file = dir.join("甲.md");
        std::fs::write(&file, "# 甲").expect("写 md 失败");
        assert!(read_dir(&file).is_err(), "文件不是目录，应当报错");
        assert!(
            read_dir(&dir.join("不存在")).is_err(),
            "不存在的路径应当报错"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sorting_is_case_insensitive_not_ascii_order() {
        let dir = temp_dir("sort");
        for name in ["b.md", "A.md", "c.md", "D.md"] {
            std::fs::write(dir.join(name), "x").expect("写 md 失败");
        }
        let listing = read_dir(&dir).expect("列目录失败");
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["A.md", "b.md", "c.md", "D.md"],
            "按字母序而不是 ASCII 序（ASCII 会先把所有大写排完）"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_name_in_two_cases_is_ordered_deterministically() {
        // Windows / macOS 的文件系统建不出只差大小写的两个文件，所以直接测比较器：
        // 不区分大小写比较相等之后，再按原字符串定序（大写在前），结果与平台无关。
        let mut entries = vec![
            Entry {
                name: "b.md".to_string(),
                path: "b.md".to_string(),
                dir: false,
            },
            Entry {
                name: "B.md".to_string(),
                path: "B.md".to_string(),
                dir: false,
            },
            Entry {
                name: "a.md".to_string(),
                path: "a.md".to_string(),
                dir: false,
            },
        ];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a.md", "B.md", "b.md"]);
    }

    #[test]
    fn markdown_extension_check_is_case_insensitive() {
        assert!(is_markdown(Path::new("a.md")));
        assert!(is_markdown(Path::new("a.MD")));
        assert!(is_markdown(Path::new("a.Markdown")));
        assert!(is_markdown(Path::new("a.txt")));
        assert!(!is_markdown(Path::new("a.pdf")));
        assert!(!is_markdown(Path::new("a")));
    }
}

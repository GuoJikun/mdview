//! 图片资源路径解析。
//!
//! 只做纯路径运算，不读文件、不依赖 Tauri：桌面层拿到结果后再决定如何加载。
//! 两条硬规则：
//!
//! 1. 远程 URL（`http(s)` / `data:`）不在这里处理，原样交给前端；
//! 2. 本地路径必须落在 Markdown 所在目录内， lexical 与 canonical 双重检查，
//!    防止 `![x](../../secret.png)` 借图片标签读走沙箱外的文件。

use std::path::{Component, Path, PathBuf};

/// 远程或内联资源，前端可以直接塞进 `img.src`。
pub fn is_inline_or_remote(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:")
}

/// 按扩展名给出 `data:` URL 需要的 MIME 类型。
pub fn mime_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

/// 把图片 URL 解析为文档目录内的本地路径。
///
/// 支持相对路径、`file://` URL 与绝对路径；后两者同样受目录沙箱约束。
/// query / fragment 不参与路径。文件不存在时仍返回规范化后的路径，让前端
/// 走一次加载失败并回退占位符。
pub fn resolve_local(base: &Path, url: &str) -> Option<PathBuf> {
    let url = url.trim();
    if url.is_empty() || is_inline_or_remote(url) {
        return None;
    }

    let lower = url.to_ascii_lowercase();
    let file_url = lower.starts_with("file://");
    let raw = if file_url { &url[7..] } else { url };
    let raw = raw.split(['?', '#']).next().unwrap_or(raw);
    let decoded = percent_decode(raw);

    #[cfg(windows)]
    let decoded = {
        // file:///C:/x 去掉 file:// 后是 /C:/x，Windows 下要丢掉那个前导斜杠。
        let bytes = decoded.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':'
        {
            decoded[1..].to_string()
        } else {
            decoded
        }
    };

    let candidate = if file_url {
        PathBuf::from(decoded)
    } else {
        base.join(decoded)
    };
    let target = lexical_normalize(&candidate)?;

    // 两边都能 canonicalize 时以真实路径为准，挡住符号链接越狱；
    // 否则退回词法判定，保证文件还没落盘时也能给出稳定结果。
    if let (Ok(real_base), Ok(real_target)) = (base.canonicalize(), target.canonicalize()) {
        return real_target.starts_with(&real_base).then_some(real_target);
    }
    let root = lexical_normalize(base)?;
    target.starts_with(&root).then_some(target)
}

fn lexical_normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(part) => out.push(part),
        }
    }
    Some(out)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_stays_under_document_dir() {
        let base = Path::new("docs");
        assert_eq!(
            resolve_local(base, "img/a.png"),
            Some(PathBuf::from("docs/img/a.png"))
        );
        assert_eq!(
            resolve_local(base, "./img/../a.png"),
            Some(PathBuf::from("docs/a.png"))
        );
    }

    #[test]
    fn traversal_and_absolute_outside_are_rejected() {
        let base = Path::new("docs");
        assert_eq!(resolve_local(base, "../secret.png"), None);
        assert_eq!(resolve_local(base, "img/../../secret.png"), None);
        assert_eq!(resolve_local(base, "/etc/passwd"), None);
    }

    #[test]
    fn remote_and_data_urls_are_left_to_the_frontend() {
        let base = Path::new("docs");
        for url in [
            "https://example.com/a.png",
            "http://example.com/a.png",
            "data:image/png;base64,AAAA",
        ] {
            assert!(is_inline_or_remote(url));
            assert_eq!(resolve_local(base, url), None);
        }
    }

    #[test]
    fn file_url_is_decoded_and_sandboxed() {
        let base = std::env::temp_dir().join("mdview-assets");
        let file = base.join("img").join("a b.png");
        let url = format!("file://{}", file.to_string_lossy().replace(' ', "%20"));
        assert_eq!(resolve_local(&base, &url), Some(file));

        let escape = format!("file://{}/../../secret.png", base.display());
        assert_eq!(resolve_local(&base, &escape), None);
    }

    #[test]
    fn query_and_fragment_do_not_become_part_of_path() {
        assert_eq!(
            resolve_local(Path::new("docs"), "a.png?v=2#frag"),
            Some(PathBuf::from("docs/a.png"))
        );
    }

    #[test]
    fn mime_comes_from_extension() {
        assert_eq!(mime_for(Path::new("a.PNG")), "image/png");
        assert_eq!(mime_for(Path::new("a.jpeg")), "image/jpeg");
        assert_eq!(mime_for(Path::new("a.svg")), "image/svg+xml");
        assert_eq!(mime_for(Path::new("a.txt")), "application/octet-stream");
    }
}

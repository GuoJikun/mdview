use std::path::Path;

fn main() {
    // 前端资源由 tauri 在编译期嵌入二进制。cargo 的 `rerun-if-changed` 只能让
    // build script 重跑；脚本输出没变时，目标 crate 依然不会重编——「改了 JS
    // 但窗口行为没变」就是这么来的。把 web 目录的内容哈希写成 rustc-env，
    // 内容一变编译指纹就变，crate 必然重编。
    println!("cargo:rerun-if-changed=web");
    println!("cargo:rustc-env=MDVIEW_WEB_STAMP={:016x}", stamp(Path::new("web")));
    tauri_build::build();
}

/// web 目录的内容指纹（FNV-1a 64）。路径与字节都进哈希：改名也算变化。
fn stamp(dir: &Path) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut stack = vec![dir.to_path_buf()];
    let mut files = Vec::new();

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            println!("cargo:rerun-if-changed={}", path.display());
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                files.push((path, bytes));
            }
        }
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, bytes) in files {
        let name = path.to_string_lossy();
        for byte in name.as_bytes().iter().chain(bytes.iter()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

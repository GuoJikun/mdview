use std::path::Path;

fn main() {
    // 前端资源由 tauri 在编译期嵌入二进制，cargo 不会因为 web/ 里某个文件变了就重编，
    // 不加这几行就会出现「改了 JS 但窗口行为没变」的假象。
    //
    // 只登记 `web` 目录是不够的：目录项不变时改文件内容不会换来重编，
    // 必须逐个文件登记。目录本身也登记一次，用来捕捉新增/删除。
    println!("cargo:rerun-if-changed=web");
    dump(Path::new("web"));
    tauri_build::build();
}

fn dump(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            dump(&path);
        }
    }
}

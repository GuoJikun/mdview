fn main() {
    // 前端资源由 tauri 在编译期嵌入二进制，cargo 默认不会因为 web/ 变化就重编，
    // 不加这一行就会出现「改了 JS 但窗口行为没变」的假象。
    println!("cargo:rerun-if-changed=web");
    tauri_build::build();
}

//! 前端测试的守门。
//!
//! 前端用例已统一到 vitest（`vitest.config.ts` 里分两个 project）：
//!   unit —— 纯逻辑（输入法状态机、上下移动几何、选区判据、边缘滚动、大纲、主题），
//!           跑 node 环境，几十毫秒。
//!   dom  —— 碰真实布局的（内核启动、点击落点、光标像素定位），跑真浏览器。
//!
//! 这里只跑 unit：dom project 要拉一个 Chromium 实例，塞进 `cargo test` 会让
//! Rust 侧的流水线变得又慢又脆（还依赖本机装没装 Chrome）。dom 那一半由
//! `pnpm test:dom` 单独负责，`pnpm build` 里的 `vue-tsc` 负责类型。
//!
//! 直接调 `node node_modules/vitest/vitest.mjs` 而不是 `pnpm test`：
//! Windows 上 `Command::new("pnpm")` 找不到 `.cmd`，而 `.mjs` 入口是跨平台的。
//!
//! 没装 Node 或没装前端依赖时整组跳过（打印原因），不让缺工具链阻塞 Rust 流水线。

use std::{path::Path, path::PathBuf, process::Command};

/// 七个纯逻辑模块仍是 `.js`（要被 node 直接加载），语法在这里单独查一遍。
const PLAIN_SCRIPTS: [&str; 7] = [
    "ime.js",
    "caretline.js",
    "selection.js",
    "dragscroll.js",
    "clipboard.js",
    "outline.js",
    "theme.js",
];

fn project_root() -> PathBuf {
    // 测试二进制的 manifest 目录是 src-tauri/，项目根在其上一级。
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn web_dir() -> PathBuf {
    project_root().join("src/editor")
}

fn node_ready() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn run_node_at(cwd: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("node")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("node 应当可执行");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
fn frontend_scripts_are_syntactically_valid() {
    if !node_ready() {
        eprintln!("跳过：本机没有 node");
        return;
    }
    for script in PLAIN_SCRIPTS {
        let (ok, output) = run_node_at(&web_dir(), &["--check", script]);
        assert!(ok, "{script} 语法错误：\n{output}");
    }
}

#[test]
fn frontend_unit_suites_pass() {
    if !node_ready() {
        eprintln!("跳过：本机没有 node");
        return;
    }
    let root = project_root();
    let vitest = root.join("node_modules/vitest/vitest.mjs");
    if !vitest.exists() {
        eprintln!("跳过：没装前端依赖（先 `pnpm install`）");
        return;
    }
    let (ok, output) = run_node_at(
        &root,
        &[
            vitest.to_str().expect("路径应当可转成 UTF-8"),
            "run",
            "--project",
            "unit",
            // 默认 reporter 在非 TTY 下只打总数，不带文件名；守门断言需要
            // 每个 suite 的名字都出现在输出里。
            "--reporter=verbose",
        ],
    );
    assert!(ok, "前端 unit 用例失败：\n{output}");
    // 六套用例的文件名都该出现在汇总里，避免某个文件被漏掉而整体仍然绿
    for suite in [
        "ime.test.ts",
        "caretline.test.ts",
        "selection.test.ts",
        "dragscroll.test.ts",
        "outline.test.ts",
        "theme.test.ts",
    ] {
        assert!(output.contains(suite), "没有跑到 {suite}：\n{output}");
    }
}

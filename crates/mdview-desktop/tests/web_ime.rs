//! 前端脚本的守门测试。
//!
//! 这一层没有 Rust 代码可测，但 `web/` 里的纯逻辑不该靠肉眼验证：
//! `ime.js`（输入法状态机）、`caretline.js`（上下移动与滚动的几何）、
//! `selection.js`（原生选区判据）与 `dragscroll.js`（拖动时的边缘自动滚动）
//! 都刻意不碰 DOM，因此可以直接用 Node 跑。
//! 这里做两件事——语法检查所有前端脚本，再把几套用例跑一遍。
//!
//! 浏览器行为本身（滚轮、真实选区）在这里测不了：合成事件不触发默认行为，
//! 那部分只能靠 `--self-test` 的不变量与真人验收。
//!
//! 没装 Node 时整组跳过（打印原因），不让缺工具链阻塞 Rust 侧的流水线。

use std::{path::PathBuf, process::Command};

fn web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web")
}

fn node_ready() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn run_node(args: &[&str]) -> (bool, String) {
    let out = Command::new("node")
        .args(args)
        .current_dir(web_dir())
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
    for script in [
        "app.js",
        "ime.js",
        "ime.test.js",
        "caretline.js",
        "caretline.test.js",
        "selection.js",
        "selection.test.js",
        "dragscroll.js",
        "dragscroll.test.js",
        "clipboard.js",
    ] {
        let (ok, output) = run_node(&["--check", script]);
        assert!(ok, "{script} 语法错误：\n{output}");
    }
}

#[test]
fn ime_state_machine_passes_its_suite() {
    if !node_ready() {
        eprintln!("跳过：本机没有 node");
        return;
    }
    let (ok, output) = run_node(&["ime.test.js"]);
    assert!(ok, "输入法状态机测试失败：\n{output}");
    // 断言用例确实跑了，避免文件被清空后「零项全过」的假绿
    assert!(output.contains("项全过"), "没有看到测试汇总：\n{output}");
}

#[test]
fn vertical_motion_geometry_passes_its_suite() {
    if !node_ready() {
        eprintln!("跳过：本机没有 node");
        return;
    }
    let (ok, output) = run_node(&["caretline.test.js"]);
    assert!(ok, "上下移动几何测试失败：\n{output}");
    assert!(output.contains("项全过"), "没有看到测试汇总：\n{output}");
}

#[test]
fn native_selection_rule_passes_its_suite() {
    if !node_ready() {
        eprintln!("跳过：本机没有 node");
        return;
    }
    let (ok, output) = run_node(&["selection.test.js"]);
    assert!(ok, "原生选区判据测试失败：\n{output}");
    assert!(output.contains("项全过"), "没有看到测试汇总：\n{output}");
}

#[test]
fn drag_autoscroll_geometry_passes_its_suite() {
    if !node_ready() {
        eprintln!("跳过：本机没有 node");
        return;
    }
    let (ok, output) = run_node(&["dragscroll.test.js"]);
    assert!(ok, "边缘自动滚动测试失败：\n{output}");
    assert!(output.contains("项全过"), "没有看到测试汇总：\n{output}");
}

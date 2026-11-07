//! Tauri 2 外壳。
//!
//! 职责只有三件：开窗、把静态页挂上去、把 IPC 转给 [`session::Session`]。
//! 文本真值与解析都在内核，本 crate 不含任何编辑逻辑。
//!
//! 用法：`mdview-desktop [file.md] [--self-test]`
//!
//! `--self-test` 会在页面接管后跑一遍输入法自检（见 `web/app.js` 里的
//! `__mdviewSelfTest`），把结果写进日志并按成败退出。它是唯一能不靠肉眼
//! 确认「composition 事件真的接上了内核」的手段。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod session;

use std::{path::PathBuf, process};

use session::{Reply, Session, Snapshot, Wire};
use tauri::Manager;

/// 前端资源的内容指纹，由 `build.rs` 注入。
///
/// 值本身没有用处，引用它是为了让 cargo 把 `MDVIEW_WEB_STAMP` 记进编译指纹：
/// 只要 `web/` 里的内容变了，crate 就会重编，嵌进二进制的页面才是新的。
#[allow(dead_code)]
const WEB_STAMP: &str = env!("MDVIEW_WEB_STAMP");

#[tauri::command]
fn snapshot(session: tauri::State<'_, Session>) -> Snapshot {
    session.snapshot()
}

#[tauri::command]
fn dispatch(session: tauri::State<'_, Session>, message: Wire) -> Reply {
    session.dispatch(message)
}

/// 前端 boot 完成后回一次，用于确认「窗口 → 页面 → IPC」整条链路活着。
/// 起不来时日志里没有这一行，比界面空白好排查。
///
/// 自检模式下，这里也是唯一可靠的启动时机：能走到 `ready` 就说明状态机
/// 已经构造、DOM 已经挂好，合成事件才有意义。
#[tauri::command]
fn ready(app: tauri::AppHandle, session: tauri::State<'_, Session>, mode: tauri::State<'_, RunMode>) -> usize {
    let snap = session.snapshot();
    log::info!(
        "前端就绪：{} 块 / {} 停靠点 / 路径={:?}",
        snap.blocks.len(),
        snap.stops,
        snap.path
    );
    if mode.self_test {
        log::info!("自检：开始跑输入法合成事件");
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.eval("window.__mdviewSelfTest && window.__mdviewSelfTest()");
        }
    }
    snap.blocks.len()
}

/// 自检回包。报告以 `OK` 开头视为通过，其余一律非零退出。
#[tauri::command]
fn selftest(app: tauri::AppHandle, report: String) {
    if report.starts_with("OK") {
        log::info!("自检通过：{report}");
        app.exit(0);
    } else {
        log::error!("自检失败：{report}");
        app.exit(1);
    }
}

struct RunMode {
    self_test: bool,
}

fn main() {
    let mut path: Option<PathBuf> = None;
    let mut self_test = false;
    for arg in std::env::args().skip(1) {
        if arg == "--self-test" {
            self_test = true;
        } else if path.is_none() {
            path = Some(PathBuf::from(arg));
        }
    }

    let log_dir = mdview_app::init_logging_default().ok();
    log::info!(
        "启动 mdview-desktop v{} 日志目录={}{}",
        env!("CARGO_PKG_VERSION"),
        log_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "<不可用>".into()),
        if self_test { "（自检模式）" } else { "" }
    );

    let session = Session::open(path).unwrap_or_else(|err| {
        log::error!("打开失败：{err}");
        eprintln!("打开失败：{err}");
        process::exit(1);
    });

    tauri::Builder::default()
        .manage(session)
        .manage(RunMode { self_test })
        .invoke_handler(tauri::generate_handler![snapshot, dispatch, ready, selftest])
        .run(tauri::generate_context!())
        .expect("窗口启动失败");
}

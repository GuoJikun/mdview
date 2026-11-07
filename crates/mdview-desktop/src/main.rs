//! Tauri 2 外壳。
//!
//! 职责只有三件：开窗、把静态页挂上去、把 IPC 转给 [`session::Session`]。
//! 文本真值与解析都在内核，本 crate 不含任何编辑逻辑。
//!
//! 用法：`mdview-desktop [file.md]`

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod session;

use std::{path::PathBuf, process};

use session::{Reply, Session, Snapshot, Wire};

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
#[tauri::command]
fn ready(session: tauri::State<'_, Session>) -> usize {
    let snap = session.snapshot();
    log::info!(
        "前端就绪：{} 块 / {} 停靠点 / 路径={:?}",
        snap.blocks.len(),
        snap.stops,
        snap.path
    );
    snap.blocks.len()
}

fn main() {
    let log_dir = mdview_app::init_logging_default().ok();
    log::info!(
        "启动 mdview-desktop v{} 日志目录={}",
        env!("CARGO_PKG_VERSION"),
        log_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "<不可用>".into())
    );

    let path = std::env::args().nth(1).map(PathBuf::from);
    let session = Session::open(path).unwrap_or_else(|err| {
        log::error!("打开失败：{err}");
        eprintln!("打开失败：{err}");
        process::exit(1);
    });

    tauri::Builder::default()
        .manage(session)
        .invoke_handler(tauri::generate_handler![snapshot, dispatch, ready])
        .run(tauri::generate_context!())
        .expect("窗口启动失败");
}

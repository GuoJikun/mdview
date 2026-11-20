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

use mdview_app::Watcher;
use session::{Reply, Session, Snapshot, Wire};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

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

/// 前端遇到 Markdown 里的本地图片时调用。
///
/// 路径解析在 Session 里完成并限制在文档目录内；这里只做读盘与 data URL 编码，
/// 不把任意路径暴露给 webview。超过上限的文件直接拒绝，避免大图撑爆 IPC。
#[tauri::command]
fn load_image(url: String, session: tauri::State<'_, Session>) -> Option<String> {
    image_data_url(&session, &url)
}

fn image_data_url(session: &Session, url: &str) -> Option<String> {
    const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
    let (path, mime) = session.resolve_image(url)?;
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > MAX_IMAGE_BYTES {
        log::warn!("图片过大，跳过：{}", path.display());
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

/// 前端 boot 完成后回一次，用于确认「窗口 → 页面 → IPC」整条链路活着。
/// 起不来时日志里没有这一行，比界面空白好排查。
///
/// 自检模式下，这里也是唯一可靠的启动时机：能走到 `ready` 就说明状态机
/// 已经构造、DOM 已经挂好，合成事件才有意义。
#[tauri::command]
fn ready(
    app: tauri::AppHandle,
    session: tauri::State<'_, Session>,
    mode: tauri::State<'_, RunMode>,
) -> usize {
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

/// 文件 → 打开文件：弹原生对话框选 .md，读盘后整体替换文档。
///
/// 用 async command 承载 blocking_pick_file（对话框会阻塞），避免卡住主 UI 线程。
/// 用户取消时返回 Ok(None)，前端据此不做任何改动。读盘/解析失败返回 Err。
///
/// `path` 可选：正常菜单调用不传，走对话框；传入时直接用该路径——给端到端自检
/// 一条不弹窗也能走完整链路的注入点。
#[tauri::command]
async fn open_file(
    app: tauri::AppHandle,
    session: tauri::State<'_, Session>,
    path: Option<PathBuf>,
) -> Result<Option<Snapshot>, String> {
    let path = match path {
        Some(p) => p,
        None => {
            let picked = app
                .dialog()
                .file()
                .set_title("打开 Markdown 文件")
                .add_filter("Markdown", &["md", "markdown", "txt"])
                .blocking_pick_file();
            let Some(file) = picked else {
                return Ok(None);
            };
            file.into_path()
                .map_err(|_| "选中的文件路径无效".to_string())?
        }
    };
    log::info!("打开文件：{}", path.display());
    session.open_file(path).map(Some)
}

/// 前端据此决定显示自绘标题栏还是让位给系统标题栏。
/// 必须和 tauri.{windows,linux}.conf.json 的 `decorations` 保持一致。
#[tauri::command]
fn use_custom_titlebar() -> bool {
    !cfg!(target_os = "macos")
}

/// 原生菜单里需要转给前端执行的动作 ID。
/// 只列自绘编辑器真正支持的动作；about / hide / quit / minimize 等由系统自己处理。
#[cfg(target_os = "macos")]
fn menu_action(id: &str) -> Option<&'static str> {
    Some(match id {
        "open-file" => "open-file",
        "save" => "save",
        "undo" => "undo",
        "redo" => "redo",
        "cut" => "cut",
        "copy" => "copy",
        "paste" => "paste",
        "select-all" => "select-all",
        "theme-system" => "theme-system",
        "theme-light" => "theme-light",
        "theme-dark" => "theme-dark",
        "mode-wysiwyg" => "mode-wysiwyg",
        "mode-reading" => "mode-reading",
        "mode-source" => "mode-source",
        "outline-prev" => "outline-prev",
        "outline-next" => "outline-next",
        "doc-start" => "doc-start",
        "doc-end" => "doc-end",
        "about" => "about",
        _ => return None,
    })
}

/// macOS 的系统菜单栏。故意不给自定义项设 accelerator：快捷键由 app.js 的
/// keydown 统一处理，两边都设会让「撤销」这类非幂等动作执行两次。
#[cfg(target_os = "macos")]
fn build_native_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{AboutMetadata, MenuBuilder, MenuItem, SubmenuBuilder};

    let item =
        |id: &'static str, text: &'static str| MenuItem::with_id(app, id, text, true, None::<&str>);

    let app_menu = SubmenuBuilder::new(app, "mdview")
        .about(Some(AboutMetadata::default()))
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let file_menu = SubmenuBuilder::new(app, "文件")
        .item(&item("open-file", "打开文件…")?)
        .item(&item("save", "保存")?)
        .separator()
        .close_window_with_text("关闭窗口")
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "编辑")
        .item(&item("undo", "撤销")?)
        .item(&item("redo", "重做")?)
        .separator()
        .item(&item("cut", "剪切")?)
        .item(&item("copy", "复制")?)
        .item(&item("paste", "粘贴")?)
        .item(&item("select-all", "全选")?)
        .build()?;

    let theme_menu = SubmenuBuilder::new(app, "主题")
        .item(&item("theme-system", "跟随系统")?)
        .item(&item("theme-light", "亮色")?)
        .item(&item("theme-dark", "暗色")?)
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "视图")
        .item(&theme_menu)
        .separator()
        .item(&item("mode-wysiwyg", "所见即所得")?)
        .item(&item("mode-reading", "阅读视图")?)
        .item(&item("mode-source", "源码视图")?)
        .build()?;

    let go_menu = SubmenuBuilder::new(app, "转到")
        .item(&item("outline-prev", "上一个标题")?)
        .item(&item("outline-next", "下一个标题")?)
        .separator()
        .item(&item("doc-start", "文档开头")?)
        .item(&item("doc-end", "文档结尾")?)
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "窗口")
        .minimize()
        .maximize()
        .separator()
        .bring_all_to_front()
        .build()?;

    let help_menu = SubmenuBuilder::new(app, "帮助")
        .item(&item("about", "关于 mdview")?)
        .build()?;

    MenuBuilder::new(app)
        .item(&app_menu)
        .item(&file_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&go_menu)
        .item(&window_menu)
        .item(&help_menu)
        .build()
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

    // 外部修改监听：起一个后台线程 poll Watcher。命中的事件还要经 Session
    // 用指纹二次确认（挡掉自己保存产生的事件），再 emit 给前端提示。
    // 自检模式不开监听——自检样张是临时文件，且不应受外部事件干扰。
    // 线程每轮读 Session 的当前路径：打开文件后路径变了，就重建 Watcher，
    // 否则会继续盯着旧文件。NotifyWatcher 的回调绑定构造时的路径，必须重建。
    let watch_session = session.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(session)
        .manage(RunMode { self_test })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            dispatch,
            load_image,
            ready,
            selftest,
            open_file,
            use_custom_titlebar
        ])
        .setup(move |app| {
            // macOS：标题栏用系统装饰，菜单也用原生菜单栏；点击项转发给前端执行。
            #[cfg(target_os = "macos")]
            {
                let menu = build_native_menu(app.handle())?;
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    if let Some(action) = menu_action(event.id().as_ref()) {
                        let _ = app.emit("menu-action", action);
                    }
                });
            }

            if self_test {
                return Ok(());
            }
            let handle = app.handle().clone();
            let session = watch_session.clone();
            std::thread::spawn(move || {
                // current 是 watcher 当前绑定的路径；None 表示没有有效监听。
                let mut current: Option<(PathBuf, mdview_app::NotifyWatcher)> = None;
                loop {
                    let target = session.watch_path();
                    if current.as_ref().map(|(p, _)| p) != target.as_ref() {
                        // 路径变了（打开文件 / 未命名 / 初次启动）：重建或撤掉监听。
                        current = target.as_ref().and_then(|p| {
                            match mdview_app::NotifyWatcher::new(p) {
                                Ok(w) => {
                                    log::info!("已启动外部修改监听：{}", p.display());
                                    Some((p.clone(), w))
                                }
                                Err(err) => {
                                    log::warn!("文件监听未启动（不影响编辑）：{err}");
                                    None
                                }
                            }
                        });
                    }
                    if let Some((path, watcher)) = current.as_mut() {
                        if !watcher.poll().is_empty() && session.externally_modified() {
                            log::info!("检测到外部修改：{}", path.display());
                            let _ = handle.emit("external-change", path.to_string_lossy());
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(400));
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("窗口启动失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_data_url_is_read_from_document_dir() {
        let dir = std::env::temp_dir().join(format!("mdview-image-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let image = dir.join("a.png");
        std::fs::write(&image, b"png-bytes").expect("写测试图片失败");
        let session = Session::from_text("![图](a.png)", Some(dir.join("doc.md")));

        let data = image_data_url(&session, "a.png").expect("图片应能读取");
        assert!(data.starts_with("data:image/png;base64,"), "{data}");
        assert_eq!(image_data_url(&session, "../secret.png"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

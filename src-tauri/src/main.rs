//! Tauri 2 外壳。
//!
//! 职责只有三件：开窗、把静态页挂上去、把 IPC 转给 [`session::Session`]。
//! 文本真值与解析都在内核，本 crate 不含任何编辑逻辑。
//!
//! 用法：`mdview-desktop [file.md] [--self-test]`
//!
//! `--self-test` 会在页面接管后跑一遍输入法自检（见前端 `src/editor/editor.ts` 里的
//! `__mdviewSelfTest`），把结果写进日志并按成败退出。它是唯一能不靠肉眼
//! 确认「composition 事件真的接上了内核」的手段。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod link;
mod recent;
mod session;
mod update;

use std::{
    path::PathBuf,
    process,
    sync::atomic::{AtomicBool, Ordering},
};

use mdview_app::Watcher;
use session::{Reply, Session, Snapshot, Wire};
use tauri::{DragDropEvent, Emitter, Manager, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_window_state::StateFlags;

/// 关窗守卫：`CloseRequested` 里发现未保存改动时拦住，交给前端确认。
///
/// 原生对话框不能在窗口事件回调里阻塞（Windows 上是嵌套消息循环，macOS/Linux
/// 上会死锁），所以回调只负责拦截 + 发事件，真正的询问走 `confirm_discard` 命令。
#[derive(Default)]
struct CloseGuard {
    /// 用户已确认丢弃改动：下一次 `CloseRequested` 直接放行，不再拦。
    allowed: AtomicBool,
}

/// 前端资源的内容指纹，由 `build.rs` 注入。
///
/// 值本身没有用处，引用它是为了让 cargo 把 `MDVIEW_WEB_STAMP` 记进编译指纹：
/// 只要 Vite 产物 `dist/` 里的内容变了，crate 就会重编，嵌进二进制的页面才是新的。
#[allow(dead_code)]
const WEB_STAMP: &str = env!("MDVIEW_WEB_STAMP");

#[tauri::command]
fn snapshot(session: tauri::State<'_, Session>) -> Snapshot {
    session.snapshot()
}

#[tauri::command]
fn dispatch(app: tauri::AppHandle, session: tauri::State<'_, Session>, message: Wire) -> Reply {
    // 另存为会把文档改绑到一个新路径：这条路径要进最近列表。
    // 先留一份，`message` 之后会被 move 进内核。
    let saved_to = match &message {
        Wire::SaveTo { path } => Some(PathBuf::from(path)),
        _ => None,
    };
    let reply = session.dispatch(message);
    if reply.saved
        && let Some(path) = saved_to
    {
        recent::remember(&app, &path);
        // macOS 原生菜单是静态快照，另存为后要把新路径补进「最近打开」
        refresh_native_menu(&app);
    }
    reply
}

/// 最近打开文件列表，最近的在前。文件菜单据此渲染子菜单。
#[tauri::command]
fn recent_files(app: tauri::AppHandle) -> Vec<String> {
    recent::list(&app)
}

/// 清空最近打开记录。
#[tauri::command]
fn clear_recent_files(app: tauri::AppHandle) {
    recent::clear(&app);
    refresh_native_menu(&app);
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

/// 「未保存的改动」确认框。返回 `true` 表示可以继续——要么本来就不脏，
/// 要么用户点了「继续」（认了这次丢失）。
///
/// 自检模式一律放行：自检会反复换文档，弹框会直接把它挂死。
fn ask_discard(app: &tauri::AppHandle, what: &str) -> bool {
    if app.state::<RunMode>().self_test {
        return true;
    }
    if !app.state::<Session>().is_dirty() {
        return true;
    }
    app.dialog()
        .message(format!("{what}会丢失当前未保存的改动。"))
        .title("有未保存的改动")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "继续".to_string(),
            "取消".to_string(),
        ))
        .blocking_show()
}

/// 关窗前确认。`action` 只决定文案：`close` 是关窗，其余按换文档处理。
#[tauri::command]
async fn confirm_discard(app: tauri::AppHandle, action: String) -> bool {
    let what = if action == "close" {
        "关闭窗口"
    } else {
        "打开另一个文件"
    };
    ask_discard(&app, what)
}

/// 用户已在确认框里点了「继续」：置放行标志再关窗，`CloseRequested` 不再拦。
#[tauri::command]
fn force_close(app: tauri::AppHandle, guard: tauri::State<'_, CloseGuard>) {
    guard.allowed.store(true, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.close();
    }
}

/// 文件 → 新建：换成未命名空文档。
///
/// 有未保存改动时先问一句；用户取消返回 `Ok(None)`，前端什么都不做。
///
/// 返回 `Result` 是 Tauri 对「带引用的 async 命令」的硬性要求（见 open_file）。
#[tauri::command]
async fn new_file(
    app: tauri::AppHandle,
    session: tauri::State<'_, Session>,
) -> Result<Option<Snapshot>, String> {
    if !ask_discard(&app, "新建文档") {
        return Ok(None);
    }
    log::info!("新建文档：切到未命名空文档");
    Ok(Some(session.new_file()))
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
    // 先问再选文件：已经决定要换文档了，别让用户选完才发现不让开。
    // 没有未保存改动时这一步直接放行，不打扰。
    if !ask_discard(&app, "打开另一个文件") {
        return Ok(None);
    }
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
    let snap = session.open_file(path.clone())?;
    // 自检反复打开样张，不该污染用户的最近列表。
    if !app.state::<RunMode>().self_test {
        recent::remember(&app, &path);
        refresh_native_menu(&app);
    }
    Ok(Some(snap))
}

/// 保存对话框：只在「内核说没有路径」或用户主动另存为时调用。
///
/// 只负责要一个路径，**不写盘**——写盘、改绑路径、刷新指纹都在内核里，
/// 那里才有原子替换与冲突判定。取消返回 `None`，此时内核状态一动不动。
///
/// `suggested` 是对话框里的预填文件名（未命名文档给「未命名.md」）。
#[tauri::command]
async fn pick_save_path(app: tauri::AppHandle, suggested: Option<String>) -> Option<PathBuf> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title("保存 Markdown 文件")
        .add_filter("Markdown", &["md", "markdown"]);
    if let Some(name) = suggested {
        dialog = dialog.set_file_name(name);
    }
    dialog.blocking_save_file()?.into_path().ok()
}

/// 前端据此决定显示自绘标题栏还是让位给系统标题栏。
/// 必须和 tauri.{windows,linux}.conf.json 的 `decorations` 保持一致。
#[tauri::command]
fn use_custom_titlebar() -> bool {
    !cfg!(target_os = "macos")
}

/// 从拖放 / 命令行参数里挑第一个能当文档打开的路径。
///
/// 只认存在的文件：拖进来的可能是文件夹或图片，交给 `open_file` 只会报错。
/// 扩展名比较不区分大小写，Windows 上 `.MD` 很常见。
fn first_markdown(paths: &[PathBuf]) -> Option<&PathBuf> {
    paths.iter().find(|path| {
        path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt")
                })
    })
}

/// 最近打开子菜单的 (菜单 id, 显示名)。id 与前端动作名一致（`recent:0`）。
///
/// 只放文件名：完整路径会把菜单撑得很宽；自绘菜单用 `title` 兜底，
/// 原生菜单没有悬停提示，宁可短一些。
fn recent_menu_entries(recents: &[String]) -> Vec<(String, String)> {
    recents
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let label = PathBuf::from(path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| path.clone());
            (format!("recent:{index}"), label)
        })
        .collect()
}

/// 动态菜单 id 是否属于「最近打开」。
///
/// 原生菜单每次重建，`menu_action` 的静态表覆盖不到这些序号，只能在事件里
/// 现判。`recent:` 后面必须是数字，避免把将来的其它 `recent-*` 动作误吞。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn is_recent_menu_id(id: &str) -> bool {
    id == "recent-clear"
        || id
            .strip_prefix("recent:")
            .is_some_and(|index| index.parse::<usize>().is_ok())
}

/// 原生菜单里需要转给前端执行的动作 ID。
/// 只列自绘编辑器真正支持的动作；about / hide / quit / minimize 等由系统自己处理。
#[cfg(target_os = "macos")]
fn menu_action(id: &str) -> Option<&'static str> {
    Some(match id {
        "open-file" => "open-file",
        "save" => "save",
        "save-as" => "save-as",
        "new-file" => "new-file",
        "undo" => "undo",
        "redo" => "redo",
        "cut" => "cut",
        "copy" => "copy",
        "paste" => "paste",
        "select-all" => "select-all",
        "find" => "find",
        "replace" => "replace",
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
        "update" => "update",
        "about" => "about",
        _ => return None,
    })
}

/// macOS 的系统菜单栏。故意不给自定义项设 accelerator：快捷键由 editor.ts 的
/// keydown 统一处理，两边都设会让「撤销」这类非幂等动作执行两次。
///
/// 最近打开列表是运行时数据，菜单只能在打开 / 另存为 / 清除后整棵重建；
/// 重建入口是 [`refresh_native_menu`]。
///
/// 函数本身不加 `cfg`：菜单 API 各桌面平台都有，留在编译单元里才能在
/// Windows 开发机上一起做类型检查（只在 macOS 调用）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn build_native_menu(
    app: &tauri::AppHandle,
    recents: &[String],
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
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

    // 最近打开：空表也要有一项占位，否则子菜单会变成空壳、点了没反应。
    let mut recent_builder = SubmenuBuilder::new(app, "最近打开");
    let entries = recent_menu_entries(recents);
    if entries.is_empty() {
        recent_builder = recent_builder.item(&MenuItem::with_id(
            app,
            "recent-empty",
            "（暂无）",
            false,
            None::<&str>,
        )?);
    } else {
        for (id, label) in entries {
            recent_builder =
                recent_builder.item(&MenuItem::with_id(app, id, label, true, None::<&str>)?);
        }
        recent_builder = recent_builder.separator().item(&MenuItem::with_id(
            app,
            "recent-clear",
            "清除记录",
            true,
            None::<&str>,
        )?);
    }
    let recent_menu = recent_builder.build()?;

    let file_menu = SubmenuBuilder::new(app, "文件")
        .item(&item("new-file", "新建")?)
        .item(&item("open-file", "打开文件…")?)
        .item(&recent_menu)
        .item(&item("save", "保存")?)
        .item(&item("save-as", "另存为…")?)
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
        .separator()
        .item(&item("find", "查找…")?)
        .item(&item("replace", "替换…")?)
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
        .item(&item("update", "检查更新…")?)
        .separator()
        .item(&item("about", "关于")?)
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

/// 最近列表变了就整棵重建原生菜单。
///
/// 命令可能跑在非 UI 线程上，菜单必须在主线程构建/安装，统一投递过去。
/// 这里不加 `cfg`：Windows 上不会调用，但留在编译单元里才能一起做类型检查。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn rebuild_native_menu(app: &tauri::AppHandle) {
    let recents = recent::list(app);
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || match build_native_menu(&handle, &recents) {
        Ok(menu) => {
            if let Err(err) = handle.set_menu(menu) {
                log::warn!("重建原生菜单失败：{err}");
            }
        }
        Err(err) => log::warn!("构建原生菜单失败：{err}"),
    }) {
        log::warn!("投递菜单重建失败：{err}");
    }
}

/// 调用点统一走这里；非 macOS 是空实现，不必到处写 `cfg`。
#[cfg(target_os = "macos")]
fn refresh_native_menu(app: &tauri::AppHandle) {
    rebuild_native_menu(app);
}

#[cfg(not(target_os = "macos"))]
fn refresh_native_menu(_app: &tauri::AppHandle) {}

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

    let start_path = path.clone();
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

    let mut builder = tauri::Builder::default();

    // 单例插件必须第一个注册：它要在其它插件之前抢占实例锁。
    // 回调运行在「已经活着」的那个实例里，第二个实例已被引擎拦截并退出。
    builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        // 有文件参数就交给已运行实例打开，否则新实例带着文档启动会「没反应」。
        // 跳过 --self-test 这类开关，只认 Markdown 扩展名的路径。
        let paths: Vec<PathBuf> = args
            .iter()
            .filter(|arg| !arg.starts_with('-'))
            .map(PathBuf::from)
            .collect();
        if let Some(path) = first_markdown(&paths) {
            log::info!("单例收到打开请求：{}", path.display());
            let _ = app.emit("single-instance-open", path.to_string_lossy().to_string());
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_focus();
        }
    }));

    builder
        // 记住窗口的尺寸、位置、最大化与全屏，下次启动恢复。
        // 窗口在三份 tauri.*.conf.json 里都是 visible: false，显示交给 setup 里的
        // show()：插件在窗口创建时就恢复几何，setup 在它之后跑，所以是先摆好再露出来，
        // 不会先按 conf 的 960×760 闪一下再跳过去。
        // 只记这四项，不含 DECORATIONS（标题栏形态由 conf 单点决定，插件再覆盖一次
        // 就成了两个来源，Windows 上会同时长出原生标题栏与自绘标题栏），
        // 也不含 VISIBLE（可见性由 setup 决定，不让它有机会持久化成 false）。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    StateFlags::SIZE
                        | StateFlags::POSITION
                        | StateFlags::MAXIMIZED
                        | StateFlags::FULLSCREEN,
                )
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        // 更新：清单拉取与签名校验都在 Rust 侧，前端只调自定义命令。
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(session)
        .manage(RunMode { self_test })
        .manage(CloseGuard::default())
        .manage(update::UpdateState::new())
        // 关窗守卫：有未保存改动就拦住，让前端问一句再放行。
        // 标题栏的 ×、Alt+F4、菜单里的「关闭窗口」最后都走这里，一处拦全部。
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                let app = window.app_handle();
                // swap 而不是 load：放行标志是**一次性**的。万一这次关闭后来没成
                // （比如被别的插件拦下），下一次关窗还得重新问，不能永久放开。
                if app
                    .state::<CloseGuard>()
                    .allowed
                    .swap(false, Ordering::SeqCst)
                {
                    return;
                }
                if !app.state::<Session>().is_dirty() {
                    return;
                }
                api.prevent_close();
                let _ = window.emit("close-requested", ());
            }
            // 拖放打开：只在真正的 Drop 上发一次事件，落点/悬停只用来做视觉反馈。
            // 打开动作仍走前端 → `open_file` 命令，未保存改动的确认与菜单打开同一条路径。
            WindowEvent::DragDrop(DragDropEvent::Enter { paths, .. }) => {
                if window.app_handle().state::<RunMode>().self_test {
                    return;
                }
                let _ = window.emit("drop-hover", first_markdown(paths).is_some());
            }
            WindowEvent::DragDrop(DragDropEvent::Leave) => {
                let _ = window.emit("drop-hover", false);
            }
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                let _ = window.emit("drop-hover", false);
                if window.app_handle().state::<RunMode>().self_test {
                    return;
                }
                let Some(path) = first_markdown(paths) else {
                    return;
                };
                log::info!("拖放打开：{}", path.display());
                let _ = window.emit("drop-open", path.to_string_lossy().to_string());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            dispatch,
            recent_files,
            clear_recent_files,
            load_image,
            link::open_url,
            ready,
            selftest,
            open_file,
            new_file,
            confirm_discard,
            force_close,
            pick_save_path,
            use_custom_titlebar,
            update::check_update,
            update::download_update,
            update::cancel_update_download,
            update::install_update
        ])
        .setup(move |app| {
            // 窗口按 conf 以隐藏状态创建，插件在创建时就恢复了尺寸/位置/最大化，
            // 这里才让它露面——顺序反了就会看到窗口先小后大地跳一下。
            // 注意：这一步早于页面加载，所以可能有极短的白底；若将来要消掉，
            // 改成前端首帧后再 invoke 一个 show 命令即可。
            if let Some(window) = app.get_webview_window("main") {
                window.show()?;
                let _ = window.set_focus();
            }

            // 启动时带文件参数（双击 .md 关联、命令行）：也算「最近打开」。
            // 自检模式不记，免得样张路径挤进用户的列表。
            if !self_test && let Some(path) = start_path.as_ref() {
                recent::remember(app.handle(), path);
            }

            // macOS：标题栏用系统装饰，菜单也用原生菜单栏；点击项转发给前端执行。
            #[cfg(target_os = "macos")]
            {
                let menu = build_native_menu(app.handle(), &recent::list(app.handle()))?;
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    let id = event.id().as_ref();
                    // 最近打开是运行时生成的 id，静态动作表里没有，直接透传
                    if is_recent_menu_id(id) {
                        let _ = app.emit("menu-action", id);
                    } else if let Some(action) = menu_action(id) {
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
                        current =
                            target
                                .as_ref()
                                .and_then(|p| match mdview_app::NotifyWatcher::new(p) {
                                    Ok(w) => {
                                        log::info!("已启动外部修改监听：{}", p.display());
                                        Some((p.clone(), w))
                                    }
                                    Err(err) => {
                                        log::warn!("文件监听未启动（不影响编辑）：{err}");
                                        None
                                    }
                                });
                    }
                    if let Some((path, watcher)) = current.as_mut()
                        && !watcher.poll().is_empty()
                        && session.externally_modified()
                    {
                        log::info!("检测到外部修改：{}", path.display());
                        let _ = handle.emit("external-change", path.to_string_lossy());
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
    fn recent_menu_entries_use_basenames_and_stable_ids() {
        let entries = recent_menu_entries(&[
            "/home/me/一.md".to_string(),
            "/home/me/子目录/二.md".to_string(),
        ]);
        assert_eq!(
            entries,
            vec![
                ("recent:0".to_string(), "一.md".to_string()),
                ("recent:1".to_string(), "二.md".to_string()),
            ]
        );
        assert!(recent_menu_entries(&[]).is_empty());
    }

    #[test]
    fn only_recent_menu_ids_are_recognised() {
        assert!(is_recent_menu_id("recent:0"));
        assert!(is_recent_menu_id("recent:12"));
        assert!(is_recent_menu_id("recent-clear"));
        // 占位项不是可点击动作；`recent:` 后面不是数字也不能吞
        assert!(!is_recent_menu_id("recent-empty"));
        assert!(!is_recent_menu_id("recent:"));
        assert!(!is_recent_menu_id("recent:x"));
        assert!(!is_recent_menu_id("open-file"));
    }

    #[test]
    fn first_markdown_skips_folders_and_other_files() {
        let dir = std::env::temp_dir().join(format!("mdview-drop-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let md = dir.join("说明.MD");
        let png = dir.join("图.png");
        std::fs::write(&md, "# 标题").expect("写测试文档失败");
        std::fs::write(&png, b"png").expect("写测试图片失败");

        assert_eq!(
            first_markdown(&[dir.clone(), png.clone(), md.clone()]),
            Some(&md),
            "文件夹与非 Markdown 文件应被跳过，扩展名大小写不敏感"
        );
        assert_eq!(first_markdown(&[]), None);
        assert_eq!(
            first_markdown(&[dir.join("不存在.md")]),
            None,
            "不存在的路径不算"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

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

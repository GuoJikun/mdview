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

mod backdrop;
mod link;
mod recent;
mod release_notes;
mod session;
mod update;

use std::{
    collections::HashMap,
    path::PathBuf,
    process,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
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
///
/// 每个窗口一份：多窗口下 A 窗口的「丢弃并关闭」不能顺手放行 B 窗口。
#[derive(Clone, Default)]
struct CloseGuards(Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>);

impl CloseGuards {
    /// 取某个窗口的放行标志，没有就补一个。回调里只做一次 swap，不能到处建。
    fn of(&self, label: &str) -> Arc<AtomicBool> {
        let mut map = self.0.lock().expect("关窗守卫锁中毒");
        map.entry(label.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    fn remove(&self, label: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(label);
        }
    }
}

/// 窗口标签 → 编辑会话。
///
/// `Session` 内部是 `Arc<Mutex<AppState>>`，克隆只复制句柄，命令拿到的是
/// 同一份文档。用注册表而不是 Tauri 的全局 `State<Session>`，是因为
/// `Manager::manage` 只有应用级作用域，没有「每个窗口一份」的容器。
#[derive(Clone, Default)]
struct Sessions(Arc<Mutex<HashMap<String, Session>>>);

impl Sessions {
    fn insert(&self, label: impl Into<String>, session: Session) {
        if let Ok(mut map) = self.0.lock() {
            map.insert(label.into(), session);
        }
    }

    fn get(&self, label: &str) -> Option<Session> {
        self.0.lock().ok()?.get(label).cloned()
    }

    fn remove(&self, label: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(label);
        }
    }

    /// 快照一份给 watcher 线程遍历，避免持锁做文件 I/O。
    fn entries(&self) -> Vec<(String, Session)> {
        self.0
            .lock()
            .map(|map| {
                map.iter()
                    .map(|(label, session)| (label.clone(), session.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 外部修改轮询：一个线程看所有窗口。
///
/// 每个窗口的文档路径可能不同（各自打开的文件），所以按 label 各建一个
/// `NotifyWatcher`；路径变了就重建（NotifyWatcher 绑定构造时的路径）。
/// 事件只发给对应窗口，不能广播——否则 B 窗口会提示 A 文件被改了。
fn watch_loop(app: tauri::AppHandle, sessions: Sessions) {
    let mut active: HashMap<String, (PathBuf, mdview_app::NotifyWatcher)> = HashMap::new();
    loop {
        let entries = sessions.entries();
        let alive: std::collections::HashSet<String> =
            entries.iter().map(|(label, _)| label.clone()).collect();

        for (label, session) in &entries {
            let target = session.watch_path();
            let unchanged = active
                .get(label)
                .is_some_and(|(path, _)| Some(path) == target.as_ref());
            if !unchanged {
                active.remove(label);
                if let Some(path) = target {
                    match mdview_app::NotifyWatcher::new(&path) {
                        Ok(watcher) => {
                            log::info!("窗口 {label} 启动外部修改监听：{}", path.display());
                            active.insert(label.clone(), (path, watcher));
                        }
                        Err(err) => {
                            log::warn!("窗口 {label} 文件监听未启动（不影响编辑）：{err}");
                        }
                    }
                }
            }
            if let Some((path, watcher)) = active.get_mut(label)
                && !watcher.poll().is_empty()
                && session.externally_modified()
                && let Some(window) = app.get_webview_window(label)
            {
                log::info!("窗口 {label} 检测到外部修改：{}", path.display());
                let _ = window.emit("external-change", path.to_string_lossy());
            }
        }

        // 窗口已关：连同它的监听一起撤掉，别让线程继续握着旧文件
        active.retain(|label, _| alive.contains(label));
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
}

/// 取调用窗口的会话。标签在注册表里找不到，说明窗口正在销毁（或注册前就发起了
/// IPC），命令如实报错，不去碰别的窗口。
fn session_of(label: &str, sessions: &Sessions) -> Result<Session, String> {
    sessions
        .get(label)
        .ok_or_else(|| format!("窗口 {label} 没有对应的文档会话"))
}

/// 前端资源的内容指纹，由 `build.rs` 注入。
///
/// 值本身没有用处，引用它是为了让 cargo 把 `MDVIEW_WEB_STAMP` 记进编译指纹：
/// 只要 Vite 产物 `dist/` 里的内容变了，crate 就会重编，嵌进二进制的页面才是新的。
#[allow(dead_code)]
const WEB_STAMP: &str = env!("MDVIEW_WEB_STAMP");

#[tauri::command]
fn snapshot(
    window: tauri::Window,
    sessions: tauri::State<'_, Sessions>,
) -> Result<Snapshot, String> {
    Ok(session_of(window.label(), &sessions)?.snapshot())
}

#[tauri::command]
fn dispatch(
    app: tauri::AppHandle,
    window: tauri::Window,
    sessions: tauri::State<'_, Sessions>,
    message: Wire,
) -> Result<Reply, String> {
    // 另存为会把文档改绑到一个新路径：这条路径要进最近列表。
    // 先留一份，`message` 之后会被 move 进内核。
    let saved_to = match &message {
        Wire::SaveTo { path } => Some(PathBuf::from(path)),
        _ => None,
    };
    let reply = session_of(window.label(), &sessions)?.dispatch(message);
    if reply.saved
        && let Some(path) = saved_to
    {
        // 最近列表是应用级数据：记一条并通知所有窗口刷新菜单
        remember_recent(&app, &path);
    }
    Ok(reply)
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
    // 其它窗口的最近菜单也要立刻变成空
    let _ = app.emit("recent-changed", ());
}

/// 前端遇到 Markdown 里的本地图片时调用。
///
/// 路径解析在 Session 里完成并限制在文档目录内；这里只做读盘与 data URL 编码，
/// 不把任意路径暴露给 webview。超过上限的文件直接拒绝，避免大图撑爆 IPC。
#[tauri::command]
fn load_image(
    window: tauri::Window,
    sessions: tauri::State<'_, Sessions>,
    url: String,
) -> Option<String> {
    let session = session_of(window.label(), &sessions).ok()?;
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
    window: tauri::WebviewWindow,
    sessions: tauri::State<'_, Sessions>,
    mode: tauri::State<'_, RunMode>,
) -> usize {
    let snapshot = session_of(window.label(), &sessions).map(|session| session.snapshot());
    let blocks = match &snapshot {
        Ok(snap) => {
            log::info!(
                "前端就绪：窗口={} / {} 块 / {} 停靠点 / 路径={:?}",
                window.label(),
                snap.blocks.len(),
                snap.stops,
                snap.path
            );
            snap.blocks.len()
        }
        Err(err) => {
            log::warn!("前端就绪，但{err}");
            0
        }
    };
    if mode.self_test {
        log::info!("自检：开始跑输入法合成事件");
        // 自检只跑主窗口；多窗口下别把合成事件打到别的窗口上
        if window.label() == "main" {
            let _ = window.eval("window.__mdviewSelfTest && window.__mdviewSelfTest()");
        }
    }
    blocks
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
fn ask_discard(
    app: &tauri::AppHandle,
    window: &tauri::Window,
    sessions: &Sessions,
    what: &str,
) -> bool {
    if app.state::<RunMode>().self_test {
        return true;
    }
    let dirty = sessions
        .get(window.label())
        .is_some_and(|session| session.is_dirty());
    if !dirty {
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
async fn confirm_discard(
    app: tauri::AppHandle,
    window: tauri::Window,
    sessions: tauri::State<'_, Sessions>,
    action: String,
) -> Result<bool, String> {
    let what = if action == "close" {
        "关闭窗口"
    } else {
        "打开另一个文件"
    };
    Ok(ask_discard(&app, &window, &sessions, what))
}

/// 用户已在确认框里点了「继续」：置放行标志再关窗，`CloseRequested` 不再拦。
#[tauri::command]
fn force_close(window: tauri::Window, guards: tauri::State<'_, CloseGuards>) {
    // 放行标志按窗口存：这里只放行正在关闭的这一个
    guards.of(window.label()).store(true, Ordering::SeqCst);
    let _ = window.close();
}

/// 文件 → 新建：换成未命名空文档。
///
/// 有未保存改动时先问一句；用户取消返回 `Ok(None)`，前端什么都不做。
///
/// 返回 `Result` 是 Tauri 对「带引用的 async 命令」的硬性要求（见 open_file）。
#[tauri::command]
async fn new_file(
    app: tauri::AppHandle,
    window: tauri::Window,
    sessions: tauri::State<'_, Sessions>,
) -> Result<Option<Snapshot>, String> {
    let session = session_of(window.label(), &sessions)?;
    if !ask_discard(&app, &window, &sessions, "新建文档") {
        return Ok(None);
    }
    log::info!("新建文档：窗口={} 切到未命名空文档", window.label());
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
    window: tauri::Window,
    sessions: tauri::State<'_, Sessions>,
    path: Option<PathBuf>,
) -> Result<Option<Snapshot>, String> {
    let session = session_of(window.label(), &sessions)?;
    // 先问再选文件：已经决定要换文档了，别让用户选完才发现不让开。
    // 没有未保存改动时这一步直接放行，不打扰。
    if !ask_discard(&app, &window, &sessions, "打开另一个文件") {
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
    log::info!("打开文件：{}（窗口={}）", path.display(), window.label());
    let snap = session.open_file(path.clone())?;
    // 自检反复打开样张，不该污染用户的最近列表。
    if !app.state::<RunMode>().self_test {
        remember_recent(&app, &path);
    }
    Ok(Some(snap))
}

/// 插入图片：选文件 → 复制到文档同级 `assets/` → 返回要写进源码的地址。
///
/// 复制而不是直接引用原路径，是因为用户多半从「下载」「桌面」里选图，绝对路径
/// 换机器或挪目录就断链。文档还没保存时没有「旁边」可放，只能退回绝对路径，
/// 由前端提示用户先保存再插图。
///
/// 返回 `Ok(None)` 表示用户取消了选择——前端什么都不做。
///
/// `path` 可选：正常菜单调用不传，走对话框；传入时直接用该文件——给端到端
/// 自检一条不弹窗也能走完整复制链路的注入点（与 `open_file` 同款）。
#[tauri::command]
async fn pick_image(
    app: tauri::AppHandle,
    window: tauri::Window,
    sessions: tauri::State<'_, Sessions>,
    path: Option<PathBuf>,
) -> Result<Option<String>, String> {
    let source = match path {
        Some(path) => path,
        None => {
            let picked = app
                .dialog()
                .file()
                .set_title("插入图片")
                .add_filter("图片", &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"])
                .blocking_pick_file();
            let Some(file) = picked else {
                return Ok(None);
            };
            file.into_path()
                .map_err(|_| "选中的图片路径无效".to_string())?
        }
    };
    let session = session_of(window.label(), &sessions)?;
    let Some(document) = session.watch_path() else {
        log::info!("插入图片：文档未保存，写绝对路径 {}", source.display());
        return Ok(Some(source.display().to_string()));
    };
    let src = copy_to_assets(&source, &document)?;
    log::info!("插入图片：{} → {}", source.display(), src);
    Ok(Some(src))
}

/// 把图片复制到文档同级 `assets/`，返回写进 Markdown 的相对地址。
///
/// 重名不覆盖：`图.png` 已存在就试 `图-1.png`、`图-2.png`……覆盖会悄悄毁掉
/// 之前插入的那张图，而这在编辑器里完全看不出来。
fn copy_to_assets(source: &std::path::Path, document: &std::path::Path) -> Result<String, String> {
    let dir = document
        .parent()
        .ok_or_else(|| "文档没有所在目录，无法放置图片".to_string())?;
    let assets = dir.join("assets");
    std::fs::create_dir_all(&assets).map_err(|err| format!("建不了 assets 目录：{err}"))?;

    let stem = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("image");
    let ext = source
        .extension()
        .and_then(|name| name.to_str())
        .unwrap_or("png");
    let mut name = format!("{stem}.{ext}");
    let mut index = 1;
    while assets.join(&name).exists() {
        if index > 999 {
            return Err("assets 里同名图片太多，换个文件名再试".to_string());
        }
        name = format!("{stem}-{index}.{ext}");
        index += 1;
    }
    std::fs::copy(source, assets.join(&name)).map_err(|err| format!("复制图片失败：{err}"))?;
    // 相对地址一律用正斜杠：Markdown 链接里的反斜杠在别的平台上会失效
    Ok(format!("assets/{name}"))
}

/// 文件 → 重新加载：把磁盘上的最新内容读回来，丢掉内存里的改动。
///
/// 只对已落盘的文档有意义；未命名文档没有磁盘来源，直接报错让前端提示，
/// 不要先弹「丢弃改动」再告诉用户没得载入。有未保存改动时和换文档一样先问一句。
#[tauri::command]
async fn reload_file(
    app: tauri::AppHandle,
    window: tauri::Window,
    sessions: tauri::State<'_, Sessions>,
) -> Result<Option<Snapshot>, String> {
    let session = session_of(window.label(), &sessions)?;
    if session.watch_path().is_none() {
        return Err("当前文档尚未保存，没有可重新加载的磁盘内容".to_string());
    }
    if !ask_discard(&app, &window, &sessions, "重新加载") {
        return Ok(None);
    }
    log::info!("重新加载（窗口={}）", window.label());
    session.reload_file()
}

/// 新窗口标签序号。`main` 留给启动窗口，新开的一律 `doc-N`。
static WINDOW_SEQ: AtomicUsize = AtomicUsize::new(1);

fn next_window_label() -> String {
    format!("doc-{}", WINDOW_SEQ.fetch_add(1, Ordering::Relaxed))
}

/// 标题栏先按文件名给一个，页面 boot 后会经 bridge 改成「脏标记 + 文件名」。
fn window_title(path: Option<&std::path::Path>) -> String {
    path.and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(|name| format!("{name} — mdview"))
        .unwrap_or_else(|| "未命名 — mdview".to_string())
}

/// 开一个新窗口，并给它配一份独立会话。
///
/// 会话必须先入注册表、再建窗口：页面 boot 会立刻 `invoke('snapshot')`，
/// 反过来的话那个 IPC 会找不到窗口对应的文档。
///
/// 窗口形态要与 `tauri.*.conf.json` 里的主窗口一致（Windows/Linux 自绘标题栏、
/// macOS 用 Overlay + 隐藏标题、Windows/macOS 带透明表层），否则新窗口会突然
/// 长出系统标题栏，或者因为 WebView2 的不透明白底把系统底材盖掉。
/// 几何交给 window-state 插件按标签恢复；没有记录时用主窗口同款 960×760。
fn create_window(
    app: &tauri::AppHandle,
    path: Option<PathBuf>,
) -> Result<tauri::WebviewWindow, String> {
    let label = next_window_label();
    let session = Session::open(path.clone())?;
    app.state::<Sessions>().insert(label.clone(), session);

    let url = tauri::WebviewUrl::App("index.html".into());
    let mut builder = tauri::WebviewWindowBuilder::new(app, label.clone(), url)
        .title(window_title(path.as_deref()))
        .inner_size(960.0, 760.0)
        .min_inner_size(680.0, 360.0)
        // 先隐藏、恢复完几何再露出来，和主窗口同一条路径
        .visible(false);
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
            // 透明表层：NSVisualEffectView 垫在 contentView 后面，窗口不透明就看不见。
            .transparent(true);
    }
    #[cfg(target_os = "windows")]
    {
        // 透明表层给 Mica 让位：WebView2 默认是不透明白底，材质会被它盖住；
        // 而前端已经按 backdrop_kind 把 CSS 底层清空，窗口不透明就成了一块白板。
        builder = builder.decorations(false).transparent(true);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        builder = builder.decorations(false);
    }

    let window = match builder.build() {
        Ok(window) => window,
        Err(err) => {
            // 窗口没建成，会话不能留在表里，否则 watcher 会一直盯着一份幽灵文档
            app.state::<Sessions>().remove(&label);
            return Err(format!("创建窗口失败：{err}"));
        }
    };
    // 与启动窗口走同一条材质路径；主题偏好由 backdrop 状态记住，避免闪亮色。
    {
        let backdrop = app.state::<backdrop::Backdrop>();
        let dark = backdrop.desired_dark();
        backdrop.apply(&window, dark);
    }
    if let Err(err) = window.show() {
        log::warn!("新窗口显示失败：{err}");
    }
    let _ = window.set_focus();

    if let Some(path) = &path
        && !app.state::<RunMode>().self_test
    {
        remember_recent(app, path);
    }
    log::info!("新建窗口：标签={label} 路径={:?}", path.as_deref());
    Ok(window)
}

/// 菜单动作只发给当前窗口。
///
/// 原生菜单是应用级的，如果 `app.emit` 广播，所有窗口都会执行同一条命令
/// （比如「撤销」在每个窗口各撤一次）。优先焦点窗口，退而求其次取任意一个。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn emit_menu_action(app: &tauri::AppHandle, action: &str) {
    let windows = app.webview_windows();
    let target = windows
        .values()
        .find(|window| window.is_focused().unwrap_or(false))
        .or_else(|| windows.values().next());
    if let Some(window) = target {
        let _ = window.emit("menu-action", action);
    }
}

/// 记一条最近打开，并让所有窗口与原生菜单同步。
///
/// 最近列表是应用级数据，多窗口下 A 窗口打开文件后 B 窗口的菜单也应立刻更新，
/// 所以额外广播一次 `recent-changed`，前端收到后重新 `recent_files`。
fn remember_recent(app: &tauri::AppHandle, path: &std::path::Path) {
    recent::remember(app, path);
    refresh_native_menu(app);
    let _ = app.emit("recent-changed", ());
}

/// 聚焦任意一个还活着的窗口。返回 false 表示一个窗口都没有。
fn focus_any_window(app: &tauri::AppHandle) -> bool {
    match app.webview_windows().values().next() {
        Some(window) => {
            let _ = window.set_focus();
            true
        }
        None => false,
    }
}

/// 文件 → 新建窗口。前端只发一条无参消息。
///
/// 必须是 async：同步命令在主线程执行，而 `WebviewWindowBuilder::build` 要等
/// 主线程的消息循环把 webview 建出来——在命令处理器里直接建窗会死锁。
/// 这里退回 async 线程池，再用 `run_on_main_thread` 把建窗动作排进主循环。
#[tauri::command]
async fn new_window(app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(err) = create_window(&handle, None) {
            log::error!("新建窗口失败：{err}");
        }
    })
    .map_err(|err| format!("新建窗口失败：{err}"))
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
        "reload-file" => "reload-file",
        "save" => "save",
        "save-as" => "save-as",
        "new-file" => "new-file",
        "new-window" => "new-window",
        "undo" => "undo",
        "redo" => "redo",
        "bold" => "bold",
        "italic" => "italic",
        "strike" => "strike",
        "inline-code" => "inline-code",
        "insert-image" => "insert-image",
        "insert-link" => "insert-link",
        "insert-table" => "insert-table",
        "insert-hr" => "insert-hr",
        "insert-code" => "insert-code",
        "insert-quote" => "insert-quote",
        "insert-task" => "insert-task",
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
        .item(&item("new-window", "新建窗口")?)
        .item(&item("open-file", "打开文件…")?)
        .item(&recent_menu)
        .separator()
        .item(&item("reload-file", "重新加载")?)
        .separator()
        .item(&item("save", "保存")?)
        .item(&item("save-as", "另存为…")?)
        .separator()
        .close_window_with_text("关闭窗口")
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "编辑")
        .item(&item("undo", "撤销")?)
        .item(&item("redo", "重做")?)
        .separator()
        .item(&item("bold", "加粗")?)
        .item(&item("italic", "斜体")?)
        .item(&item("strike", "删除线")?)
        .item(&item("inline-code", "行内代码")?)
        .separator()
        .item(&item("cut", "剪切")?)
        .item(&item("copy", "复制")?)
        .item(&item("paste", "粘贴")?)
        .item(&item("select-all", "全选")?)
        .separator()
        .item(&item("find", "查找…")?)
        .item(&item("replace", "替换…")?)
        .build()?;

    let insert_menu = SubmenuBuilder::new(app, "插入")
        .item(&item("insert-image", "图片…")?)
        .item(&item("insert-link", "链接…")?)
        .separator()
        .item(&item("insert-table", "表格")?)
        .item(&item("insert-hr", "分隔线")?)
        .separator()
        .item(&item("insert-code", "代码块")?)
        .item(&item("insert-quote", "引用")?)
        .item(&item("insert-task", "任务列表")?)
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
        .item(&insert_menu)
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

    // 外部修改监听：起一个后台线程轮询所有窗口的 Watcher。命中的事件还要经
    // Session 用指纹二次确认（挡掉自己保存产生的事件），再 emit 给对应窗口。
    // 自检模式不开监听——自检样张是临时文件，且不应受外部事件干扰。
    // 线程每轮读每个会话的当前路径：打开文件后路径变了就重建 Watcher，
    // 否则会继续盯着旧文件。NotifyWatcher 的回调绑定构造时的路径，必须重建。

    let mut builder = tauri::Builder::default();

    // 单例插件必须第一个注册：它要在其它插件之前抢占实例锁。
    // 回调运行在「已经活着」的那个实例里，第二个实例已被引擎拦截并退出。
    builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        // 有文件参数就新开一个窗口打开它——这是多窗口的核心：第二个实例不是
        // 把文件塞进现有窗口（那会顶掉用户正在编辑的文档），而是各窗口各一份会话。
        // 跳过 --self-test 这类开关，只认 Markdown 扩展名的路径。
        let paths: Vec<PathBuf> = args
            .iter()
            .filter(|arg| !arg.starts_with('-'))
            .map(PathBuf::from)
            .collect();
        let target = first_markdown(&paths).cloned();
        let handle = app.clone();
        // 窗口创建必须在主线程；回调线程由插件决定，统一投递。
        if let Err(err) = app.run_on_main_thread(move || {
            if let Some(path) = target {
                log::info!("单例收到打开请求：{}", path.display());
                if let Err(err) = create_window(&handle, Some(path)) {
                    log::error!("{err}");
                    focus_any_window(&handle);
                }
            } else if !focus_any_window(&handle) {
                // 所有窗口都关了（macOS 常见）：新开一个，不然没有任何界面
                if let Err(err) = create_window(&handle, None) {
                    log::error!("{err}");
                }
            }
        }) {
            log::error!("投递单例打开请求失败：{err}");
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
        // 会话按窗口标签存；关窗守卫同理。启动窗口的会话在 setup 里登记。
        .manage(Sessions::default())
        .manage(CloseGuards::default())
        .manage(backdrop::Backdrop::default())
        .manage(RunMode { self_test })
        .manage(update::UpdateState::new())
        // 关窗守卫：有未保存改动就拦住，让前端问一句再放行。
        // 标题栏的 ×、Alt+F4、菜单里的「关闭窗口」最后都走这里，一处拦全部。
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                let app = window.app_handle();
                // swap 而不是 load：放行标志是**一次性**的。万一这次关闭后来没成
                // （比如被别的插件拦下），下一次关窗还得重新问，不能永久放开。
                if app
                    .state::<CloseGuards>()
                    .of(window.label())
                    .swap(false, Ordering::SeqCst)
                {
                    return;
                }
                // 会话缺失说明窗口正在销毁，直接放行，别拦成关不掉的窗口
                let dirty = app
                    .state::<Sessions>()
                    .get(window.label())
                    .is_some_and(|session| session.is_dirty());
                if !dirty {
                    return;
                }
                api.prevent_close();
                let _ = window.emit("close-requested", ());
            }
            // 窗口真的销毁了：把会话与守卫从注册表摘掉，watcher 下一轮也会撤监听。
            WindowEvent::Destroyed => {
                let app = window.app_handle();
                app.state::<Sessions>().remove(window.label());
                app.state::<CloseGuards>().remove(window.label());
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
            reload_file,
            pick_image,
            new_file,
            new_window,
            confirm_discard,
            force_close,
            pick_save_path,
            use_custom_titlebar,
            backdrop::backdrop_kind,
            backdrop::sync_backdrop,
            backdrop::set_backdrop_mode,
            update::check_update,
            update::download_update,
            update::cancel_update_download,
            update::install_update
        ])
        .setup(move |app| {
            // 启动窗口由 conf 创建，先给它登记会话，页面 boot 才查得到。
            app.state::<Sessions>().insert("main", session);

            // 窗口按 conf 以隐藏状态创建，插件在创建时就恢复了尺寸/位置/最大化，
            // 这里才让它露面——顺序反了就会看到窗口先小后大地跳一下。
            // 注意：这一步早于页面加载，所以可能有极短的白底；若将来要消掉，
            // 改成前端首帧后再 invoke 一个 show 命令即可。
            if let Some(window) = app.get_webview_window("main") {
                {
                    let backdrop = app.state::<backdrop::Backdrop>();
                    let dark = backdrop.desired_dark();
                    backdrop.apply(&window, dark);
                }
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
                        emit_menu_action(app, id);
                    } else if let Some(action) = menu_action(id) {
                        emit_menu_action(app, action);
                    }
                });
            }

            if self_test {
                return Ok(());
            }
            let handle = app.handle().clone();
            let sessions = app.state::<Sessions>().inner().clone();
            std::thread::spawn(move || watch_loop(handle, sessions));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("窗口启动失败")
        .run(|app, event| {
            // macOS：所有窗口关掉后应用不退出，点 Dock 图标会收到 Reopen，
            // 这时必须补一个窗口，否则「点开没反应」。
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } = event
                && !has_visible_windows
                && let Err(err) = create_window(app, None)
            {
                log::error!("重开窗口失败：{err}");
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_are_scoped_by_window_label() {
        let sessions = Sessions::default();
        sessions.insert("main", Session::from_text("甲", None));
        sessions.insert("doc-1", Session::from_text("乙", None));

        assert_eq!(sessions.get("main").unwrap().snapshot().source, "甲");
        assert_eq!(sessions.get("doc-1").unwrap().snapshot().source, "乙");
        assert!(sessions.get("doc-2").is_none(), "没登记的窗口不应拿到会话");

        // 关掉一个窗口不能影响另一个
        sessions.remove("doc-1");
        assert!(sessions.get("doc-1").is_none());
        assert_eq!(sessions.get("main").unwrap().snapshot().source, "甲");
    }

    #[test]
    fn close_guards_are_per_window() {
        let guards = CloseGuards::default();
        guards.of("main").store(true, Ordering::SeqCst);
        assert!(guards.of("main").load(Ordering::SeqCst));
        assert!(
            !guards.of("doc-1").load(Ordering::SeqCst),
            "一个窗口的放行标志不能顺带放行别的窗口"
        );

        guards.remove("main");
        assert!(
            !guards.of("main").load(Ordering::SeqCst),
            "清除后重建的守卫必须是干净的一次性标志"
        );
    }

    #[test]
    fn window_labels_are_unique_and_never_main() {
        let first = next_window_label();
        let second = next_window_label();
        assert_ne!(first, second);
        assert!(first.starts_with("doc-"));
        assert!(second.starts_with("doc-"));
        assert_ne!(first, "main", "main 永远留给启动窗口");
    }

    #[test]
    fn window_title_prefers_the_file_name() {
        assert_eq!(
            window_title(Some(std::path::Path::new("/a/b/稿.md"))),
            "稿.md — mdview"
        );
        assert_eq!(window_title(None), "未命名 — mdview");
    }

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
    fn copy_to_assets_never_overwrites_and_returns_relative_path() {
        let dir = std::env::temp_dir().join(format!("mdview-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("建测试目录失败");
        let source = dir.join("src").join("图.png");
        std::fs::write(&source, b"first").expect("写测试图片失败");
        let document = dir.join("doc.md");
        std::fs::write(&document, "").expect("写测试文档失败");

        let first = copy_to_assets(&source, &document).expect("首次复制应成功");
        assert_eq!(first, "assets/图.png", "相对地址要用正斜杠");

        std::fs::write(&source, b"second").expect("改写测试图片失败");
        let second = copy_to_assets(&source, &document).expect("再次复制应成功");
        assert_eq!(second, "assets/图-1.png", "重名必须改名，不能覆盖");
        assert_eq!(
            std::fs::read(dir.join("assets").join("图.png")).unwrap(),
            b"first",
            "之前那张图要原样保留——覆盖在编辑器里看不出来"
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

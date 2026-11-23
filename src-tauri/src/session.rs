//! 窗口与前端之间的会话层。
//!
//! 这一层**不依赖任何 Tauri 类型**，因此可以用单元测试完整验证 IPC 契约：
//! 前端发的 JSON 能否翻译成 `Message`、回复里的增量帧与光标是否正确。
//!
//! 前端只做两件事：把停靠点画成像素、把输入翻译成消息。文本真值始终在内核里。

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use mdview_app::{AppState, Effect, FileStore, Message, StdFileStore, ViewMode};
use mdview_markdown::{BlockFrame, mime_for, resolve_local};
use serde::{Deserialize, Serialize};

/// 前端发来的消息。
///
/// 刻意不复用 `mdview_app::Message`：内核语义不该为了序列化去背上 serde 依赖，
/// 这层只负责翻译。`kind` 字段与 `src/editor/editor.ts` 里的一一对应。
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Wire {
    InsertText {
        text: String,
    },
    Backspace,
    DeleteForward,
    Newline,
    HardBreak,
    MoveCaret {
        delta: i32,
        extend: bool,
    },
    CompositionStart,
    CompositionCommit {
        text: String,
    },
    /// 取消组合（Esc / 失焦 / 空提交）。不送这一条，内核会永远挂着 composing。
    CompositionCancel,
    /// 前端按「停靠点 → 像素」映射算出落点，直接报全局序号
    SetCaret {
        stop: usize,
        extend: bool,
    },
    Select {
        anchor: usize,
        head: usize,
    },
    Undo,
    Redo,
    Save,
    /// 另存为：前端在弹出的保存对话框里拿到路径后补发这条。
    /// 与 `Save` 分开，是因为「写哪个文件」这个决定只有宿主能做。
    SaveTo {
        path: String,
    },
    /// 全选（Ctrl+A）。原生选区已被清掉（见 `selection.js`），全选只能走内核
    SelectAll,
    Copy,
    Cut,
    /// 源码视图整篇替换：把 textarea 里的 Markdown 源文本整篇写回内核。
    ReplaceSource {
        text: String,
    },
    SetMode {
        mode: String,
    },
    /// 表格内跨格。delta 为 1 / -1
    TableMoveCell {
        delta: i32,
    },
    /// Tab / Shift+Tab：列表按层级缩进，其余块插入 / 删除两个空格。
    Indent {
        outdent: bool,
    },
    /// 改链接 / 图片：只带要改的字段，缺省表示保持原值。
    EditLink {
        stop: usize,
        url: Option<String>,
        text: Option<String>,
        #[serde(default)]
        remove: bool,
    },
    TableInsertRow {
        after: bool,
    },
    TableDeleteRow,
    TableInsertColumn {
        after: bool,
    },
    TableDeleteColumn,
}

impl Wire {
    fn into_message(self) -> Message {
        match self {
            Wire::InsertText { text } => Message::InsertText { text },
            Wire::Backspace => Message::Backspace,
            Wire::DeleteForward => Message::DeleteForward,
            Wire::Newline => Message::Newline,
            Wire::HardBreak => Message::HardBreak,
            Wire::MoveCaret { delta, extend } => Message::MoveCaret {
                delta: delta as isize,
                extend,
            },
            Wire::CompositionStart => Message::CompositionStart,
            Wire::CompositionCommit { text } => Message::CompositionCommit { text },
            Wire::CompositionCancel => Message::CompositionCancel,
            Wire::SetCaret { stop, extend } => Message::SetCaret { stop, extend },
            Wire::Select { anchor, head } => Message::Select { anchor, head },
            Wire::Undo => Message::Undo,
            Wire::Redo => Message::Redo,
            Wire::Save => Message::Save,
            Wire::SaveTo { path } => Message::SaveTo {
                path: PathBuf::from(path),
            },
            Wire::SelectAll => Message::SelectAll,
            Wire::Copy => Message::Copy,
            Wire::Cut => Message::Cut,
            Wire::ReplaceSource { text } => Message::ReplaceSource { text },
            Wire::SetMode { mode } => Message::SetMode(match mode.as_str() {
                "reading" => ViewMode::Reading,
                "source" => ViewMode::Source,
                _ => ViewMode::Wysiwyg,
            }),
            Wire::TableMoveCell { delta } => Message::TableMoveCell { delta },
            Wire::Indent { outdent } => Message::Indent { outdent },
            Wire::EditLink {
                stop,
                url,
                text,
                remove,
            } => Message::EditLink {
                stop,
                url,
                text,
                remove,
            },
            Wire::TableInsertRow { after } => Message::TableInsertRow { after },
            Wire::TableDeleteRow => Message::TableDeleteRow,
            Wire::TableInsertColumn { after } => Message::TableInsertColumn { after },
            Wire::TableDeleteColumn => Message::TableDeleteColumn,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BlockDto {
    pub id: usize,
    pub base: usize,
    pub end: usize,
    pub html: String,
}

#[derive(Debug, Serialize, Default)]
pub struct CaretDto {
    pub stop: usize,
    pub char_offset: usize,
    pub block: Option<usize>,
    pub visual: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub path: Option<String>,
    pub dirty: bool,
    pub stops: usize,
    /// 正文字数（可见文本，不含 Markdown 标记与空白）。状态栏直接显示。
    pub words: usize,
    pub blocks: Vec<BlockDto>,
    pub caret: Option<CaretDto>,
    /// 选区停靠点区间（已归一），`null` 表示没有选区
    pub selection: Option<[usize; 2]>,
    /// 大纲（标题项），供侧栏渲染与滚动定位
    pub outline: Vec<OutlineDto>,
    /// Markdown 源文本真值。源码视图直接显示与编辑这一份，不是渲染后的 HTML。
    pub source: String,
}

#[derive(Debug, Serialize, Default)]
pub struct Reply {
    pub revision: u64,
    pub dirty: bool,
    /// 正文字数，随每次编辑回包，状态栏不必等下一次快照。
    pub words: usize,
    /// 增量重绘：替换块节点 [start, end)
    pub patch: Option<PatchDto>,
    /// 全量重绘：整个块列表
    pub full: Option<Vec<BlockDto>>,
    pub caret: Option<CaretDto>,
    pub selection: Option<[usize; 2]>,
    /// 要写入系统剪贴板的文本（Ctrl+C / Ctrl+X）。前端拿到后自己写剪贴板，
    /// 窗口层不去碰它——那需要额外的宿主能力，而文本真值本来就在内核里。
    pub clipboard: Option<String>,
    /// 同一段内容的 HTML 版本，供富文本粘贴目标（Word / 飞书）保留格式。
    /// 由选区源码重新解析得到；解析失败时退化为纯文本，不影响复制。
    pub clipboard_html: Option<String>,
    pub conflict: bool,
    pub saved: bool,
    /// 内核没有落盘路径，需要宿主先问用户要一个（另存为）。
    /// 置位时 `dirty` / `saved` 都不动：什么都没发生。
    pub needs_path: bool,
    /// 写盘失败的原始原因。与 `conflict` 分开，前端据此给不同提示。
    pub error: Option<String>,
    /// 回包时的文档路径。另存为成功后前端据此更新标题栏。
    pub path: Option<String>,
    /// 大纲随每次编辑回包，前端据此增量更新侧栏
    pub outline: Option<Vec<OutlineDto>>,
    /// 内核里的 Markdown 源文本真值，供源码视图在任何编辑后同步。
    pub source: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PatchDto {
    pub start: usize,
    pub end: usize,
    pub old_len: usize,
    pub new_len: usize,
    pub html: String,
}

/// 大纲项。`block` 是块索引，前端据此找到对应 DOM 元素定位滚动。
#[derive(Debug, Serialize, Clone)]
pub struct OutlineDto {
    pub level: u8,
    pub title: String,
    pub block: usize,
}

#[derive(Clone)]
pub struct Session {
    state: Arc<Mutex<AppState>>,
    files: StdFileStore,
}

impl Session {
    pub fn open(path: Option<PathBuf>) -> Result<Self, String> {
        let files = StdFileStore;
        let content = match &path {
            Some(path) => files
                .load(path)
                .map_err(|err| format!("{}: {err}", path.display()))?,
            None => String::new(),
        };
        Ok(Self::from_text(&content, path))
    }

    pub fn from_text(content: &str, path: Option<PathBuf>) -> Self {
        let files = StdFileStore;
        Self {
            state: Arc::new(Mutex::new(AppState::open(content, path, &files))),
            files,
        }
    }

    /// 换成一份空文档（未命名）。
    ///
    /// 走和启动同一条构造路径：解析、指纹、光标归零、脏标记清零都一致。
    /// 调用方负责在 `dirty` 时先问过用户——这里不做确认，内核不认识对话框。
    pub fn new_file(&self) -> Snapshot {
        let fresh = AppState::open("", None, &self.files);
        *self.state.lock().expect("状态锁中毒") = fresh;
        self.snapshot()
    }

    /// 是否有未保存的改动。关窗与换文档前的守卫据此决定要不要问一句。
    pub fn is_dirty(&self) -> bool {
        self.state.lock().is_ok_and(|app| app.dirty)
    }

    /// 打开磁盘上的另一个文件，整体替换当前文档状态。
    ///
    /// 复用 `AppState::open` 走和启动同一条构造路径（解析、指纹、光标归零都一致）；
    /// 读盘失败时返回错误且不改动现有状态。返回新快照供前端全量重绘。
    pub fn open_file(&self, path: PathBuf) -> Result<Snapshot, String> {
        let content = self
            .files
            .load(&path)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        let fresh = AppState::open(&content, Some(path), &self.files);
        *self.state.lock().expect("状态锁中毒") = fresh;
        Ok(self.snapshot())
    }

    /// 从磁盘重新读入当前文件，整体替换文档状态。
    ///
    /// 只对已经落盘的文档有意义：未命名文档没有磁盘来源，返回 `Ok(None)`，
    /// 由调用方决定怎么提示。读盘失败时不改动现有状态——先 `load` 再替换，
    /// 文件被删或没权限时用户的缓冲区还在。
    ///
    /// 撤销栈随旧 `AppState` 一起丢弃：重新加载之后的历史不该混进旧内容。
    pub fn reload_file(&self) -> Result<Option<Snapshot>, String> {
        let path = {
            let app = self.state.lock().expect("状态锁中毒");
            app.path.clone()
        };
        let Some(path) = path else {
            return Ok(None);
        };
        self.open_file(path).map(Some)
    }

    /// 把图片 URL 解析为当前文档目录内的文件，并给出 MIME 类型。
    ///
    /// 沙箱在 `mdview_markdown::resolve_local` 里做；这里只负责提供基准目录。
    /// 未保存的文档退到进程工作目录，行为仍然可预期。
    pub fn resolve_image(&self, url: &str) -> Option<(PathBuf, &'static str)> {
        let app = self.state.lock().ok()?;
        let base = app
            .path
            .as_deref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())?;
        let path = resolve_local(&base, url)?;
        Some((path.clone(), mime_for(&path)))
    }

    pub fn snapshot(&self) -> Snapshot {
        let app = self.state.lock().expect("状态锁中毒");
        Snapshot {
            revision: app.document.revision().0,
            path: app.path.as_ref().map(|p| p.display().to_string()),
            dirty: app.dirty,
            stops: app.layout.stops(),
            words: app.word_count(),
            blocks: blocks_of(app.frames()),
            caret: caret_of(&app),
            selection: selection_of(&app),
            outline: outline_of(&app),
            source: app.document.text(),
        }
    }

    pub fn dispatch(&self, wire: Wire) -> Reply {
        let mut app = self.state.lock().expect("状态锁中毒");
        let effects = app.update(wire.into_message(), &self.files);
        let mut reply = Reply {
            revision: app.document.revision().0,
            dirty: app.dirty,
            words: app.word_count(),
            caret: caret_of(&app),
            selection: selection_of(&app),
            outline: Some(outline_of(&app)),
            path: app.path.as_ref().map(|p| p.display().to_string()),
            source: Some(app.document.text()),
            ..Reply::default()
        };
        for effect in effects {
            match effect {
                Effect::Patch { patch, .. } => {
                    reply.patch = Some(PatchDto {
                        start: patch.replace.start,
                        end: patch.replace.end,
                        old_len: patch.old_len,
                        new_len: patch.new_len,
                        html: patch.html,
                    });
                }
                Effect::Render => reply.full = Some(blocks_of(app.frames())),
                Effect::MoveCursor {
                    stop,
                    char_offset,
                    block,
                    visual,
                } => {
                    reply.caret = Some(CaretDto {
                        stop,
                        char_offset,
                        block,
                        visual,
                    });
                }
                Effect::Persist => reply.saved = true,
                Effect::NeedsPath => reply.needs_path = true,
                Effect::WriteFailed { message } => reply.error = Some(message),
                Effect::Conflict => reply.conflict = true,
                Effect::Clipboard { text } => {
                    // 复制出去的是源码；富文本目标需要 HTML，就地按同一份源码重渲染
                    reply.clipboard_html = Some(
                        mdview_markdown::MarkdownParser::gfm()
                            .parse(&text)
                            .to_html(&text),
                    );
                    reply.clipboard = Some(text);
                }
                // 增量解析调度由内核内部实现；选区高亮随 Reply 顶层字段一起下发
                Effect::ScheduleParse { .. } | Effect::Selection { .. } => {}
            }
        }
        // 撤销/重做等结构性变化没有 MoveCursor 时，光标仍要重新报一次
        if reply.caret.is_none() {
            reply.caret = caret_of(&app);
        }
        reply
    }

    /// 磁盘内容是否被外部改动。用于 Watcher 事件的二次确认，挡掉自己保存
    /// 产生的事件（保存后指纹已刷新），只把真正的外部改动报给 UI。
    pub fn externally_modified(&self) -> bool {
        let app = self.state.lock().expect("状态锁中毒");
        app.externally_modified(&self.files)
    }

    /// 是否有可监听的文件路径（无标题文档不需要 Watcher）。
    pub fn watch_path(&self) -> Option<PathBuf> {
        self.state.lock().expect("状态锁中毒").path.clone()
    }

    #[cfg(test)]
    fn text(&self) -> String {
        self.state.lock().expect("状态锁中毒").document.text()
    }
}

fn blocks_of(frames: &[BlockFrame]) -> Vec<BlockDto> {
    frames
        .iter()
        .map(|frame| BlockDto {
            id: frame.id,
            base: frame.base,
            end: frame.end,
            html: frame.html.clone(),
        })
        .collect()
}

fn outline_of(app: &AppState) -> Vec<OutlineDto> {
    app.parsed
        .outline
        .iter()
        .map(|item| OutlineDto {
            level: item.level,
            title: item.title.clone(),
            block: item.block_id.0,
        })
        .collect()
}

fn caret_of(app: &AppState) -> Option<CaretDto> {
    let stop = app.selection.head;
    let (block, visual) = match app.layout.locate(stop) {
        Some((id, local)) => (
            Some(id.0),
            app.layout.layout(id).and_then(|l| l.visual(local)),
        ),
        None => (None, None),
    };
    let char_offset = app
        .layout
        .byte(stop)
        .map(|byte| app.document.byte_to_char(byte))
        .unwrap_or_else(|| app.document.len_chars());
    Some(CaretDto {
        stop,
        char_offset,
        block,
        visual,
    })
}

fn selection_of(app: &AppState) -> Option<[usize; 2]> {
    if app.selection.is_collapsed() {
        return None;
    }
    let (lo, hi) = app.selection.normalized();
    Some([lo, hi])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(src: &str) -> Session {
        Session::from_text(src, None)
    }

    #[test]
    fn image_paths_resolve_against_document_dir() {
        let dir = std::env::temp_dir().join("mdview-session-assets");
        let doc = dir.join("doc.md");
        let session = Session::from_text("![图](img/a.png)", Some(doc));

        assert_eq!(
            session.resolve_image("img/a.png"),
            Some((dir.join("img").join("a.png"), "image/png"))
        );
        assert_eq!(session.resolve_image("../secret.png"), None);
        assert_eq!(session.resolve_image("https://example.com/a.png"), None);
    }

    #[test]
    fn reload_reads_the_file_back_from_disk() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mdview-reload-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.md");
        std::fs::write(&path, "旧内容").unwrap();
        let session = Session::open(Some(path.clone())).unwrap();

        // 模拟别的进程改文件
        std::fs::write(&path, "新内容").unwrap();
        let snap = session.reload_file().unwrap().expect("有路径就该返回快照");
        assert_eq!(session.text(), "新内容");
        assert!(!session.is_dirty(), "重新加载后应当是干净状态");
        assert_eq!(snap.source, "新内容");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reload_without_a_path_does_nothing() {
        let session = session("未命名");
        assert!(session.reload_file().unwrap().is_none());
        assert_eq!(session.text(), "未命名");
    }

    #[test]
    fn wire_json_matches_frontend_contract() {
        // 这两个字面量必须与前端 src/editor/editor.ts 里发的一致，改一边就要改另一边
        let wire: Wire = serde_json::from_str(r#"{"kind":"insertText","text":"甲"}"#).unwrap();
        assert!(matches!(wire, Wire::InsertText { ref text } if text == "甲"));

        let wire: Wire =
            serde_json::from_str(r#"{"kind":"setCaret","stop":3,"extend":true}"#).unwrap();
        assert!(matches!(
            wire,
            Wire::SetCaret {
                stop: 3,
                extend: true
            }
        ));

        assert!(serde_json::from_str::<Wire>(r#"{"kind":"backspace"}"#).is_ok());
        // 取消组合漏接会在内核里留下永久挂起，契约里必须有这一条
        let wire: Wire = serde_json::from_str(r#"{"kind":"compositionCancel"}"#).unwrap();
        assert!(matches!(wire, Wire::CompositionCancel));
        // 全选 / 复制 / 剪切：前端只发 kind，没有参数
        let wire: Wire = serde_json::from_str(r#"{"kind":"selectAll"}"#).unwrap();
        assert!(matches!(wire, Wire::SelectAll));
        let wire: Wire = serde_json::from_str(r#"{"kind":"copy"}"#).unwrap();
        assert!(matches!(wire, Wire::Copy));
        let wire: Wire = serde_json::from_str(r#"{"kind":"cut"}"#).unwrap();
        assert!(matches!(wire, Wire::Cut));
        // 源码视图整篇替换：camelCase 的 kind，带原始 Markdown 文本
        let wire: Wire =
            serde_json::from_str("{\"kind\":\"replaceSource\",\"text\":\"# 标题\\n\\n正文\"}")
                .unwrap();
        assert!(matches!(wire, Wire::ReplaceSource { ref text } if text == "# 标题\n\n正文"));

        // 表格操作：前端用 camelCase 的 kind，内核收到的是同一组语义
        let wire: Wire = serde_json::from_str(r#"{"kind":"tableMoveCell","delta":1}"#).unwrap();
        assert!(matches!(wire, Wire::TableMoveCell { delta: 1 }));
        let wire: Wire = serde_json::from_str(r#"{"kind":"tableInsertRow","after":true}"#).unwrap();
        assert!(matches!(wire, Wire::TableInsertRow { after: true }));
        assert!(serde_json::from_str::<Wire>(r#"{"kind":"tableDeleteRow"}"#).is_ok());
        let wire: Wire =
            serde_json::from_str(r#"{"kind":"tableInsertColumn","after":false}"#).unwrap();
        assert!(matches!(wire, Wire::TableInsertColumn { after: false }));
        assert!(serde_json::from_str::<Wire>(r#"{"kind":"tableDeleteColumn"}"#).is_ok());
        // Tab / Shift+Tab：布尔字段决定方向
        let wire: Wire = serde_json::from_str(r#"{"kind":"indent","outdent":true}"#).unwrap();
        assert!(matches!(wire, Wire::Indent { outdent: true }));
        // 链接编辑：url/text 可缺省（表示保持原值），remove 缺省为 false
        let wire: Wire =
            serde_json::from_str(r#"{"kind":"editLink","stop":3,"url":"https://b"}"#).unwrap();
        assert!(matches!(
            wire,
            Wire::EditLink {
                stop: 3,
                url: Some(_),
                text: None,
                remove: false
            }
        ));
    }

    #[test]
    fn snapshot_carries_raw_markdown_source() {
        let src = "# 标题\n\n**粗体** 与 `代码`";
        let snap = session(src).snapshot();
        assert_eq!(
            snap.source, src,
            "快照必须带原始 Markdown，不是渲染后的 HTML"
        );
        assert!(!snap.source.contains("<h1"), "下发的不能是 to_html 产物");
    }

    #[test]
    fn replace_source_rewrites_whole_document_and_can_undo() {
        let session = session("旧内容");
        let reply = session.dispatch(Wire::ReplaceSource {
            text: "# 新标题\n\n新正文".to_string(),
        });
        assert_eq!(session.text(), "# 新标题\n\n新正文");
        assert!(reply.dirty, "源码视图的编辑必须置脏标记");
        assert_eq!(reply.source.as_deref(), Some("# 新标题\n\n新正文"));
        // 整篇替换是一步撤销：撤销后回到替换前的整篇内容
        session.dispatch(Wire::Undo);
        assert_eq!(session.text(), "旧内容");
    }

    #[test]
    fn set_mode_does_not_change_rendered_blocks() {
        // 回归：模式切换本身不改变正文投影，差异由前端按模式切换显示哪块 DOM 决定
        let session = session("# 标题\n\n正文");
        let before = session.snapshot().blocks;
        let reply = session.dispatch(Wire::SetMode {
            mode: "source".to_string(),
        });
        let after = session.snapshot().blocks;
        assert_eq!(before.len(), after.len());
        assert_eq!(
            before[0].html, after[0].html,
            "同一块在不同模式下 HTML 应一致"
        );
        assert!(
            reply.source.is_some(),
            "切到源码视图的回包要带 source 供前端同步"
        );
    }

    #[test]
    fn table_ops_flow_through_dispatch() {
        let table = "| 甲 | 乙 |\n| --- | --- |\n| 丙 | 丁 |\n";
        let session = session(table);
        // 光标停在第一格，Tab 应跨到第二格并回报新位置
        let first = session.dispatch(Wire::SetCaret {
            stop: 0,
            extend: false,
        });
        let reply = session.dispatch(Wire::TableMoveCell { delta: 1 });
        assert!(reply.caret.is_some(), "跨格必须回报新光标");
        assert_ne!(first.caret.map(|c| c.stop), reply.caret.map(|c| c.stop));
    }

    #[test]
    fn select_all_then_copy_reports_the_whole_source() {
        let session = session("甲\n\n乙");
        let reply = session.dispatch(Wire::SelectAll);
        assert_eq!(
            reply.selection,
            Some([0, session.snapshot().stops - 1]),
            "全选要覆盖到最后一个停靠点"
        );
        // 全选不移动光标，避免视口被拽到文末
        assert_eq!(reply.caret.unwrap().stop, 0);

        let reply = session.dispatch(Wire::Copy);
        assert_eq!(reply.clipboard.as_deref(), Some("甲\n\n乙"));
        // 复制不改文档
        assert_eq!(session.text(), "甲\n\n乙");
        assert!(!reply.dirty);
    }

    #[test]
    fn copy_reports_source_and_a_rich_html_version() {
        let session = session("**粗体**");
        session.dispatch(Wire::SelectAll);
        let reply = session.dispatch(Wire::Copy);
        assert_eq!(reply.clipboard.as_deref(), Some("**粗体**"));
        let html = reply.clipboard_html.expect("复制要附带 HTML 版本");
        assert!(
            html.contains("<strong>粗体</strong>"),
            "富文本复制应保留加粗，实际：{html}"
        );
        // 复制不改文档
        assert_eq!(session.text(), "**粗体**");
    }

    #[test]
    fn cut_returns_the_text_and_deletes_it() {
        let session = session("甲乙丙");
        session.dispatch(Wire::Select { anchor: 3, head: 6 });
        let reply = session.dispatch(Wire::Cut);
        assert_eq!(reply.clipboard.as_deref(), Some("乙"));
        assert_eq!(session.text(), "甲丙");
        assert!(reply.patch.is_some(), "剪切同样走增量重绘");
        assert_eq!(reply.selection, None);
        assert!(reply.dirty);
    }

    #[test]
    fn open_file_replaces_state_and_updates_path() {
        let session = session("旧内容");
        session.dispatch(Wire::InsertText {
            text: "未保存".into(),
        });
        assert!(session.snapshot().dirty);

        let dir = std::env::temp_dir().join(format!("mdview-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.md");
        std::fs::write(&path, "# 新标题\n\n正文").unwrap();

        let snap = session.open_file(path.clone()).unwrap();
        assert_eq!(session.text(), "# 新标题\n\n正文");
        assert_eq!(snap.path.as_deref(), Some(path.to_str().unwrap()));
        assert!(!snap.dirty, "打开新文件后脏标记应清空");
        assert_eq!(snap.revision, 0);
        assert_eq!(snap.blocks.len(), 2, "新文档应解析出标题块和正文块");
        assert!(snap.stops > 0);

        // 读不到文件时不能破坏当前状态
        let missing = dir.join("不存在.md");
        assert!(session.open_file(missing).is_err());
        assert_eq!(session.text(), "# 新标题\n\n正文");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn snapshot_exposes_blocks_and_caret() {
        let snap = session("甲\n\n乙").snapshot();
        assert_eq!(snap.blocks.len(), 2);
        assert_eq!(snap.revision, 0);
        assert!(!snap.dirty);
        assert!(snap.blocks[1].html.contains("乙"));
        // 块首停靠点序号必须连续，前端据此拼接
        assert_eq!(snap.blocks[1].base, snap.blocks[0].end + 1);
        assert_eq!(snap.caret.unwrap().stop, 0);
    }

    #[test]
    fn snapshot_reports_word_count_and_reply_updates_it() {
        let session = session("# 标题\n\n正文");
        assert_eq!(session.snapshot().words, 4, "标记与空白不计");
        session.dispatch(Wire::SelectAll);
        let reply = session.dispatch(Wire::InsertText {
            text: "甲乙".into(),
        });
        assert_eq!(reply.words, 2, "回包要带最新字数");
    }

    #[test]
    fn typing_returns_patch_and_caret() {
        let session = session("甲\n\n乙");
        let reply = session.dispatch(Wire::SetCaret {
            stop: 1,
            extend: false,
        });
        assert_eq!(reply.caret.unwrap().stop, 1);

        let reply = session.dispatch(Wire::InsertText { text: "丙".into() });
        let patch = reply.patch.expect("应当只发增量帧");
        assert_eq!(patch.start, 0);
        // 块 0 多了一个停靠点，块 1 的 data-b 随之平移，尾部必须一并替换
        assert_eq!(patch.end, 2);
        assert_eq!((patch.old_len, patch.new_len), (2, 2));
        // 每字符一个 <span>，所以「甲丙」不会连续出现
        assert!(patch.html.contains(">丙<"));
        assert_eq!(session.text(), "甲丙\n\n乙");
        assert!(reply.dirty);
        // 光标落在新字之后，仍是第 2 个停靠点
        assert_eq!(reply.caret.unwrap().stop, 2);
    }

    #[test]
    fn undo_asks_for_full_redraw() {
        let session = session("甲");
        session.dispatch(Wire::InsertText { text: "乙".into() });
        let reply = session.dispatch(Wire::Undo);
        assert!(reply.full.is_some(), "撤销会改变块结构，必须全量重绘");
        assert!(reply.patch.is_none());
        assert_eq!(session.text(), "甲");
    }

    #[test]
    fn enter_returns_two_blocks_after_split() {
        let session = session("甲");
        session.dispatch(Wire::SetCaret {
            stop: 1,
            extend: false,
        });
        let reply = session.dispatch(Wire::Newline);
        assert_eq!(session.text(), "甲\n\n");
        assert!(reply.patch.is_some());
        let snap = session.snapshot();
        assert_eq!(snap.blocks.len(), 2);
        assert_eq!(snap.caret.unwrap().block, Some(1));
    }

    /// 新建：文档回到未命名空态，脏标记清零，保存随即要求路径。
    #[test]
    fn new_file_resets_to_an_untitled_document() {
        let session = session("甲");
        session.dispatch(Wire::InsertText { text: "乙".into() });
        assert!(session.is_dirty(), "输入之后应当是脏的");

        let snap = session.new_file();
        assert_eq!(snap.path, None, "新文档没有路径");
        assert!(!snap.dirty);
        assert_eq!(session.text(), "");
        assert!(snap.stops >= 1, "空文档也要有一个可落光标的段落");
        assert!(!session.is_dirty());

        // 新建之后按保存必须问路径，而不是假装写过了
        let reply = session.dispatch(Wire::Save);
        assert!(reply.needs_path);
        assert!(!reply.saved);
    }

    /// 走完整条链路：IPC 消息 → 内核写盘 → 改绑路径 → 回包带新路径。
    /// 用临时目录，跑完清掉，不留垃圾。
    #[test]
    fn save_to_writes_the_file_and_reports_the_new_path() {
        let dir = std::env::temp_dir().join("mdview-save-as");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("新稿.md");
        let _ = std::fs::remove_file(&target);
        let target_str = target.display().to_string();

        let session = session("甲");
        session.dispatch(Wire::InsertText { text: "乙".into() });
        let reply = session.dispatch(Wire::SaveTo {
            path: target_str.clone(),
        });

        assert!(reply.saved, "写成功要报 saved");
        assert_eq!(
            reply.path.as_deref(),
            Some(target_str.as_str()),
            "回包要带新路径"
        );
        assert!(!reply.dirty);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "乙甲",
            "光标在文首，新字插在前面"
        );
        assert_eq!(
            session.snapshot().path.as_deref(),
            Some(target_str.as_str())
        );

        let _ = std::fs::remove_file(&target);
    }

    /// 无路径保存必须如实说「我不知道往哪写」。
    ///
    /// 这条测试此前断言的是相反的行为（`assert!(reply.saved)` + 清 dirty），
    /// 等于把「界面说已保存、实际什么都没写」固定成了规格——任何人来修这个
    /// 坑都会先被它挡住。现在改为要求宿主补一条 `SaveTo`。
    #[test]
    fn saving_an_untitled_document_asks_for_a_path() {
        let session = session("甲");
        session.dispatch(Wire::InsertText { text: "乙".into() });
        let reply = session.dispatch(Wire::Save);
        assert!(reply.needs_path, "无路径应要求宿主补一个路径");
        assert!(!reply.saved, "没写盘就不能报已保存");
        assert!(reply.dirty, "脏标记必须保留，否则关窗时会以为没改动");
        assert!(reply.path.is_none());
    }

    #[test]
    fn selection_is_reported_after_extend() {
        let session = session("甲乙丙");
        let reply = session.dispatch(Wire::SetCaret {
            stop: 2,
            extend: true,
        });
        assert_eq!(reply.selection, Some([0, 2]));
        let reply = session.dispatch(Wire::SetCaret {
            stop: 1,
            extend: false,
        });
        assert_eq!(reply.selection, None, "收起光标后不应再报选区");
    }
}

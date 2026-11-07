//! 窗口与前端之间的会话层。
//!
//! 这一层**不依赖任何 Tauri 类型**，因此可以用单元测试完整验证 IPC 契约：
//! 前端发的 JSON 能否翻译成 `Message`、回复里的增量帧与光标是否正确。
//!
//! 前端只做两件事：把停靠点画成像素、把输入翻译成消息。文本真值始终在内核里。

use std::{path::PathBuf, sync::Mutex};

use mdview_app::{AppState, Effect, FileStore, Message, StdFileStore, ViewMode};
use mdview_markdown::BlockFrame;
use serde::{Deserialize, Serialize};

/// 前端发来的消息。
///
/// 刻意不复用 `mdview_app::Message`：内核语义不该为了序列化去背上 serde 依赖，
/// 这层只负责翻译。`kind` 字段与 `app.js` 里的一一对应。
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
    SetMode {
        mode: String,
    },
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
            Wire::SetCaret { stop, extend } => Message::SetCaret { stop, extend },
            Wire::Select { anchor, head } => Message::Select { anchor, head },
            Wire::Undo => Message::Undo,
            Wire::Redo => Message::Redo,
            Wire::Save => Message::Save,
            Wire::SetMode { mode } => Message::SetMode(match mode.as_str() {
                "reading" => ViewMode::Reading,
                "source" => ViewMode::Source,
                _ => ViewMode::Wysiwyg,
            }),
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
    pub blocks: Vec<BlockDto>,
    pub caret: Option<CaretDto>,
    /// 选区停靠点区间（已归一），`null` 表示没有选区
    pub selection: Option<[usize; 2]>,
}

#[derive(Debug, Serialize, Default)]
pub struct Reply {
    pub revision: u64,
    pub dirty: bool,
    /// 增量重绘：替换块节点 [start, end)
    pub patch: Option<PatchDto>,
    /// 全量重绘：整个块列表
    pub full: Option<Vec<BlockDto>>,
    pub caret: Option<CaretDto>,
    pub selection: Option<[usize; 2]>,
    pub conflict: bool,
    pub saved: bool,
}

#[derive(Debug, Serialize)]
pub struct PatchDto {
    pub start: usize,
    pub end: usize,
    pub old_len: usize,
    pub new_len: usize,
    pub html: String,
}

pub struct Session {
    state: Mutex<AppState>,
    files: StdFileStore,
}

impl Session {
    pub fn open(path: Option<PathBuf>) -> Result<Self, String> {
        let files = StdFileStore;
        let content = match &path {
            Some(path) => files.load(path).map_err(|err| format!("{}: {err}", path.display()))?,
            None => String::new(),
        };
        Ok(Self::from_text(&content, path))
    }

    pub fn from_text(content: &str, path: Option<PathBuf>) -> Self {
        let files = StdFileStore;
        Self {
            state: Mutex::new(AppState::open(content, path, &files)),
            files,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let app = self.state.lock().expect("状态锁中毒");
        Snapshot {
            revision: app.document.revision().0,
            path: app.path.as_ref().map(|p| p.display().to_string()),
            dirty: app.dirty,
            stops: app.layout.stops(),
            blocks: blocks_of(app.frames()),
            caret: caret_of(&app),
            selection: selection_of(&app),
        }
    }

    pub fn dispatch(&self, wire: Wire) -> Reply {
        let mut app = self.state.lock().expect("状态锁中毒");
        let effects = app.update(wire.into_message(), &self.files);
        let mut reply = Reply {
            revision: app.document.revision().0,
            dirty: app.dirty,
            caret: Some(caret_of(&app).unwrap_or_default()),
            selection: selection_of(&app),
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
                Effect::Conflict => reply.conflict = true,
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
    fn wire_json_matches_frontend_contract() {
        // 这两个字面量必须与 web/app.js 里发的一致，改一边就要改另一边
        let wire: Wire = serde_json::from_str(r#"{"kind":"insertText","text":"甲"}"#).unwrap();
        assert!(matches!(wire, Wire::InsertText { ref text } if text == "甲"));

        let wire: Wire = serde_json::from_str(r#"{"kind":"setCaret","stop":3,"extend":true}"#).unwrap();
        assert!(matches!(wire, Wire::SetCaret { stop: 3, extend: true }));

        assert!(serde_json::from_str::<Wire>(r#"{"kind":"backspace"}"#).is_ok());
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
    fn typing_returns_patch_and_caret() {
        let session = session("甲\n\n乙");
        let reply = session.dispatch(Wire::SetCaret { stop: 1, extend: false });
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
        session.dispatch(Wire::SetCaret { stop: 1, extend: false });
        let reply = session.dispatch(Wire::Newline);
        assert_eq!(session.text(), "甲\n\n");
        assert!(reply.patch.is_some());
        let snap = session.snapshot();
        assert_eq!(snap.blocks.len(), 2);
        assert_eq!(snap.caret.unwrap().block, Some(1));
    }

    #[test]
    fn saving_untitled_document_still_reports_persisted() {
        let session = session("甲");
        let reply = session.dispatch(Wire::Save);
        assert!(reply.saved, "无路径时不写盘，但仍要清 dirty");
        assert!(!reply.dirty);
    }

    #[test]
    fn selection_is_reported_after_extend() {
        let session = session("甲乙丙");
        let reply = session.dispatch(Wire::SetCaret { stop: 2, extend: true });
        assert_eq!(reply.selection, Some([0, 2]));
        let reply = session.dispatch(Wire::SetCaret { stop: 1, extend: false });
        assert_eq!(reply.selection, None, "收起光标后不应再报选区");
    }
}

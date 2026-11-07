use std::{ops::Range, path::PathBuf};

use log::{info, warn};
use mdview_core::Document;
use mdview_markdown::{MarkdownParser, ParseResult, SourceMap};

use crate::ports::{FileStore, Fingerprint, RenderFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// 所见即所得：源码为真值，视图是其投影
    Wysiwyg,
    Reading,
    Source,
}

impl ViewMode {
    fn label(self) -> &'static str {
        match self {
            ViewMode::Wysiwyg => "wysiwyg",
            ViewMode::Reading => "reading",
            ViewMode::Source => "source",
        }
    }
}

#[derive(Debug)]
pub enum Message {
    /// span 为 char index（core 语义）
    Edit { span: Range<usize>, insert: String },
    Undo,
    Redo,
    SetMode(ViewMode),
    Save,
    /// byte offset（Markdown / DOM 语义）
    Click { byte_offset: usize },
    Reload { content: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// 生产实现：把该 Revision 的脏块投递到后台线程做增量解析。
    ScheduleParse { revision: u64 },
    Render,
    Persist,
    MoveCursor { char_offset: usize, block: Option<usize> },
    Conflict,
}

pub struct AppState {
    pub document: Document,
    pub parsed: ParseResult,
    pub source_map: SourceMap,
    pub mode: ViewMode,
    pub dirty: bool,
    pub path: Option<PathBuf>,
    saved_fingerprint: Option<Fingerprint>,
    parser: MarkdownParser,
}

impl AppState {
    pub fn open(
        content: impl AsRef<str>,
        path: Option<PathBuf>,
        files: &impl FileStore,
    ) -> Self {
        let saved_fingerprint = path.as_deref().and_then(|p| files.fingerprint(p).ok());
        let mut state = Self {
            document: Document::new(content),
            parsed: ParseResult::default(),
            source_map: SourceMap::default(),
            mode: ViewMode::Wysiwyg,
            dirty: false,
            path,
            saved_fingerprint,
            parser: MarkdownParser::gfm(),
        };
        state.reparse();
        state
    }

    /// 骨架：同步全量重解析，保证可测试与确定性。
    /// 生产实现改为「脏块失效 + 后台增量解析 + Revision 校验后回写」。
    fn reparse(&mut self) {
        let source = self.document.text();
        self.parsed = self.parser.parse(&source);
        self.source_map = self.parsed.source_map();
    }

    pub fn render_frame(&self) -> RenderFrame {
        RenderFrame {
            revision: self.document.revision().0,
            html: self.parsed.to_html(),
            blocks: self.parsed.blocks.len(),
            mode: self.mode.label(),
        }
    }

    pub fn update(&mut self, msg: Message, files: &impl FileStore) -> Vec<Effect> {
        let is_undo = matches!(&msg, Message::Undo);
        match msg {
            Message::Edit { span, insert } => {
                if self.document.edit(span, &insert).is_err() {
                    return Vec::new();
                }
                self.dirty = true;
                self.reparse();
                vec![
                    Effect::ScheduleParse {
                        revision: self.document.revision().0,
                    },
                    Effect::Render,
                ]
            }
            Message::Undo | Message::Redo => {
                let changed = if is_undo {
                    self.document.undo().is_some()
                } else {
                    self.document.redo().is_some()
                };
                if !changed {
                    return Vec::new();
                }
                self.dirty = true;
                self.reparse();
                vec![Effect::Render]
            }
            Message::SetMode(mode) => {
                self.mode = mode;
                vec![Effect::Render]
            }
            Message::Save => {
                let Some(path) = self.path.clone() else {
                    return vec![Effect::Persist];
                };
                if let Some(expected) = self.saved_fingerprint {
                    if files.fingerprint(&path).ok() != Some(expected) {
                        warn!("保存被拒：{} 已被外部修改", path.display());
                        return vec![Effect::Conflict];
                    }
                }
                if let Err(err) = files.save(&path, &self.document.text()) {
                    warn!("写入失败 {}: {err}", path.display());
                    return vec![Effect::Conflict];
                }
                info!("已保存 {}（revision={}）", path.display(), self.document.revision().0);
                self.saved_fingerprint = files.fingerprint(&path).ok();
                self.dirty = false;
                vec![Effect::Persist]
            }
            Message::Click { byte_offset } => {
                let block = self.source_map.block_at_byte(byte_offset).map(|id| id.0);
                // byte -> char 换算：中文文档不做这一步会定位到错误的字。
                let char_offset = self.document.byte_to_char(byte_offset);
                vec![Effect::MoveCursor { char_offset, block }]
            }
            Message::Reload { content } => {
                self.document = Document::new(content);
                self.reparse();
                self.dirty = false;
                vec![Effect::Render]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{CollectingSink, InMemoryFileStore, RenderSink};

    fn fixture() -> (InMemoryFileStore, AppState) {
        let mut files = InMemoryFileStore::default();
        files.insert("note.md", "# 标题\n\n正文内容\n");
        let app = AppState::open("# 标题\n\n正文内容\n", Some(PathBuf::from("note.md")), &files);
        (files, app)
    }

    #[test]
    fn edit_updates_revision_and_html() {
        let (files, mut app) = fixture();
        let before = app.render_frame();
        let effects = app.update(
            Message::Edit {
                span: 0..0,
                insert: "> ".into(),
            },
            &files,
        );
        let after = app.render_frame();
        assert_eq!(after.revision, before.revision + 1);
        assert_ne!(after.html, before.html);
        assert!(after.html.contains("<blockquote>"));
        assert!(app.dirty);
        assert!(effects.contains(&Effect::Render));

        let mut sink = CollectingSink::default();
        sink.render(&after);
        assert_eq!(sink.frames.len(), 1);
    }

    #[test]
    fn preview_click_converts_byte_to_char_offset() {
        let (files, mut app) = fixture();
        // "正文内容" 前有 "# 标题\n\n" = 2 + 6 + 2 = 10 bytes，且均为多字节字符。
        let byte_offset = app.parsed.blocks[1].byte.start;
        let effects = app.update(Message::Click { byte_offset }, &files);
        assert_eq!(
            effects,
            vec![Effect::MoveCursor {
                char_offset: app.document.byte_to_char(byte_offset),
                block: Some(1),
            }]
        );
        assert!(app.document.byte_to_char(byte_offset) < byte_offset);
    }

    #[test]
    fn save_detects_external_conflict() {
        let (mut files, mut app) = fixture();
        app.update(
            Message::Edit {
                span: 0..0,
                insert: "> ".into(),
            },
            &files,
        );
        files.insert("note.md", "external change\n");
        assert_eq!(app.update(Message::Save, &files), vec![Effect::Conflict]);
        assert!(app.dirty);
    }

    #[test]
    fn save_succeeds_without_external_change() {
        let (files, mut app) = fixture();
        app.update(Message::Edit { span: 0..0, insert: "> ".into() }, &files);
        assert_eq!(app.update(Message::Save, &files), vec![Effect::Persist]);
        assert!(!app.dirty);
    }
}

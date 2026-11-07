use std::{ops::Range, path::PathBuf};

use log::{info, warn};
use mdview_core::Document;
use mdview_markdown::{
    BlockFrame, BlockKind, Caret, DocumentLayout, Gravity, MarkdownParser, ParseResult, Patch,
    Selection, SourceMap, block_frames, diff_frames,
};

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
    /// span 为 char index（core 语义）。脚本化编辑与测试用，等价于「替换 + 落光标」。
    Edit { span: Range<usize>, insert: String },
    /// 键盘或输入法提交的一段文本。有选区时替换选区。
    InsertText { text: String },
    Backspace,
    DeleteForward,
    /// 回车：按块类型拆分（段落插空行、代码块插单换行、列表/引用延续标记）
    Newline,
    /// Shift+Enter：段内硬换行（写 `\` + 换行，反斜杠进隐藏集）
    HardBreak,
    /// 键盘移动。`delta` 以停靠点为单位，跨块自动连续；`extend` 为 Shift 扩展选区
    MoveCaret { delta: isize, extend: bool },
    /// 输入法组合开始：此后重解析与重绘全部挂起（硬约束 9）
    CompositionStart,
    /// 输入法提交：整段文本作为**一次**事务落盘，一次撤销即可回退整个词
    CompositionCommit { text: String },
    /// 输入法取消（Esc、失焦、空提交）：必须复位挂起态，否则内核永久吞输入
    CompositionCancel,
    Undo,
    Redo,
    SetMode(ViewMode),
    Save,
    /// byte offset（Markdown / DOM 语义）
    Click { byte_offset: usize },
    /// 全局停靠点序号。前端已持有「停靠点 → 像素」映射，点击/拖动直接报停靠点，
    /// 省掉一次 byte → 停靠点的吸附。
    SetCaret { stop: usize, extend: bool },
    /// 选区两端都是 byte offset，交给布局层吸附到停靠点
    Select { anchor: usize, head: usize },
    Reload { content: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// 生产实现：把该 Revision 的脏块投递到后台线程做增量解析。
    ScheduleParse { revision: u64 },
    /// 全量重绘。只在结构可能整体变化时发出（切换模式、重载、撤销）。
    Render,
    /// 增量重绘：用 `patch.html` 替换第 `patch.replace` 段块节点。
    /// 编辑一律走增量，前端拿到它就不必重建整页。
    Patch { revision: u64, patch: Patch },
    Persist,
    MoveCursor {
        /// 光标所在的全局停靠点序号。前端用它找 `data-b` / `data-a` 节点定像素。
        stop: usize,
        char_offset: usize,
        block: Option<usize>,
        /// 块内视觉列。前端据此对齐 caret 的 x 坐标，不必自己数隐藏标记。
        visual: Option<usize>,
    },
    /// 选区已吸附到可见文本，`byte` 可直接用于按块切高亮。
    Selection {
        byte: Range<usize>,
        blocks: Vec<usize>,
    },
    Conflict,
}

pub struct AppState {
    pub document: Document,
    pub parsed: ParseResult,
    pub layout: DocumentLayout,
    pub source_map: SourceMap,
    pub mode: ViewMode,
    pub dirty: bool,
    pub path: Option<PathBuf>,
    /// 当前选区。两端是**全局停靠点索引**，不是 byte。
    pub selection: Selection,
    /// 输入法组合期间为真：所有编辑消息被吞掉，只等 `CompositionCommit` 一次性提交。
    pub composing: bool,
    /// 上一次渲染的分块帧，供增量重绘求差集。
    frames: Vec<BlockFrame>,
    saved_fingerprint: Option<Fingerprint>,
    parser: MarkdownParser,
}

/// 退格 / Delete 落在标记上时，「整体删掉什么」的两种目标。
///
/// 纯字节删除在这里必错：`**粗**` 少一个 `*` 就变成可见的 `*粗**`。
enum MarkerEdit {
    /// 成对标记，两头一起抽掉
    Pair {
        open: Range<usize>,
        close: Range<usize>,
    },
    /// 独占区间：标题 / 列表 / 引用的前缀，或图片这类原子控件
    Run(Range<usize>),
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
            layout: DocumentLayout::default(),
            source_map: SourceMap::default(),
            mode: ViewMode::Wysiwyg,
            dirty: false,
            path,
            selection: Selection::caret(0),
            composing: false,
            frames: Vec::new(),
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
        self.layout = DocumentLayout::build(&source, &self.parsed);
        self.frames = block_frames(&source, &self.parsed, &self.layout);
    }

    /// 当前渲染帧（每块一份 HTML + 全局停靠点区间）。
    pub fn frames(&self) -> &[BlockFrame] {
        &self.frames
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
            Message::Edit { span, insert } => self.apply(span, &insert),
            Message::InsertText { text } => self.insert_text(&text),
            Message::Backspace => self.delete_backward(),
            Message::DeleteForward => self.delete_forward(),
            Message::Newline => self.newline(),
            Message::HardBreak => self.hard_break(),
            Message::MoveCaret { delta, extend } => self.move_caret(delta, extend),
            Message::CompositionStart => {
                self.composing = true;
                // 不重绘：preedit 交给浏览器自己渲染，内核此时不得插手 DOM
                Vec::new()
            }
            Message::CompositionCommit { text } => {
                self.composing = false;
                self.insert_text(&text)
            }
            // 取消候选（Esc、失焦）：必须复位 composing。漏了这一条，
            // 内核会永远停在挂起态，之后所有输入被静默吞掉。
            Message::CompositionCancel => {
                self.composing = false;
                Vec::new()
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
                // 撤销会改变块数，旧停靠点索引可能越界，先夹紧再报光标
                self.clamp_selection();
                vec![Effect::Render, self.cursor_effect()]
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
                // 点击点可能落在被折叠的标记里（`**` 的中间），先吸附到可见停靠点，
                // 否则光标会被塞进标记内部，下一次输入就把 `**` 拆散。
                let (block, visual, snapped, global) =
                    match self.layout.stop_at_byte(byte_offset, Gravity::Upstream) {
                        Some(global) => {
                            let (id, local) = self
                                .layout
                                .locate(global)
                                .expect("全局停靠点必定能定位回块");
                            (
                                Some(id.0),
                                self.layout.layout(id).and_then(|l| l.visual(local)),
                                self.layout.byte(global).unwrap_or(byte_offset),
                                global,
                            )
                        }
                        None => (None, None, byte_offset, 0),
                    };
                self.selection = Selection::caret(global);
                // byte -> char 换算：中文文档不做这一步会定位到错误的字。
                let char_offset = self.document.byte_to_char(snapped);
                vec![Effect::MoveCursor {
                    stop: global,
                    char_offset,
                    block,
                    visual,
                }]
            }
            Message::SetCaret { stop, extend } => {
                let last = self.layout.stops().saturating_sub(1);
                let stop = stop.min(last);
                if extend {
                    self.selection.head = stop;
                } else {
                    self.selection = Selection::caret(stop);
                }
                let mut effects = vec![self.cursor_effect()];
                if let Some(sel) = self.selection_effect() {
                    effects.push(sel);
                }
                effects
            }
            Message::Select { anchor, head } => {
                let snap = |byte| {
                    self.layout
                        .stop_at_byte(byte, Gravity::Upstream)
                        .unwrap_or(0)
                };
                self.selection = Selection::new(snap(anchor), snap(head));
                self.selection_effect().into_iter().collect()
            }
            Message::Reload { content } => {
                self.document = Document::new(content);
                self.reparse();
                self.clamp_selection();
                self.dirty = false;
                vec![Effect::Render, self.cursor_effect()]
            }
        }
    }

    // ---- 编辑闭环 -------------------------------------------------------

    /// 唯一的文本写入口：一次事务 = 一步撤销，之后统一重解析、重定位光标、求增量帧。
    fn apply(&mut self, span: Range<usize>, insert: &str) -> Vec<Effect> {
        self.apply_with_caret(span, insert, None)
    }

    /// `caret` 为 `None` 时落在插入文本之后（普通输入）；显式给出时用于
    /// 「成对删标记」——删掉 `**` 后光标必须留在词首，而不是跳到词尾。
    fn apply_with_caret(
        &mut self,
        span: Range<usize>,
        insert: &str,
        caret: Option<usize>,
    ) -> Vec<Effect> {
        if span.is_empty() && insert.is_empty() {
            return Vec::new();
        }
        if self.document.edit(span.clone(), insert).is_err() {
            return Vec::new();
        }
        self.dirty = true;
        let caret = caret.unwrap_or(span.start + insert.chars().count());
        // 先摘走旧帧再重解析，避免为求差集克隆整份 HTML
        let old = std::mem::take(&mut self.frames);
        self.reparse();
        self.selection = Selection::caret(self.stop_at_char(caret));
        let revision = self.document.revision().0;
        vec![
            Effect::ScheduleParse { revision },
            Effect::Patch {
                revision,
                patch: diff_frames(&old, &self.frames),
            },
            self.cursor_effect(),
        ]
    }

    fn insert_text(&mut self, text: &str) -> Vec<Effect> {
        if self.composing || text.is_empty() {
            return Vec::new();
        }
        match self.selection_chars() {
            Some(sel) => self.apply(sel, text),
            None => {
                let at = self.caret_char_offset();
                self.apply(at..at, text)
            }
        }
    }

    fn delete_backward(&mut self) -> Vec<Effect> {
        if self.composing {
            return Vec::new();
        }
        if let Some(sel) = self.selection_chars() {
            return self.apply(self.expand_deletion(sel), "");
        }
        let Some(byte) = self.caret_byte() else {
            return Vec::new();
        };
        let at = self.document.byte_to_char(byte);
        if at == 0 {
            return Vec::new();
        }
        // 前一个字符若落在折叠标记里，按整体删除，绝不啃单个字节
        let prev = self.document.char_to_byte(at - 1);
        if let Some(edit) = self.marker_edit_at(prev) {
            return self.delete_marker(edit);
        }
        self.apply(self.expand_deletion(at - 1..at), "")
    }

    fn delete_forward(&mut self) -> Vec<Effect> {
        if self.composing {
            return Vec::new();
        }
        if let Some(sel) = self.selection_chars() {
            return self.apply(self.expand_deletion(sel), "");
        }
        let Some(byte) = self.caret_byte() else {
            return Vec::new();
        };
        let at = self.document.byte_to_char(byte);
        if at >= self.document.len_chars() {
            return Vec::new();
        }
        if let Some(edit) = self.marker_edit_at(byte) {
            return self.delete_marker(edit);
        }
        self.apply(self.expand_deletion(at..at + 1), "")
    }

    /// 把删除区间扩到「整块语法单元」的边界。
    ///
    /// 两件事都必须在**同一次事务**里做完：
    ///
    /// 1. 与折叠标记相交就吃到标记边界。只删掉半个 `**` 会留下可见的 `*`，
    ///    选区恰好从标记中间起手时很容易踩到。
    /// 2. 容器内容被吃空就**连标记一起删**。删掉 `**粗**` 里的 `粗` 之后源码是
    ///    `****`，pulldown 已经不把它当容器，四个星号会变成可见文字——
    ///    这时再想补救也找不到配对了，所以只能在删的当下处理。
    fn expand_deletion(&self, span: Range<usize>) -> Range<usize> {
        if span.is_empty() {
            return span;
        }
        let start = self.document.char_to_byte(span.start);
        let end = self.document.char_to_byte(span.end);
        // 跨块删除不在这里处理：块的边界本身就是语法边界
        let Some(block) = self
            .parsed
            .blocks
            .iter()
            .find(|b| b.byte.start <= start && start < b.byte.end)
        else {
            return span;
        };
        if end > block.byte.end {
            return span;
        }

        let (mut lo, mut hi) = (start, end);
        for pair in &block.markers {
            for range in [&pair.open, &pair.close] {
                if range.start < hi && lo < range.end {
                    lo = lo.min(range.start);
                    hi = hi.max(range.end);
                }
            }
        }
        for range in &block.hidden {
            if range.start < hi && lo < range.end {
                lo = lo.min(range.start);
                hi = hi.max(range.end);
            }
        }

        // 内容全被吃掉就继续向外扩，嵌套容器（`**a *b* c**`）会连锁收起
        loop {
            let mut grown = false;
            for pair in &block.markers {
                let content = pair.open.end..pair.close.start;
                let swallowed = lo <= content.start && content.end <= hi;
                if swallowed && (lo > pair.open.start || hi < pair.close.end) {
                    lo = lo.min(pair.open.start);
                    hi = hi.max(pair.close.end);
                    grown = true;
                }
            }
            if !grown {
                break;
            }
        }

        self.document.byte_to_char(lo)..self.document.byte_to_char(hi)
    }

    /// 判断这个 byte 是否落在「不可按字节删」的位置。
    ///
    /// 三种情况：成对标记（`**`、`` ` ``、`[]()`）、独占的折叠区间（标题/列表前缀）、
    /// 原子控件（图片、分隔线）。删掉其中任意一个字节都会把语法写坏。
    fn marker_edit_at(&self, byte: usize) -> Option<MarkerEdit> {
        let block = self
            .parsed
            .blocks
            .iter()
            .find(|b| b.byte.start <= byte && byte < b.byte.end)?;
        if let Some(pair) = block.marker_pair_at(byte) {
            return Some(MarkerEdit::Pair {
                open: pair.open.clone(),
                close: pair.close.clone(),
            });
        }
        if let Some(run) = block.hidden_range_at(byte) {
            return Some(MarkerEdit::Run(run));
        }
        block
            .control_span_at(byte)
            .map(|span| MarkerEdit::Run(span.byte.clone()))
    }

    fn delete_marker(&mut self, edit: MarkerEdit) -> Vec<Effect> {
        match edit {
            // 一对标记：把中间的内容原样留下，只抽掉两头。
            // 一次事务完成，撤销一次即可恢复，不会出现「还原了一半标记」的中间态。
            MarkerEdit::Pair { open, close } => {
                let keep = self.document.slice_bytes(open.end..close.start);
                let span =
                    self.document.byte_to_char(open.start)..self.document.byte_to_char(close.end);
                let caret = self.document.byte_to_char(open.start);
                self.apply_with_caret(span, &keep, Some(caret))
            }
            MarkerEdit::Run(range) => {
                let span =
                    self.document.byte_to_char(range.start)..self.document.byte_to_char(range.end);
                let caret = self.document.byte_to_char(range.start);
                self.apply_with_caret(span, "", Some(caret))
            }
        }
    }

    fn hard_break(&mut self) -> Vec<Effect> {
        if self.composing {
            return Vec::new();
        }
        if let Some(sel) = self.selection_chars() {
            return self.apply(sel, "\\\n");
        }
        let at = self.caret_char_offset();
        self.apply(at..at, "\\\n")
    }

    /// 回车。分隔符取决于光标所在块的类型：
    ///
    /// - 段落 / 标题：`\n\n`，因为 Markdown 的段落边界就是空行；
    /// - 代码块 / 表格 / HTML：`\n`，插空行会把块一分为二；
    /// - 列表 / 引用：`\n` + 重复本行标记，且有序列表序号递增；
    /// - 标记之后尚无内容（空项）：删掉标记本身，回车即退出该层。
    fn newline(&mut self) -> Vec<Effect> {
        if self.composing {
            return Vec::new();
        }
        if let Some(sel) = self.selection_chars() {
            // 有选区时回车等价于「删掉选区 + 分段」，一次事务完成，只需撤销一次
            return self.apply(sel, "\n\n");
        }
        let Some((id, _)) = self.layout.locate(self.selection.head) else {
            return Vec::new();
        };
        let kind = self.parsed.blocks[id.0].kind.clone();
        match kind {
            BlockKind::CodeBlock { .. } | BlockKind::Html | BlockKind::Table => {
                let at = self.caret_char_offset();
                self.apply(at..at, "\n")
            }
            BlockKind::List { .. } | BlockKind::BlockQuote => {
                let Some((line_start, prefix)) = self.line_prefix() else {
                    return self.split_paragraph();
                };
                match analyze_line(&prefix) {
                    Some(info) if !info.empty_item => {
                        let at = self.caret_char_offset();
                        self.apply(at..at, &format!("\n{}", info.repeat))
                    }
                    Some(_) => {
                        let end = self.caret_byte().unwrap_or(line_start + prefix.len());
                        let start = self.document.byte_to_char(line_start);
                        let at = self.document.byte_to_char(end);
                        // 空项正好是文档的最后一行时，这一行连行尾换行都没有：
                        // 删掉标记后整篇只剩列表本身，没有块能承接光标，
                        // `stop_at_byte` 只能把它退回上一行的末尾——用户看到的
                        // 是「回车把光标送回列表最后一行」，而且再也离不开列表
                        // （下一次回车又造出一个空项）。这里把整行换成空段落。
                        if end == self.document.len_bytes() {
                            return self.apply_with_caret(start..at, "\n", Some(start + 1));
                        }
                        self.apply_bytes(line_start..end, "")
                    }
                    None => self.split_paragraph(),
                }
            }
            _ => self.split_paragraph(),
        }
    }

    fn split_paragraph(&mut self) -> Vec<Effect> {
        let at = self.caret_char_offset();
        self.apply(at..at, "\n\n")
    }

    fn move_caret(&mut self, delta: isize, extend: bool) -> Vec<Effect> {
        let Some((id, local)) = self.layout.locate(self.selection.head) else {
            return Vec::new();
        };
        let mut caret = Caret {
            block: id,
            stop: local,
        };
        for _ in 0..delta.unsigned_abs() {
            let next = if delta > 0 {
                caret.next(&self.layout)
            } else {
                caret.prev(&self.layout)
            };
            let Some(next) = next else { break };
            caret = next;
        }
        let Some(global) = caret.to_global(&self.layout) else {
            return Vec::new();
        };
        if extend {
            self.selection.head = global;
        } else {
            self.selection = Selection::caret(global);
        }
        let mut effects = vec![self.cursor_effect()];
        if let Some(sel) = self.selection_effect() {
            effects.push(sel);
        }
        effects
    }

    // ---- 坐标换算 -------------------------------------------------------

    /// 光标所在的 byte。停靠点保证落在字符边界，因此可直接用作源码下标。
    fn caret_byte(&self) -> Option<usize> {
        self.layout.byte(self.selection.head)
    }

    fn caret_char_offset(&self) -> usize {
        self.caret_byte()
            .map(|byte| self.document.byte_to_char(byte))
            .unwrap_or_else(|| self.document.len_chars())
    }

    /// 非折叠选区 → char 区间（跨块时含块间空行）。
    fn selection_chars(&self) -> Option<Range<usize>> {
        if self.selection.is_collapsed() {
            return None;
        }
        let byte = self.selection.byte_range(&self.layout)?;
        Some(self.document.byte_to_char(byte.start)..self.document.byte_to_char(byte.end))
    }

    /// char 位置 → 停靠点。
    ///
    /// 用 `Upstream`：刚打的字右侧若紧跟闭合标记（`**粗**` 的尾部），光标必须留在
    /// 标记**之前**，否则下一次输入就落到样式外面。`Upstream` 贴住左词末尾正是这个语义。
    fn stop_at_char(&self, char_offset: usize) -> usize {
        let byte = self.document.char_to_byte(char_offset);
        self.layout
            .stop_at_byte(byte, Gravity::Upstream)
            .unwrap_or(0)
    }

    fn apply_bytes(&mut self, range: Range<usize>, insert: &str) -> Vec<Effect> {
        let span =
            self.document.byte_to_char(range.start)..self.document.byte_to_char(range.end);
        self.apply(span, insert)
    }

    /// 光标所在行从行首到光标的源码前缀（识别列表 / 引用标记用）。
    fn line_prefix(&self) -> Option<(usize, String)> {
        let byte = self.caret_byte()?;
        let mut width = 64usize;
        loop {
            let start = byte.saturating_sub(width);
            let aligned = self
                .document
                .char_to_byte(self.document.byte_to_char(start));
            let chunk = self.document.slice_bytes(aligned..byte);
            if let Some(pos) = chunk.rfind('\n') {
                let line_start = aligned + pos + 1;
                return Some((line_start, self.document.slice_bytes(line_start..byte)));
            }
            if aligned == 0 || width >= 8192 {
                return Some((aligned, chunk));
            }
            width = (width * 4).min(8192);
        }
    }

    // ---- 效果组装 -------------------------------------------------------

    fn cursor_effect(&self) -> Effect {
        let global = self.selection.head;
        let (block, visual) = match self.layout.locate(global) {
            Some((id, local)) => (
                Some(id.0),
                self.layout.layout(id).and_then(|l| l.visual(local)),
            ),
            None => (None, None),
        };
        Effect::MoveCursor {
            stop: global,
            char_offset: self.caret_char_offset(),
            block,
            visual,
        }
    }

    fn selection_effect(&self) -> Option<Effect> {
        if self.selection.is_collapsed() {
            return None;
        }
        let byte = self.selection.byte_range(&self.layout)?;
        let blocks = self
            .selection
            .blocks(&self.layout)
            .into_iter()
            .map(|id| id.0)
            .collect();
        Some(Effect::Selection { byte, blocks })
    }

    fn clamp_selection(&mut self) {
        let last = self.layout.stops().saturating_sub(1);
        let anchor = self.selection.anchor.min(last);
        let head = self.selection.head.min(last);
        self.selection = Selection::new(anchor, head);
    }
}

/// 行首块级标记的解析结果。
struct LineInfo {
    /// 回车后应重复的前缀（引用 + 缩进 + 标记，有序列表序号已递增）
    repeat: String,
    /// 标记之后还没有内容，回车应退出该层
    empty_item: bool,
}

/// 从「行首到光标」的源码前缀里识别 `> `、缩进、`- ` / `1. ` 这类块级标记。
fn analyze_line(prefix: &str) -> Option<LineInfo> {
    let mut rest = prefix;
    let mut quote = String::new();
    loop {
        let trimmed = rest.trim_start_matches([' ', '\t']);
        let Some(after) = trimmed.strip_prefix('>') else {
            break;
        };
        let after = after.strip_prefix(' ').unwrap_or(after);
        quote.push_str("> ");
        rest = after;
    }

    let indent_len = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let indent = &rest[..indent_len];
    let body = &rest[indent_len..];

    let bullet = ['-', '*', '+']
        .into_iter()
        .find(|c| body.starts_with(*c) && body[c.len_utf8()..].starts_with(' '));

    let (marker, after_marker) = if let Some(c) = bullet {
        (format!("{c} "), body[c.len_utf8()..].trim_start_matches(' '))
    } else {
        let digits = body.chars().take_while(|c| c.is_ascii_digit()).count();
        let delim = body[digits..].chars().next();
        match delim {
            Some(d) if digits > 0 && (d == '.' || d == ')') => {
                let after = &body[digits + d.len_utf8()..];
                if !after.starts_with(' ') {
                    return None;
                }
                let number: u64 = body[..digits].parse().unwrap_or(0);
                (
                    format!("{}{d} ", number + 1),
                    after.trim_start_matches(' '),
                )
            }
            // 没有列表标记：引用块本身也要延续 `> `
            _ if !quote.is_empty() => {
                let empty = body.trim().is_empty();
                return Some(LineInfo {
                    repeat: format!("{quote}{indent}"),
                    empty_item: empty,
                });
            }
            _ => return None,
        }
    };

    Some(LineInfo {
        repeat: format!("{quote}{indent}{marker}"),
        empty_item: after_marker.trim().is_empty(),
    })
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

    fn open(files: &mut InMemoryFileStore, src: &str) -> AppState {
        files.insert("t.md", src);
        AppState::open(src, Some(PathBuf::from("t.md")), files)
    }

    fn caret_byte(app: &AppState) -> usize {
        app.layout.byte(app.selection.head).unwrap_or(0)
    }

    fn click(app: &mut AppState, files: &InMemoryFileStore, byte: usize) {
        app.update(Message::Click { byte_offset: byte }, files);
    }

    fn type_text(app: &mut AppState, files: &InMemoryFileStore, text: &str) {
        app.update(
            Message::InsertText {
                text: text.to_owned(),
            },
            files,
        );
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
        assert!(effects.iter().any(|e| matches!(e, Effect::Patch { .. })));

        let mut sink = CollectingSink::default();
        sink.render(&after);
        assert_eq!(sink.frames.len(), 1);
    }

    #[test]
    fn typing_hash_does_not_flip_the_line_into_a_heading() {
        // 逐字输入 `#`：CommonMark 允许裸 `#` 成标题，但编辑器里那会先跳成 H1
        // 再在输入文字后掉回段落。空格才是「我要开始写标题」的信号。
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "");

        type_text(&mut app, &files, "#");
        assert_eq!(app.frames()[0].kind, "paragraph", "裸 # 不该升格为标题");
        assert!(
            !app.frames()[0].html.contains("class=\"mk\""),
            "# 必须看得见，不能被当成折叠标记"
        );

        type_text(&mut app, &files, " ");
        assert_eq!(app.frames()[0].kind, "heading", "敲了空格才是标题");

        type_text(&mut app, &files, "标题");
        assert_eq!(app.frames()[0].kind, "heading", "补内容之后不该退回段落");
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
                stop: app.layout.stop_at_byte(byte_offset, Gravity::Upstream).unwrap(),
                char_offset: app.document.byte_to_char(byte_offset),
                block: Some(1),
                visual: Some(0),
            }]
        );
        assert!(app.document.byte_to_char(byte_offset) < byte_offset);
    }

    #[test]
    fn click_inside_marker_snaps_out_of_it() {
        let mut files = InMemoryFileStore::default();
        files.insert("a.md", "**加粗**\n");
        let mut app = AppState::open("**加粗**\n", Some(PathBuf::from("a.md")), &files);
        // byte 1 落在开头的 `**` 中间
        let effects = app.update(Message::Click { byte_offset: 1 }, &files);
        let Effect::MoveCursor { char_offset, .. } = effects[0] else {
            panic!("应产生移动光标效果");
        };
        // 吸附到「加」之前，也就是 char 2
        assert_eq!(char_offset, 2);
    }

    #[test]
    fn selection_spans_blocks_and_keeps_blank_line() {
        let (files, mut app) = fixture();
        let source = app.document.text();
        let effects = app.update(
            Message::Select {
                anchor: 5,
                head: app.parsed.blocks[1].byte.start + 6,
            },
            &files,
        );
        let Some(Effect::Selection { byte, blocks }) = effects.first() else {
            panic!("应产生选区效果");
        };
        assert_eq!(blocks, &vec![0, 1]);
        // 选区包含块间空行，否则复制出来会把两段粘在一起
        assert_eq!(&source[byte.clone()], "题\n\n正文");
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

    // ---- 编辑闭环 -------------------------------------------------------

    #[test]
    fn insert_text_advances_caret_after_the_new_char() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "正文");
        click(&mut app, &files, 0);
        type_text(&mut app, &files, "新");
        assert_eq!(app.document.text(), "新正文");
        assert_eq!(caret_byte(&app), 3, "光标应落在新字之后");
    }

    #[test]
    fn typing_inside_bold_stays_inside_the_markers() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**加粗**");
        click(&mut app, &files, 2);
        type_text(&mut app, &files, "很");
        assert_eq!(app.document.text(), "**很加粗**");
        // 光标必须留在闭合标记之前，否则下一个字会掉到加粗外面
        assert_eq!(caret_byte(&app), 5);
        let effects = app.update(
            Message::InsertText {
                text: "多".into(),
            },
            &files,
        );
        assert_eq!(app.document.text(), "**很多加粗**");
        assert!(effects.iter().any(|e| matches!(e, Effect::MoveCursor { .. })));
    }

    #[test]
    fn backspace_deletes_previous_char_with_chinese() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "中文abc");
        click(&mut app, &files, 3); // 「中」之后
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "文abc");
        assert_eq!(caret_byte(&app), 0);
    }

    #[test]
    fn backspace_inside_strong_removes_the_marker_pair() {
        // 光标停在「标记之后、文字之前」的双停靠点。纯字节删除会啃掉一个 `*`，
        // 留下可见的 `*粗**`；正确做法是把两头 `**` 一起抽掉。
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**粗**");
        click(&mut app, &files, 2);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "粗");
        assert_eq!(app.parsed.blocks[0].hidden, Vec::new());
        // 光标落在词首，而不是跳到词尾
        assert_eq!(caret_byte(&app), 0);
        // 一次事务 = 一步撤销
        app.update(Message::Undo, &files);
        assert_eq!(app.document.text(), "**粗**");
    }

    #[test]
    fn backspace_at_bold_start_keeps_the_neighbours() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "请**粗**体");
        click(&mut app, &files, 5);
        app.update(Message::Backspace, &files);
        // 只去掉加粗，前后文一字不动
        assert_eq!(app.document.text(), "请粗体");
        assert_eq!(caret_byte(&app), 3);
    }

    #[test]
    fn backspace_after_heading_prefix_removes_the_prefix() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "# 标题");
        click(&mut app, &files, 2);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "标题");
        assert_eq!(app.parsed.blocks[0].kind, BlockKind::Paragraph);
    }

    #[test]
    fn delete_before_closing_marker_removes_the_pair() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**粗**");
        click(&mut app, &files, 5);
        app.update(Message::DeleteForward, &files);
        assert_eq!(app.document.text(), "粗");
    }

    #[test]
    fn backspace_on_last_visible_char_collapses_the_container() {
        // 删掉 `**粗**` 里唯一的字之后，源码会变成 `****`——pulldown 不再把
        // 它当容器，四个星号就成了可见文字。必须在那一次事务里连标记一起删。
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "正文**粗**");
        let after_char = "正文**粗".len();
        click(&mut app, &files, after_char);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "正文");
        // 一次撤销完整还原，不留「还原一半标记」的中间态
        app.update(Message::Undo, &files);
        assert_eq!(app.document.text(), "正文**粗**");
    }

    #[test]
    fn container_with_remaining_text_is_not_collapsed() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**粗体**");
        click(&mut app, &files, "**粗".len());
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "**体**");
    }

    #[test]
    fn strikethrough_and_code_collapse_like_bold() {
        // 删除线、行内代码走的是同一套配对：删空内容后都会留下可见的标记
        for (source, deleted) in [("前~~删~~后", "前后"), ("前`用`后", "前后")] {
            let mut files = InMemoryFileStore::default();
            let mut app = open(&mut files, source);
            // 光标停在内容最后一个字之后（汉字 3 字节）
            let after_char = source.find(|c| c == '删' || c == '用').unwrap() + 3;
            click(&mut app, &files, after_char);
            app.update(Message::Backspace, &files);
            assert_eq!(app.document.text(), deleted, "源码 {source}");
        }
    }

    /// 词首的 Delete 必须删字，不能变成「整对去格式」。
    ///
    /// 块首 `**` 的左右两端落在同一视觉列上，若两个停靠点都保留，
    /// 光标可能停在标记内部，Delete 就把整对标记抽掉——用户看到的是
    /// 「加粗没了、字还在」，与退格完全一样，而他想删的是第一个字。
    #[test]
    fn delete_at_bold_head_removes_first_char_not_formatting() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**很粗**");
        click(&mut app, &files, 2);
        app.update(Message::DeleteForward, &files);
        assert_eq!(app.document.text(), "**粗**");
        // 光标留在词首，下一次 Delete 继续删下一个字
        assert_eq!(caret_byte(&app), 2);
    }

    /// 同一起点：退格仍是「去格式、留文字」，两条键位分工必须分开。
    #[test]
    fn backspace_at_bold_head_strips_the_pair_and_keeps_text() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**很粗**");
        click(&mut app, &files, 2);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "很粗");
        assert_eq!(caret_byte(&app), 0);
    }

    /// 词首再按左移不能「空转」：原来会掉进块首标记里，视觉不动却换了位置。
    #[test]
    fn left_arrow_at_bold_head_does_not_enter_the_marker() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**很粗**");
        click(&mut app, &files, 2);
        let before = caret_byte(&app);
        app.update(Message::MoveCaret { delta: -1, extend: false }, &files);
        assert_eq!(caret_byte(&app), before, "词首左移应当无动作，而不是进标记");
        // 并且此时 Delete 仍然是删字
        app.update(Message::DeleteForward, &files);
        assert_eq!(app.document.text(), "**粗**");
    }

    #[test]
    fn delete_forward_on_last_visible_char_collapses_the_container() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**粗**");
        click(&mut app, &files, 2);
        app.update(Message::DeleteForward, &files);
        assert_eq!(app.document.text(), "");
    }

    #[test]
    fn selection_covering_container_content_collapses_it() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "甲**粗**乙");
        // 只选中内容，标记留在原处
        app.update(Message::Select { anchor: 5, head: 8 }, &files);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "甲乙");
    }

    #[test]
    fn selection_touching_half_a_marker_expands_to_the_whole_pair() {
        // 选区从内容末尾切进闭合标记一个字节：不扩就是 `**` 被削成 `*`
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "甲**粗**乙");
        app.update(Message::Select { anchor: 5, head: 9 }, &files);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "甲乙");
    }

    #[test]
    fn nested_containers_collapse_in_one_go() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**a *b* c**");
        // 选中整段可见内容，外层加粗也应随之收起
        app.update(Message::Select { anchor: 2, head: 9 }, &files);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "");
    }

    #[test]
    fn deletion_outside_any_container_is_unchanged() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "前缀 **粗** 后缀");
        let end = app.document.len_bytes();
        click(&mut app, &files, end);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "前缀 **粗** 后");
    }

    #[test]
    fn backspace_on_image_removes_the_whole_syntax() {
        // 图片在源码里是 `![图](a.png)`，可见只有一个占位符；
        // 按字节退格会把 `)` 单独删掉，把链接写坏。
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "前![图](a.png)");
        let end = app.document.len_bytes();
        click(&mut app, &files, end);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "前");
    }

    #[test]
    fn backspace_on_plain_text_is_unchanged() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "普通文字");
        click(&mut app, &files, 12);
        app.update(Message::Backspace, &files);
        assert_eq!(app.document.text(), "普通文");
    }

    #[test]
    fn backspace_at_block_start_merges_paragraphs() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "甲\n\n乙");
        click(&mut app, &files, 5); // 「乙」之前
        app.update(Message::Backspace, &files);
        // 删掉的是空行里最后一个换行，两段合成一段
        assert_eq!(app.document.text(), "甲\n乙");
        assert_eq!(app.parsed.blocks.len(), 1);
    }

    #[test]
    fn delete_forward_removes_next_char() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "abc");
        click(&mut app, &files, 0);
        app.update(Message::DeleteForward, &files);
        assert_eq!(app.document.text(), "bc");
        assert_eq!(caret_byte(&app), 0);
    }

    #[test]
    fn typing_replaces_selection() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "甲乙丙");
        app.update(Message::Select { anchor: 3, head: 6 }, &files);
        type_text(&mut app, &files, "丁");
        // 只替换选中的「乙」，前后字保留
        assert_eq!(app.document.text(), "甲丁丙");
        assert_eq!(caret_byte(&app), 6);
    }

    #[test]
    fn enter_splits_paragraph_and_lands_in_empty_one() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "abc");
        click(&mut app, &files, 3);
        app.update(Message::Newline, &files);
        assert_eq!(app.document.text(), "abc\n\n");
        assert_eq!(app.parsed.blocks.len(), 2, "末尾应出现空段落");
        assert_eq!(app.layout.locate(app.selection.head).unwrap().0 .0, 1);
        // 继续打字应写进新段落，而不是接在上一段行尾
        type_text(&mut app, &files, "d");
        assert_eq!(app.document.text(), "abc\n\nd");
    }

    #[test]
    fn enter_on_list_item_continues_marker() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "- 甲");
        click(&mut app, &files, 5);
        app.update(Message::Newline, &files);
        assert_eq!(app.document.text(), "- 甲\n- ");
        type_text(&mut app, &files, "乙");
        assert_eq!(app.document.text(), "- 甲\n- 乙");
        // 两项都还在同一个列表块里，第二项不能再冒出 `- ` 源码
        assert!(matches!(app.parsed.blocks[0].kind, BlockKind::List { ordered: false }));
    }

    #[test]
    fn enter_on_ordered_list_increments_number() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "1. 甲");
        let end = app.document.len_bytes();
        click(&mut app, &files, end);
        app.update(Message::Newline, &files);
        assert_eq!(app.document.text(), "1. 甲\n2. ");
    }

    #[test]
    fn enter_on_empty_item_leaves_the_list() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "- 甲\n- ");
        click(&mut app, &files, 8);
        app.update(Message::Newline, &files);
        assert_eq!(app.document.text(), "- 甲\n\n");
        assert_eq!(app.parsed.blocks.len(), 2);
        // 光标必须落在列表之后的新段落，而不是退回列表最后一行
        assert_eq!(app.layout.locate(app.selection.head).unwrap().0 .0, 1);
        type_text(&mut app, &files, "乙");
        assert_eq!(app.document.text(), "- 甲\n\n乙");
    }

    #[test]
    fn enter_twice_walks_out_of_the_last_list() {
        // 末尾的列表项没有行尾换行（刚起窗、光标型输入常见）：连按两次回车
        // 必须走到列表外面，否则光标退回最后一行、再也离不开列表
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "- 甲");
        click(&mut app, &files, 5);
        app.update(Message::Newline, &files);
        assert_eq!(app.document.text(), "- 甲\n- ");
        app.update(Message::Newline, &files);
        assert_eq!(app.document.text(), "- 甲\n\n");
        assert_eq!(
            app.layout.locate(app.selection.head).unwrap().0 .0,
            1,
            "光标退回了列表最后一行"
        );
        type_text(&mut app, &files, "乙");
        assert_eq!(app.document.text(), "- 甲\n\n乙");
    }

    #[test]
    fn enter_continues_blockquote() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "> 引用");
        let end = app.document.len_bytes();
        click(&mut app, &files, end);
        app.update(Message::Newline, &files);
        assert_eq!(app.document.text(), "> 引用\n> ");
    }

    #[test]
    fn enter_in_code_block_inserts_single_newline() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "```\nab\n```");
        click(&mut app, &files, 6);
        app.update(Message::Newline, &files);
        assert_eq!(app.document.text(), "```\nab\n\n```");
    }

    #[test]
    fn hard_break_splits_the_line_into_one_extra_column() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "ab");
        click(&mut app, &files, 1);
        app.update(Message::HardBreak, &files);
        assert_eq!(app.document.text(), "a\\\nb");
        // 反斜杠 + 换行合成一个 Break 片段：视觉上只占一列（一个 <br>）
        let block = &app.parsed.blocks[0];
        let brk = block
            .spans
            .iter()
            .find(|s| matches!(s.kind, mdview_markdown::SpanKind::Break { hard: true }))
            .expect("应产生硬换行片段");
        assert_eq!(brk.byte, 1..3);
        // 光标落在换行之后、b 之前
        assert_eq!(caret_byte(&app), 3);
    }

    #[test]
    fn composition_swallows_typing_and_commits_once() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "正文");
        click(&mut app, &files, 0);

        assert!(app.update(Message::CompositionStart, &files).is_empty());
        // 组合期间的输入一律吞掉，否则会和 preedit 双重插入
        assert!(app
            .update(
                Message::InsertText {
                    text: "x".into()
                },
                &files
            )
            .is_empty());
        assert_eq!(app.document.text(), "正文");

        app.update(
            Message::CompositionCommit {
                text: "你好".into(),
            },
            &files,
        );
        assert_eq!(app.document.text(), "你好正文");
        assert!(!app.composing);
        // 一次组合 = 一步撤销
        app.update(Message::Undo, &files);
        assert_eq!(app.document.text(), "正文");
    }

    #[test]
    fn composition_cancel_releases_the_suspended_state() {
        // 取消候选（Esc / 失焦）不发这一条，内核就永远停在 composing，
        // 之后所有输入被静默吞掉——用户视角是「编辑器死了」。
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "正文");
        click(&mut app, &files, 0);

        app.update(Message::CompositionStart, &files);
        assert!(app.composing);
        app.update(Message::CompositionCancel, &files);
        assert!(!app.composing);

        app.update(Message::InsertText { text: "甲".into() }, &files);
        assert_eq!(app.document.text(), "甲正文");
    }

    #[test]
    fn arrow_keys_cross_hidden_markers() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "**a**");
        click(&mut app, &files, 0);
        let mut path = vec![caret_byte(&app)];
        for _ in 0..3 {
            app.update(Message::MoveCaret { delta: 1, extend: false }, &files);
            path.push(caret_byte(&app));
        }
        // 块首点击已越过开头的 `**`（落在 2），之后每次 +1 站：2 -> 3 -> 5，末尾不越界
        assert_eq!(path, vec![2, 3, 5, 5]);
    }

    #[test]
    fn shift_arrow_extends_selection() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "甲乙丙");
        click(&mut app, &files, 0);
        let effects = app.update(
            Message::MoveCaret {
                delta: 2,
                extend: true,
            },
            &files,
        );
        let Some(Effect::Selection { byte, .. }) = effects.last() else {
            panic!("Shift+方向键应产生选区");
        };
        assert_eq!(byte, &(0..6));
        assert_eq!(&app.document.text()[byte.clone()], "甲乙");
    }

    #[test]
    fn patch_rewrites_only_the_edited_block() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "甲\n\n乙\n\n丙\n");
        // 等长替换：块内停靠点数量不变，后续块序号也不变，理想情况下只换中间那块
        app.update(Message::Select { anchor: 5, head: 8 }, &files);
        let effects = app.update(
            Message::InsertText {
                text: "丁".into(),
            },
            &files,
        );
        let Some(Effect::Patch { patch, .. }) = effects
            .iter()
            .find(|e| matches!(e, Effect::Patch { .. }))
        else {
            panic!("编辑必须产出增量帧");
        };
        assert_eq!(patch.replace, 1..2, "只应替换第 2 块");
        assert!(patch.html.contains("丁"));
        assert_eq!(patch.old_len, patch.new_len);
    }

    #[test]
    fn patch_covers_tail_when_stop_indices_shift() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "甲\n\n乙\n\n丙\n");
        app.update(Message::Select { anchor: 5, head: 8 }, &files);
        let effects = app.update(
            Message::InsertText {
                text: "丁戊".into(),
            },
            &files,
        );
        let Some(Effect::Patch { patch, .. }) = effects
            .iter()
            .find(|e| matches!(e, Effect::Patch { .. }))
        else {
            panic!("编辑必须产出增量帧");
        };
        // 停靠点总数变了，后续块的 data-b 全部平移，必须连尾部一起替换
        assert_eq!(patch.replace, 1..3);
    }

    #[test]
    fn undo_after_typing_restores_text_and_caret() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "abc");
        click(&mut app, &files, 3);
        type_text(&mut app, &files, "d");
        assert_eq!(app.document.text(), "abcd");
        app.update(Message::Undo, &files);
        assert_eq!(app.document.text(), "abc");
        assert!(app.selection.head < app.layout.stops());
    }

    #[test]
    fn empty_document_accepts_first_keystroke() {
        let mut files = InMemoryFileStore::default();
        let mut app = open(&mut files, "");
        assert_eq!(app.layout.stops(), 1, "空文档也要有一个可落光标的段落");
        type_text(&mut app, &files, "首");
        assert_eq!(app.document.text(), "首");
        assert_eq!(caret_byte(&app), 3);
    }

    #[test]
    fn analyze_line_recognises_markers() {
        let info = analyze_line("- 甲").expect("应识别无序列表");
        assert_eq!(info.repeat, "- ");
        assert!(!info.empty_item);

        let info = analyze_line("1. 甲").expect("应识别有序列表");
        assert_eq!(info.repeat, "2. ");

        let info = analyze_line("- ").expect("应识别空项");
        assert!(info.empty_item);

        let info = analyze_line("> 引用").expect("应识别引用");
        assert_eq!(info.repeat, "> ");

        let info = analyze_line("  * 甲").expect("应保留缩进");
        assert_eq!(info.repeat, "  * ");

        assert!(analyze_line("普通段落").is_none());
        assert!(analyze_line("2026 年").is_none(), "数字后没跟 . 或 ) 不算列表");
    }
}


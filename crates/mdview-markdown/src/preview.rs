//! 生成可交互的投影页，用浏览器验证视觉布局模型。
//!
//! 这一层不是最终 UI，而是**标定工具**：把停靠点表渲染成真实 DOM，让浏览器
//! 做断行、字距、缩放的活，再用肉眼与状态栏核对三件事——
//!
//! 1. 折叠的标记是否真的零宽（`**` 前后两个停靠点是否落在同一像素列）；
//! 2. 光标左右移动是否跨过标记而不停在标记里；
//! 3. 跨块选区的 byte 区间是否包含块间空行。
//!
//! 每个停靠点对应一个 DOM 元素，用 `data-b`（进入）与 `data-a`（离开）标出它
//! 覆盖的停靠点区间。光标的像素位置因此不需要任何字符测量：
//! `stop k` 的 x 就是 `data-a=k` 元素的右边缘，没有则取 `data-b=k` 的左边缘。
//!
//! 生产实现会把「每字符一个元素」换成「每片段一个元素 + 内核 align 换算」，
//! 否则英文单词会被拆成字符、任意处都能断行。

use std::{fmt::Write as _, ops::Range};

use crate::block::{escape, Block, BlockKind, ParseResult, SpanKind};
use crate::layout::DocumentLayout;
use crate::project::MarkdownParser;

pub fn preview_html(src: &str) -> String {
    let parsed = MarkdownParser::gfm().parse(src);
    let doc = DocumentLayout::build(src, &parsed);

    let mut body = String::new();
    for frame in block_frames(src, &parsed, &doc) {
        body.push_str(&frame.html);
    }

    let mut out = String::with_capacity(body.len() + STYLE.len() + SCRIPT.len() + 2048);
    out.push_str(
        "<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>mdview 投影验证</title>\n<style>\n",
    );
    out.push_str(STYLE);
    out.push_str("</style>\n</head>\n<body>\n<div id=\"doc\" tabindex=\"0\">\n");
    out.push_str(&body);
    out.push_str("\n</div>\n<div id=\"caret\"></div>\n<div id=\"bar\"></div>\n<script>\n");
    out.push_str(SCRIPT);
    out.push_str("\n</script>\n</body>\n</html>\n");
    out
}

/// 单个块的 HTML 帧。
///
/// `base` / `end` 是该块首尾停靠点的**全局**序号：前端替换节点后不必重新编号，
/// 因为紧随其后的块会因序号变化而一并被替换（见 [`diff_frames`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockFrame {
    pub id: usize,
    pub kind: &'static str,
    pub base: usize,
    pub end: usize,
    pub html: String,
}

/// 一次增量重绘：用 `html` 替换第 `replace` 段的块节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub old_len: usize,
    pub new_len: usize,
    pub replace: Range<usize>,
    pub html: String,
}

impl Patch {
    /// 是否什么都没变（编辑被合并进同一块且未改内容时会出现）。
    pub fn is_empty(&self) -> bool {
        self.replace.is_empty() && self.old_len == self.new_len
    }
}

/// 全文档分块帧。与 [`preview_html`] 共用同一套渲染代码，保证标定页与编辑页一致。
pub fn block_frames(src: &str, parsed: &ParseResult, doc: &DocumentLayout) -> Vec<BlockFrame> {
    let mut frames = Vec::with_capacity(parsed.blocks.len());
    for block in &parsed.blocks {
        let Some(html) = block_html(src, block, doc) else {
            continue;
        };
        let Some(base) = doc.global(block.id, 0) else {
            continue;
        };
        let end = base + doc.layout(block.id).map(|l| l.len()).unwrap_or(1) - 1;
        frames.push(BlockFrame {
            id: block.id.0,
            kind: block.kind.label(),
            base,
            end,
            html,
        });
    }
    frames
}

/// 最长公共前缀 / 后缀求差集：只替换中间真正变化的那一段块。
///
/// 不能用块 id 做 diff：一次编辑会让其后所有块的 byte 起点与停靠点序号整体平移，
/// 按 id 比对等价于全量重绘。前后缀比对至少保证「改动前后的块原样不动」。
pub fn diff_frames(old: &[BlockFrame], new: &[BlockFrame]) -> Patch {
    let shared = old.len().min(new.len());
    let head = old
        .iter()
        .zip(new)
        .take_while(|(a, b)| a.html == b.html)
        .count();
    let tail = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(shared - head)
        .take_while(|(a, b)| a.html == b.html)
        .count();
    let html = new[head..new.len() - tail]
        .iter()
        .map(|frame| frame.html.as_str())
        .collect();
    Patch {
        old_len: old.len(),
        new_len: new.len(),
        replace: head..new.len() - tail,
        html,
    }
}

fn block_html(src: &str, block: &Block, doc: &DocumentLayout) -> Option<String> {
    let layout = doc.layout(block.id)?;
    let stops = layout.stops();
    if stops.is_empty() {
        return None;
    }
    let first = doc.global(block.id, 0)?;
    let last = first + stops.len() - 1;

    let mut out = String::new();
    let _ = write!(
        out,
        "<section class=\"blk kind-{}\" data-block=\"{}\" data-bb=\"{first}\" data-be=\"{last}\">",
        block.kind.label(),
        block.id.0
    );

    if matches!(block.kind, BlockKind::Rule) {
        // 分隔线没有可见文本，整块就是一个控件
        let _ = write!(
            out,
            "<span class=\"atom rule\" data-b=\"{first}\" data-a=\"{last}\" \
             data-byte=\"{}\" data-block=\"{}\">────────</span>",
            block.byte.start, block.id.0
        );
        out.push_str("</section>\n");
        return Some(out);
    }

    let (open, close) = wrapper(&block.kind);
    // 列表的每一项要各自成为一个 `.li`，否则整块只有一个项目符号。
    // 项与项之间的硬换行由下面的 Break 分支负责拆行。
    let is_list = matches!(block.kind, BlockKind::List { .. });
    if is_list {
        out.push_str("<div class=\"p list\"><div class=\"li\">");
    } else {
        out.push_str(open);
    }
    for index in 0..stops.len().saturating_sub(1) {
        write_segment(&mut out, src, block, stops, index, first + index);
    }
    // 块尾锚点：最后一个停靠点没有对应字符，单独给一个零宽元素供光标停靠
    let _ = write!(
        out,
        "<span class=\"eob\" data-b=\"{last}\" data-byte=\"{}\" data-block=\"{}\">&#8203;</span>",
        block.byte.end, block.id.0
    );
    if is_list {
        out.push_str("</div></div>");
    } else {
        out.push_str(close);
    }
    out.push_str("</section>\n");
    Some(out)
}

fn write_segment(
    out: &mut String,
    src: &str,
    block: &Block,
    stops: &[crate::layout::Stop],
    index: usize,
    global: usize,
) {
    let stop = stops[index];
    let end = stops[index + 1].byte;
    let next = global + 1;
    let block_id = block.id.0;

    // 折叠标记：零宽，两个停靠点落在同一列
    if let Some(hidden) = block.hidden_at(stop.byte) {
        // 表格的竖线折叠后单元格会粘成一串，补一个纯装饰的间隙。
        // 它不带 data-b，不参与停靠点，光标位置不受影响。
        if matches!(block.kind, BlockKind::Table) {
            let raw = src.get(stop.byte..block.hidden[hidden].end).unwrap_or("");
            // 单元格之间的隐藏区间是 `" | "`，不以竖线开头，所以判包含而非前缀
            if raw.contains('|') && !raw.contains('-') && !raw.contains('\n') {
                out.push_str("<span class=\"sep\"></span>");
            }
        }
        let _ = write!(
            out,
            "<span class=\"mk\" data-b=\"{global}\" data-a=\"{next}\" \
             data-byte=\"{}\" data-block=\"{block_id}\">&#8203;</span>",
            stop.byte
        );
        return;
    }

    let Some(span_index) = block.span_at(stop.byte) else {
        return;
    };
    let span = &block.spans[span_index];
    match &span.kind {
        SpanKind::Image => {
            let _ = write!(
                out,
                "<span class=\"atom img\" data-b=\"{global}\" data-a=\"{next}\" \
                 data-byte=\"{}\" data-block=\"{block_id}\" title=\"{}\">图</span>",
                stop.byte,
                escape(&span.text)
            );
        }
        SpanKind::TaskMarker { checked } => {
            let _ = write!(
                out,
                "<span class=\"atom task\" data-b=\"{global}\" data-a=\"{next}\" \
                 data-byte=\"{}\" data-block=\"{block_id}\">{}</span>",
                stop.byte,
                if *checked { "☑" } else { "☐" }
            );
        }
        SpanKind::Rule => {
            let _ = write!(
                out,
                "<span class=\"atom rule\" data-b=\"{global}\" data-a=\"{next}\" \
                 data-byte=\"{}\" data-block=\"{block_id}\">────────</span>",
                stop.byte
            );
        }
        SpanKind::Break { .. } => {
            // 换行不占列宽但占一列位置。列表里换成 `.li` 边界，让每项各带项目符号；
            // 其余情况交给 <br>。只标 before，让下一个字符元素承担 after，
            // 光标才不会定位到零高矩形上。
            if matches!(block.kind, BlockKind::List { .. }) {
                let _ = write!(
                    out,
                    "</div><div class=\"li\"><span class=\"mk\" data-b=\"{global}\" \
                     data-byte=\"{}\" data-block=\"{block_id}\">&#8203;</span>",
                    stop.byte
                );
            } else {
                let _ = write!(
                    out,
                    "<br class=\"nl\" data-b=\"{global}\" data-byte=\"{}\" data-block=\"{block_id}\">",
                    stop.byte
                );
            }
        }
        _ => {
            let ch = src[stop.byte..end.min(src.len())].chars().next();
            let Some(ch) = ch else {
                return;
            };
            if ch == '\n' {
                let _ = write!(
                    out,
                    "<br class=\"nl\" data-b=\"{global}\" data-byte=\"{}\" data-block=\"{block_id}\">",
                    stop.byte
                );
                return;
            }
            let class = char_class(block, span_index);
            let _ = write!(
                out,
                "<span class=\"ch {class}\" data-b=\"{global}\" data-a=\"{next}\" \
                 data-byte=\"{}\" data-block=\"{block_id}\">{}</span>",
                stop.byte,
                escape(&ch.to_string())
            );
        }
    }
}

fn char_class(block: &Block, span_index: usize) -> String {
    let span = &block.spans[span_index];
    let style = &span.style;
    let mut class = String::new();
    if style.strong {
        class.push_str("b ");
    }
    if style.emphasis {
        class.push_str("i ");
    }
    if style.strikethrough {
        class.push_str("d ");
    }
    if style.code {
        class.push_str("c ");
    }
    if style.link.is_some() && !matches!(span.kind, SpanKind::Image) {
        class.push_str("a ");
    }
    class.trim_end().to_owned()
}

fn wrapper(kind: &BlockKind) -> (&'static str, &'static str) {
    match kind {
        BlockKind::Heading(1) => ("<h1>", "</h1>"),
        BlockKind::Heading(2) => ("<h2>", "</h2>"),
        BlockKind::Heading(3) => ("<h3>", "</h3>"),
        BlockKind::Heading(4) => ("<h4>", "</h4>"),
        BlockKind::Heading(_) => ("<h5>", "</h5>"),
        BlockKind::CodeBlock { .. } => ("<div class=\"code\">", "</div>"),
        BlockKind::BlockQuote => ("<blockquote>", "</blockquote>"),
        BlockKind::Table => ("<div class=\"p table\">", "</div>"),
        _ => ("<div class=\"p\">", "</div>"),
    }
}

const STYLE: &str = r#"
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  font: 16px/1.85 -apple-system, "Segoe UI", "Microsoft YaHei", "PingFang SC", sans-serif;
  color: #1f2328; background: #fff;
  -webkit-font-smoothing: antialiased;
}
#doc {
  max-width: 780px; margin: 0 auto; padding: 64px 32px 45vh;
  outline: none; cursor: text;
}
.blk { position: relative; margin: 0 0 1.1em; white-space: pre-wrap; }
.p { margin: 0; }
h1, h2, h3, h4, h5, h6 { margin: 1.5em 0 .6em; line-height: 1.35; font-weight: 650; }
h1 { font-size: 1.9em; } h2 { font-size: 1.55em; } h3 { font-size: 1.3em; }
h4 { font-size: 1.12em; } h5, h6 { font-size: 1em; }
.code {
  font-family: "Cascadia Code", Consolas, "Courier New", monospace;
  font-size: .92em; line-height: 1.6; white-space: pre;
  background: #f6f8fa; border: 1px solid #eaecef; border-radius: 6px;
  padding: .9em 1em; overflow-x: auto;
}
blockquote {
  margin: 0; padding-left: 1em; border-left: 3px solid #dfe2e5; color: #57606a;
}
.kind-list { padding-left: 1.5em; }
.li { position: relative; }
.li::before { content: "•"; position: absolute; left: -1.05em; color: #8b949e; }
.li:has(.atom.task)::before { content: none; }
.li:has(.atom.task) { padding-left: .1em; }
.sep { display: inline-block; width: .55em; }
.atom.rule { display: block; color: #d0d7de; letter-spacing: .1em; }
.ch { }
.b { font-weight: 650; }
.i { font-style: italic; }
.d { text-decoration: line-through; color: #6e7781; }
.c {
  font-family: "Cascadia Code", Consolas, monospace; font-size: .9em;
  background: #eff1f3; border-radius: 3px;
}
.a { color: #0969da; text-decoration: underline; text-underline-offset: 2px; }
.mk { color: transparent; }
.eob, .mk { user-select: none; }
.atom {
  display: inline-block; vertical-align: baseline;
}
.atom.img {
  min-width: 1.6em; padding: 0 .35em; border: 1px dashed #b6bcc4; border-radius: 4px;
  color: #57606a; font-size: .82em; text-align: center; background: #fafbfc;
}
.atom.task { color: #57606a; }
.sel { background: rgba(37, 99, 235, .18); border-radius: 2px; }
#caret {
  position: absolute; width: 2px; background: #0969da; border-radius: 1px;
  pointer-events: none; display: none;
}
#caret.blink { animation: caret-blink 1.05s steps(1) infinite; }
@keyframes caret-blink { 50% { opacity: 0; } }
#bar {
  position: fixed; left: 0; right: 0; bottom: 0; z-index: 10;
  display: flex; flex-wrap: wrap; gap: 18px; align-items: center;
  padding: 8px 18px; font: 12px/1.6 ui-monospace, Consolas, monospace;
  color: #57606a; background: rgba(255, 255, 255, .94);
  border-top: 1px solid #eaecef; backdrop-filter: blur(6px);
}
#bar b { color: #1f2328; font-weight: 600; }
#bar .hint { margin-left: auto; color: #8b949e; }
"#;

const SCRIPT: &str = r#"
(function () {
  var docEl = document.getElementById('doc');
  var caretEl = document.getElementById('caret');
  var barEl = document.getElementById('bar');

  var byBefore = new Map();
  var byAfter = new Map();
  var marks = [];
  var maxStop = 0;

  document.querySelectorAll('[data-b]').forEach(function (el) {
    var b = Number(el.dataset.b);
    if (b > maxStop) maxStop = b;
    if (!byBefore.has(b)) byBefore.set(b, el);
    if (el.hasAttribute('data-a')) {
      var a = Number(el.dataset.a);
      if (a > maxStop) maxStop = a;
      if (!byAfter.has(a)) byAfter.set(a, el);
    }
    marks.push(el);
  });

  var state = { anchor: 0, head: 0 };
  var selected = [];

  function rectOf(stop) {
    var probes = [
      { el: byAfter.get(stop), edge: 'right' },
      { el: byBefore.get(stop), edge: 'left' }
    ];
    for (var i = 0; i < probes.length; i++) {
      var probe = probes[i];
      if (!probe.el) continue;
      var r = probe.el.getBoundingClientRect();
      if (r.height > 0) {
        return {
          x: probe.edge === 'right' ? r.right : r.left,
          y: r.top, h: r.height, el: probe.el
        };
      }
    }
    return null;
  }

  function blockOf(stop) {
    var el = byBefore.get(stop) || byAfter.get(stop);
    return el ? el.closest('.blk') : null;
  }

  function move(delta, extend) {
    var next = Math.min(Math.max(state.head + delta, 0), maxStop);
    if (extend) state.head = next;
    else { state.anchor = next; state.head = next; }
    render(true);
  }

  function jump(edge, extend) {
    var blk = blockOf(state.head);
    if (!blk) return;
    var target = Number(edge === 'start' ? blk.dataset.bb : blk.dataset.be);
    if (extend) state.head = target;
    else { state.anchor = target; state.head = target; }
    render(true);
  }

  function render(scroll) {
    selected.forEach(function (el) { el.classList.remove('sel'); });
    selected = [];

    var lo = Math.min(state.anchor, state.head);
    var hi = Math.max(state.anchor, state.head);
    if (lo !== hi) {
      marks.forEach(function (el) {
        var b = Number(el.dataset.b);
        var a = el.hasAttribute('data-a') ? Number(el.dataset.a) : b + 1;
        if (b >= lo && a <= hi && !el.classList.contains('mk') && !el.classList.contains('eob')) {
          el.classList.add('sel');
          selected.push(el);
        }
      });
    }

    var pos = rectOf(state.head);
    if (pos) {
      caretEl.style.display = 'block';
      caretEl.style.left = (pos.x + window.scrollX) + 'px';
      caretEl.style.top = (pos.y + window.scrollY) + 'px';
      caretEl.style.height = Math.max(pos.h, 18) + 'px';
      if (scroll) pos.el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    } else {
      caretEl.style.display = 'none';
    }

    var el = byBefore.get(state.head) || byAfter.get(state.head);
    var blk = el ? el.closest('.blk') : null;
    var blocks = document.querySelectorAll('.blk').length;
    barEl.innerHTML =
      '<span>块 <b>' + (blk ? Number(blk.dataset.block) + 1 : '-') + '/' + blocks + '</b></span>' +
      '<span>停靠点 <b>' + state.head + '</b></span>' +
      '<span>byte <b>' + (el ? el.dataset.byte : '-') + '</b></span>' +
      '<span>选区 <b>' + (hi - lo) + '</b> 站</span>' +
      '<span class="hint">← → 移动 · Shift 扩展 · Home/End 到块首尾</span>';
  }

  function stopFromPoint(x, y) {
    var el = document.elementFromPoint(x, y);
    if (!el) return null;
    var hit = el.closest('[data-b]');
    if (!hit) return null;
    var b = Number(hit.dataset.b);
    if (!hit.hasAttribute('data-a')) return b;
    var a = Number(hit.dataset.a);
    var r = hit.getBoundingClientRect();
    if (r.width === 0) return b;
    return x > r.left + r.width / 2 ? a : b;
  }

  var dragging = false;

  docEl.addEventListener('mousedown', function (e) {
    var stop = stopFromPoint(e.clientX, e.clientY);
    if (stop === null) return;
    e.preventDefault();
    if (e.shiftKey) state.head = stop;
    else { state.anchor = stop; state.head = stop; }
    dragging = true;
    docEl.focus({ preventScroll: true });
    render(false);
  });

  window.addEventListener('mouseup', function () { dragging = false; });
  window.addEventListener('mousemove', function (e) {
    if (!dragging) return;
    var stop = stopFromPoint(e.clientX, e.clientY);
    if (stop === null || stop === state.head) return;
    state.head = stop;
    render(false);
  });

  docEl.addEventListener('keydown', function (e) {
    switch (e.key) {
      case 'ArrowLeft': move(-1, e.shiftKey); break;
      case 'ArrowRight': move(1, e.shiftKey); break;
      case 'Home': jump('start', e.shiftKey); break;
      case 'End': jump('end', e.shiftKey); break;
      default: return;
    }
    e.preventDefault();
  });

  window.addEventListener('scroll', function () { render(false); }, { passive: true });
  window.addEventListener('resize', function () { render(false); });

  state.head = 0;
  render(false);
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_visible_char_gets_an_element() {
        let html = preview_html("**加粗**与`code`");
        assert!(html.contains("data-block=\"0\""));
        // 可见字符与「能停靠的」折叠标记都必须是独立元素，否则无法按停靠点定位。
        // 块首的 `**` 不产生停靠点（见 layout 的 block_start_marker_is_not_a_resting_place），
        // 因此这里只有 3 个标记元素。
        assert_eq!(html.matches("class=\"mk\"").count(), 3);
        assert_eq!(html.matches("class=\"ch ").count(), 7);
    }

    #[test]
    fn markup_is_escaped() {
        let html = preview_html("点击 <script>alert(1)</script> 这里\n");
        // 每个字符独立成元素，所以断言的是尖括号被转义而非整段字符串
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;"));
        assert!(html.contains("&gt;"));
    }

    #[test]
    fn block_boundaries_are_recorded() {
        let html = preview_html("# 甲\n\n乙\n");
        assert_eq!(html.matches("class=\"blk").count(), 2);
        assert!(html.contains("data-bb=\"0\""));
        assert!(html.contains("data-be="));
    }

    #[test]
    fn stop_indices_are_global_across_blocks() {
        // 第二块的 data-b 必须接在第一块之后，否则跨块选区无从判断顺序
        let html = preview_html("甲\n\n乙\n");
        let second = html
            .split("data-block=\"1\" data-bb=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next());
        assert_eq!(second, Some("2"));
    }

    fn frames_of(src: &str) -> Vec<BlockFrame> {
        let parsed = MarkdownParser::gfm().parse(src);
        let doc = DocumentLayout::build(src, &parsed);
        block_frames(src, &parsed, &doc)
    }

    #[test]
    fn frames_are_exactly_the_page_body() {
        let src = "# 甲\n\n乙**丙**\n\n- 丁\n";
        let joined: String = frames_of(src).iter().map(|f| f.html.as_str()).collect();
        // 标定页与编辑页必须出自同一套渲染代码，否则调好的坐标在编辑态不成立
        assert!(preview_html(src).contains(&joined));
    }

    #[test]
    fn diff_replaces_only_changed_block() {
        // 等长替换：停靠点总数不变，后续块的 data-b 也不变，只该换中间那块
        let patch = diff_frames(&frames_of("甲\n\n乙\n\n丙\n"), &frames_of("甲\n\n丁\n\n丙\n"));
        assert_eq!(patch.replace, 1..2);
        assert!(patch.html.contains("丁"));
        assert_eq!(patch.old_len, 3);
    }

    #[test]
    fn diff_covers_tail_when_stop_count_changes() {
        // 停靠点总数一变，后面每块的 data-b 都要重编号，只能连尾部一起替换
        let patch = diff_frames(&frames_of("甲\n\n乙\n\n丙\n"), &frames_of("甲\n\n丁戊\n\n丙\n"));
        assert_eq!(patch.replace, 1..3);
    }

    #[test]
    fn trailing_empty_paragraph_gets_its_own_frame() {
        let frames = frames_of("abc\n\n");
        assert_eq!(frames.len(), 2);
        assert!(frames[1].html.contains("class=\"eob\""));
        assert_eq!(frames[1].base, frames[1].end, "空段落只有一个停靠点");
    }
}

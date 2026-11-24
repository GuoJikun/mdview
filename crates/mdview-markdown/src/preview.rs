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

use crate::block::{
    Block, BlockKind, ColumnAlign, ParseResult, SpanKind, escape, escape_attr, item_marker,
};
use crate::highlight::language_word;
use crate::layout::DocumentLayout;
use crate::project::MarkdownParser;

/// 投影页里图片可以直接用 URL；编辑态则先挂透明像素，等前端装载本地文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageMode {
    Direct,
    Deferred,
}

const TRANSPARENT_PIXEL: &str =
    "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

pub fn preview_html(src: &str) -> String {
    let parsed = MarkdownParser::gfm().parse(src);
    let doc = DocumentLayout::build(src, &parsed);

    let mut body = String::new();
    for frame in block_frames_inner(src, &parsed, &doc, ImageMode::Direct) {
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
    block_frames_inner(src, parsed, doc, ImageMode::Deferred)
}

fn block_frames_inner(
    src: &str,
    parsed: &ParseResult,
    doc: &DocumentLayout,
    images: ImageMode,
) -> Vec<BlockFrame> {
    let mut frames = Vec::with_capacity(parsed.blocks.len());
    for block in &parsed.blocks {
        let Some(html) = block_html(src, block, doc, images) else {
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

/// 表格网格渲染。
///
/// 关键：**不改变停靠点集合**。每个 `<th>/<td>` 里塞进去的仍是原来那一串
/// `write_segment` 输出，`data-b` / `data-a` 原样保留，只是按单元格重新分组。
/// 竖线、分隔行与行尾换行不属于任何单元格，它们的零宽停靠点就近挂在格子内，
/// 否则 `<span>` 会挂在 `<tr>` 下被浏览器搬走，光标就找不到像素位置了。
fn table_html(
    src: &str,
    block: &Block,
    grid: &crate::block::TableGrid,
    stops: &[crate::layout::Stop],
    first: usize,
    images: ImageMode,
) -> String {
    if grid.rows.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("<table class=\"md-table\">");
    let mut open_row: Option<usize> = None;
    let mut open_cell: Option<(usize, usize)> = None;

    for (index, stop) in stops.iter().enumerate() {
        // 孤立停靠点归到当前格；表格开头的归第一格
        let (row, col) = grid.locate(stop.byte).or(open_cell).unwrap_or((0, 0));
        if open_row != Some(row) {
            if let Some(prev) = open_row {
                out.push_str(if grid.rows[prev].head {
                    "</th>"
                } else {
                    "</td>"
                });
                out.push_str("</tr>");
                if prev == 0 {
                    out.push_str("</thead><tbody>");
                }
            } else {
                out.push_str("<thead>");
            }
            out.push_str("<tr>");
            open_row = Some(row);
            open_cell = None;
        }
        if open_cell != Some((row, col)) {
            if open_cell.is_some() {
                out.push_str(if grid.rows[row].head {
                    "</th>"
                } else {
                    "</td>"
                });
            }
            let table_row = &grid.rows[row];
            let tag = if table_row.head { "th" } else { "td" };
            let align = match grid.alignment.get(col).copied().unwrap_or_default() {
                ColumnAlign::Left => " al",
                ColumnAlign::Center => " ac",
                ColumnAlign::Right => " ar",
                ColumnAlign::None => "",
            };
            if let Some(cell) = table_row.cells.get(col) {
                let _ = write!(
                    out,
                    "<{tag} class=\"cell{align}\" data-cell=\"{row},{col}\" \
                     data-byte=\"{}\" data-cend=\"{}\" data-block=\"{}\">",
                    cell.byte.start, cell.byte.end, block.id.0
                );
            }
            open_cell = Some((row, col));
        }
        // 块尾锚点：最后一个停靠点没有后继，write_segment 用不到它
        if index + 1 == stops.len() {
            let _ = write!(
                out,
                "<span class=\"eob\" data-b=\"{}\" data-byte=\"{}\" data-block=\"{}\">&#8203;</span>",
                first + index,
                block.byte.end,
                block.id.0
            );
            continue;
        }
        // 行尾换行在网格里不是可见字符。改成零宽标记，既保住停靠点的像素位置，
        // 又不会在单元格里多顶出一行。
        if block
            .span_at(stop.byte)
            .is_some_and(|i| matches!(block.spans[i].kind, SpanKind::Break { .. }))
        {
            let _ = write!(
                out,
                "<span class=\"mk\" data-b=\"{}\" data-byte=\"{}\" data-block=\"{}\">&#8203;</span>",
                first + index,
                stop.byte,
                block.id.0
            );
            continue;
        }
        write_segment(&mut out, src, block, stops, index, first + index, images);
    }

    if let Some(row) = open_row {
        out.push_str(if grid.rows[row].head {
            "</th>"
        } else {
            "</td>"
        });
        out.push_str("</tr>");
        out.push_str(if row == 0 { "</thead>" } else { "</tbody>" });
    }
    out.push_str("</table>");
    out
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

fn block_html(src: &str, block: &Block, doc: &DocumentLayout, images: ImageMode) -> Option<String> {
    let layout = doc.layout(block.id)?;
    let stops = layout.stops();
    if stops.is_empty() {
        return None;
    }
    let first = doc.global(block.id, 0)?;
    let last = first + stops.len() - 1;

    let mut out = String::new();
    // 语言标签挂在块容器上，由 CSS 伪元素画在右上角；不参与逐字符停靠点。
    // 围栏后没有语言时（缩进代码块、裸 ```）就不加属性。
    let lang = match &block.kind {
        BlockKind::CodeBlock { info } => language_word(info.as_deref()),
        _ => None,
    };
    let lang_attr = match lang {
        Some(lang) => format!(" data-lang=\"{}\"", escape_attr(lang)),
        None => String::new(),
    };
    // 脚注定义挂 id 与编号：引用处的 Ctrl/Cmd+Click 据此跳转；
    // 序号由 CSS ::before 显示，源码里的 `[^label]: ` 仍是折叠标记。
    let fn_attr = match &block.kind {
        BlockKind::FootnoteDefinition { number, .. } => {
            format!(" id=\"fn{number}\" data-fn=\"{number}\"")
        }
        _ => String::new(),
    };
    let _ = write!(
        out,
        "<section class=\"blk kind-{}\" data-block=\"{}\" data-bb=\"{first}\" data-be=\"{last}\"{lang_attr}{fn_attr}>",
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

    // 表格走专用网格渲染：把同一组线性停靠点铺进 <table> 的单元格，
    // 每格仍是原来的 data-b/data-a，光标与选区完全不变。
    if let Some(grid) = &block.table {
        out.push_str(&table_html(src, block, grid, stops, first, images));
        out.push_str("</section>\n");
        return Some(out);
    }

    let (open, close) = wrapper(&block.kind);
    // 列表的每一项要各自成为一个 `.li`，否则整块只有一个项目符号。
    // 项与项之间的硬换行由下面的 Break 分支负责拆行。
    let is_list = matches!(block.kind, BlockKind::List { .. });
    if is_list {
        out.push_str("<div class=\"p list\">");
        out.push_str(&li_open(block.first_list_depth()));
        push_item_marker(&mut out, src, block.byte.start);
        out.push('>');
    } else {
        out.push_str(open);
    }
    for index in 0..stops.len().saturating_sub(1) {
        // 块级公式的字符套一层 `.md`：编辑态/阅读态共用这一份 DOM，
        // 让 `$$\n…\n$$` 视觉上成一块居中区域，而不是散在行内的源码字符。
        // 包裹层不带 data-b，不参与停靠点与选区。
        let cur_math = is_display_math(block, stops[index].byte);
        if cur_math && (index == 0 || !is_display_math(block, stops[index - 1].byte)) {
            out.push_str("<span class=\"md\">");
        }
        write_segment(&mut out, src, block, stops, index, first + index, images);
        if cur_math
            && (index + 1 == stops.len().saturating_sub(1)
                || !is_display_math(block, stops[index + 1].byte))
        {
            out.push_str("</span>");
        }
    }
    // 块尾锚点：最后一个停靠点没有对应字符，单独给一个零宽元素供光标停靠
    let _ = write!(
        out,
        "<span class=\"eob\" data-b=\"{last}\" data-byte=\"{}\" data-block=\"{}\">&#8203;</span>",
        block.byte.end, block.id.0
    );
    // 回链放在块尾锚点之后：没有 data-b，不占停靠点，点击只跳转。
    if let BlockKind::FootnoteDefinition {
        number, references, ..
    } = &block.kind
    {
        out.push_str("<span class=\"fn-back\">");
        for (i, byte) in references.iter().enumerate() {
            let mark = if references.len() == 1 {
                "↩".to_owned()
            } else {
                format!("↩{}", i + 1)
            };
            let _ = write!(
                out,
                "<a href=\"#fnref{number}_{byte}\" data-fnback=\"{number}\" data-fnbyte=\"{byte}\">{mark}</a> "
            );
        }
        out.push_str("</span>");
    }
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
    images: ImageMode,
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
            // 整张图片仍是一个原子控件：停靠点挂在外层 span 上，内层 img 不参与寻址。
            // data-asset 保留原始 URL；桌面端据此在沙箱内读取本地文件并换成 data URL。
            let url = span.style.link.as_deref().unwrap_or("");
            let src = match images {
                ImageMode::Direct => escape_attr(url),
                ImageMode::Deferred => TRANSPARENT_PIXEL.to_string(),
            };
            let _ = write!(
                out,
                "<span class=\"atom img\" data-b=\"{global}\" data-a=\"{next}\" \
                 data-byte=\"{}\" data-block=\"{block_id}\" data-asset=\"{}\" \
                 title=\"{}\"><img alt=\"\" src=\"{}\"></span>",
                stop.byte,
                escape_attr(url),
                escape_attr(&span.text),
                src
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
                out.push_str("</div>");
                out.push_str(&li_open(block.list_depth_after(stop.byte)));
                // 有序项把序号附着在这个新行上：`1. ` 那几个字节是折叠的，
                // 视图里必须有个东西代替它，否则整份列表都长成圆点。
                push_item_marker(out, src, stop.byte + 1);
                let _ = write!(
                    out,
                    "><span class=\"mk\" data-b=\"{global}\" \
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
                // 代码块末尾的换行只是最后一行收尾，不是空行。它后面紧接着
                // 闭合围栏与块尾锚点（都是零宽元素），再输出 `<br>` 会把它们
                // 顶到下一行，代码框底部就多出一行空白。换成零宽标记，
                // 停靠点与光标位置不变（表格行尾同理）。
                if is_code_tail_newline(block, stops, index) {
                    let _ = write!(
                        out,
                        "<span class=\"mk\" data-b=\"{global}\" data-byte=\"{}\" \
                         data-block=\"{block_id}\">&#8203;</span>",
                        stop.byte
                    );
                    return;
                }
                let _ = write!(
                    out,
                    "<br class=\"nl\" data-b=\"{global}\" data-byte=\"{}\" data-block=\"{block_id}\">",
                    stop.byte
                );
                return;
            }
            let class = char_class(block, span_index, stop.byte);
            // 链接字符带上目标地址：URL 在投影里是折叠的（`.mk`），前端拿不到
            // 它就无从知道该打开哪里。**每个字符各挂一份**——点击命中的是单个
            // 字符元素，它和同链接的其它字符是平级兄弟，`closest` 只向上找，
            // 挂在一处、指望邻居读到是不成立的。
            let href = match span.style.link.as_deref() {
                Some(url) if !url.is_empty() => {
                    format!(" data-href=\"{}\"", escape_attr(url))
                }
                _ => String::new(),
            };
            // 脚注引用的每个字符带编号与 byte（同链接一样逐字符挂），前端据此
            // Ctrl/Cmd+Click 跳到 `#fnN`；byte 让多处引用各有唯一锚点供回链定位。
            let fnref = match span.kind {
                SpanKind::FootnoteReference { number, .. } => {
                    // 第一个字符携带锚点 id，与 to_html / 回链的 `fnref{n}_{byte}` 对齐
                    let anchor = if stop.byte == span.byte.start {
                        format!(" id=\"fnref{number}_{}\"", span.byte.start)
                    } else {
                        String::new()
                    };
                    format!(
                        "{anchor} data-fnref=\"{number}\" data-fnbyte=\"{}\"",
                        span.byte.start
                    )
                }
                _ => String::new(),
            };
            let _ = write!(
                out,
                "<span class=\"ch {class}\"{href}{fnref} data-b=\"{global}\" data-a=\"{next}\" \
                 data-byte=\"{}\" data-block=\"{block_id}\">{}</span>",
                stop.byte,
                escape(&ch.to_string())
            );
        }
    }
}

/// 代码块末尾的换行：它之后直到块尾锚点之间全是隐藏区间（闭合围栏）。
///
/// 中间真正的空行后面还有可见换行，仍会正常输出 `<br>`；只有最后一行的
/// 收尾换行被折叠成零宽标记，避免代码框底部多出一行空白。
fn is_code_tail_newline(block: &Block, stops: &[crate::layout::Stop], index: usize) -> bool {
    matches!(block.kind, BlockKind::CodeBlock { .. })
        && stops[index + 1..stops.len().saturating_sub(1)]
            .iter()
            .all(|stop| block.hidden_at(stop.byte).is_some())
}

/// 把这一项的**有序标记**（`1.`、`2)`）写进元素的 `data-m` 属性。
///
/// 序号不是用 CSS counter 数出来的，而是逐项从源码读的：用户写下 `3. 甲`，
/// 视图里就该显示 `3.`——他可能从别处粘来半截列表，也可能就想从 3 开始。
/// counter 会把它悄悄改成 1，「看到的就是文件里的样子」这条就破了。
/// 编辑视图的 `<div class="li">` 起始标签。`data-depth` 是层级信号，
/// CSS 据此缩进并切换项目符号；`data-b/data-a` 仍由后文按原顺序输出，
/// 光标停靠点一个不变。
fn li_open(depth: Option<usize>) -> String {
    match depth {
        Some(0) | None => "<div class=\"li\"".to_owned(),
        Some(d) => format!("<div class=\"li\" data-depth=\"{d}\""),
    }
}

fn push_item_marker(out: &mut String, src: &str, start: usize) {
    if let Some(marker) = item_marker(src, start) {
        let _ = write!(out, " data-m=\"{}\"", escape(&marker));
    }
}

/// 这个 byte 是否落在块级公式的可见源码里。用于在 DOM 里套一层包裹元素，
/// 让 `$$…$$` 与行内 `$…$` 在视觉上分开；不影响任何停靠点。
fn is_display_math(block: &Block, byte: usize) -> bool {
    block
        .span_at(byte)
        .is_some_and(|i| matches!(block.spans[i].kind, SpanKind::Math { display: true }))
}

fn char_class(block: &Block, span_index: usize, byte: usize) -> String {
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
    if matches!(span.kind, SpanKind::FootnoteReference { .. }) {
        class.push_str("fnref ");
    }
    // 公式字符逐字可寻址：只加样式类，不当原子控件，光标/选区逻辑不变。
    // 块级公式的 `.md` 包裹由 block_html 统一套，这里只负责逐字着色。
    if matches!(span.kind, SpanKind::Math { .. }) {
        class.push_str("m ");
    }
    // 代码块的词法着色。`Block::tokens` 只对 CodeBlock 非空，其它块查不到，
    // 因此不需要在这里再判一次块类型。
    if let Some(token) = token_class(block, byte) {
        class.push_str(token);
        class.push(' ');
    }
    class.trim_end().to_owned()
}

/// byte 落在哪个高亮片段里。`tokens` 按 byte 升序且互不重叠。
fn token_class(block: &Block, byte: usize) -> Option<&'static str> {
    let index = block.tokens.partition_point(|t| t.byte.end <= byte);
    let token = block.tokens.get(index)?;
    (token.byte.start <= byte).then(|| token.kind.class())
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
        BlockKind::FootnoteDefinition { .. } => ("<div class=\"p fn-def\">", "</div>"),
        _ => ("<div class=\"p\">", "</div>"),
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;

    const TABLE: &str = "| 左 | 中 | 右 |\n| :-- | :-: | --: |\n| a | **b** | c |\n| d | e | f |\n";

    #[test]
    fn footnote_frames_have_anchors_and_backlinks() {
        let src = "正文[^a]与[^a]。\n\n[^a]: 脚注内容。\n";
        let parsed = MarkdownParser::gfm().parse(src);
        let doc = DocumentLayout::build(src, &parsed);
        let html = block_frames(src, &parsed, &doc)
            .iter()
            .map(|f| f.html.as_str())
            .collect::<String>();
        assert!(html.contains("id=\"fn1\""), "{html}");
        assert_eq!(html.matches("data-fnref=\"1\"").count(), 8, "{html}");
        assert_eq!(html.matches("id=\"fnref1_").count(), 2, "{html}");
        assert_eq!(html.matches("data-fnback=\"1\"").count(), 2, "{html}");
    }

    #[test]
    fn table_renders_as_grid_with_every_stop() {
        let parsed = MarkdownParser::gfm().parse(TABLE);
        let doc = DocumentLayout::build(TABLE, &parsed);
        let frames = block_frames(TABLE, &parsed, &doc);
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        let html = &frame.html;

        assert!(html.contains("<table class=\"md-table\">"));
        assert!(html.contains("<thead>"));
        assert!(html.contains("</thead><tbody>"));
        assert_eq!(html.matches("<th ").count(), 3, "表头三格");
        assert_eq!(html.matches("<td ").count(), 6, "正文两行各三格");
        assert!(html.contains("class=\"cell al\""), "左对齐");
        assert!(html.contains("class=\"cell ac\""), "居中");
        assert!(html.contains("class=\"cell ar\""), "右对齐");

        // 渲染前是线性停靠点表，渲染后一个都不能丢，否则前端定位不到光标
        for stop in frame.base..=frame.end {
            assert!(
                html.contains(&format!("data-b=\"{stop}\"")),
                "表格 HTML 缺少停靠点 {stop}"
            );
        }
    }

    #[test]
    fn table_frame_bounds_match_layout() {
        let parsed = MarkdownParser::gfm().parse(TABLE);
        let doc = DocumentLayout::build(TABLE, &parsed);
        let frame = &block_frames(TABLE, &parsed, &doc)[0];
        let end = frame.base + doc.layout(parsed.blocks[0].id).unwrap().len() - 1;
        assert_eq!(frame.end, end);
    }
}

const STYLE: &str = r#"
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  font: 14px/1.85 -apple-system, "Segoe UI", "Microsoft YaHei", "PingFang SC", sans-serif;
  color: #1f2328; background: #fff;
  -webkit-font-smoothing: antialiased;
}
/* Markdown 投影的短类名一律限定在 #doc 下，与外壳 UI 隔离（桌面端同理）。 */
#doc {
  max-width: 780px; margin: 0 auto; padding: 64px 32px 45vh;
  outline: none; cursor: text;
}
#doc .blk { position: relative; margin: 0 0 1.1em; white-space: pre-wrap; }
#doc .p { margin: 0; }
#doc h1, #doc h2, #doc h3, #doc h4, #doc h5, #doc h6 {
  margin: 1.5em 0 .6em; line-height: 1.35; font-weight: 650;
}
#doc h1 { font-size: 1.9em; } #doc h2 { font-size: 1.55em; } #doc h3 { font-size: 1.3em; }
#doc h4 { font-size: 1.12em; } #doc h5, #doc h6 { font-size: 1em; }
#doc .code {
  font-family: "Cascadia Code", Consolas, "Courier New", monospace;
  font-size: .92em; line-height: 1.6; white-space: pre;
  background: #f6f8fa; border: 1px solid #eaecef; border-radius: 6px;
  padding: .9em 1em; overflow-x: auto;
}
/* 代码块语言标签：与桌面端 app.css 同一套做法，伪元素不参与停靠点。 */
#doc .kind-codeblock[data-lang]::after {
  content: attr(data-lang);
  position: absolute; top: .45em; right: .75em;
  padding: 0 .4em;
  font: 500 11px/1.5 -apple-system, "Segoe UI", "Microsoft YaHei", sans-serif;
  color: #8b949e; background: #f6f8fa;
  border-radius: 3px;
  pointer-events: none; user-select: none;
}
#doc blockquote {
  margin: 0; padding-left: 1em; border-left: 3px solid #dfe2e5; color: #57606a;
}
#doc .kind-list { padding-left: 1.5em; }
#doc .li { position: relative; }
#doc .li::before { content: "•"; position: absolute; left: -1.05em; color: #8b949e; }
/* 有序项：序号右对齐、向左生长。`data-m` 是源码里那一项自己的标记
   （`1.` / `2)`），多位数不会把正文顶开，也不会被 counter 改号。 */
#doc .li[data-m]::before { content: attr(data-m); left: auto; right: calc(100% + .35em); }
#doc .li:has(.atom.task)::before { content: none; }
#doc .li:has(.atom.task) { padding-left: .1em; }
/* 嵌套列表层级：与桌面端 app.css 保持一致 */
#doc .li[data-depth="1"] { margin-left: 1.5em; }
#doc .li[data-depth="2"] { margin-left: 3em; }
#doc .li[data-depth="3"] { margin-left: 4.5em; }
#doc .li[data-depth="4"] { margin-left: 6em; }
#doc .li[data-depth="1"]::before { content: "○"; }
#doc .li[data-depth="2"]::before { content: "▪"; }
#doc .li[data-depth="3"]::before { content: "▸"; }
#doc .li[data-depth="4"]::before { content: "·"; }
#doc .li[data-depth][data-m]::before { content: attr(data-m); left: auto; right: calc(100% + .35em); }
#doc .sep { display: inline-block; width: .55em; }
/* 表格边框与 4px 圆角由最外层 table 承担；内部线只画右/下两边，
   separate + spacing 0 + overflow 才能让圆角裁住内格，又不会叠成 2px。 */
#doc .md-table {
  border-collapse: separate; border-spacing: 0;
  border: 1px solid #d0d7de; border-radius: 4px; overflow: hidden;
  margin: .2em 0;
}
#doc .md-table th, #doc .md-table td {
  border-right: 1px solid #d0d7de; border-bottom: 1px solid #d0d7de;
  padding: .35em .7em; vertical-align: top;
  min-width: 3em; white-space: pre-wrap;
}
#doc .md-table th:last-child, #doc .md-table td:last-child { border-right: 0; }
#doc .md-table tr:last-child th, #doc .md-table tr:last-child td { border-bottom: 0; }
#doc .md-table th { background: #f6f8fa; font-weight: 650; }
#doc .md-table .al { text-align: left; }
#doc .md-table .ac { text-align: center; }
#doc .md-table .ar { text-align: right; }
#doc .md-table .sep { display: none; }
/* 与桌面端一致：横线铺满整行，透明字符仍占一列供光标停靠。 */
#doc .atom.rule { display: block; position: relative; color: transparent; }
#doc .atom.rule::after {
  content: ""; position: absolute; left: 0; right: 0; top: 50%;
  border-top: 1px solid #d0d7de; pointer-events: none;
}
#doc .ch { }
#doc .b { font-weight: 650; }
#doc .i { font-style: italic; }
#doc .d { text-decoration: line-through; color: #6e7781; }
#doc .c {
  font-family: "Cascadia Code", Consolas, monospace; font-size: .9em;
  background: #eff1f3; border-radius: 3px;
}
/* 数学公式：定界符折叠，源码用衬线斜体 + 淡底，和行内代码区分 */
#doc .m {
  font-family: "Cambria Math", "Times New Roman", serif; font-style: italic;
  color: #8250df; background: #f6f8fa; border-radius: 3px;
}
#doc .md { display: block; text-align: center; padding: .25em 0; margin: .3em 0; }
/* 代码块语法着色（与桌面端 app.css 同一套 class） */
#doc .tk-comment { color: #6e7781; font-style: italic; }
#doc .tk-keyword { color: #cf222e; }
#doc .tk-type { color: #953800; }
#doc .tk-literal { color: #0550ae; }
#doc .tk-string { color: #0a3069; }
#doc .tk-number { color: #0550ae; }
#doc .tk-function { color: #8250df; }
#doc .tk-variable { color: #953800; }
#doc .tk-tag { color: #116329; }
#doc .tk-attribute { color: #0550ae; }
#doc .tk-macro { color: #8250df; font-weight: 600; }
#doc .tk-annotation { color: #0550ae; }
#doc .tk-operator { color: #57606a; }
#doc .tk-inserted { color: #116329; background: #e6ffec; }
#doc .tk-deleted { color: #cf222e; background: #ffebe9; }
#doc .a { color: #0969da; text-decoration: underline; text-underline-offset: 2px; }
/* 脚注：引用仍显示源码 `[^x]`，但按上标样式画；定义块用序号前缀与回链 */
#doc .fnref { color: #8250df; font-size: .82em; vertical-align: super; line-height: 0; cursor: pointer; }
#doc .kind-footnote { border-top: 1px solid #d8dee4; margin-top: 1.2em; padding-top: .5em; }
#doc .kind-footnote .p.fn-def { padding-left: 1.9em; color: #57606a; font-size: .92em; }
#doc .kind-footnote::before {
  content: attr(data-fn) "."; position: absolute; left: 0; top: .5em;
  color: #57606a; font-size: .92em;
}
#doc .fn-back a { color: #0969da; text-decoration: none; margin-left: .35em; }
#doc .mk { color: transparent; }
#doc .eob, #doc .mk { user-select: none; }
#doc .atom {
  display: inline-block; vertical-align: baseline;
}
#doc .atom.img {
  min-width: 1.6em; padding: 0 .35em; border: 1px dashed #b6bcc4; border-radius: 4px;
  color: #57606a; font-size: .82em; text-align: center; background: #fafbfc;
}
#doc .atom.img::before { content: "图"; }
#doc .atom.img img { display: none; max-width: 100%; max-height: 360px; width: auto; height: auto; border-radius: 3px; }
#doc .atom.img.loaded { padding: 0; border-color: transparent; background: transparent; }
#doc .atom.img.loaded::before { content: none; }
#doc .atom.img.loaded img { display: block; }
#doc .atom.task { color: #57606a; }
#doc .sel { background: rgba(37, 99, 235, .18); border-radius: 2px; }
#caret {
  position: absolute; width: 2px; background: #0969da; border-radius: 1px;
  pointer-events: none; display: none;
}
#caret.blink { animation: caret-blink 1.05s steps(1) infinite; }
@keyframes caret-blink { 50% { opacity: 0; } }
#bar {
  position: fixed; left: 0; right: 0; bottom: 0; z-index: 10;
  display: flex; flex-wrap: wrap; gap: 18px; align-items: center;
  padding: 8px 18px; font: 12px/1 ui-monospace, Consolas, monospace;
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

  // 投影页与 Markdown 同目录，图片 URL 可以直接交给浏览器。
  document.querySelectorAll('.atom.img[data-asset]').forEach(function (holder) {
    var img = holder.querySelector('img');
    if (!img) return;
    img.addEventListener('load', function () { holder.classList.add('loaded'); });
    img.addEventListener('error', function () { holder.classList.add('failed'); });
    if (img.src) img.src = img.src;
  });

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
    fn code_block_chars_carry_token_classes() {
        let html = preview_html("```rust\nlet a = 1;\n```");
        assert!(html.contains("tk-keyword"), "缺关键字着色: {html}");
        assert!(html.contains("tk-number"), "缺数字着色: {html}");
        // 着色只加 class，不改变停靠点：`let a = 1;` 仍是 10 个逐字符元素
        assert_eq!(html.matches("class=\"ch ").count(), 10);
    }

    #[test]
    fn code_block_chars_carry_macro_and_annotation_classes() {
        let html = preview_html("```rust\n#[derive(Debug)]\nprintln!(\"hi\");\n```");
        assert!(html.contains("tk-macro"), "缺宏着色: {html}");
        assert!(html.contains("tk-annotation"), "缺注解着色: {html}");
        // 宏与注解的 class 只加在字符元素上，停靠点数不变
        assert_eq!(
            html.matches("class=\"ch ").count(),
            "#[derive(Debug)]".len() + "println!(\"hi\");".len()
        );
    }

    #[test]
    fn code_block_tail_newline_does_not_add_a_blank_line() {
        // 末尾换行只收尾最后一行（闭合围栏与 EOB 都在同一行），
        // 不能再断出一行；源码里真正的空行仍要保留。
        let one = preview_html("```\n甲\n```");
        assert_eq!(one.matches("<br class=\"nl\"").count(), 0, "{one}");

        let one_blank = preview_html("```\n甲\n\n```");
        assert_eq!(
            one_blank.matches("<br class=\"nl\"").count(),
            1,
            "一个空行应只产生一个 <br>: {one_blank}"
        );

        let two_blanks = preview_html("```\n甲\n\n\n```");
        assert_eq!(
            two_blanks.matches("<br class=\"nl\"").count(),
            2,
            "两个空行应产生两个 <br>: {two_blanks}"
        );
    }

    #[test]
    fn code_block_shows_its_language() {
        let rust = preview_html("```rust\nlet a = 1;\n```");
        assert!(rust.contains("kind-codeblock"), "{rust}");
        assert!(rust.contains("data-lang=\"rust\""), "{rust}");

        // info 字符串常写成 `{.python}` 或带额外选项，标签只取语言词本身
        let python = preview_html("```{.python}\nprint(1)\n```");
        assert!(python.contains("data-lang=\"python\""), "{python}");

        // 裸围栏与缩进代码块没有语言，不该凭空造一个标签
        let plain = preview_html("```\n纯文本\n```");
        assert!(!plain.contains("data-lang="), "{plain}");
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
    fn ordered_items_carry_their_own_number() {
        // `1. ` 那三个字节是折叠的，视图里必须有个东西代替它，
        // 否则有序列表与无序列表长得一模一样（全是圆点）
        let html = preview_html("1. 甲\n2. 乙\n");
        assert!(html.contains("data-m=\"1.\""), "{html}");
        assert!(html.contains("data-m=\"2.\""), "{html}");
    }

    #[test]
    fn unordered_items_keep_the_bullet() {
        let html = preview_html("- 甲\n- 乙\n");
        assert!(!html.contains("data-m="), "无序项应交给 CSS 的项目符号");
    }

    #[test]
    fn ordered_marker_keeps_delimiter_and_start_number() {
        // 序号按源码原样显示：用户从 3 开始、或者用 `1)` 都照做，不擅自改成 1.
        let html = preview_html("3) 甲\n4) 乙\n");
        assert!(html.contains("data-m=\"3)\""), "{html}");
        assert!(html.contains("data-m=\"4)\""), "{html}");
    }

    #[test]
    fn list_split_by_blank_line_still_renders_as_list() {
        // 松散列表被空行切成两块，第二块也得是 `.list` 而不是普通段落
        let frames = frames_of("1. 甲\n2. 乙\n\n3. 丙\n4. 丁\n");
        assert_eq!(frames.len(), 2);
        for frame in &frames {
            assert!(frame.html.contains("class=\"p list\""), "{}", frame.html);
            assert!(frame.html.contains("data-m="), "{}", frame.html);
        }
    }

    #[test]
    fn image_is_an_atomic_asset_node() {
        let frames = frames_of("![图](a.png)");
        let html = &frames[0].html;
        assert!(html.contains("class=\"atom img\""), "{html}");
        assert!(html.contains("data-asset=\"a.png\""), "{html}");
        assert!(html.contains("title=\"图\""), "{html}");
        assert!(
            html.contains("<img alt=\"\" src=\"data:image/gif;base64,"),
            "{html}"
        );
        assert!(html.contains(&format!("data-b=\"{}\"", frames[0].base)));
        assert!(html.contains(&format!("data-a=\"{}\"", frames[0].base + 1)));
        // 投影页与文档同目录，可以直接用相对路径装载
        assert!(preview_html("![图](a.png)").contains("src=\"a.png\""));
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
        let patch = diff_frames(
            &frames_of("甲\n\n乙\n\n丙\n"),
            &frames_of("甲\n\n丁\n\n丙\n"),
        );
        assert_eq!(patch.replace, 1..2);
        assert!(patch.html.contains("丁"));
        assert_eq!(patch.old_len, 3);
    }

    #[test]
    fn diff_covers_tail_when_stop_count_changes() {
        // 停靠点总数一变，后面每块的 data-b 都要重编号，只能连尾部一起替换
        let patch = diff_frames(
            &frames_of("甲\n\n乙\n\n丙\n"),
            &frames_of("甲\n\n丁戊\n\n丙\n"),
        );
        assert_eq!(patch.replace, 1..3);
    }

    #[test]
    fn link_chars_carry_their_target() {
        // URL 在投影里是折叠的，字符必须自带目标地址——前端没有它就无从知道
        // 该打开哪里。链接里的**每个字符各挂一份**：点击命中的是单个字符元素，
        // 它和同链接的其它字符是平级兄弟，`closest` 只向上找，读不到邻居。
        let html: String = frames_of("[示例](https://example.com/a?b=1)\n")
            .iter()
            .map(|f| f.html.as_str())
            .collect();
        assert_eq!(
            html.matches("data-href=\"https://example.com/a?b=1\"")
                .count(),
            2
        );
        // 普通正文不能凭空长出 href
        let plain: String = frames_of("普通文字\n")
            .iter()
            .map(|f| f.html.as_str())
            .collect();
        assert!(!plain.contains("data-href"), "{plain}");
    }

    #[test]
    fn link_target_is_attribute_escaped() {
        // 引号若原样落进属性，文档内容就能提前闭合并改写投影出来的 DOM 结构
        let html: String = frames_of("[x](<https://example.com/?q=\">)\n")
            .iter()
            .map(|f| f.html.as_str())
            .collect();
        assert!(
            html.contains("data-href=\"https://example.com/?q=&quot;\""),
            "{html}"
        );
        assert!(!html.contains("?q=\">"), "{html}");
    }

    #[test]
    fn trailing_empty_paragraph_gets_its_own_frame() {
        let frames = frames_of("abc\n\n");
        assert_eq!(frames.len(), 2);
        assert!(frames[1].html.contains("class=\"eob\""));
        assert_eq!(frames[1].base, frames[1].end, "空段落只有一个停靠点");
    }
}

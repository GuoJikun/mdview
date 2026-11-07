use std::ops::Range;

/// 块在文档内的序号。编辑后按位置重排，不做跨版本持久化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub usize);

/// 块类型。语义以 pulldown-cmark 的首个 Start 事件为准，不做字符串猜判。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    List { ordered: bool },
    BlockQuote,
    CodeBlock { info: Option<String> },
    Table,
    Rule,
    Html,
}

impl BlockKind {
    pub fn label(&self) -> &'static str {
        match self {
            BlockKind::Heading(_) => "heading",
            BlockKind::Paragraph => "paragraph",
            BlockKind::List { .. } => "list",
            BlockKind::BlockQuote => "blockquote",
            BlockKind::CodeBlock { .. } => "codeblock",
            BlockKind::Table => "table",
            BlockKind::Rule => "rule",
            BlockKind::Html => "html",
        }
    }
}

/// 内联样式的累计值。由 Start/End 事件栈折叠得出，渲染层据此套用 class。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub strong: bool,
    pub emphasis: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub heading: Option<u8>,
    /// 链接目标。图片也复用此字段存 url。
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanKind {
    Text,
    Code,
    Link,
    /// 原子控件：整个 `![alt](url)` 折叠为一个节点，alt 存于 text。
    Image,
    /// 段落内换行。必须保留，隐藏它会让排版塌成一行。
    Break { hard: bool },
    /// 分隔线，无文本，渲染为控件。
    Rule,
    /// `[ ]` / `[x]`，无文本，渲染为复选框。
    TaskMarker { checked: bool },
    FootnoteReference,
}

/// 一个可见片段。
///
/// `byte` 恒为**原始源码**的字节区间；`text` 是投影出的可视文本。
/// 两者长度不一定相等：转义（`a\*b`）与 HTML 实体会让 `byte` 更长，
/// 映射时用 [`align`] 而不是简单偏移。
#[derive(Debug, Clone)]
pub struct Span {
    pub kind: SpanKind,
    pub byte: Range<usize>,
    pub text: String,
    pub style: Style,
}

impl Span {
    pub fn is_control(&self) -> bool {
        matches!(
            self.kind,
            SpanKind::Image | SpanKind::Rule | SpanKind::TaskMarker { .. }
        )
    }
}

/// 一个块：完整源码区间 + 可见片段序列 + 折叠掉的标记区间。
#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    pub kind: BlockKind,
    /// 去掉尾部换行后的源码区间
    pub byte: Range<usize>,
    pub spans: Vec<Span>,
    /// 差集结果，按 byte 升序且互不相交。渲染时折叠为零宽单元。
    pub hidden: Vec<Range<usize>>,
}

impl Block {
    /// 纯文本投影，供大纲、搜索与状态栏使用。
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for span in &self.spans {
            match span.kind {
                SpanKind::Break { .. } => out.push(' '),
                SpanKind::Rule | SpanKind::TaskMarker { .. } => {}
                _ => out.push_str(&span.text),
            }
        }
        out.trim().to_owned()
    }

    /// byte 落在哪个可见片段内。
    pub fn span_at(&self, byte: usize) -> Option<usize> {
        self.spans
            .iter()
            .position(|s| s.byte.start <= byte && byte < s.byte.end)
    }

    /// byte 是否落在被折叠的标记里。
    pub fn hidden_at(&self, byte: usize) -> Option<usize> {
        self.hidden
            .iter()
            .position(|r| r.start <= byte && byte < r.end)
    }

    /// 把任意 byte 归一到最近的可见位置：落在隐藏区间时吸附到区间右侧。
    pub fn snap_visible(&self, byte: usize) -> usize {
        match self.hidden_at(byte) {
            Some(i) => self.hidden[i].end,
            None => byte,
        }
    }

    /// 渲染为 HTML 片段。阅读模式与调试输出共用，不参与编辑态排版。
    pub fn to_html(&self) -> String {
        let mut out = String::new();
        let tag = match &self.kind {
            BlockKind::Heading(level) => format!("h{level}"),
            BlockKind::Paragraph => "p".to_owned(),
            BlockKind::List { ordered } => {
                if *ordered {
                    "ol"
                } else {
                    "ul"
                }
                .to_owned()
            }
            BlockKind::BlockQuote => "blockquote".to_owned(),
            BlockKind::CodeBlock { .. } => "pre".to_owned(),
            BlockKind::Table => "table".to_owned(),
            BlockKind::Rule => "div".to_owned(),
            BlockKind::Html => "p".to_owned(),
        };
        out.push_str(&format!(
            "<section class=\"md-block md-{}\" data-block=\"{}\" data-byte-start=\"{}\" data-byte-end=\"{}\"><{tag}>",
            self.kind.label(),
            self.id.0,
            self.byte.start,
            self.byte.end
        ));
        for span in &self.spans {
            self.push_span(&mut out, span);
        }
        out.push_str(&format!("</{tag}></section>"));
        out
    }

    fn push_span(&self, out: &mut String, span: &Span) {
        match &span.kind {
            SpanKind::Break { hard } => {
                out.push_str(if *hard { "<br>" } else { " " });
            }
            SpanKind::Rule => out.push_str("<hr>"),
            SpanKind::TaskMarker { checked } => out.push_str(&format!(
                "<input type=\"checkbox\" disabled{}>",
                if *checked { " checked" } else { "" }
            )),
            SpanKind::Image => {
                let url = span.style.link.as_deref().unwrap_or("");
                out.push_str(&format!(
                    "<img src=\"{}\" alt=\"{}\">",
                    escape_attr(url),
                    escape_attr(&span.text)
                ));
            }
            SpanKind::Code => out.push_str(&format!("<code>{}</code>", escape(&span.text))),
            SpanKind::Link => out.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                escape_attr(span.style.link.as_deref().unwrap_or("")),
                escape(&span.text)
            )),
            SpanKind::FootnoteReference => {
                out.push_str(&format!("<sup>{}</sup>", escape(&span.text)))
            }
            SpanKind::Text => {
                if span.style.strong {
                    out.push_str("<strong>");
                }
                if span.style.emphasis {
                    out.push_str("<em>");
                }
                if span.style.strikethrough {
                    out.push_str("<del>");
                }
                out.push_str(&escape(&span.text));
                if span.style.strikethrough {
                    out.push_str("</del>");
                }
                if span.style.emphasis {
                    out.push_str("</em>");
                }
                if span.style.strong {
                    out.push_str("</strong>");
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutlineItem {
    pub level: u8,
    pub title: String,
    pub byte_offset: usize,
    pub block_id: BlockId,
}

#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub blocks: Vec<Block>,
    pub outline: Vec<OutlineItem>,
}

impl ParseResult {
    pub fn source_map(&self) -> crate::sourcemap::SourceMap {
        crate::sourcemap::SourceMap::build(&self.blocks)
    }

    pub fn to_html(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            out.push_str(&block.to_html());
            out.push('\n');
        }
        out
    }

    pub fn count<F: Fn(&BlockKind) -> bool>(&self, pred: F) -> usize {
        self.blocks.iter().filter(|b| pred(&b.kind)).count()
    }

    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.blocks.get(id.0)
    }
}

pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn escape_attr(s: &str) -> String {
    escape(s).replace('"', "&quot;")
}

/// 源码区间与投影文本的对齐：返回文本第 `target` 个字符在源码中的字节偏移。
///
/// 转义（`a\*b` 源码 4 字节、文本 3 字符）和 HTML 实体（`&amp;` 源码 5 字节、
/// 文本 1 字符）都会让两者不等长，不能用 `start + char_offset` 直接算。
pub fn align(src: &str, text: &str, target: usize) -> usize {
    let mut si = 0usize;
    let mut ti = 0usize;
    let mut chars = text.chars().peekable();
    while ti < target {
        let Some(sc) = src[si..].chars().next() else {
            return src.len();
        };
        // 转义前缀只存在于源码侧，文本侧不消耗字符
        if sc == '\\' && si + 1 < src.len() {
            si += 1;
            continue;
        }
        // 实体在源码侧多字节、文本侧一个字符
        if sc == '&' {
            if let Some(len) = entity_len(&src[si..]) {
                si += len;
                chars.next();
                ti += 1;
                continue;
            }
        }
        si += sc.len_utf8();
        chars.next();
        ti += 1;
    }
    si
}

fn entity_len(s: &str) -> Option<usize> {
    if !s.starts_with('&') {
        return None;
    }
    let end = s.find(';')?;
    if !(2..=10).contains(&end) {
        return None;
    }
    let body = &s[1..end];
    let valid = body
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '#')
        && body
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '#');
    valid.then_some(end + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_skips_escape_prefix() {
        // 源码 "a\*b"（4 字节），文本 "a*b"（3 字符）
        assert_eq!(align(r"a\*b", "a*b", 0), 0);
        assert_eq!(align(r"a\*b", "a*b", 2), 3);
        assert_eq!(align(r"a\*b", "a*b", 3), 4);
    }

    #[test]
    fn align_handles_chinese() {
        let src = "中文abc";
        let text = "中文abc";
        assert_eq!(align(src, text, 2), 6);
        assert_eq!(align(src, text, 5), 9);
    }

    #[test]
    fn align_handles_entity() {
        // 源码 "&amp;x"（6 字节），文本 "&x"（2 字符）
        assert_eq!(align("&amp;x", "&x", 1), 5);
        assert_eq!(align("&amp;x", "&x", 2), 6);
    }
}

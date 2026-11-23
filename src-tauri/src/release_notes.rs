//! GitHub Release 正文（Markdown）→ 更新弹窗里的只读 HTML 片段。
//!
//! 为什么不用 `pulldown_cmark::html::push_html`：那个实现会把链接渲染成
//! `<a href>`，也会把正文里的原始 HTML 原样透传。前者会让 WebView 自己
//! 在弹窗里导航走（同 [`crate::link`] 的理由：中键、拖拽、右键菜单都拦不住，
//! 一导航整个应用就没了）；后者等于把远端 Release 正文当成可执行内容注入页面。
//! 所以这里走事件流自己拼 HTML：
//!
//! - 所有文本转义（`& < > " '`），属性同样转义；
//! - 链接只输出 `data-href`，且必须过 [`is_allowed`] 白名单，由前端点击后
//!   交给 [`crate::link::open_url`]；
//! - 图片只认 `http(s)`，其余整段丢弃；
//! - 正文里的原始 HTML（含 `<details>` 这类折叠块）整段丢弃，只保留 Markdown
//!   结构。CHANGELOG 是发布说明的唯一来源，里面不该有 HTML。
//!
//! 输出只包含本文件自己写的标签，前端可以直接 `v-html`。

use std::fmt::Write as _;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::link::is_allowed;

/// 把 Release 正文渲染成可直接 `v-html` 的 HTML 片段。
pub fn render(markdown: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut out = String::with_capacity(markdown.len() * 3 / 2 + 64);

    // 链接栈：记录每个 `<a>` 是否真的输出过（href 没过白名单时只留文字）。
    let mut links: Vec<bool> = Vec::new();
    // 图片里的文本是 alt，要收集完再写 `<img>`，不能边读边输出。
    let mut image: Option<Image> = None;
    // 表格对齐与当前列号，只在 `<table>` 生命周期内有效。
    let mut alignment: Vec<Alignment> = Vec::new();
    let mut column = 0usize;
    let mut in_head = false;

    for event in Parser::new_ext(markdown, options) {
        if image.is_some() {
            match event {
                Event::Text(text) | Event::Code(text) => {
                    if let Some(image) = image.as_mut() {
                        image.alt.push_str(&text);
                    }
                }
                Event::End(TagEnd::Image) => {
                    if let Some(image) = image.take() {
                        push_image(&mut out, &image);
                    }
                }
                // 图片内部不允许嵌套块级标签，其余事件一律丢弃。
                _ => {}
            }
            continue;
        }

        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => out.push_str("<p>"),
                Tag::Heading { level, .. } => {
                    let _ = write!(out, "<{}>", heading_tag(level));
                }
                Tag::BlockQuote(_) => out.push_str("<blockquote>"),
                Tag::CodeBlock(kind) => push_code_open(&mut out, kind),
                Tag::List(start) => match start {
                    Some(first) if first != 1 => {
                        let _ = write!(out, "<ol start=\"{first}\">");
                    }
                    Some(_) => out.push_str("<ol>"),
                    None => out.push_str("<ul>"),
                },
                Tag::Item => out.push_str("<li>"),
                Tag::Table(aligns) => {
                    alignment = aligns;
                    column = 0;
                    out.push_str("<table class=\"notes-table\">");
                }
                Tag::TableHead => {
                    in_head = true;
                    // 表头没有 `TableRow` 事件，`<tr>` 只能自己补。
                    out.push_str("<thead><tr>");
                }
                Tag::TableRow => {
                    column = 0;
                    out.push_str("<tr>");
                }
                Tag::TableCell => {
                    let tag = if in_head { "th" } else { "td" };
                    match alignment.get(column).copied().unwrap_or(Alignment::None) {
                        Alignment::Left => {
                            let _ = write!(out, "<{tag} class=\"al\">");
                        }
                        Alignment::Center => {
                            let _ = write!(out, "<{tag} class=\"ac\">");
                        }
                        Alignment::Right => {
                            let _ = write!(out, "<{tag} class=\"ar\">");
                        }
                        Alignment::None => {
                            let _ = write!(out, "<{tag}>");
                        }
                    }
                }
                Tag::Emphasis => out.push_str("<em>"),
                Tag::Strong => out.push_str("<strong>"),
                Tag::Strikethrough => out.push_str("<del>"),
                Tag::Link { dest_url, .. } => {
                    let allowed = is_allowed(&dest_url);
                    if allowed {
                        let _ = write!(out, "<a data-href=\"{}\">", escape(&dest_url));
                    }
                    links.push(allowed);
                }
                Tag::Image {
                    dest_url, title, ..
                } => {
                    image = Some(Image {
                        url: dest_url.to_string(),
                        title: title.to_string(),
                        alt: String::new(),
                    });
                }
                // 脚注 / 定义列表 / 元数据块都没启用；HtmlBlock 整段丢弃。
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => out.push_str("</p>"),
                TagEnd::Heading(level) => {
                    let _ = write!(out, "</{}>", heading_tag(level));
                }
                TagEnd::BlockQuote(_) => out.push_str("</blockquote>"),
                TagEnd::CodeBlock => out.push_str("</code></pre>"),
                TagEnd::List(ordered) => out.push_str(if ordered { "</ol>" } else { "</ul>" }),
                TagEnd::Item => out.push_str("</li>"),
                TagEnd::Table => out.push_str("</tbody></table>"),
                TagEnd::TableHead => {
                    in_head = false;
                    column = 0;
                    out.push_str("</tr></thead><tbody>");
                }
                TagEnd::TableRow => out.push_str("</tr>"),
                TagEnd::TableCell => {
                    let tag = if in_head { "th" } else { "td" };
                    let _ = write!(out, "</{tag}>");
                    column += 1;
                }
                TagEnd::Emphasis => out.push_str("</em>"),
                TagEnd::Strong => out.push_str("</strong>"),
                TagEnd::Strikethrough => out.push_str("</del>"),
                // guard 里 `pop` 了：链接没输出过 `<a>` 时落到 `_` 分支，栈也已弹掉。
                TagEnd::Link if links.pop() == Some(true) => out.push_str("</a>"),
                // 图片的关闭在循环开头的 `image` 分支里处理。
                _ => {}
            },
            Event::Text(text) => out.push_str(&escape(&text)),
            Event::Code(code) => {
                let _ = write!(out, "<code>{}</code>", escape(&code));
            }
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push_str("<br>"),
            Event::Rule => out.push_str("<hr>"),
            Event::TaskListMarker(checked) => {
                let mark = if checked { "\u{2611}" } else { "\u{2610}" };
                let _ = write!(out, "<span class=\"notes-task\">{mark}</span>");
            }
            // 原始 HTML 一律丢弃：远端正文不该往页面里塞标签。
            Event::Html(_) | Event::InlineHtml(_) => {}
            _ => {}
        }
    }

    out
}

/// 待写出的图片：`alt` 来自标签内部的事件，需要先收集。
struct Image {
    url: String,
    title: String,
    alt: String,
}

fn push_image(out: &mut String, image: &Image) {
    if !is_web_url(&image.url) {
        return;
    }
    let url = image.url.trim();
    let _ = write!(
        out,
        "<img src=\"{}\" alt=\"{}\"",
        escape(url),
        escape(&image.alt)
    );
    if !image.title.trim().is_empty() {
        let _ = write!(out, " title=\"{}\"", escape(image.title.trim()));
    }
    out.push('>');
}

fn push_code_open(out: &mut String, kind: CodeBlockKind<'_>) {
    match kind {
        CodeBlockKind::Fenced(info) => {
            let lang = info.split_whitespace().next().unwrap_or("");
            if lang.is_empty() {
                out.push_str("<pre><code>");
            } else {
                let _ = write!(out, "<pre><code class=\"language-{}\">", escape(lang));
            }
        }
        CodeBlockKind::Indented => out.push_str("<pre><code>"),
    }
}

fn heading_tag(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "h1",
        HeadingLevel::H2 => "h2",
        HeadingLevel::H3 => "h3",
        HeadingLevel::H4 => "h4",
        HeadingLevel::H5 => "h5",
        HeadingLevel::H6 => "h6",
    }
}

/// 图片地址白名单：只放行 http(s)，`mailto` 之类不是图片。
fn is_web_url(url: &str) -> bool {
    let url = url.trim();
    if url.is_empty() || url.len() > 4096 || url.chars().any(char::is_control) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_lists_and_inline_code() {
        let html = render("## 修复\n\n- 修复：`a < b` 的判断\n- 第二项\n");
        assert!(html.contains("<h2>修复</h2>"), "{html}");
        assert!(html.contains("<ul>"), "{html}");
        assert!(
            html.contains("<li>修复：<code>a &lt; b</code> 的判断</li>"),
            "{html}"
        );
        assert!(html.contains("<li>第二项</li>"), "{html}");
    }

    #[test]
    fn text_is_escaped() {
        let html = render("a & b，1 < 2，\"引号\"与'撇号'");
        assert!(html.contains("a &amp; b"), "{html}");
        assert!(html.contains("1 &lt; 2"), "{html}");
        assert!(html.contains("&quot;引号&quot;"), "{html}");
        assert!(html.contains("&#39;撇号&#39;"), "{html}");
    }

    #[test]
    fn link_becomes_data_href_and_unsafe_scheme_drops_it() {
        let html = render("[发布页](https://example.com/x?a=1&b=2) [坏链接](javascript:alert(1))");
        assert!(
            html.contains("data-href=\"https://example.com/x?a=1&amp;b=2\""),
            "{html}"
        );
        assert!(!html.contains("javascript"), "{html}");
        assert!(html.contains("坏链接"), "{html}");
    }

    #[test]
    fn image_keeps_web_url_and_alt() {
        let html = render("![截图](https://example.com/a.png \"标题\")");
        assert_eq!(
            html,
            "<p><img src=\"https://example.com/a.png\" alt=\"截图\" title=\"标题\"></p>",
        );
    }

    #[test]
    fn non_web_image_is_dropped() {
        let html = render("![x](file:///etc/passwd)");
        assert_eq!(html, "<p></p>");
    }

    #[test]
    fn code_fence_language_is_stamped() {
        let html = render("```rust\nfn main() {}\n```\n");
        assert!(
            html.contains("<pre><code class=\"language-rust\">"),
            "{html}"
        );
        assert!(html.contains("fn main() {}"), "{html}");
    }

    #[test]
    fn raw_html_block_is_dropped() {
        let html = render("<details>\n<summary>展开</summary>\n\n正文\n\n</details>\n");
        assert!(!html.contains("<details>"), "{html}");
        assert!(html.contains("<p>正文</p>"), "{html}");
    }

    #[test]
    fn nested_list_and_blockquote() {
        let html = render("> 引用\n\n1. 一\n2. 二\n\n   - 子项\n");
        assert!(html.contains("<blockquote>"), "{html}");
        assert!(html.contains("<ol>"), "{html}");
        assert!(html.contains("<ul>"), "{html}");
    }

    #[test]
    fn empty_input_stays_empty() {
        assert!(render("").is_empty());
    }

    #[test]
    fn table_gets_head_body_and_alignment() {
        let html = render("| 甲 | 乙 |\n| :- | -: |\n| 1 | 2 |\n");
        assert_eq!(
            html,
            "<table class=\"notes-table\"><thead><tr><th class=\"al\">甲</th>\
             <th class=\"ar\">乙</th></tr></thead><tbody><tr><td class=\"al\">1</td>\
             <td class=\"ar\">2</td></tr></tbody></table>",
        );
    }
}

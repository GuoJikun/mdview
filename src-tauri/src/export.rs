//! 导出单文件 HTML。
//!
//! 复用阅读态渲染（`ParseResult::to_html`），把相对路径的图片内联成 data URL，
//! 再裹一份**独立**的样式表——导出件要在没装 mdview 的机器上双击就能看，
//! 不能依赖应用的 CSS 变量与 JavaScript。
//!
//! 刻意不做 PDF：Tauri 2 没有跨平台打印 API，硬凑出来的 PDF 不是排版错乱
//! 就是缺字体。现实路径是「导出 HTML → 浏览器打印成 PDF」，菜单里也这么写。

/// 导出页的样式。浅色定死，不跟系统暗色：这份文件多半会被打印或发给别人，
/// 跟随系统的暗色只会让对方的 PDF 变成黑底白字。
const STYLE: &str = r#"
* { box-sizing: border-box; }
body {
  margin: 0; padding: 48px 24px 96px;
  font: 15px/1.85 -apple-system, "Segoe UI", "Microsoft YaHei", "PingFang SC", sans-serif;
  color: #1f2328; background: #fff; -webkit-font-smoothing: antialiased;
}
.md-doc { max-width: 800px; margin: 0 auto; }
.md-block { position: relative; margin: 0 0 1.1em; }
p { margin: 0; }
h1, h2, h3, h4, h5, h6 { margin: 1.5em 0 .6em; line-height: 1.35; font-weight: 650; }
h1 { font-size: 1.9em; } h2 { font-size: 1.55em; } h3 { font-size: 1.3em; }
h4 { font-size: 1.12em; } h5, h6 { font-size: 1em; }
a { color: #0969da; text-decoration: none; }
a:hover { text-decoration: underline; }
img { max-width: 100%; height: auto; border-radius: 4px; }
hr { border: 0; border-top: 1px solid #d0d7de; margin: 1.6em 0; }
blockquote {
  margin: 0 0 1.1em; padding: .35em 1em;
  border-left: 3px solid #dfe2e5; color: #57606a;
}
ul, ol { margin: 0 0 1.1em; padding-left: 1.7em; }
li { margin: .2em 0; }
code {
  padding: .12em .32em; border-radius: 4px; background: #eff1f3;
  font-family: "Cascadia Code", Consolas, "Courier New", monospace; font-size: .9em;
}
.md-code { margin: 0 0 1.1em; }
.md-code code, .md-code pre, pre.md-code {
  display: block; padding: .9em 1em; overflow-x: auto;
  background: #f6f8fa; border: 1px solid #eaecef; border-radius: 6px;
  font-size: .92em; line-height: 1.6; white-space: pre;
}
table { border-collapse: collapse; margin: 0 0 1.1em; border: 1px solid #d0d7de; }
th, td { border-color: #d0d7de; }
th { background: #f6f8fa; font-weight: 650; }
input[type="checkbox"] { margin-right: .35em; }
.fn { color: #57606a; font-size: .92em; }
.fn-num { font-weight: 600; }
.fn-back a { margin-left: .2em; }
/* 代码高亮：GitHub 浅色配色，与应用内一致 */
.tk-comment { color: #6e7781; } .tk-keyword { color: #cf222e; }
.tk-type { color: #953800; } .tk-literal { color: #0550ae; }
.tk-string { color: #0a3069; } .tk-number { color: #0550ae; }
.tk-function { color: #8250df; } .tk-variable { color: #953800; }
.tk-tag { color: #116329; } .tk-attribute { color: #0550ae; }
.tk-macro { color: #8250df; } .tk-annotation { color: #0550ae; }
.tk-operator { color: #57606a; }
.tk-inserted { color: #116329; background: #e6ffec; }
.tk-deleted { color: #cf222e; background: #ffebe9; }
@media print {
  body { padding: 0; font-size: 12pt; }
  .md-doc { max-width: none; }
  a { color: inherit; }
}
"#;

/// 把渲染结果裹成一页可以单独打开的 HTML。
pub(crate) fn page(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>{}</style>\n</head>\n<body>\n\
         <main class=\"md-doc\">\n{}\n</main>\n</body>\n</html>\n",
        escape_title(title),
        STYLE,
        body
    )
}

/// 标题里只可能是文件名或文档的第一个标题，转义一下 `&` `<` `>` 就够。
fn escape_title(title: &str) -> String {
    title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 把 `<img src="…">` 里的本地相对路径换成 data URL。
///
/// 网络地址与已经是 data URL 的保持原样——内联它们既没意义，还会让文件暴涨。
/// 读不到的文件（被删、越界、超过大小上限）也保留原地址：宁可留一条断链，
/// 也不要把图片悄悄丢掉，用户至少还能看出这里原本有图。
pub(crate) fn inline_images(html: &str, resolve: impl Fn(&str) -> Option<String>) -> String {
    const OPEN: &str = "<img src=\"";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find(OPEN) {
        let (before, tail) = rest.split_at(pos + OPEN.len());
        out.push_str(before);
        let Some(end) = tail.find('"') else {
            out.push_str(tail);
            return out;
        };
        let raw = &tail[..end];
        // 属性里的 `&` 被 escape_attr 写成了 `&amp;`，拿去读文件前要还原
        let url = raw.replace("&amp;", "&");
        if is_remote(&url) {
            out.push_str(raw);
        } else {
            match resolve(&url) {
                Some(data) => out.push_str(&data),
                None => out.push_str(raw),
            }
        }
        out.push('"');
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

fn is_remote(url: &str) -> bool {
    url.starts_with("data:")
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_images_become_data_urls_and_remote_ones_are_left_alone() {
        let html = "<p><img src=\"assets/图.png\" alt=\"\"><img src=\"https://a/b.png\" alt=\"\">\
                    <img src=\"data:image/png;base64,AA\" alt=\"\"></p>";
        let out = inline_images(html, |url| {
            (url == "assets/图.png").then(|| "data:image/png;base64,ZZ".to_string())
        });
        assert!(out.contains("src=\"data:image/png;base64,ZZ\""), "{out}");
        assert!(out.contains("src=\"https://a/b.png\""), "{out}");
        assert!(out.contains("src=\"data:image/png;base64,AA\""), "{out}");
    }

    #[test]
    fn missing_images_keep_their_original_path() {
        // 断链也要看得见：直接删掉 src 会让用户以为「这里本来没图」
        let html = "<img src=\"assets/没了.png\" alt=\"\">";
        let out = inline_images(html, |_| None);
        assert_eq!(out, html);
    }

    #[test]
    fn escaped_ampersands_are_decoded_before_resolving() {
        let html = "<img src=\"a&amp;b.png\" alt=\"\">";
        let out = inline_images(html, |url| {
            assert_eq!(url, "a&b.png", "读文件前必须还原实体");
            Some("data:image/png;base64,CC".to_string())
        });
        assert!(out.contains("CC"), "{out}");
    }

    #[test]
    fn page_wraps_body_with_style_and_escapes_title() {
        let out = page("A & B <草稿>", "<p>正文</p>");
        assert!(out.starts_with("<!DOCTYPE html>"), "{out}");
        assert!(
            out.contains("<title>A &amp; B &lt;草稿&gt;</title>"),
            "{out}"
        );
        assert!(out.contains(".tk-keyword"), "样式表要带代码高亮配色");
        assert!(out.contains("<p>正文</p>"));
    }
}

//! 代码块高亮：把围栏代码块的内容切成粗粒度语义片段。
//!
//! 不引第三方语法库。这里只需要「关键字 / 类型 / 字符串 / 注释 / 数字 / 函数」
//! 这一档着色，自带的词法扫描足以覆盖常见语言，也保持 `mdview-markdown` 的
//! 零依赖约束（见 PLAN 第 12 节）。
//!
//! 产出的 [`Token`] 与投影层共用 **byte offset** 坐标系：渲染层按字符停靠点
//! 查表套 class，不改变停靠点集合，也不参与编辑。未知语言返回空表，按原样
//! 显示代码。

use std::ops::Range;

mod langs;
mod lexer;

#[cfg(test)]
mod tests;

/// 片段语义。渲染层把它翻译成 CSS class（见 [`TokenKind::class`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Comment,
    Keyword,
    Type,
    Literal,
    String,
    Number,
    Function,
    Variable,
    Tag,
    Attribute,
    /// Rust `println!` / `vec!` 这类宏调用，以及 C 预处理指令。
    Macro,
    /// Rust `#[derive]`、`@dataclass`、`@Override` 这类属性与注解。
    Annotation,
    Operator,
}

impl TokenKind {
    /// CSS class 名。桌面端 `app.css` 与 CLI 验证页的样式都按这套命名。
    pub fn class(self) -> &'static str {
        match self {
            TokenKind::Comment => "tk-comment",
            TokenKind::Keyword => "tk-keyword",
            TokenKind::Type => "tk-type",
            TokenKind::Literal => "tk-literal",
            TokenKind::String => "tk-string",
            TokenKind::Number => "tk-number",
            TokenKind::Function => "tk-function",
            TokenKind::Variable => "tk-variable",
            TokenKind::Tag => "tk-tag",
            TokenKind::Attribute => "tk-attribute",
            TokenKind::Macro => "tk-macro",
            TokenKind::Annotation => "tk-annotation",
            TokenKind::Operator => "tk-operator",
        }
    }
}

/// 一个着色片段。`byte` 是**整篇源码**的绝对区间，且保证非空、互不重叠。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub byte: Range<usize>,
    pub kind: TokenKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Rust,
    Js,
    Ts,
    Python,
    Json,
    Html,
    Css,
    Shell,
    Go,
    C,
    Cpp,
    Java,
    Sql,
    Yaml,
    Toml,
    Markdown,
    Plain,
}

/// 围栏 info 字符串里的语言词。`rust`、`{.python}`、`js,ignore` 都只取第一个词，
/// 供语法高亮与代码块上的语言标签共用，避免两处各猜一套。
pub(crate) fn language_word(info: Option<&str>) -> Option<&str> {
    let info = info?;
    let word = info
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim()
        .trim_start_matches('.')
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .next()
        .unwrap_or("")
        .trim_matches('.');
    (!word.is_empty()).then_some(word)
}

impl Lang {
    fn from_info(info: Option<&str>) -> Self {
        let Some(word) = language_word(info) else {
            return Lang::Plain;
        };
        match word.to_ascii_lowercase().as_str() {
            "rust" | "rs" => Lang::Rust,
            "js" | "javascript" | "jsx" | "mjs" | "cjs" => Lang::Js,
            "ts" | "typescript" | "tsx" => Lang::Ts,
            "py" | "python" | "python3" => Lang::Python,
            "json" | "jsonc" => Lang::Json,
            "html" | "htm" | "xml" | "svg" | "vue" | "svelte" => Lang::Html,
            "css" | "scss" | "less" => Lang::Css,
            "sh" | "bash" | "shell" | "zsh" | "fish" => Lang::Shell,
            "go" | "golang" => Lang::Go,
            "c" | "h" => Lang::C,
            "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hxx" => Lang::Cpp,
            "java" | "kt" | "kotlin" | "cs" | "csharp" | "c#" => Lang::Java,
            "sql" | "postgres" | "postgresql" | "mysql" | "sqlite" => Lang::Sql,
            "yaml" | "yml" => Lang::Yaml,
            "toml" | "ini" | "conf" => Lang::Toml,
            "md" | "markdown" => Lang::Markdown,
            _ => Lang::Plain,
        }
    }
}

/// 对一段代码做词法着色。
///
/// `content` 是代码块**正文**在整篇源码里的 byte 区间（不含围栏），`info` 是
/// 围栏后的语言标记。返回的区间已经加上 `content.start`，可直接与
/// `Block::spans` 的 byte 对齐。
pub fn highlight(info: Option<&str>, src: &str, content: Range<usize>) -> Vec<Token> {
    let Some(code) = src.get(content.clone()) else {
        return Vec::new();
    };
    if code.is_empty() {
        return Vec::new();
    }
    let lang = Lang::from_info(info);
    let tokens = match lang {
        Lang::Plain => return Vec::new(),
        Lang::Json => langs::web::json_tokens(code, content.start),
        Lang::Html => langs::web::html_tokens(code, content.start),
        Lang::Css => langs::web::css_tokens(code, content.start),
        Lang::Yaml => langs::data::yaml_tokens(code, content.start),
        Lang::Toml => langs::data::toml_tokens(code, content.start),
        Lang::Markdown => langs::markdown::markdown_tokens(code, content.start),
        _ => lexer::code_tokens(lang, code, content.start),
    };
    let tokens = normalize(tokens, &content);
    debug_assert!(
        tokens
            .iter()
            .all(|t| src.is_char_boundary(t.byte.start) && src.is_char_boundary(t.byte.end))
    );
    tokens
}

/// 把各 scanner 的产出收敛到统一契约：非空、在 `content` 内、按起点升序、
/// 互不重叠，并合并相邻同 kind。渲染层（`token_class` 的二分与
/// `push_code_span` 的顺序切片）都以此为前提，不能只靠注释约定。
fn normalize(mut tokens: Vec<Token>, content: &Range<usize>) -> Vec<Token> {
    tokens.retain(|t| {
        t.byte.start < t.byte.end && content.start <= t.byte.start && t.byte.end <= content.end
    });
    tokens.sort_by_key(|t| (t.byte.start, t.byte.end));

    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    for token in tokens {
        if let Some(last) = out.last_mut() {
            if token.byte.start < last.byte.end {
                // 重叠时保留先出现的片段：各 scanner 按源码顺序推进，先到的
                // 通常是外层结构（如标签/字符串），内层片段宁可丢掉也不越界。
                continue;
            }
            if token.byte.start == last.byte.end && token.kind == last.kind {
                last.byte.end = token.byte.end;
                continue;
            }
        }
        out.push(token);
    }
    out
}

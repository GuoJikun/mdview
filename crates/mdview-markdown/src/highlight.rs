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
    let mut tokens = match lang {
        Lang::Plain => return Vec::new(),
        Lang::Json => json_tokens(code, content.start),
        Lang::Html => html_tokens(code, content.start),
        Lang::Css => css_tokens(code, content.start),
        Lang::Yaml => yaml_tokens(code, content.start),
        Lang::Toml => toml_tokens(code, content.start),
        Lang::Markdown => markdown_tokens(code, content.start),
        _ => code_tokens(lang, code, content.start),
    };
    tokens.sort_by_key(|t| (t.byte.start, t.byte.end));
    tokens
}

// ---- 通用词法 ---------------------------------------------------------

/// 语言相关的扫描参数。`line_comments` / `block_comment` 决定注释边界，
/// 其余三个表决定标识符归类。
struct Spec {
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    literals: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// SQL 这类大小写不敏感的语言。
    case_insensitive: bool,
    /// `#` 只有位于行首或空白之后才算注释（Shell / YAML）。
    hash_needs_boundary: bool,
}

fn spec(lang: Lang) -> Spec {
    match lang {
        Lang::Rust => Spec {
            keywords: RUST_KEYWORDS,
            types: RUST_TYPES,
            literals: RUST_LITERALS,
            line_comments: &["//"],
            block_comment: Some(("/*", "*/")),
            case_insensitive: false,
            hash_needs_boundary: false,
        },
        Lang::Js | Lang::Ts => Spec {
            keywords: if lang == Lang::Ts {
                TS_KEYWORDS
            } else {
                JS_KEYWORDS
            },
            types: if lang == Lang::Ts { TS_TYPES } else { JS_TYPES },
            literals: JS_LITERALS,
            line_comments: &["//"],
            block_comment: Some(("/*", "*/")),
            case_insensitive: false,
            hash_needs_boundary: false,
        },
        Lang::Python => Spec {
            keywords: PYTHON_KEYWORDS,
            types: PYTHON_TYPES,
            literals: PYTHON_LITERALS,
            line_comments: &["#"],
            block_comment: None,
            case_insensitive: false,
            hash_needs_boundary: false,
        },
        Lang::Shell => Spec {
            keywords: SHELL_KEYWORDS,
            types: &[],
            literals: SHELL_LITERALS,
            line_comments: &["#"],
            block_comment: None,
            case_insensitive: false,
            hash_needs_boundary: true,
        },
        Lang::Go => Spec {
            keywords: GO_KEYWORDS,
            types: GO_TYPES,
            literals: GO_LITERALS,
            line_comments: &["//"],
            block_comment: Some(("/*", "*/")),
            case_insensitive: false,
            hash_needs_boundary: false,
        },
        Lang::C => Spec {
            keywords: C_KEYWORDS,
            types: C_TYPES,
            literals: C_LITERALS,
            line_comments: &["//"],
            block_comment: Some(("/*", "*/")),
            case_insensitive: false,
            hash_needs_boundary: false,
        },
        Lang::Cpp => Spec {
            keywords: CPP_KEYWORDS,
            types: CPP_TYPES,
            literals: C_LITERALS,
            line_comments: &["//"],
            block_comment: Some(("/*", "*/")),
            case_insensitive: false,
            hash_needs_boundary: false,
        },
        Lang::Java => Spec {
            keywords: JAVA_KEYWORDS,
            types: JAVA_TYPES,
            literals: C_LITERALS,
            line_comments: &["//"],
            block_comment: Some(("/*", "*/")),
            case_insensitive: false,
            hash_needs_boundary: false,
        },
        Lang::Sql => Spec {
            keywords: SQL_KEYWORDS,
            types: SQL_TYPES,
            literals: SQL_LITERALS,
            line_comments: &["--"],
            block_comment: Some(("/*", "*/")),
            case_insensitive: true,
            hash_needs_boundary: false,
        },
        _ => Spec {
            keywords: &[],
            types: &[],
            literals: &[],
            line_comments: &[],
            block_comment: None,
            case_insensitive: false,
            hash_needs_boundary: false,
        },
    }
}

struct Sink {
    base: usize,
    out: Vec<Token>,
}

impl Sink {
    fn new(base: usize) -> Self {
        Self {
            base,
            out: Vec::new(),
        }
    }

    fn push(&mut self, start: usize, end: usize, kind: TokenKind) {
        if start < end {
            self.out.push(Token {
                byte: self.base + start..self.base + end,
                kind,
            });
        }
    }

    fn finish(self) -> Vec<Token> {
        self.out
    }
}

fn code_tokens(lang: Lang, code: &str, base: usize) -> Vec<Token> {
    let spec = spec(lang);
    let mut sink = Sink::new(base);
    let mut i = 0;

    while i < code.len() {
        let c = code[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }

        if let Some((open, close)) = spec.block_comment
            && code[i..].starts_with(open)
        {
            let end = code[i + open.len()..]
                .find(close)
                .map(|p| i + open.len() + p + close.len())
                .unwrap_or(code.len());
            sink.push(i, end, TokenKind::Comment);
            i = end;
            continue;
        }

        if let Some(end) = line_comment_end(&spec, code, i) {
            sink.push(i, end, TokenKind::Comment);
            i = end;
            continue;
        }

        if lang == Lang::Shell && c == '$' {
            let end = shell_var_end(code, i);
            sink.push(i, end, TokenKind::Variable);
            i = end;
            continue;
        }

        if let Some(end) = string_end(lang, code, i) {
            sink.push(i, end, TokenKind::String);
            i = end;
            continue;
        }

        if c.is_ascii_digit() {
            let end = number_end(code, i);
            sink.push(i, end, TokenKind::Number);
            i = end;
            continue;
        }

        if is_ident_start(c) {
            let mut j = i + c.len_utf8();
            while j < code.len() {
                let d = code[j..].chars().next().unwrap();
                if is_ident_continue(d) {
                    j += d.len_utf8();
                } else {
                    break;
                }
            }
            let word = &code[i..j];
            let after = skip_space(code, j);
            let is_call = code[after..].starts_with('(')
                || (lang == Lang::Rust && code[after..].starts_with('!'));
            if let Some(kind) = word_kind(&spec, word, is_call) {
                sink.push(i, j, kind);
            }
            i = j;
            continue;
        }

        if is_operator(c) {
            sink.push(i, i + c.len_utf8(), TokenKind::Operator);
            i += c.len_utf8();
            continue;
        }

        i += c.len_utf8();
    }

    sink.finish()
}

fn line_comment_end(spec: &Spec, code: &str, i: usize) -> Option<usize> {
    for &prefix in spec.line_comments {
        if !code[i..].starts_with(prefix) {
            continue;
        }
        if prefix == "#" && spec.hash_needs_boundary && i > 0 {
            let prev = code[..i].chars().next_back().unwrap_or('\n');
            if !prev.is_whitespace() {
                continue;
            }
        }
        return Some(code[i..].find('\n').map(|p| i + p).unwrap_or(code.len()));
    }
    None
}

fn word_kind(spec: &Spec, word: &str, is_call: bool) -> Option<TokenKind> {
    if contains(spec.literals, word, spec.case_insensitive) {
        return Some(TokenKind::Literal);
    }
    if contains(spec.keywords, word, spec.case_insensitive) {
        return Some(TokenKind::Keyword);
    }
    if is_call {
        return Some(TokenKind::Function);
    }
    if contains(spec.types, word, spec.case_insensitive) {
        return Some(TokenKind::Type);
    }
    None
}

fn contains(list: &[&str], word: &str, case_insensitive: bool) -> bool {
    list.iter().any(|k| {
        if case_insensitive {
            k.eq_ignore_ascii_case(word)
        } else {
            *k == word
        }
    })
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_numeric()
}

fn is_operator(c: char) -> bool {
    matches!(
        c,
        '=' | '+' | '-' | '*' | '/' | '%' | '<' | '>' | '!' | '&' | '|' | '^' | '~' | '?' | ':'
    )
}

fn skip_space(code: &str, mut i: usize) -> usize {
    while i < code.len() {
        let c = code[i..].chars().next().unwrap();
        if c == ' ' || c == '\t' || c == '\r' {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    i
}

/// 从 `start` 处的引号扫到闭合引号，返回结束位置（不含）。
///
/// `allow_newline` 为假时，遇到换行视为未闭合（返回 `None`），避免一个落单的
/// 引号把整段后续代码都吞成字符串。
fn quote_end(
    s: &str,
    start: usize,
    quote: char,
    escapes: bool,
    allow_newline: bool,
) -> Option<usize> {
    let mut i = start + quote.len_utf8();
    while i < s.len() {
        let c = s[i..].chars().next()?;
        if escapes && c == '\\' {
            i += 1;
            if i < s.len() {
                i += s[i..].chars().next()?.len_utf8();
            }
            continue;
        }
        if c == quote {
            return Some(i + quote.len_utf8());
        }
        if c == '\n' && !allow_newline {
            return None;
        }
        i += c.len_utf8();
    }
    None
}

/// 单字符字面量：`'a'`、`'\n'`、`'\u{1F600}'`。
///
/// Rust 的生命周期 `'a` 没有闭合引号，这里返回 `None`，不会被染成字符串。
fn single_char_end(s: &str, start: usize) -> Option<usize> {
    let rest = &s[start..];
    if !rest.starts_with('\'') {
        return None;
    }
    let mut j = 1;
    let c = rest[j..].chars().next()?;
    if c == '\\' {
        j += 1;
        let escape = rest[j..].chars().next()?;
        if escape == 'u' && rest[j + 1..].starts_with('{') {
            j += rest[j..].find('}')? + 1;
        } else {
            j += escape.len_utf8();
        }
    } else {
        if c == '\'' || c == '\n' {
            return None;
        }
        j += c.len_utf8();
    }
    rest[j..].starts_with('\'').then_some(j + 1)
}

fn triple_quote_end(s: &str, start: usize, quote: char) -> Option<usize> {
    let pat = if quote == '"' { "\"\"\"" } else { "'''" };
    s[start + 3..].find(pat).map(|p| start + 3 + p + 3)
}

fn string_end(lang: Lang, code: &str, i: usize) -> Option<usize> {
    let rest = &code[i..];
    let c = rest.chars().next()?;
    // 下面各分支返回的偏移都相对于 `rest`，最后统一加上 `i`。
    let relative = match lang {
        Lang::Rust => {
            // r"..." / r#"..."# / br#"..."# / b"..." / b'...' / c"..."
            let mut p = 0;
            if rest.starts_with('b') || rest.starts_with('c') {
                p = 1;
            }
            if rest[p..].starts_with('r') {
                let mut q = p + 1;
                let mut hashes = 0;
                while rest[q..].starts_with('#') {
                    hashes += 1;
                    q += 1;
                }
                if rest[q..].starts_with('"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    Some(
                        rest[q + 1..]
                            .find(&close)
                            .map(|x| q + 1 + x + close.len())
                            .unwrap_or(rest.len()),
                    )
                } else if rest.starts_with("b\"") || rest.starts_with("c\"") {
                    quote_end(rest, 1, '"', true, false)
                } else {
                    None
                }
            } else if rest.starts_with("b\"") || rest.starts_with("c\"") {
                quote_end(rest, 1, '"', true, false)
            } else if rest.starts_with("b'") {
                single_char_end(rest, 1)
            } else if c == '"' {
                quote_end(rest, 0, '"', true, false)
            } else if c == '\'' {
                single_char_end(rest, 0)
            } else {
                None
            }
        }
        Lang::Python => {
            let bytes = rest.as_bytes();
            let mut p = 0;
            while p < 2
                && p < bytes.len()
                && matches!(bytes[p].to_ascii_lowercase(), b'r' | b'b' | b'u' | b'f')
            {
                p += 1;
            }
            let quote = *rest.as_bytes().get(p)? as char;
            if quote != '"' && quote != '\'' {
                None
            } else if rest[p..].starts_with(&format!("{quote}{quote}{quote}")) {
                triple_quote_end(rest, p, quote)
            } else {
                quote_end(rest, p, quote, true, false)
            }
        }
        Lang::Js | Lang::Ts => {
            if c == '"' || c == '\'' {
                quote_end(rest, 0, c, true, false)
            } else if c == '`' {
                quote_end(rest, 0, '`', true, true)
            } else {
                None
            }
        }
        Lang::Shell => {
            if c == '"' {
                quote_end(rest, 0, '"', true, true)
            } else if c == '\'' {
                quote_end(rest, 0, '\'', false, true)
            } else if c == '`' {
                quote_end(rest, 0, '`', true, true)
            } else {
                None
            }
        }
        Lang::Go => {
            if c == '"' || c == '\'' {
                quote_end(rest, 0, c, true, false)
            } else if c == '`' {
                quote_end(rest, 0, '`', false, true)
            } else {
                None
            }
        }
        Lang::Cpp => {
            // C++ 原始字符串 R"delim(...)delim"
            if rest.starts_with("R\"")
                && let Some(open) = rest.find('(')
            {
                let delim = &rest[2..open];
                let close = format!("){delim}\"");
                Some(
                    rest[open + 1..]
                        .find(&close)
                        .map(|p| open + 1 + p + close.len())
                        .unwrap_or(rest.len()),
                )
            } else if c == '"' || c == '\'' {
                quote_end(rest, 0, c, true, false)
            } else {
                None
            }
        }
        Lang::C | Lang::Java | Lang::Sql => {
            if c == '"' || c == '\'' {
                quote_end(rest, 0, c, true, false)
            } else {
                None
            }
        }
        _ => None,
    };
    relative.map(|end| i + end)
}

fn number_end(code: &str, start: usize) -> usize {
    let bytes = code.as_bytes();
    let mut i = start;
    if bytes[i] == b'0'
        && i + 1 < bytes.len()
        && matches!(bytes[i + 1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
    {
        i += 2;
        while i < bytes.len() && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
            i += 1;
        }
    } else {
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
        if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
        }
        if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
            let mut j = i + 1;
            if j < bytes.len() && matches!(bytes[j], b'+' | b'-') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_digit() {
                i = j;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
    }
    // 数值后缀：1u32 / 1.0f64 / 10n
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

fn shell_var_end(code: &str, start: usize) -> usize {
    let rest = &code[start..];
    if rest.starts_with("${") {
        return rest
            .find('}')
            .map(|p| start + p + 1)
            .unwrap_or(start + rest.len().min(1));
    }
    let mut i = start + 1;
    while i < code.len() {
        let c = code[i..].chars().next().unwrap();
        if is_ident_continue(c) || matches!(c, '@' | '*' | '#' | '?' | '$' | '!' | '-' | '0'..='9')
        {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    i
}

// ---- JSON -------------------------------------------------------------

fn json_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    let mut i = 0;
    while i < code.len() {
        let c = code[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        if c == '"' {
            let end = quote_end(code, i, '"', true, false).unwrap_or(i + 1);
            let after = skip_space(code, end);
            let kind = if code[after..].starts_with(':') {
                TokenKind::Attribute
            } else {
                TokenKind::String
            };
            sink.push(i, end, kind);
            i = end;
            continue;
        }
        if c.is_ascii_digit()
            || (c == '-' && code[i + 1..].starts_with(|d: char| d.is_ascii_digit()))
        {
            let end = number_end(code, i);
            sink.push(i, end, TokenKind::Number);
            i = end;
            continue;
        }
        if let Some((end, kind)) = word_at(code, i) {
            sink.push(i, end, kind);
            i = end;
            continue;
        }
        if matches!(c, '{' | '}' | '[' | ']' | ',' | ':') {
            sink.push(i, i + 1, TokenKind::Operator);
        }
        i += c.len_utf8();
    }
    sink.finish()
}

/// JSON / YAML 值里的 true / false / null。
fn word_at(code: &str, i: usize) -> Option<(usize, TokenKind)> {
    let rest = &code[i..];
    for word in ["true", "false", "null"] {
        if rest.starts_with(word)
            && !rest[word.len()..]
                .chars()
                .next()
                .is_some_and(is_ident_continue)
        {
            return Some((i + word.len(), TokenKind::Literal));
        }
    }
    None
}

// ---- HTML / XML -------------------------------------------------------

fn html_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    let mut i = 0;
    while i < code.len() {
        let rest = &code[i..];
        if let Some(stripped) = rest.strip_prefix("<!--") {
            let end = stripped
                .find("-->")
                .map(|p| i + 4 + p + 3)
                .unwrap_or(code.len());
            sink.push(i, end, TokenKind::Comment);
            i = end;
            continue;
        }
        if rest.starts_with("<!") {
            let end = rest.find('>').map(|p| i + p + 1).unwrap_or(code.len());
            sink.push(i, end, TokenKind::Comment);
            i = end;
            continue;
        }
        if let Some(stripped) = rest.strip_prefix('<') {
            // 只有 `<` 后面紧跟标签名 / `/` / `!` / `?` 才当标签；
            // 正文里的 `a < b` 不能把后半行都吞成属性。
            let next = stripped.chars().next();
            let is_tag =
                next.is_some_and(|c| c.is_alphabetic() || c == '/' || c == '!' || c == '?');
            if !is_tag {
                sink.push(i, i + 1, TokenKind::Operator);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            sink.push(i, j, TokenKind::Operator);
            if code[j..].starts_with('/') {
                sink.push(j, j + 1, TokenKind::Operator);
                j += 1;
            }
            let name_start = j;
            while j < code.len() {
                let c = code[j..].chars().next().unwrap();
                if is_ident_continue(c) || matches!(c, '-' | ':' | '.') {
                    j += c.len_utf8();
                } else {
                    break;
                }
            }
            let name = code[name_start..j].to_owned();
            if !name.is_empty() {
                sink.push(name_start, j, TokenKind::Tag);
            }
            // 属性区：标签名之后到 `>` 之前
            while j < code.len() {
                let c = code[j..].chars().next().unwrap();
                if c == '>' {
                    sink.push(j, j + 1, TokenKind::Operator);
                    j += 1;
                    break;
                }
                if c == '/' || c == '=' {
                    sink.push(j, j + 1, TokenKind::Operator);
                    j += 1;
                    continue;
                }
                if c.is_whitespace() {
                    j += c.len_utf8();
                    continue;
                }
                if c == '"' || c == '\'' {
                    let end = quote_end(code, j, c, false, true).unwrap_or(j + 1);
                    sink.push(j, end, TokenKind::String);
                    j = end;
                    continue;
                }
                if is_ident_start(c) || matches!(c, '-' | ':' | '@') {
                    let start = j;
                    j += c.len_utf8();
                    while j < code.len() {
                        let d = code[j..].chars().next().unwrap();
                        if is_ident_continue(d) || matches!(d, '-' | ':' | '.' | '@') {
                            j += d.len_utf8();
                        } else {
                            break;
                        }
                    }
                    sink.push(start, j, TokenKind::Attribute);
                    continue;
                }
                j += c.len_utf8();
            }
            // <script> / <style> 内部再按 JS / CSS 扫一遍
            if name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style") {
                let close = format!("</{}", name.to_ascii_lowercase());
                if let Some(offset) = find_ci(&code[j..], &close) {
                    let end = j + offset;
                    let inner = &code[j..end];
                    let mut inner_tokens = if name.eq_ignore_ascii_case("style") {
                        css_tokens(inner, base + j)
                    } else {
                        code_tokens(Lang::Js, inner, base + j)
                    };
                    sink.out.append(&mut inner_tokens);
                    j = end;
                }
            }
            i = j;
            continue;
        }
        i += rest.chars().next().unwrap().len_utf8();
    }
    sink.finish()
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

// ---- CSS --------------------------------------------------------------

fn css_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    let mut i = 0;
    let mut depth = 0usize;

    while i < code.len() {
        let rest = &code[i..];
        let c = rest.chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("/*") {
            let end = stripped
                .find("*/")
                .map(|p| i + 2 + p + 2)
                .unwrap_or(code.len());
            sink.push(i, end, TokenKind::Comment);
            i = end;
            continue;
        }
        if c == '"' || c == '\'' {
            let end = quote_end(code, i, c, true, true).unwrap_or(i + 1);
            sink.push(i, end, TokenKind::String);
            i = end;
            continue;
        }
        if c == '{' {
            depth += 1;
            sink.push(i, i + 1, TokenKind::Operator);
            i += 1;
            continue;
        }
        if c == '}' {
            depth = depth.saturating_sub(1);
            sink.push(i, i + 1, TokenKind::Operator);
            i += 1;
            continue;
        }
        if c == '@' {
            let mut j = i + 1;
            while j < code.len() {
                let d = code[j..].chars().next().unwrap();
                if is_ident_continue(d) || d == '-' {
                    j += d.len_utf8();
                } else {
                    break;
                }
            }
            sink.push(i, j, TokenKind::Keyword);
            i = j;
            continue;
        }
        if c == '#' {
            let mut j = i + 1;
            while j < code.len() && code.as_bytes()[j].is_ascii_hexdigit() {
                j += 1;
            }
            if matches!(j - i - 1, 3 | 4 | 6 | 8) {
                sink.push(i, j, TokenKind::Number);
                i = j;
                continue;
            }
            while j < code.len() {
                let d = code[j..].chars().next().unwrap();
                if is_ident_continue(d) || d == '-' {
                    j += d.len_utf8();
                } else {
                    break;
                }
            }
            if j > i + 1 {
                sink.push(i, j, TokenKind::Tag);
                i = j;
                continue;
            }
            sink.push(i, i + 1, TokenKind::Operator);
            i += 1;
            continue;
        }
        if c == '.' && code[i + 1..].starts_with(is_ident_start) {
            let mut j = i + 1;
            while j < code.len() {
                let d = code[j..].chars().next().unwrap();
                if is_ident_continue(d) || d == '-' {
                    j += d.len_utf8();
                } else {
                    break;
                }
            }
            sink.push(i, j, TokenKind::Tag);
            i = j;
            continue;
        }
        if is_ident_start(c)
            || (c == '-'
                && code[i + 1..].starts_with(|d: char| d == '-' || d.is_alphabetic() || d == '_'))
        {
            let start = i;
            let mut j = i;
            if c == '-' {
                j += 1;
                if code[j..].starts_with('-') {
                    j += 1;
                }
            }
            while j < code.len() {
                let d = code[j..].chars().next().unwrap();
                if is_ident_continue(d) || d == '-' {
                    j += d.len_utf8();
                } else {
                    break;
                }
            }
            let word = &code[start..j];
            let after = skip_space(code, j);
            if depth > 0 && code[after..].starts_with(':') && !code[after..].starts_with("::") {
                sink.push(start, j, TokenKind::Attribute);
            } else if code[after..].starts_with('(') {
                sink.push(start, j, TokenKind::Function);
            } else if word.starts_with("--") {
                sink.push(start, j, TokenKind::Variable);
            } else if depth == 0 {
                sink.push(start, j, TokenKind::Tag);
            } else if contains(CSS_LITERALS, word, true) {
                sink.push(start, j, TokenKind::Literal);
            }
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let end = number_end(code, i);
            sink.push(i, end, TokenKind::Number);
            i = end;
            continue;
        }
        if is_operator(c) {
            sink.push(i, i + c.len_utf8(), TokenKind::Operator);
        }
        i += c.len_utf8();
    }

    sink.finish()
}

// ---- YAML / TOML ------------------------------------------------------

fn for_each_line(code: &str, mut f: impl FnMut(usize, &str)) {
    let mut start = 0;
    for (idx, ch) in code.char_indices() {
        if ch == '\n' {
            f(start, &code[start..idx]);
            start = idx + 1;
        }
    }
    if start < code.len() {
        f(start, &code[start..]);
    }
}

fn yaml_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    for_each_line(code, |start, line| {
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        if rest.is_empty() {
            return;
        }
        if rest.starts_with('#') {
            sink.push(start + indent, start + line.len(), TokenKind::Comment);
            return;
        }
        if rest.starts_with("---") || rest.starts_with("...") {
            sink.push(start + indent, start + indent + 3, TokenKind::Operator);
            return;
        }
        let mut pos = indent;
        if rest.starts_with("- ") {
            sink.push(start + pos, start + pos + 1, TokenKind::Operator);
            pos += 2;
        }
        let tail = &line[pos..];
        if let Some(colon) = yaml_colon(tail) {
            let key = tail[..colon].trim_end();
            if !key.is_empty() && !key.contains(char::is_whitespace) {
                sink.push(start + pos, start + pos + key.len(), TokenKind::Attribute);
                sink.push(
                    start + pos + colon,
                    start + pos + colon + 1,
                    TokenKind::Operator,
                );
                let value = pos + colon + 1;
                value_tokens(&mut sink, &line[value..], start + value);
                return;
            }
        }
        value_tokens(&mut sink, tail, start + pos);
    });
    sink.finish()
}

/// 找 YAML 的键值分隔冒号：引号内不算，且冒号后必须是空白或行尾。
fn yaml_colon(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (idx, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '#' => return None,
            ':' => {
                let next = s[idx + 1..].chars().next();
                if next.is_none_or(|n| n.is_whitespace()) {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn toml_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    for_each_line(code, |start, line| {
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        if rest.is_empty() {
            return;
        }
        if rest.starts_with('#') {
            sink.push(start + indent, start + line.len(), TokenKind::Comment);
            return;
        }
        if rest.starts_with('[') {
            let end = rest.find(']').map(|p| p + 1).unwrap_or(rest.len());
            sink.push(start + indent, start + indent + end, TokenKind::Tag);
            value_tokens(&mut sink, &line[indent + end..], start + indent + end);
            return;
        }
        if let Some(eq) = unquoted_char(rest, '=') {
            let key = rest[..eq].trim_end();
            if !key.is_empty() {
                sink.push(
                    start + indent,
                    start + indent + key.len(),
                    TokenKind::Attribute,
                );
                sink.push(
                    start + indent + eq,
                    start + indent + eq + 1,
                    TokenKind::Operator,
                );
                let value = indent + eq + 1;
                value_tokens(&mut sink, &line[value..], start + value);
                return;
            }
        }
        value_tokens(&mut sink, rest, start + indent);
    });
    sink.finish()
}

/// 引号外的第一个 `needle`。
fn unquoted_char(s: &str, needle: char) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (idx, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '#' => return None,
            c if c == needle => return Some(idx),
            _ => {}
        }
    }
    None
}

/// YAML / TOML 的标量值：字符串、数字、布尔、注释与少量分隔符。
fn value_tokens(sink: &mut Sink, s: &str, base: usize) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        if c == '#' && (i == 0 || s[..i].chars().next_back().is_some_and(char::is_whitespace)) {
            sink.push(base + i, base + s.len(), TokenKind::Comment);
            return;
        }
        if c == '"' || c == '\'' {
            let end = quote_end(s, i, c, true, true).unwrap_or(i + 1);
            sink.push(base + i, base + end, TokenKind::String);
            i = end;
            continue;
        }
        if c.is_ascii_digit() || (c == '-' && s[i + 1..].starts_with(|d: char| d.is_ascii_digit()))
        {
            let end = number_end(s, i);
            sink.push(base + i, base + end, TokenKind::Number);
            i = end;
            continue;
        }
        if is_ident_start(c) || c == '~' {
            let start = i;
            i += c.len_utf8();
            while i < s.len() {
                let d = s[i..].chars().next().unwrap();
                if is_ident_continue(d) || d == '-' || d == '.' {
                    i += d.len_utf8();
                } else {
                    break;
                }
            }
            let word = &s[start..i];
            if matches!(
                word.to_ascii_lowercase().as_str(),
                "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off"
            ) {
                sink.push(base + start, base + i, TokenKind::Literal);
            }
            continue;
        }
        if matches!(
            c,
            '[' | ']' | '{' | '}' | ',' | ':' | '|' | '>' | '&' | '*' | '!' | '?'
        ) {
            sink.push(base + i, base + i + 1, TokenKind::Operator);
        }
        i += c.len_utf8();
    }
}

// ---- Markdown ---------------------------------------------------------

fn markdown_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    let mut fence: Option<(char, usize)> = None;

    for_each_line(code, |start, line| {
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        if rest.is_empty() {
            return;
        }
        if let Some((fch, count)) = fence {
            let marker: String = std::iter::repeat_n(fch, count).collect();
            if rest.starts_with(&marker) {
                sink.push(start + indent, start + indent + count, TokenKind::Keyword);
                fence = None;
            }
            return;
        }
        if rest.starts_with("```") || rest.starts_with("~~~") {
            let fch = rest.chars().next().unwrap();
            let count = rest.chars().take_while(|c| *c == fch).count();
            sink.push(start + indent, start + indent + count, TokenKind::Keyword);
            let info = rest[count..].trim();
            if !info.is_empty() {
                sink.push(
                    start + indent + count,
                    start + indent + rest.len(),
                    TokenKind::Attribute,
                );
            }
            fence = Some((fch, count));
            return;
        }

        let hashes = rest.chars().take_while(|c| *c == '#').count();
        let mut pos = indent;
        if (1..=6).contains(&hashes) && rest[hashes..].starts_with(' ') {
            sink.push(start + indent, start + indent + hashes, TokenKind::Keyword);
            pos += hashes;
        } else if rest.starts_with('>') {
            sink.push(start + indent, start + indent + 1, TokenKind::Operator);
            pos += 1;
        } else if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
            sink.push(start + indent, start + indent + 1, TokenKind::Operator);
            pos += 2;
        } else {
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0
                && matches!(rest[digits..].chars().next(), Some('.' | ')'))
                && rest[digits + 1..].starts_with(' ')
            {
                sink.push(
                    start + indent,
                    start + indent + digits + 1,
                    TokenKind::Operator,
                );
                pos += digits + 2;
            }
        }

        // 行内：反引号代码、强调标记
        let mut i = pos;
        while i < line.len() {
            let tail = &line[i..];
            if tail.starts_with('`') {
                let count = tail.chars().take_while(|c| *c == '`').count();
                let pat = "`".repeat(count);
                if let Some(offset) = tail[count..].find(&pat) {
                    let end = i + count + offset + count;
                    sink.push(start + i, start + end, TokenKind::String);
                    i = end;
                    continue;
                }
            }
            if tail.starts_with("**") || tail.starts_with("__") || tail.starts_with("~~") {
                sink.push(start + i, start + i + 2, TokenKind::Operator);
                i += 2;
                continue;
            }
            if tail.starts_with('*') || tail.starts_with('_') {
                sink.push(start + i, start + i + 1, TokenKind::Operator);
                i += 1;
                continue;
            }
            i += tail.chars().next().unwrap().len_utf8();
        }
    });

    sink.finish()
}

// ---- 关键字表 ---------------------------------------------------------

const RUST_KEYWORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "const",
    "continue",
    "crate",
    "dyn",
    "else",
    "enum",
    "extern",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "match",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "return",
    "self",
    "Self",
    "static",
    "struct",
    "super",
    "trait",
    "type",
    "unsafe",
    "use",
    "where",
    "while",
    "yield",
    "macro_rules",
    "union",
];

const RUST_LITERALS: &[&str] = &["true", "false"];

const RUST_TYPES: &[&str] = &[
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f32", "f64", "String", "Vec", "Option", "Result", "Box", "Rc", "Arc",
    "HashMap", "HashSet", "BTreeMap", "BTreeSet", "Cow", "Path", "PathBuf", "OsStr", "OsString",
];

const JS_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

const JS_LITERALS: &[&str] = &["true", "false", "null", "undefined", "NaN", "Infinity"];

const JS_TYPES: &[&str] = &[
    "String", "Number", "Boolean", "Object", "Array", "Promise", "Map", "Set", "Symbol", "BigInt",
    "Error", "Date", "RegExp",
];

const TS_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "asserts",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "infer",
    "instanceof",
    "interface",
    "is",
    "keyof",
    "let",
    "module",
    "namespace",
    "new",
    "of",
    "override",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "satisfies",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "type",
    "typeof",
    "unique",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

const TS_TYPES: &[&str] = &[
    "any", "unknown", "never", "void", "string", "number", "boolean", "object", "symbol", "bigint",
    "Array", "Promise", "Record", "Partial", "Required", "Pick", "Omit", "Readonly", "Map", "Set",
    "Date", "RegExp", "Error",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda",
    "match", "case", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
    "yield", "self",
];

const PYTHON_LITERALS: &[&str] = &["True", "False", "None"];

const PYTHON_TYPES: &[&str] = &[
    "int",
    "float",
    "complex",
    "str",
    "bytes",
    "bool",
    "list",
    "dict",
    "set",
    "tuple",
    "object",
    "type",
    "range",
    "Exception",
    "ValueError",
    "TypeError",
    "KeyError",
    "RuntimeError",
    "AttributeError",
    "IndexError",
];

const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "elif", "else", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "coproc", "return", "exit", "export", "local", "readonly",
    "declare", "unset", "shift", "source", "alias", "eval", "exec", "set", "trap", "wait", "read",
];

const SHELL_LITERALS: &[&str] = &["true", "false"];

const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

const GO_LITERALS: &[&str] = &["true", "false", "nil", "iota"];

const GO_TYPES: &[&str] = &[
    "bool",
    "byte",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "any",
];

const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for",
    "goto", "if", "inline", "register", "restrict", "return", "sizeof", "static", "struct",
    "switch", "typedef", "union", "volatile", "while",
];

const C_LITERALS: &[&str] = &["true", "false", "NULL", "nullptr"];

const C_TYPES: &[&str] = &[
    "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void", "size_t",
    "ssize_t", "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t",
    "uint64_t", "bool", "FILE",
];

const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "asm",
    "auto",
    "break",
    "case",
    "catch",
    "class",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "const_cast",
    "continue",
    "co_await",
    "co_return",
    "co_yield",
    "decltype",
    "default",
    "delete",
    "do",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "final",
    "for",
    "friend",
    "goto",
    "if",
    "import",
    "inline",
    "module",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "operator",
    "override",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "return",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "using",
    "virtual",
    "volatile",
    "while",
];

const CPP_TYPES: &[&str] = &[
    "bool",
    "char",
    "double",
    "float",
    "int",
    "long",
    "short",
    "signed",
    "unsigned",
    "void",
    "size_t",
    "string",
    "vector",
    "map",
    "set",
    "unique_ptr",
    "shared_ptr",
    "optional",
    "variant",
];

const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "interface",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "sealed",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "var",
    "volatile",
    "while",
    "yield",
];

const JAVA_TYPES: &[&str] = &[
    "boolean", "byte", "char", "double", "float", "int", "long", "short", "void", "String",
    "Object", "List", "Map", "Set", "Optional", "Integer", "Long", "Double", "Boolean",
];

const SQL_KEYWORDS: &[&str] = &[
    "add",
    "all",
    "alter",
    "and",
    "as",
    "asc",
    "begin",
    "between",
    "by",
    "case",
    "column",
    "commit",
    "constraint",
    "create",
    "database",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "exists",
    "foreign",
    "from",
    "full",
    "group",
    "having",
    "if",
    "in",
    "index",
    "inner",
    "insert",
    "into",
    "is",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "not",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "over",
    "partition",
    "primary",
    "recursive",
    "references",
    "right",
    "rollback",
    "select",
    "set",
    "table",
    "then",
    "transaction",
    "union",
    "unique",
    "update",
    "values",
    "view",
    "when",
    "where",
    "with",
];

const SQL_LITERALS: &[&str] = &["true", "false", "null"];

const SQL_TYPES: &[&str] = &[
    "bigint",
    "blob",
    "boolean",
    "char",
    "date",
    "decimal",
    "double",
    "float",
    "int",
    "integer",
    "json",
    "jsonb",
    "numeric",
    "serial",
    "smallint",
    "text",
    "timestamp",
    "uuid",
    "varchar",
];

const CSS_LITERALS: &[&str] = &[
    "absolute",
    "auto",
    "block",
    "bold",
    "center",
    "dashed",
    "dotted",
    "fixed",
    "flex",
    "grid",
    "hidden",
    "inherit",
    "initial",
    "inline",
    "italic",
    "left",
    "middle",
    "none",
    "normal",
    "nowrap",
    "pointer",
    "relative",
    "right",
    "solid",
    "sticky",
    "top",
    "transparent",
    "unset",
    "visible",
    "wrap",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(code: &str, lang: &str) -> Vec<(String, TokenKind)> {
        highlight(Some(lang), code, 0..code.len())
            .into_iter()
            .map(|t| (code[t.byte].to_owned(), t.kind))
            .collect()
    }

    #[test]
    fn rust_marks_keywords_strings_numbers_and_comments() {
        let code = "let x = 42; // 说明\nlet s = \"hi\";";
        let found = tokens(code, "rust");
        assert!(found.contains(&("let".into(), TokenKind::Keyword)));
        assert!(found.contains(&("42".into(), TokenKind::Number)));
        assert!(found.contains(&("\"hi\"".into(), TokenKind::String)));
        assert!(
            found
                .iter()
                .any(|(t, k)| *k == TokenKind::Comment && t.starts_with("//"))
        );
    }

    #[test]
    fn rust_lifetime_is_not_a_string() {
        let found = tokens("fn f<'a>(x: &'a str) {}", "rust");
        assert!(
            !found.iter().any(|(_, k)| *k == TokenKind::String),
            "生命周期被误判成字符串: {found:?}"
        );
    }

    #[test]
    fn json_keys_are_attributes_and_values_are_strings() {
        let found = tokens(r#"{"name": "mdview", "count": 3}"#, "json");
        assert!(found.contains(&("\"name\"".into(), TokenKind::Attribute)));
        assert!(found.contains(&("\"mdview\"".into(), TokenKind::String)));
        assert!(found.contains(&("3".into(), TokenKind::Number)));
    }

    #[test]
    fn html_marks_tag_name_and_attributes() {
        let found = tokens(r#"<a class="x" href='y'>链接</a>"#, "html");
        assert!(found.contains(&("a".into(), TokenKind::Tag)));
        assert!(found.contains(&("class".into(), TokenKind::Attribute)));
        assert!(found.contains(&("\"x\"".into(), TokenKind::String)));
    }

    #[test]
    fn css_marks_property_inside_a_block() {
        let found = tokens("body { color: red; margin: 0; }", "css");
        assert!(found.contains(&("body".into(), TokenKind::Tag)));
        assert!(found.contains(&("color".into(), TokenKind::Attribute)));
        assert!(found.contains(&("0".into(), TokenKind::Number)));
    }

    #[test]
    fn yaml_marks_key_before_colon() {
        let found = tokens("name: mdview\ncount: 3\n", "yaml");
        assert!(found.contains(&("name".into(), TokenKind::Attribute)));
        assert!(found.contains(&("count".into(), TokenKind::Attribute)));
        assert!(found.contains(&("3".into(), TokenKind::Number)));
    }

    #[test]
    fn shell_marks_variables() {
        let found = tokens("echo $HOME ${PATH}", "bash");
        assert!(found.contains(&("$HOME".into(), TokenKind::Variable)));
        assert!(found.contains(&("${PATH}".into(), TokenKind::Variable)));
    }

    #[test]
    fn sql_is_case_insensitive() {
        let found = tokens("SELECT id FROM users WHERE id = 1", "sql");
        assert!(found.contains(&("SELECT".into(), TokenKind::Keyword)));
        assert!(found.contains(&("FROM".into(), TokenKind::Keyword)));
    }

    #[test]
    fn unknown_language_produces_nothing() {
        assert!(tokens("let x = 1", "brainfuck").is_empty());
    }

    #[test]
    fn tokens_stay_inside_the_content_and_on_char_boundaries() {
        let src = "前缀\n```rust\nlet 中文 = \"值\";\n```\n后缀";
        let content = src.find("let").unwrap()..src.rfind("```").unwrap();
        let tokens = highlight(Some("rust"), src, content.clone());
        assert!(!tokens.is_empty());
        for token in &tokens {
            assert!(content.start <= token.byte.start && token.byte.end <= content.end);
            assert!(src.is_char_boundary(token.byte.start));
            assert!(src.is_char_boundary(token.byte.end));
        }
    }
}

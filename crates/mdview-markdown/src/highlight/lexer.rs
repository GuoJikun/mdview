//! 通用词法扫描器：注释、字符串、数字、标识符与关键字表驱动的分类。
//! 专用语言扫描器在 `langs` 下，复用这里的 `Sink` 与边界辅助函数。

use super::langs::code::*;
use super::{Lang, Token, TokenKind};

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

pub(crate) struct Sink {
    base: usize,
    pub(crate) out: Vec<Token>,
}

impl Sink {
    pub(crate) fn new(base: usize) -> Self {
        Self {
            base,
            out: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, start: usize, end: usize, kind: TokenKind) {
        if start < end {
            self.out.push(Token {
                byte: self.base + start..self.base + end,
                kind,
            });
        }
    }

    pub(crate) fn finish(self) -> Vec<Token> {
        self.out
    }
}

pub(crate) fn code_tokens(lang: Lang, code: &str, base: usize) -> Vec<Token> {
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

        if lang == Lang::Rust
            && c == '#'
            && let Some(end) = attribute_end(code, i)
        {
            sink.push(i, end, TokenKind::Annotation);
            i = end;
            continue;
        }

        if matches!(lang, Lang::C | Lang::Cpp) && c == '#' && at_line_start(code, i) {
            if let Some(end) = directive_end(code, i) {
                sink.push(i, end, TokenKind::Macro);
                i = end;
                continue;
            }
        }

        if matches!(lang, Lang::Python | Lang::Ts | Lang::Java)
            && c == '@'
            && annotation_allowed(lang, code, i)
            && let Some(end) = annotation_end(code, i)
        {
            sink.push(i, end, TokenKind::Annotation);
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
            // Rust 的 `name!` 是宏调用而非函数，先于关键字/类型判定：
            // `macro_rules!` 这类关键字也不该盖掉宏语义。
            let is_macro = lang == Lang::Rust && code[after..].starts_with('!');
            let is_call = code[after..].starts_with('(');
            let kind = if is_macro {
                Some(TokenKind::Macro)
            } else {
                word_kind(&spec, word, is_call)
            };
            if let Some(kind) = kind {
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

/// Rust 的 `#[...]` / `#![...]` 属性：扫到配对 `]`，跨行也照样包住。
/// 未闭合时返回 `None`，让 `#` 保持原样，不当成注解吞掉后续代码。
fn attribute_end(code: &str, start: usize) -> Option<usize> {
    let rest = &code[start..];
    let inner = rest
        .strip_prefix("#![")
        .or_else(|| rest.strip_prefix("#["))?;
    let mut depth = 1usize;
    let mut i = start + (rest.len() - inner.len());
    while i < code.len() {
        let c = code[i..].chars().next()?;
        if c == '"' || c == '\'' {
            if let Some(end) = quote_end(code, i, c, true, true) {
                i = end;
                continue;
            }
        }
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    None
}

/// C / C++ 预处理指令（`#include`、`#define` 等）：只染色 `#[A-Za-z_]+`。
fn directive_end(code: &str, start: usize) -> Option<usize> {
    let rest = &code[start + 1..];
    let first = rest.chars().next()?;
    if !is_ident_start(first) {
        return None;
    }
    let mut i = start + 1 + first.len_utf8();
    while i < code.len() {
        let c = code[i..].chars().next()?;
        if is_ident_continue(c) {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    Some(i)
}

/// 当前行在 `i` 之前只有空白（允许缩进的前置空格）。
fn at_line_start(code: &str, i: usize) -> bool {
    code[..i]
        .rsplit('\n')
        .next()
        .is_some_and(|s| s.chars().all(char::is_whitespace))
}

/// `@` 是否可能作为注解起点：Python 只在行首认装饰器（其余是矩阵乘），
/// TS / Java 只要前一个非空字符是行首或运算符即可。
fn annotation_allowed(lang: Lang, code: &str, i: usize) -> bool {
    let before = code[..i].trim_end_matches([' ', '\t', '\r']);
    let prev = before.chars().next_back();
    match lang {
        Lang::Python => at_line_start(code, i),
        _ => prev.is_none_or(|c| !is_ident_continue(c)),
    }
}

/// 扫描 `@word` / `@pkg.word`：含点号路径，不含后面的调用括号。
fn annotation_end(code: &str, start: usize) -> Option<usize> {
    let rest = &code[start + 1..];
    let first = rest.chars().next()?;
    if !is_ident_start(first) {
        return None;
    }
    let mut i = start + 1 + first.len_utf8();
    while i < code.len() {
        let c = code[i..].chars().next()?;
        if is_ident_continue(c) {
            i += c.len_utf8();
        } else if c == '.' && code[i + 1..].chars().next().is_some_and(is_ident_start) {
            i += 1;
        } else {
            break;
        }
    }
    Some(i)
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

/// 在有序关键字表里查词。表由 `langs::code` 按字典序维护，走二分而不是线性扫描。
pub(crate) fn contains(list: &[&str], word: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        // 大小写不敏感的表（SQL / CSS）内容全是 ASCII 小写，小写化后再查即可。
        let lower = word.to_ascii_lowercase();
        list.binary_search(&lower.as_str()).is_ok()
    } else {
        list.binary_search(&word).is_ok()
    }
}

pub(crate) fn is_ident_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphabetic()
}

pub(crate) fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_numeric()
}

pub(crate) fn is_operator(c: char) -> bool {
    matches!(
        c,
        '=' | '+' | '-' | '*' | '/' | '%' | '<' | '>' | '!' | '&' | '|' | '^' | '~' | '?' | ':'
    )
}

pub(crate) fn skip_space(code: &str, mut i: usize) -> usize {
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
pub(crate) fn quote_end(
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

pub(crate) fn number_end(code: &str, start: usize) -> usize {
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

// ---- YAML / TOML ------------------------------------------------------

pub(crate) fn for_each_line(code: &str, mut f: impl FnMut(usize, &str)) {
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

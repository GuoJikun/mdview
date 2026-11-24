//! 通用词法扫描器：注释、字符串、数字、标识符与关键字表驱动的分类。
//! 专用语言扫描器在 `langs` 下，复用这里的 `Sink` 与边界辅助函数。

use super::langs::code::*;
use super::{Lang, Token, TokenKind};

// ---- 通用词法 ---------------------------------------------------------

/// 普通引号规则。带特殊语法的语言（Rust 原始字符串、C# 的 `@"…"`、Lua 的
/// `[[…]]`）仍在 [`string_end`] 里按 `lang` 单独处理，这里只描述默认那一档，
/// 避免 `string_end` 变成三十个分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quotes {
    /// 不认引号（LaTeX / Makefile）。
    None,
    /// 只有 `"`，反斜杠转义。
    Double,
    /// `"` 与 `'`，反斜杠转义。
    Both,
    /// `"` 与 `'`，不做转义处理（PowerShell / VB：反斜杠是普通字符）。
    RawBoth,
    /// `Both` 之上再认 `"""` / `'''` 三引号。
    Triple,
}

/// 语言相关的扫描参数。`line_comments` / `block_comment` 决定注释边界，
/// 三个表决定标识符归类，`quotes` 决定默认的字符串边界。
struct Spec {
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    literals: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// SQL / VB / PowerShell 这类大小写不敏感的语言。
    case_insensitive: bool,
    /// `#` 只有位于行首或空白之后才算注释（Shell / YAML / Nginx）。
    hash_needs_boundary: bool,
    quotes: Quotes,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            keywords: &[],
            types: &[],
            literals: &[],
            line_comments: &[],
            block_comment: None,
            case_insensitive: false,
            hash_needs_boundary: false,
            quotes: Quotes::None,
        }
    }
}

fn spec(lang: Lang) -> Spec {
    let (keywords, types, literals) = tables(lang);
    let (line_comments, block_comment, case_insensitive, hash_needs_boundary, quotes) =
        syntax(lang);
    Spec {
        keywords,
        types,
        literals,
        line_comments,
        block_comment,
        case_insensitive,
        hash_needs_boundary,
        quotes,
    }
}

/// 词表三元组：关键字 / 类型 / 字面量。新增语言时这里加一行，
/// 再在 [`syntax`] 里给出注释与引号规则。
fn tables(
    lang: Lang,
) -> (
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
) {
    match lang {
        Lang::Rust => (RUST_KEYWORDS, RUST_TYPES, RUST_LITERALS),
        Lang::Js => (JS_KEYWORDS, JS_TYPES, JS_LITERALS),
        Lang::Ts => (TS_KEYWORDS, TS_TYPES, JS_LITERALS),
        Lang::Python => (PYTHON_KEYWORDS, PYTHON_TYPES, PYTHON_LITERALS),
        Lang::Shell => (SHELL_KEYWORDS, &[], SHELL_LITERALS),
        Lang::Go => (GO_KEYWORDS, GO_TYPES, GO_LITERALS),
        Lang::C => (C_KEYWORDS, C_TYPES, C_LITERALS),
        Lang::Cpp => (CPP_KEYWORDS, CPP_TYPES, C_LITERALS),
        Lang::Java => (JAVA_KEYWORDS, JAVA_TYPES, C_LITERALS),
        Lang::Sql => (SQL_KEYWORDS, SQL_TYPES, SQL_LITERALS),

        Lang::Kotlin => (KOTLIN_KEYWORDS, KOTLIN_TYPES, KOTLIN_LITERALS),
        Lang::Csharp => (CSHARP_KEYWORDS, CSHARP_TYPES, CSHARP_LITERALS),
        Lang::Swift => (SWIFT_KEYWORDS, SWIFT_TYPES, SWIFT_LITERALS),
        Lang::Dart => (DART_KEYWORDS, DART_TYPES, DART_LITERALS),
        Lang::Scala => (SCALA_KEYWORDS, SCALA_TYPES, SCALA_LITERALS),
        Lang::Groovy => (GROOVY_KEYWORDS, GROOVY_TYPES, GROOVY_LITERALS),
        Lang::ObjC => (OBJC_KEYWORDS, OBJC_TYPES, OBJC_LITERALS),
        Lang::Zig => (ZIG_KEYWORDS, ZIG_TYPES, ZIG_LITERALS),

        Lang::Php => (PHP_KEYWORDS, PHP_TYPES, PHP_LITERALS),
        Lang::Ruby => (RUBY_KEYWORDS, RUBY_TYPES, RUBY_LITERALS),
        Lang::Perl => (PERL_KEYWORDS, &[], PERL_LITERALS),
        Lang::Lua => (LUA_KEYWORDS, LUA_TYPES, LUA_LITERALS),
        Lang::PowerShell => (POWERSHELL_KEYWORDS, &[], POWERSHELL_LITERALS),
        Lang::R => (R_KEYWORDS, R_TYPES, R_LITERALS),

        Lang::Elixir => (ELIXIR_KEYWORDS, ELIXIR_TYPES, ELIXIR_LITERALS),
        Lang::Haskell => (HASKELL_KEYWORDS, HASKELL_TYPES, HASKELL_LITERALS),
        Lang::Julia => (JULIA_KEYWORDS, JULIA_TYPES, JULIA_LITERALS),
        Lang::Clojure => (CLOJURE_KEYWORDS, &[], CLOJURE_LITERALS),
        Lang::Erlang => (ERLANG_KEYWORDS, &[], ERLANG_LITERALS),
        Lang::Ocaml => (OCAML_KEYWORDS, OCAML_TYPES, OCAML_LITERALS),
        Lang::Fsharp => (FSHARP_KEYWORDS, FSHARP_TYPES, FSHARP_LITERALS),
        Lang::Vb => (VB_KEYWORDS, VB_TYPES, VB_LITERALS),

        Lang::Solidity => (SOLIDITY_KEYWORDS, SOLIDITY_TYPES, SOLIDITY_LITERALS),
        Lang::GraphQl => (GRAPHQL_KEYWORDS, GRAPHQL_TYPES, GRAPHQL_LITERALS),
        Lang::Protobuf => (PROTOBUF_KEYWORDS, PROTOBUF_TYPES, PROTOBUF_LITERALS),
        Lang::Hcl => (HCL_KEYWORDS, HCL_TYPES, HCL_LITERALS),

        // 行导向的配置语言只用到注释与引号规则，关键字表供专用扫描器取用。
        Lang::Nginx => (NGINX_KEYWORDS, &[], NGINX_LITERALS),
        Lang::Dockerfile => (DOCKERFILE_KEYWORDS, &[], &[]),
        Lang::Makefile => (MAKEFILE_KEYWORDS, &[], &[]),
        _ => (&[], &[], &[]),
    }
}

/// 注释与引号规则：(行注释前缀, 块注释定界符, 大小写不敏感, `#` 需前置空白,
/// 引号规则)。
type Syntax = (
    &'static [&'static str],
    Option<(&'static str, &'static str)>,
    bool,
    bool,
    Quotes,
);

/// 注释与引号规则。
fn syntax(lang: Lang) -> Syntax {
    // `None` 在这里既可能是 `Option::None` 也可能是 `Quotes::None`，
    // 只引入需要的变体，避免遮蔽 prelude。
    use Quotes::{Both, Double, RawBoth, Triple};
    match lang {
        // 已有语言：字符串规则在 `string_end` 里按 lang 特判，这里只管注释。
        Lang::Rust | Lang::Js | Lang::Ts | Lang::Go | Lang::C | Lang::Cpp | Lang::Java => {
            (&["//"], Some(("/*", "*/")), false, false, Quotes::None)
        }
        Lang::Python => (&["#"], None, false, false, Quotes::None),
        Lang::Shell => (&["#"], None, false, true, Quotes::None),
        Lang::Sql => (&["--"], Some(("/*", "*/")), true, false, Quotes::None),

        // C 家族新成员
        Lang::Kotlin | Lang::Dart | Lang::Scala | Lang::Groovy => {
            (&["//"], Some(("/*", "*/")), false, false, Triple)
        }
        Lang::Swift => (&["//"], Some(("/*", "*/")), false, false, Double),
        Lang::Zig | Lang::Solidity | Lang::Protobuf => {
            (&["//"], Some(("/*", "*/")), false, false, Both)
        }
        // C# 的 `@"…"` 与 Objective-C 的 `@"…"` 在 `string_end` 里特判。
        Lang::Csharp | Lang::ObjC => (&["//"], Some(("/*", "*/")), false, false, Quotes::None),

        // 脚本语言
        Lang::Php => (&["//", "#"], Some(("/*", "*/")), false, false, Both),
        Lang::Ruby | Lang::R | Lang::Perl => (&["#"], None, false, false, Both),
        Lang::PowerShell => (&["#"], Some(("<#", "#>")), true, false, RawBoth),
        Lang::Lua => (&["--"], None, false, false, Quotes::None),
        Lang::Vb => (&["'"], None, true, false, RawBoth),

        // 函数式与逻辑
        Lang::Elixir | Lang::GraphQl => (&["#"], None, false, false, Triple),
        Lang::Haskell => (&["--"], Some(("{-", "-}")), false, false, Double),
        Lang::Julia => (&["#"], Some(("#=", "=#")), false, false, Triple),
        Lang::Clojure => (&[";"], None, false, false, Double),
        Lang::Erlang => (&["%"], None, false, false, Both),
        Lang::Ocaml => (&[], Some(("(*", "*)")), false, false, Both),
        Lang::Fsharp => (&["//"], Some(("(*", "*)")), false, false, Triple),

        Lang::Hcl => (&["#", "//"], Some(("/*", "*/")), false, false, Double),
        Lang::Latex => (&["%"], None, false, false, Quotes::None),

        Lang::Nginx => (&["#"], None, true, true, Quotes::None),
        Lang::Dockerfile => (&["#"], None, true, true, Double),
        Lang::Makefile => (&["#"], None, false, false, Quotes::None),
        Lang::Diff => (&[], None, false, false, Quotes::None),
        _ => (&[], None, false, false, Quotes::None),
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

        // Lua 的 `--[[ … ]]` 跨行块注释：必须先于 `--` 行注释判定。
        if lang == Lang::Lua
            && code[i..].starts_with("--[[")
            && let Some(end) = lua_long_bracket_end(code, i + 2)
        {
            sink.push(i, end, TokenKind::Comment);
            i = end;
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

        if matches!(lang, Lang::C | Lang::Cpp)
            && c == '#'
            && at_line_start(code, i)
            && let Some(end) = directive_end(code, i)
        {
            sink.push(i, end, TokenKind::Macro);
            i = end;
            continue;
        }

        if matches!(
            lang,
            Lang::Python | Lang::Ts | Lang::Java | Lang::Elixir | Lang::ObjC
        ) && c == '@'
            && annotation_allowed(lang, code, i)
            && let Some(end) = annotation_end(code, i)
        {
            sink.push(i, end, TokenKind::Annotation);
            i = end;
            continue;
        }

        // `$var` / `${var}`：Shell 与沿用同一套写法的脚本语言。
        if c == '$'
            && matches!(
                lang,
                Lang::Shell | Lang::Php | Lang::Perl | Lang::PowerShell | Lang::Ruby
            )
        {
            let end = shell_var_end(code, i, lang == Lang::PowerShell);
            sink.push(i, end, TokenKind::Variable);
            i = end;
            continue;
        }

        // Ruby 的 `@ivar` / `@@cvar`。
        if lang == Lang::Ruby
            && c == '@'
            && let Some(end) = ruby_var_end(code, i)
        {
            sink.push(i, end, TokenKind::Variable);
            i = end;
            continue;
        }

        // Elixir 的原子 `:ok`：只有前面不是标识符时才算（`a: 1` 的冒号属于键）。
        if lang == Lang::Elixir
            && c == ':'
            && let Some(end) = elixir_atom_end(code, i)
        {
            sink.push(i, end, TokenKind::Literal);
            i = end;
            continue;
        }

        // LaTeX 的 `\section` / `\\` 控制序列。
        if lang == Lang::Latex && c == '\\' {
            let end = latex_command_end(code, i);
            sink.push(i, end, TokenKind::Macro);
            i = end;
            continue;
        }

        // PHP 起始标记：`<?php` / `<?=` 否则会被拆成运算符加标识符。
        if lang == Lang::Php
            && let Some(end) = php_open_tag_end(code, i)
        {
            sink.push(i, end, TokenKind::Macro);
            i = end;
            continue;
        }

        if let Some(end) = string_end(lang, &spec, code, i) {
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
        if (c == '"' || c == '\'')
            && let Some(end) = quote_end(code, i, c, true, true)
        {
            i = end;
            continue;
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
/// TS / Java / Elixir / Objective-C 只要前一个非空字符不是标识符即可。
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
        // 大小写不敏感的表（SQL / CSS / VB / PowerShell）内容全是 ASCII 小写，
        // 小写化后再查即可。
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

/// 按 [`Quotes`] 认一段普通字符串，返回相对 `rest` 的结束偏移。
fn scan_quotes(quotes: Quotes, rest: &str) -> Option<usize> {
    let c = rest.chars().next()?;
    let (delims, triple, escapes): (&[char], bool, bool) = match quotes {
        Quotes::None => return None,
        Quotes::Double => (&['"'], false, true),
        Quotes::Both => (&['"', '\''], false, true),
        Quotes::RawBoth => (&['"', '\''], false, false),
        Quotes::Triple => (&['"', '\''], true, true),
    };
    if !delims.contains(&c) {
        return None;
    }
    if triple {
        let pat = format!("{c}{c}{c}");
        if rest.starts_with(&pat) {
            return rest[3..].find(&pat).map(|p| 3 + p + 3);
        }
    }
    quote_end(rest, 0, c, escapes, false)
}

fn string_end(lang: Lang, spec: &Spec, code: &str, i: usize) -> Option<usize> {
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
        // C# 的 `@"…"` / `$@"…"` / `@$"…"` 逐字字符串：不认反斜杠，可跨行。
        Lang::Csharp => {
            if let Some(prefix) = ["$@\"", "@$\"", "@\""]
                .iter()
                .find(|p| rest.starts_with(**p))
                .map(|p| p.len())
            {
                Some(
                    rest[prefix..]
                        .find('"')
                        .map(|p| prefix + p + 1)
                        .unwrap_or(rest.len()),
                )
            } else if c == '"' || c == '\'' {
                quote_end(rest, 0, c, true, false)
            } else {
                None
            }
        }
        // Objective-C 的 `@"…"` 与 C 的引号。
        Lang::ObjC => {
            if rest.starts_with("@\"") {
                quote_end(rest, 1, '"', true, true)
            } else if c == '"' || c == '\'' {
                quote_end(rest, 0, c, true, false)
            } else {
                None
            }
        }
        // Lua 的 `[[ … ]]` 长字符串。
        Lang::Lua => {
            if rest.starts_with("[[") {
                lua_long_bracket_end(rest, 0)
            } else if c == '"' || c == '\'' {
                quote_end(rest, 0, c, true, false)
            } else {
                None
            }
        }
        _ => scan_quotes(spec.quotes, rest),
    };
    relative.map(|end| i + end)
}

pub(crate) fn number_end(code: &str, start: usize) -> usize {
    let bytes = code.as_bytes();
    let mut i = start;
    // JSON / TOML 允许前导负号，调用方只在 `-` 紧跟数字时才进来。这里必须吃掉
    // 符号：否则首字节是 `-` 时下面的循环一步都不走，原样返回 `start`，调用方的
    // `i = end` 不推进，扫描器就会死循环。
    if i < bytes.len() && matches!(bytes[i], b'-' | b'+') {
        i += 1;
    }
    if i + 1 < bytes.len()
        && bytes[i] == b'0'
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

/// `$var` / `${var}` / `$@`。`allow_colon` 让 PowerShell 的 `$env:PATH` 整段算一个变量。
pub(crate) fn shell_var_end(code: &str, start: usize, allow_colon: bool) -> usize {
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
        if is_ident_continue(c)
            || matches!(c, '@' | '*' | '#' | '?' | '$' | '!' | '-' | '0'..='9')
            || (allow_colon && c == ':')
        {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    i
}

/// Ruby 的 `@ivar` / `@@cvar`：`@` 后必须紧跟标识符首字符。
fn ruby_var_end(code: &str, start: usize) -> Option<usize> {
    let rest = &code[start + 1..];
    let (offset, rest) = match rest.strip_prefix('@') {
        Some(tail) => (2, tail),
        None => (1, rest),
    };
    rest.chars().next().filter(|c| is_ident_start(*c))?;
    let mut i = start + offset;
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

/// Elixir 原子 `:ok` / `:error`。键值对 `a: 1` 的冒号前面是标识符，不认。
fn elixir_atom_end(code: &str, start: usize) -> Option<usize> {
    let prev = code[..start].chars().next_back();
    if prev.is_some_and(is_ident_continue) {
        return None;
    }
    let rest = &code[start + 1..];
    rest.chars().next().filter(|c| is_ident_start(*c))?;
    let mut i = start + 1;
    while i < code.len() {
        let c = code[i..].chars().next()?;
        if is_ident_continue(c) || matches!(c, '?' | '!') {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    Some(i)
}

/// LaTeX 控制序列：`\section`、`\\`、`\%`。至少吃掉一个字符，保证推进。
fn latex_command_end(code: &str, start: usize) -> usize {
    let rest = &code[start + 1..];
    let Some(first) = rest.chars().next() else {
        return start + 1;
    };
    if !first.is_alphabetic() {
        return start + 1 + first.len_utf8();
    }
    let mut i = start + 1 + first.len_utf8();
    while i < code.len() {
        let c = code[i..].chars().next().unwrap();
        if c.is_alphabetic() {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    i
}

/// PHP 起始标记 `<?php` / `<?=` / `<?`。
fn php_open_tag_end(code: &str, start: usize) -> Option<usize> {
    let rest = &code[start..];
    if rest.starts_with("<?php") {
        Some(start + 5)
    } else if rest.starts_with("<?=") {
        Some(start + 3)
    } else if rest.starts_with("<?") {
        Some(start + 2)
    } else {
        None
    }
}

/// Lua 长括号结构 `[[…]]` / `--[[…]]`：`start` 指向第一个 `[`。
fn lua_long_bracket_end(code: &str, start: usize) -> Option<usize> {
    let rest = code.get(start..)?;
    let body = rest.strip_prefix("[[")?;
    Some(start + 2 + body.find("]]").map(|p| p + 2).unwrap_or(body.len()))
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

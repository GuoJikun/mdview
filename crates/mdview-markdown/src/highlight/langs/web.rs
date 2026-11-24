//! JSON / HTML / CSS 扫描器。

use super::super::lexer::{
    Sink, code_tokens, contains, is_ident_continue, is_ident_start, is_operator, number_end,
    quote_end, skip_space,
};
use super::super::{Lang, Token, TokenKind};
use super::code::CSS_LITERALS;

// ---- JSON -------------------------------------------------------------

pub(crate) fn json_tokens(code: &str, base: usize) -> Vec<Token> {
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

pub(crate) fn html_tokens(code: &str, base: usize) -> Vec<Token> {
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

pub(crate) fn css_tokens(code: &str, base: usize) -> Vec<Token> {
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

//! YAML / TOML 扫描器，共用逐行解析与标量着色。

use super::super::lexer::{
    Sink, for_each_line, is_ident_continue, is_ident_start, number_end, quote_end,
};
use super::super::{Token, TokenKind};

pub(crate) fn yaml_tokens(code: &str, base: usize) -> Vec<Token> {
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

pub(crate) fn toml_tokens(code: &str, base: usize) -> Vec<Token> {
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

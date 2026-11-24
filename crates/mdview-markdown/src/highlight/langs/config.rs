//! Dockerfile / Makefile / Nginx 的行导向扫描器。
//!
//! 这三种格式的语义由「一行里的第一个词」决定（指令、目标、赋值），通用词法器
//! 分不清 `RUN` 与普通标识符，所以按行解析；值部分再复用通用的引号与变量扫描，
//! 保证与 Shell 那套 `$var` / `"…"` 行为一致。

use super::super::lexer::{
    Sink, contains, for_each_line, is_ident_continue, is_ident_start, number_end, quote_end,
    shell_var_end,
};
use super::super::{Lang, Token, TokenKind};
use super::code::{DOCKERFILE_KEYWORDS, MAKEFILE_KEYWORDS, NGINX_KEYWORDS, NGINX_LITERALS};

pub(crate) fn tokens(lang: Lang, code: &str, base: usize) -> Vec<Token> {
    match lang {
        Lang::Dockerfile => dockerfile_tokens(code, base),
        Lang::Makefile => makefile_tokens(code, base),
        Lang::Nginx => nginx_tokens(code, base),
        _ => Vec::new(),
    }
}

// ---- Dockerfile -------------------------------------------------------

pub(crate) fn dockerfile_tokens(code: &str, base: usize) -> Vec<Token> {
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
        // 第一个纯字母词是指令；大小写不敏感（`from` 也合法）。
        let name_end = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let word = &rest[..name_end];
        if !word.is_empty() && contains(DOCKERFILE_KEYWORDS, word, true) {
            sink.push(
                start + indent,
                start + indent + name_end,
                TokenKind::Keyword,
            );
        }
        scan_values(&mut sink, &rest[name_end..], start + indent + name_end);
    });
    sink.finish()
}

// ---- Makefile ---------------------------------------------------------

pub(crate) fn makefile_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    for_each_line(code, |start, line| {
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        if rest.is_empty() {
            return;
        }
        // Make 没有字符串概念，`#` 出现即为注释（行首或行内）。
        if rest.starts_with('#') {
            sink.push(start + indent, start + line.len(), TokenKind::Comment);
            return;
        }
        // 配方行：以 tab 开头，前导 `@` / `-` / `+` 是执行修饰符。
        if line.starts_with('\t') {
            let mut pos = line.len() - line.trim_start().len();
            while let Some(c) = line[pos..].chars().next() {
                if matches!(c, '@' | '-' | '+') {
                    sink.push(start + pos, start + pos + 1, TokenKind::Operator);
                    pos += 1;
                } else {
                    break;
                }
            }
            scan_values(&mut sink, &line[pos..], start + pos);
            return;
        }
        // `NAME = value` / `NAME := value` / `NAME ?= value` / `NAME += value`
        if let Some((name_end, op_start, op_end)) = assignment(rest) {
            sink.push(
                start + indent,
                start + indent + name_end,
                TokenKind::Attribute,
            );
            sink.push(
                start + indent + op_start,
                start + indent + op_end,
                TokenKind::Operator,
            );
            scan_values(&mut sink, &rest[op_end..], start + indent + op_end);
            return;
        }
        // `target: deps`
        if let Some(colon) = rest.find(':') {
            let name = rest[..colon].trim_end();
            if !name.is_empty() {
                sink.push(start + indent, start + indent + name.len(), TokenKind::Tag);
            }
            sink.push(
                start + indent + colon,
                start + indent + colon + 1,
                TokenKind::Operator,
            );
            scan_values(&mut sink, &rest[colon + 1..], start + indent + colon + 1);
            return;
        }
        // `include` / `export` / `define` 这类指令。
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if contains(MAKEFILE_KEYWORDS, &rest[..word_end], false) {
            sink.push(
                start + indent,
                start + indent + word_end,
                TokenKind::Keyword,
            );
            scan_values(&mut sink, &rest[word_end..], start + indent + word_end);
            return;
        }
        scan_values(&mut sink, rest, start + indent);
    });
    sink.finish()
}

/// 变量赋值：返回 `(名字长度, 操作符起点, 操作符终点)`，都相对传入切片。
fn assignment(s: &str) -> Option<(usize, usize, usize)> {
    let eq = s.find('=')?;
    let mut op_start = eq;
    if let Some(prev) = s[..op_start].chars().next_back()
        && matches!(prev, ':' | '?' | '+' | '!')
    {
        op_start -= prev.len_utf8();
    }
    let name = s[..op_start].trim_end();
    if name.is_empty() || name.contains([':', '=']) {
        return None;
    }
    Some((name.len(), op_start, eq + 1))
}

// ---- Nginx ------------------------------------------------------------

pub(crate) fn nginx_tokens(code: &str, base: usize) -> Vec<Token> {
    let mut sink = Sink::new(base);
    for_each_line(code, |start, line| {
        let mut i = 0;
        // 指令位置由结构决定：行首，或紧跟在 `{` / `}` / `;` 之后。单行写多条
        // 指令（`server { listen 80; }`）时只认行首会漏掉后面那些。
        let mut at_directive = true;
        while i < line.len() {
            let c = line[i..].chars().next().unwrap();
            if c.is_whitespace() {
                i += c.len_utf8();
                continue;
            }
            if c == '#' {
                sink.push(start + i, start + line.len(), TokenKind::Comment);
                return;
            }
            if c == '"' || c == '\'' {
                let end = quote_end(line, i, c, true, false).unwrap_or(i + 1);
                sink.push(start + i, start + end, TokenKind::String);
                i = end;
                at_directive = false;
                continue;
            }
            if c == '$' {
                let end = shell_var_end(line, i, false);
                sink.push(start + i, start + end, TokenKind::Variable);
                i = end;
                at_directive = false;
                continue;
            }
            if matches!(c, '{' | '}' | ';') {
                sink.push(start + i, start + i + 1, TokenKind::Operator);
                i += 1;
                at_directive = true;
                continue;
            }
            // `if ($host) { … }` 里的圆括号只是分隔符，不改变指令位置。
            if matches!(c, '(' | ')') {
                sink.push(start + i, start + i + 1, TokenKind::Operator);
                i += 1;
                continue;
            }
            if c.is_ascii_digit() {
                let end = number_end(line, i);
                sink.push(start + i, start + end, TokenKind::Number);
                i = end;
                at_directive = false;
                continue;
            }
            if is_ident_start(c) {
                let mut j = i + c.len_utf8();
                while j < line.len() {
                    let d = line[j..].chars().next().unwrap();
                    if is_ident_continue(d) || d == '-' {
                        j += d.len_utf8();
                    } else {
                        break;
                    }
                }
                let word = &line[i..j];
                if at_directive && contains(NGINX_KEYWORDS, word, true) {
                    sink.push(start + i, start + j, TokenKind::Keyword);
                } else if !at_directive && contains(NGINX_LITERALS, word, true) {
                    sink.push(start + i, start + j, TokenKind::Literal);
                }
                at_directive = false;
                i = j;
                continue;
            }
            i += c.len_utf8();
        }
    });
    sink.finish()
}

// ---- 值部分 -----------------------------------------------------------

/// 值部分：引号字符串、`$var` / `$(VAR)`、`--flag`、数字与行内 `#` 注释。
fn scan_values(sink: &mut Sink, s: &str, base: usize) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c == '#' && (i == 0 || s[..i].chars().next_back().is_some_and(char::is_whitespace)) {
            sink.push(base + i, base + s.len(), TokenKind::Comment);
            return;
        }
        if c == '"' || c == '\'' {
            let end = quote_end(s, i, c, true, false).unwrap_or(i + 1);
            sink.push(base + i, base + end, TokenKind::String);
            i = end;
            continue;
        }
        if c == '$' {
            let end = balanced_var_end(s, i).unwrap_or_else(|| shell_var_end(s, i, false));
            sink.push(base + i, base + end, TokenKind::Variable);
            i = end;
            continue;
        }
        if s[i..].starts_with("--") {
            let end = flag_end(s, i);
            if end > i + 2 {
                sink.push(base + i, base + end, TokenKind::Attribute);
                i = end;
                continue;
            }
        }
        if c.is_ascii_digit() {
            let end = number_end(s, i);
            sink.push(base + i, base + end, TokenKind::Number);
            i = end;
            continue;
        }
        if is_ident_start(c) {
            // 镜像名、路径这类整体跳过，避免把 `node:20` 拆成两段。
            i += c.len_utf8();
            while i < s.len() {
                let d = s[i..].chars().next().unwrap();
                if is_ident_continue(d) || matches!(d, '.' | '/' | ':' | '-') {
                    i += d.len_utf8();
                } else {
                    break;
                }
            }
            continue;
        }
        i += c.len_utf8();
    }
}

/// `$(VAR)` / `${VAR}`（Make 的变量写法，允许嵌套一层括号）。
fn balanced_var_end(s: &str, start: usize) -> Option<usize> {
    let rest = &s[start..];
    let close = if rest.starts_with("$(") {
        ')'
    } else if rest.starts_with("${") {
        '}'
    } else {
        return None;
    };
    let mut depth = 0usize;
    let mut i = start + 1;
    while i < s.len() {
        let c = s[i..].chars().next()?;
        if c == '(' || c == '{' {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += c.len_utf8();
    }
    None
}

/// `--name` / `--name=value` 里的参数名。
fn flag_end(s: &str, start: usize) -> usize {
    let mut i = start + 2;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    i
}

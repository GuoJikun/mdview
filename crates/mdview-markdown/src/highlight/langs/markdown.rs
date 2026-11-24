//! Markdown 源码自身的粗粒度高亮：标题、围栏、行内代码与强调标记。

use super::super::lexer::{Sink, for_each_line};
use super::super::{Token, TokenKind};

// ---- Markdown ---------------------------------------------------------

pub(crate) fn markdown_tokens(code: &str, base: usize) -> Vec<Token> {
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

use std::ops::Range;

/// 以空行为边界切块；围栏代码块内的空行不切分。
///
/// 返回的区间已去掉尾部换行——块尾的 `\n` 不属于任何可见片段，
/// 留在区间里会无谓地进入隐藏集。
///
/// 注意：这里不做「块级独立解析」。M0 探针实测表明按块单独喂给
/// pulldown-cmark 会丢失跨块的引用式链接定义，而全量解析 200KB 仅
/// 2.3ms，因此解析始终针对整篇文档，切块只用来划分归属。
pub fn split_blocks(src: &str) -> Vec<Range<usize>> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut current: Option<usize> = None;
    let mut in_fence: Option<(u8, usize)> = None;
    let mut offset = 0usize;

    for line in src.split_inclusive('\n') {
        let line_end = offset + line.len();
        let marker = fence_marker(line.trim_start().as_bytes());

        if let Some((open, len)) = in_fence {
            if let Some((m, n)) = marker {
                if m == open && n >= len {
                    in_fence = None;
                }
            }
        } else if let Some((m, n)) = marker {
            in_fence = Some((m, n));
        }

        let blank = in_fence.is_none() && line.trim().is_empty();
        if blank {
            if let Some(start) = current.take() {
                spans.push(trim_eol(start, offset, src));
            }
        } else if current.is_none() {
            current = Some(offset);
        }
        offset = line_end;
    }

    if let Some(start) = current {
        spans.push(trim_eol(start, src.len(), src));
    }
    spans.retain(|r| r.start < r.end);
    // 末尾空段落：源码以空行结束（含整篇为空）时补一个零长块。
    // 没有它，「在文档末尾回车」得到的光标只能贴在上一段行尾——视觉上回不到新行。
    if spans.is_empty() || ends_with_blank_line(src) {
        spans.push(src.len()..src.len());
    }
    spans
}

/// 源码是否以空行结束。空行 = 最后一个换行与它之前的换行之间只有空白。
fn ends_with_blank_line(src: &str) -> bool {
    let Some(head) = src.strip_suffix('\n') else {
        return false;
    };
    head.rsplit('\n').next().unwrap_or("").trim().is_empty()
}

fn fence_marker(bytes: &[u8]) -> Option<(u8, usize)> {
    let first = *bytes.first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let count = bytes.iter().take_while(|b| **b == first).count();
    (count >= 3).then_some((first, count))
}

fn trim_eol(start: usize, mut end: usize, src: &str) -> Range<usize> {
    let bytes = src.as_bytes();
    while end > start && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
        end -= 1;
    }
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<&str> {
        split_blocks(src).into_iter().map(|r| &src[r]).collect()
    }

    #[test]
    fn splits_on_blank_lines() {
        assert_eq!(texts("# 标题\n\n第一段\n"), vec!["# 标题", "第一段"]);
    }

    #[test]
    fn keeps_blank_lines_inside_fence() {
        let src = "```rust\nlet a = 1;\n\nlet b = 2;\n```\n";
        assert_eq!(texts(src).len(), 1);
    }

    #[test]
    fn crlf_is_trimmed() {
        assert_eq!(texts("a\r\n\r\nb\r\n"), vec!["a", "b"]);
    }

    #[test]
    fn leading_blank_lines_are_skipped() {
        assert_eq!(texts("\n\n\nabc\n"), vec!["abc"]);
    }

    #[test]
    fn empty_source_keeps_one_empty_block() {
        // 编辑器必须永远有一个可落光标的段落，否则空文档没有 caret 目标
        assert_eq!(split_blocks(""), vec![0..0]);
        assert_eq!(split_blocks("\n\n"), vec![2..2]);
    }

    #[test]
    fn trailing_blank_line_yields_empty_paragraph() {
        // "abc\n\n" 的最后一行是空行，回车后光标要有落点
        assert_eq!(texts("abc\n\n"), vec!["abc", ""]);
        assert_eq!(split_blocks("abc\n\n").last().unwrap(), &(5..5));
        // 只有一个换行时属于段尾，不额外造块
        assert_eq!(texts("abc\n"), vec!["abc"]);
    }
}

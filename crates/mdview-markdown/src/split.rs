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
    spans
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
    fn empty_source_has_no_blocks() {
        assert!(split_blocks("").is_empty());
        assert!(split_blocks("\n\n").is_empty());
    }
}

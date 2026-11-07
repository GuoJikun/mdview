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
    // 当前这段连续空行里每一行的起点，用来算「段落空行」
    let mut blank_run: Vec<usize> = Vec::new();
    // 空段落的插入点。零长块要等 `retain` 之后再并进去：
    // 那一步本意是丢掉 `trim_eol` 挤出来的空区间，会把空段落一起误杀。
    let mut paragraphs: Vec<usize> = Vec::new();

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
            blank_run.push(offset);
        } else {
            collect_blank_paragraphs(&mut paragraphs, &blank_run, spans.is_empty());
            blank_run.clear();
            if current.is_none() {
                current = Some(offset);
            }
        }
        offset = line_end;
    }

    if let Some(start) = current {
        spans.push(trim_eol(start, src.len(), src));
    }
    spans.retain(|r| r.start < r.end);
    for at in paragraphs {
        let index = spans.partition_point(|r| r.start < at);
        spans.insert(index, at..at);
    }
    // 末尾空段落：源码以空行结束（含整篇为空）时补一个零长块。
    // 没有它，「在文档末尾回车」得到的光标只能贴在上一段行尾——视觉上回不到新行。
    // 注意这里只管**收尾**的那段空行：文档中间的空段落在 collect_blank_paragraphs 里出。
    if spans.is_empty() || ends_with_blank_line(src) {
        spans.push(src.len()..src.len());
    }
    spans
}

/// 把一段**块与块之间**的连续空行翻译成空段落。
///
/// 单独一个空行是分隔符，不属于任何块——这是 CommonMark 的事实，也是停靠点
/// 稀疏的来源。但编辑器需要一个「先空着、等着打字」的段落：用户把光标停在那里，
/// 敲下的第一个字应该自成一派。空段落只能靠**两头都被空行夹住**来表达：
///
/// ```text
/// - 甲
///            ← 分隔
///            ← 空段落（光标在这里）
///            ← 分隔
/// 正文
/// ```
///
/// 少一头，打字的那一刻文字就粘到隔壁块上（`- 甲\n乙` 是列表项的懒续行）。
/// 所以三行起才认空段落：首尾两行留给分隔，中间每一行是一个空段落。
/// 文档开头那段空行不算（`leading`）——那里没有上一块可分隔，按惯例直接丢掉。
fn collect_blank_paragraphs(out: &mut Vec<usize>, run: &[usize], leading: bool) {
    if run.len() < 3 || leading {
        return;
    }
    out.extend_from_slice(&run[1..run.len() - 1]);
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

    #[test]
    fn two_blank_lines_are_just_a_separator() {
        // 两头不够夹：中间那行要是当成空段落，打第一个字就会粘到隔壁块上
        assert_eq!(texts("甲\n\n\n乙\n"), vec!["甲", "乙"]);
    }

    #[test]
    fn three_blank_lines_hold_one_empty_paragraph() {
        // 首尾留给分隔，中间那行是可落光标的空段落
        assert_eq!(texts("甲\n\n\n\n乙\n"), vec!["甲", "", "乙"]);
        assert_eq!(split_blocks("甲\n\n\n\n乙\n")[1], 5..5);
    }

    #[test]
    fn every_middle_blank_line_of_a_long_run_is_a_paragraph() {
        // 连按回车时中间可能攒下更多空行，每一行都该能停光标
        assert_eq!(texts("甲\n\n\n\n\n乙\n"), vec!["甲", "", "", "乙"]);
        assert_eq!(split_blocks("甲\n\n\n\n\n乙\n")[1..3], [5..5, 6..6]);
    }

    #[test]
    fn trailing_run_makes_only_the_end_paragraph() {
        // 收尾那段空行由 `ends_with_blank_line` 统一处理，不再重复造块
        assert_eq!(texts("甲\n\n\n\n"), vec!["甲", ""]);
        assert_eq!(split_blocks("甲\n\n\n\n").last().unwrap(), &(7..7));
    }
}

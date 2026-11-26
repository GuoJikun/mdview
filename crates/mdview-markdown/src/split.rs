use std::ops::Range;

/// 以空行为边界切块；围栏代码块内的空行不切分，缩进代码块内部的空行也不切分。
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
    // 当前块到目前为止是否「整块都是缩进代码」：块首缩进，且此后每一非空行也缩进
    let mut indented_code = false;
    // 当前这段连续空行里每一行的起点，用来算「段落空行」
    let mut blank_run: Vec<usize> = Vec::new();
    // 空段落的插入点。零长块要等 `retain` 之后再并进去：
    // 那一步本意是丢掉 `trim_eol` 挤出来的空区间，会把空段落一起误杀。
    let mut paragraphs: Vec<usize> = Vec::new();

    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut starts = Vec::with_capacity(lines.len() + 1);
    let mut at = 0usize;
    for line in &lines {
        starts.push(at);
        at += line.len();
    }
    starts.push(at);

    for (index, line) in lines.iter().enumerate() {
        let offset = starts[index];
        let marker = fence_marker(line.trim_start().as_bytes());

        if let Some((open, len)) = in_fence {
            // 闭合围栏标记之后只允许空白：`` ```rust `` 是**开启**标记的样子
            // （围栏内贴示例时很常见），带 info string 的一律不能当闭合。
            if let Some((m, n, bare)) = marker
                && m == open
                && n >= len
                && bare
            {
                in_fence = None;
            }
        } else if let Some((m, n, _)) = marker {
            in_fence = Some((m, n));
        }

        let blank = in_fence.is_none() && line.trim().is_empty();
        if blank {
            // 缩进代码块里的空行是块内的一部分（CommonMark：`    a\n\n    b`
            // 是一块缩进代码），切开会把后半段降级成普通段落，还会让片段区间
            // 越过块边界。判据是「本块一直是缩进代码」+「下一个非空行仍缩进」。
            if in_fence.is_none() && indented_code && next_line_stays_indented(&lines, index + 1) {
                continue;
            }
            if let Some(start) = current.take() {
                spans.push(trim_eol(start, offset, src));
            }
            indented_code = false;
            blank_run.push(offset);
        } else {
            collect_blank_paragraphs(&mut paragraphs, &blank_run, spans.is_empty());
            blank_run.clear();
            match current {
                None => {
                    current = Some(offset);
                    indented_code = is_indented_code(line);
                }
                // 中途出现非缩进行（懒续行、围栏闭合等），本块就不再是纯代码块
                Some(_) if !is_indented_code(line) => indented_code = false,
                Some(_) => {}
            }
        }
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

/// 这一行是否像缩进代码块的正文：缩进 4 个空格或一个制表符。
fn is_indented_code(line: &str) -> bool {
    if line.trim().is_empty() {
        return false;
    }
    let body = line.trim_start_matches(' ');
    body.starts_with('\t') || line.len() - body.len() >= 4
}

/// 从 `from` 起跳过空行后，下一行是否仍缩进（只有这样才能把空行留在代码块里）。
/// 到文件末尾不算：收尾的空行要留给「末尾空段落」那条规则。
fn next_line_stays_indented(lines: &[&str], from: usize) -> bool {
    lines
        .get(from..)
        .and_then(|rest| rest.iter().find(|line| !line.trim().is_empty()))
        .is_some_and(|line| is_indented_code(line))
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

/// 行首的围栏标记：`(字符, 数量, 标记之后是否只剩空白)`。
///
/// 第三个分量区分「开启」与「闭合」：CommonMark 规定闭合围栏后面只能是空白，
/// 而开启围栏允许带 info string（`` ```rust ``）。只看反引号数量会把围栏里
/// 贴的示例代码当成结束标记，块边界与解析器对不上。
fn fence_marker(bytes: &[u8]) -> Option<(u8, usize, bool)> {
    let first = *bytes.first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let count = bytes.iter().take_while(|b| **b == first).count();
    if count < 3 {
        return None;
    }
    let bare = bytes[count..]
        .iter()
        .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'));
    Some((first, count, bare))
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

    #[test]
    fn blank_line_inside_indented_code_does_not_split() {
        // CommonMark：`    a\n\n    b` 是一块缩进代码，空行属于块内
        assert_eq!(texts("    a\n\n    b\n"), vec!["    a\n\n    b"]);
        assert_eq!(texts("    a\n\n\n    b\n"), vec!["    a\n\n\n    b"]);
    }

    #[test]
    fn tab_indented_code_keeps_its_blank_lines() {
        assert_eq!(texts("\ta\n\n\tb\n"), vec!["\ta\n\n\tb"]);
    }

    #[test]
    fn blank_line_after_indented_code_still_splits() {
        // 下一行不缩进，代码块到此为止
        assert_eq!(texts("    a\n\n正文\n"), vec!["    a", "正文"]);
        // 块首不是缩进行时，不能因为两侧都缩进就并块
        assert_eq!(texts("正文\n   续行\n\n   代码\n").len(), 2);
    }

    #[test]
    fn fence_with_info_string_does_not_close_outer_fence() {
        // 围栏里贴示例时很常见：`` ```rust `` 带 info string，不是闭合标记
        let src = "````\n```rust\nlet a = 1;\n\n正文\n````\n";
        assert_eq!(texts(src).len(), 1, "{:?}", texts(src));
        // 真正的闭合（标记后只有空白）照旧生效
        let src = "````\n```rust\nlet a = 1;\n````\n\n正文\n";
        assert_eq!(texts(src), vec!["````\n```rust\nlet a = 1;\n````", "正文"]);
    }
}

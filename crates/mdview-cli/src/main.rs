//! 无头入口：验证 core / markdown / app 三层链路，不依赖任何 GUI。
//!
//!   mdview <file> [outline|stats|blocks|spans|layout|html]
//!   mdview demo

use std::{env, fs, process};

use log::{error, info, warn};
use mdview_app::{
    AppState, CollectingSink, Effect, InMemoryFileStore, Message, RenderSink, ViewMode,
};
use mdview_core::Document;
use mdview_markdown::{Block, BlockKind, DocumentLayout, MarkdownParser, ParseResult};

const DEMO: &str = "# 标题\n\n你好 **world**\n\n```rust\nfn main() {}\n```\n";

fn main() {
    // 日志目录固定在可执行文件同级，装到只读目录时会回落到 LOCALAPPDATA
    let log_dir = mdview_app::init_logging_default().ok();
    let args: Vec<String> = env::args().skip(1).collect();
    info!(
        "启动 mdview v{} args={args:?} 日志目录={}",
        env!("CARGO_PKG_VERSION"),
        log_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "<不可用>".into())
    );

    if args.is_empty() {
        warn!("未提供参数，打印用法");
        usage();
        return;
    }

    if args[0] == "demo" {
        if args.get(1).map(String::as_str) == Some("edit") {
            run_edit_demo();
        } else {
            run_demo();
        }
        return;
    }

    let command = args.get(1).map(String::as_str).unwrap_or("outline");
    let path = &args[0];
    let content = fs::read_to_string(path).unwrap_or_else(|err| {
        error!("读取失败 {path}: {err}");
        eprintln!("读取失败 {path}: {err}");
        process::exit(1);
    });
    let parsed = MarkdownParser::gfm().parse(&content);

    match command {
        "outline" => print_outline(&parsed),
        "stats" => print_stats(&content, &parsed),
        "blocks" => print_blocks(&parsed),
        "spans" => print_spans(&content, &parsed),
        "layout" => print_layout(&content, &parsed),
        "preview" => write_preview(path, &content),
        "html" => print!("{}", parsed.to_html(&content)),
        other => {
            warn!("未知子命令 {other}");
            usage();
            process::exit(2);
        }
    }
    info!("命令 {command} 处理完成：{}", path);
}

fn print_outline(parsed: &ParseResult) {
    if parsed.outline.is_empty() {
        println!("(无标题)");
        return;
    }
    for item in &parsed.outline {
        println!(
            "{}{}  [byte {}]",
            "  ".repeat(item.level.saturating_sub(1) as usize),
            item.title,
            item.byte_offset
        );
    }
}

fn print_stats(content: &str, parsed: &ParseResult) {
    let doc = Document::new(content);
    let spans: usize = parsed.blocks.iter().map(|b| b.spans.len()).sum();
    let hidden: usize = parsed.blocks.iter().map(|b| b.hidden.len()).sum();
    let hidden_bytes: usize = parsed
        .blocks
        .iter()
        .flat_map(|b| b.hidden.iter())
        .map(|r| r.len())
        .sum();

    println!("字符数     : {}", doc.len_chars());
    println!("字节数     : {}", doc.len_bytes());
    println!("块数       : {}", parsed.blocks.len());
    println!("可见片段   : {spans}");
    println!("隐藏区间   : {hidden}（{hidden_bytes} 字节）");
    println!("大纲条目   : {}", parsed.outline.len());
    for (label, pred) in kind_table() {
        let n = parsed.count(pred);
        if n > 0 {
            println!("  {label}: {n}");
        }
    }
}

fn kind_table() -> Vec<(&'static str, fn(&BlockKind) -> bool)> {
    vec![
        ("heading", |k| matches!(k, BlockKind::Heading(_))),
        ("paragraph", |k| matches!(k, BlockKind::Paragraph)),
        ("list", |k| matches!(k, BlockKind::List { .. })),
        ("blockquote", |k| matches!(k, BlockKind::BlockQuote)),
        ("codeblock", |k| matches!(k, BlockKind::CodeBlock { .. })),
        ("table", |k| matches!(k, BlockKind::Table)),
        ("rule", |k| matches!(k, BlockKind::Rule)),
        ("html", |k| matches!(k, BlockKind::Html)),
    ]
}

fn print_blocks(parsed: &ParseResult) {
    for block in &parsed.blocks {
        let preview: String = block.plain_text().chars().take(36).collect();
        println!(
            "#{:<3} {:<12} bytes {:>4}..{:<4} spans={:<3} hidden={:<3} {preview}",
            block.id.0,
            block.kind.label(),
            block.byte.start,
            block.byte.end,
            block.spans.len(),
            block.hidden.len()
        );
    }
}

/// 逐块打印投影结果：⟦⟧ 内是被折叠的标记。
fn print_spans(content: &str, parsed: &ParseResult) {
    for block in &parsed.blocks {
        println!(
            "\n#{} {} bytes {}..{}",
            block.id.0,
            block.kind.label(),
            block.byte.start,
            block.byte.end
        );
        println!("  投影 {}", annotate(content, block));
        for span in &block.spans {
            println!(
                "    {:?} {:>4}..{:<4} {:?}",
                span.kind, span.byte.start, span.byte.end, span.text
            );
        }
    }
}

/// 逐块打印停靠点表：`列` 是视觉列，`@` 是源码字节。
/// 同一列连续出现两次，说明这里有两个停靠点——中间隔着被折叠的标记。
fn print_layout(content: &str, parsed: &ParseResult) {
    let doc = DocumentLayout::build(content, parsed);
    println!(
        "全文档：停靠点 {} 个，视觉列 {} 列",
        doc.stops(),
        doc.width()
    );
    for block in &parsed.blocks {
        let Some(layout) = doc.layout(block.id) else {
            continue;
        };
        println!(
            "\n#{} {} bytes {}..{}  列宽 {}  停靠点 {}",
            block.id.0,
            block.kind.label(),
            block.byte.start,
            block.byte.end,
            layout.width(),
            layout.len()
        );
        for (index, stop) in layout.stops().iter().enumerate() {
            let end = layout
                .stops()
                .get(index + 1)
                .map(|next| next.byte)
                .unwrap_or(block.byte.end)
                .min(block.byte.end);
            let label = match block.hidden_at(stop.byte) {
                Some(hidden) => format!(
                    "⟦{}⟧",
                    content[stop.byte..block.hidden[hidden].end].replace('\n', "⏎")
                ),
                None => match content.get(stop.byte..end).and_then(|s| s.chars().next()) {
                    Some('\n') => "⏎".to_owned(),
                    Some(c) => c.to_string(),
                    None => "<块尾>".to_owned(),
                },
            };
            let shared = if index > 0 && layout.stops()[index - 1].visual == stop.visual {
                "  ← 同列"
            } else {
                ""
            };
            println!("  列 {:<3} @{:<5} {label}{shared}", stop.visual, stop.byte);
        }
    }
}

fn annotate(content: &str, block: &Block) -> String {    let mut out = String::new();
    let mut cursor = block.byte.start;
    for range in &block.hidden {
        if range.start > cursor {
            out.push_str(&content[cursor..range.start]);
        }
        out.push('⟦');
        out.push_str(&content[range.clone()]);
        out.push('⟧');
        cursor = range.end;
    }
    if cursor < block.byte.end {
        out.push_str(&content[cursor..block.byte.end]);
    }
    out.replace('\n', "⏎")
}

fn run_demo() {
    let mut files = InMemoryFileStore::default();
    files.insert("demo.md", DEMO);
    let mut app = AppState::open(DEMO, Some("demo.md".into()), &files);

    let frame = app.render_frame();
    println!(
        "[open] revision={} blocks={} mode={}",
        frame.revision, frame.blocks, frame.mode
    );
    println!(
        "[outline] {:?}",
        app.parsed.outline.iter().map(|i| &i.title).collect::<Vec<_>>()
    );

    let effects = app.update(
        Message::Edit {
            span: 0..0,
            insert: "> ".into(),
        },
        &files,
    );
    println!(
        "[edit] text={:?} effects={effects:?}",
        app.document.text().lines().next()
    );

    let byte_offset = app.parsed.blocks[1].byte.start;
    let effects = app.update(Message::Click { byte_offset }, &files);
    println!("[click] byte={byte_offset} effects={effects:?}");

    let effects = app.update(Message::SetMode(ViewMode::Reading), &files);
    println!("[mode] effects={effects:?}");

    let effects = app.update(Message::Undo, &files);
    println!(
        "[undo] text={:?} effects={effects:?}",
        app.document.text().lines().next()
    );

    let effects = app.update(Message::Save, &files);
    println!("[save] effects={effects:?} dirty={}", app.dirty);

    let mut sink = CollectingSink::default();
    sink.render(&app.render_frame());
    println!(
        "[sink] frames={} revision={}",
        sink.frames.len(),
        sink.frames[0].revision
    );
}

/// 生成可交互的投影页，用浏览器核对停靠点与光标位置。
fn write_preview(path: &str, content: &str) {
    let html = mdview_markdown::preview_html(content);
    let out = std::path::Path::new(path).with_extension("preview.html");
    match fs::write(&out, html) {
        Ok(()) => {
            info!("生成投影页 {}", out.display());
            println!("已生成投影页 {}", out.display());
        }
        Err(err) => {
            error!("写入失败 {}: {err}", out.display());
            eprintln!("写入失败 {}: {err}", out.display());
            process::exit(1);
        }
    }
}

/// 键盘脚本回放：把「输入 → 事务 → 重解析 → 光标回填 → 增量帧」整条链路打印出来。
/// 这是 M3 编辑闭环的无头验证入口，不需要任何 GUI。
fn run_edit_demo() {
    let mut files = InMemoryFileStore::default();
    files.insert("edit.md", String::new());
    let mut app = AppState::open("", Some("edit.md".into()), &files);
    println!(
        "[open] 空文档：停靠点 {} 个（空段落保证光标有落点）",
        app.layout.stops()
    );

    step(
        &mut app,
        &files,
        "输入「标题」",
        Message::InsertText {
            text: "标题".into(),
        },
    );
    step(&mut app, &files, "回车分段", Message::Newline);
    step(
        &mut app,
        &files,
        "输入「正文**粗**」",
        Message::InsertText {
            text: "正文**粗**".into(),
        },
    );

    // 光标点进加粗标记内部再打字：标记不能被拆散，光标也不能被推到样式外面
    let inside = app.document.text().find('粗').unwrap_or(0);
    step(
        &mut app,
        &files,
        "点进加粗内部",
        Message::Click { byte_offset: inside },
    );
    step(
        &mut app,
        &files,
        "输入「很」",
        Message::InsertText {
            text: "很".into(),
        },
    );
    step(
        &mut app,
        &files,
        "左移一格",
        Message::MoveCaret {
            delta: -1,
            extend: false,
        },
    );
    step(&mut app, &files, "退格", Message::Backspace);
    step(&mut app, &files, "撤销", Message::Undo);

    // 删空容器：第一次退格只吃掉一个字（容器还有内容），第二次连标记一起收起。
    // 不收的话源码会变成 `****`，pulldown 已不把它当容器，四个星号就成了可见文字。
    let text = app.document.text();
    let end_of_content = text.find("很粗").map(|at| at + "很粗".len()).unwrap_or(0);
    step(
        &mut app,
        &files,
        "点到加粗词尾",
        Message::Click {
            byte_offset: end_of_content,
        },
    );
    step(&mut app, &files, "退格删掉「粗」", Message::Backspace);
    step(
        &mut app,
        &files,
        "再退格：内容删空，标记一并收起",
        Message::Backspace,
    );
    // 两步各占一次事务，撤销两次才回到最初
    step(&mut app, &files, "撤销回「**很**」", Message::Undo);
    step(&mut app, &files, "再撤销回「**很粗**」", Message::Undo);
    step(&mut app, &files, "保存", Message::Save);
}

fn step(app: &mut AppState, files: &InMemoryFileStore, label: &str, msg: Message) {
    let effects = app.update(msg, files);
    let text = app.document.text().replace('\n', "⏎");
    let caret = match app.layout.locate(app.selection.head) {
        Some((id, local)) => format!(
            "块 {} 视觉列 {:?}",
            id.0,
            app.layout.layout(id).and_then(|l| l.visual(local))
        ),
        None => "无（文档为空）".to_owned(),
    };
    println!("[{label}]");
    println!("   源码 {text:?}");
    println!("   光标 {caret}  停靠点 {}", app.selection.head);
    if let Some(Effect::Patch { patch, .. }) = effects
        .iter()
        .find(|e| matches!(e, Effect::Patch { .. }))
    {
        println!(
            "   增量 替换块 {:?} / 共 {} 块，HTML {} 字节",
            patch.replace,
            patch.new_len,
            patch.html.len()
        );
    }
    let names: Vec<&str> = effects.iter().map(effect_name).collect();
    println!("   效果 {names:?}");
}

fn effect_name(effect: &Effect) -> &'static str {
    match effect {
        Effect::ScheduleParse { .. } => "ScheduleParse",
        Effect::Render => "Render",
        Effect::Patch { .. } => "Patch",
        Effect::Persist => "Persist",
        Effect::MoveCursor { .. } => "MoveCursor",
        Effect::Selection { .. } => "Selection",
        Effect::Clipboard { .. } => "Clipboard",
        Effect::Conflict => "Conflict",
    }
}

fn usage() {
    println!("mdview <file> [outline|stats|blocks|spans|layout|preview|html]");
    println!("mdview demo        状态机与文件端到端（edit/click/undo/save）");
    println!("mdview demo edit   键盘脚本回放：输入 → 光标 → 增量帧");
}

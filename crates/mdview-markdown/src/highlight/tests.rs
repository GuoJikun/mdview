use super::*;

fn tokens(code: &str, lang: &str) -> Vec<(String, TokenKind)> {
    highlight(Some(lang), code, 0..code.len())
        .into_iter()
        .map(|t| (code[t.byte].to_owned(), t.kind))
        .collect()
}

#[test]
fn rust_marks_keywords_strings_numbers_and_comments() {
    let code = "let x = 42; // 说明\nlet s = \"hi\";";
    let found = tokens(code, "rust");
    assert!(found.contains(&("let".into(), TokenKind::Keyword)));
    assert!(found.contains(&("42".into(), TokenKind::Number)));
    assert!(found.contains(&("\"hi\"".into(), TokenKind::String)));
    assert!(
        found
            .iter()
            .any(|(t, k)| *k == TokenKind::Comment && t.starts_with("//"))
    );
}

#[test]
fn rust_lifetime_is_not_a_string() {
    let found = tokens("fn f<'a>(x: &'a str) {}", "rust");
    assert!(
        !found.iter().any(|(_, k)| *k == TokenKind::String),
        "生命周期被误判成字符串: {found:?}"
    );
}

#[test]
fn rust_macro_calls_are_macros_not_functions() {
    let code = "println!(\"hi\");\nlet v = vec![1, 2];\nmacro_rules! demo { () => {} }";
    let found = tokens(code, "rust");
    assert!(
        found.contains(&("println".into(), TokenKind::Macro)),
        "{found:?}"
    );
    assert!(
        found.contains(&("vec".into(), TokenKind::Macro)),
        "{found:?}"
    );
    assert!(
        found.contains(&("macro_rules".into(), TokenKind::Macro)),
        "{found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|(t, k)| *k == TokenKind::Function && t == "println"),
        "宏调用不该染成函数: {found:?}"
    );
}

#[test]
fn rust_attributes_are_annotations() {
    let code = "#[derive(Debug)]\nstruct S;\n#![allow(dead_code)]";
    let found = tokens(code, "rust");
    assert!(
        found.contains(&("#[derive(Debug)]".into(), TokenKind::Annotation)),
        "{found:?}"
    );
    assert!(
        found.contains(&("#![allow(dead_code)]".into(), TokenKind::Annotation)),
        "{found:?}"
    );
}

#[test]
fn unclosed_rust_attribute_does_not_swallow_the_rest() {
    let found = tokens("#[derive(Debug)\nlet x = 1;", "rust");
    assert!(
        found
            .iter()
            .any(|(t, k)| *k == TokenKind::Keyword && t == "let"),
        "未闭合属性吞掉了后续代码: {found:?}"
    );
}

#[test]
fn python_decorators_are_annotations_but_at_is_not() {
    let found = tokens("@dataclass\nclass A: pass", "python");
    assert!(
        found.contains(&("@dataclass".into(), TokenKind::Annotation)),
        "{found:?}"
    );
    let indented = tokens("class A:\n    @property\n    def x(self): pass", "python");
    assert!(
        indented.contains(&("@property".into(), TokenKind::Annotation)),
        "缩进的装饰器应被识别: {indented:?}"
    );
    let matrix = tokens("result = a @ b", "python");
    assert!(
        !matrix.iter().any(|(_, k)| *k == TokenKind::Annotation),
        "矩阵乘 `@` 不该染成注解: {matrix:?}"
    );
}

#[test]
fn ts_and_java_annotations_are_marked() {
    let ts = tokens("@Injectable()\nclass Service {}", "ts");
    assert!(
        ts.contains(&("@Injectable".into(), TokenKind::Annotation)),
        "{ts:?}"
    );
    let java = tokens(
        "@Override\npublic String toString() { return \"\"; }",
        "java",
    );
    assert!(
        java.contains(&("@Override".into(), TokenKind::Annotation)),
        "{java:?}"
    );
}

#[test]
fn c_preprocessor_directives_are_macros() {
    let found = tokens("#include <stdio.h>\n#define MAX 10", "c");
    assert!(
        found.contains(&("#include".into(), TokenKind::Macro)),
        "{found:?}"
    );
    assert!(
        found.contains(&("#define".into(), TokenKind::Macro)),
        "{found:?}"
    );
}

#[test]
fn json_keys_are_attributes_and_values_are_strings() {
    let found = tokens(r#"{"name": "mdview", "count": 3}"#, "json");
    assert!(found.contains(&("\"name\"".into(), TokenKind::Attribute)));
    assert!(found.contains(&("\"mdview\"".into(), TokenKind::String)));
    assert!(found.contains(&("3".into(), TokenKind::Number)));
}

#[test]
fn html_marks_tag_name_and_attributes() {
    let found = tokens(r#"<a class="x" href='y'>链接</a>"#, "html");
    assert!(found.contains(&("a".into(), TokenKind::Tag)));
    assert!(found.contains(&("class".into(), TokenKind::Attribute)));
    assert!(found.contains(&("\"x\"".into(), TokenKind::String)));
}

#[test]
fn css_marks_property_inside_a_block() {
    let found = tokens("body { color: red; margin: 0; }", "css");
    assert!(found.contains(&("body".into(), TokenKind::Tag)));
    assert!(found.contains(&("color".into(), TokenKind::Attribute)));
    assert!(found.contains(&("0".into(), TokenKind::Number)));
}

#[test]
fn yaml_marks_key_before_colon() {
    let found = tokens("name: mdview\ncount: 3\n", "yaml");
    assert!(found.contains(&("name".into(), TokenKind::Attribute)));
    assert!(found.contains(&("count".into(), TokenKind::Attribute)));
    assert!(found.contains(&("3".into(), TokenKind::Number)));
}

#[test]
fn shell_marks_variables() {
    let found = tokens("echo $HOME ${PATH}", "bash");
    assert!(found.contains(&("$HOME".into(), TokenKind::Variable)));
    assert!(found.contains(&("${PATH}".into(), TokenKind::Variable)));
}

#[test]
fn sql_is_case_insensitive() {
    let found = tokens("SELECT id FROM users WHERE id = 1", "sql");
    assert!(found.contains(&("SELECT".into(), TokenKind::Keyword)));
    assert!(found.contains(&("FROM".into(), TokenKind::Keyword)));
}

#[test]
fn unknown_language_produces_nothing() {
    assert!(tokens("let x = 1", "brainfuck").is_empty());
}

#[test]
fn tokens_stay_inside_the_content_and_on_char_boundaries() {
    let src = "前缀\n```rust\nlet 中文 = \"值\";\n```\n后缀";
    let content = src.find("let").unwrap()..src.rfind("```").unwrap();
    let tokens = highlight(Some("rust"), src, content.clone());
    assert!(!tokens.is_empty());
    for token in &tokens {
        assert!(content.start <= token.byte.start && token.byte.end <= content.end);
        assert!(src.is_char_boundary(token.byte.start));
        assert!(src.is_char_boundary(token.byte.end));
    }
}

fn assert_invariants(src: &str, content: &Range<usize>, tokens: &[Token]) {
    let mut prev_end = content.start;
    for token in tokens {
        assert!(
            content.start <= token.byte.start,
            "token 越过 content 左界: {token:?}"
        );
        assert!(token.byte.start < token.byte.end, "空 token: {token:?}");
        assert!(
            token.byte.end <= content.end,
            "token 越过 content 右界: {token:?}"
        );
        assert!(prev_end <= token.byte.start, "token 重叠或乱序: {token:?}");
        assert!(
            src.is_char_boundary(token.byte.start),
            "起点非字符边界: {token:?}"
        );
        assert!(
            src.is_char_boundary(token.byte.end),
            "终点非字符边界: {token:?}"
        );
        prev_end = token.byte.end;
    }
}

#[test]
fn normalize_enforces_token_contract() {
    let content = 0..6;
    let tokens = vec![
        Token {
            byte: 0..2,
            kind: TokenKind::Keyword,
        },
        // 与前一个重叠，应丢弃
        Token {
            byte: 1..3,
            kind: TokenKind::String,
        },
        // 相邻同 kind，应与前一个合并
        Token {
            byte: 3..4,
            kind: TokenKind::Keyword,
        },
        Token {
            byte: 4..6,
            kind: TokenKind::Keyword,
        },
        // 越界与空片段，应丢弃
        Token {
            byte: 6..7,
            kind: TokenKind::Number,
        },
        Token {
            byte: 2..2,
            kind: TokenKind::Operator,
        },
    ];
    let out = normalize(tokens, &content);
    let got: Vec<_> = out
        .iter()
        .map(|t| (t.byte.start, t.byte.end, t.kind))
        .collect();
    assert_eq!(
        got,
        vec![(0, 2, TokenKind::Keyword), (3, 6, TokenKind::Keyword)]
    );
}

#[test]
fn scanners_respect_contract_on_tricky_unicode_inputs() {
    let cases = [
        (
            "rust",
            "let 中 = \"未闭合\nfn f<'a>() {}\nr#\"raw\"#; /* 未闭合",
        ),
        ("rust", "/* 未闭合 中文"),
        ("js", "const s = `模板 ${x} 中`;\n// 注\n/* 未闭合"),
        ("ts", "interface 人 { 名: string }\n@装饰器() class 类 {}"),
        ("python", "s = '''未闭合 中文\nx = f\"{名}!\" # 注"),
        ("json", "{\"键\": \"值\", \"bad\": \"未闭合"),
        (
            "html",
            "<a href='x'>中文</a>\n<script>let x = \"中\";</script>\n<style>a{color:red}</style>",
        ),
        ("css", "a { content: \"中\"; /* 未闭合"),
        ("yaml", "键: 值 # 注\n列表:\n  - 中文"),
        ("toml", "键 = \"值\" # 注\n[表]\n数 = 1"),
        ("markdown", "# 标题\n```rust\nlet 中 = 1;\n```\n"),
        ("sql", "SELECT 名 FROM 表 WHERE id = 1 -- 注"),
        ("bash", "echo $HOME ${路径} # 注"),
    ];
    for (lang, body) in cases {
        let src = format!("前缀中文\n```{lang}\n{body}\n```\n后缀");
        let start = src.find(body).unwrap();
        let content = start..start + body.len();
        let tokens = highlight(Some(lang), &src, content.clone());
        assert!(!tokens.is_empty(), "语言 {lang} 没有产出 token");
        assert_invariants(&src, &content, &tokens);
    }
}

#[test]
fn scanners_never_break_token_contract_on_mixed_unicode() {
    const ALPHABET: &[char] = &[
        'a', '中', '"', '\'', '`', '\\', '#', '<', '>', '/', '*', '{', '}', '[', ']', ':', '$',
        '-', '_', '\n',
    ];
    const LANGS: &[&str] = &[
        "rust", "js", "ts", "python", "json", "html", "css", "yaml", "toml", "markdown", "sql",
        "bash",
    ];
    let mut state = 0x1234_5678_9abc_def0u64;
    for len in [0usize, 1, 2, 3, 5, 8, 13, 21, 34, 55] {
        for _ in 0..20 {
            let mut src = String::new();
            for _ in 0..len {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let idx = (state >> 33) as usize % ALPHABET.len();
                src.push(ALPHABET[idx]);
            }
            for lang in LANGS {
                let content = 0..src.len();
                let tokens = highlight(Some(lang), &src, content.clone());
                assert_invariants(&src, &content, &tokens);
            }
        }
    }
}

#[test]
fn keyword_tables_are_sorted_for_binary_search() {
    use super::langs::code::*;
    let tables: &[(&str, &[&str])] = &[
        ("RUST_KEYWORDS", RUST_KEYWORDS),
        ("RUST_LITERALS", RUST_LITERALS),
        ("RUST_TYPES", RUST_TYPES),
        ("JS_KEYWORDS", JS_KEYWORDS),
        ("JS_LITERALS", JS_LITERALS),
        ("JS_TYPES", JS_TYPES),
        ("TS_KEYWORDS", TS_KEYWORDS),
        ("TS_TYPES", TS_TYPES),
        ("PYTHON_KEYWORDS", PYTHON_KEYWORDS),
        ("PYTHON_LITERALS", PYTHON_LITERALS),
        ("PYTHON_TYPES", PYTHON_TYPES),
        ("SHELL_KEYWORDS", SHELL_KEYWORDS),
        ("SHELL_LITERALS", SHELL_LITERALS),
        ("GO_KEYWORDS", GO_KEYWORDS),
        ("GO_LITERALS", GO_LITERALS),
        ("GO_TYPES", GO_TYPES),
        ("C_KEYWORDS", C_KEYWORDS),
        ("C_LITERALS", C_LITERALS),
        ("C_TYPES", C_TYPES),
        ("CPP_KEYWORDS", CPP_KEYWORDS),
        ("CPP_TYPES", CPP_TYPES),
        ("JAVA_KEYWORDS", JAVA_KEYWORDS),
        ("JAVA_TYPES", JAVA_TYPES),
        ("SQL_KEYWORDS", SQL_KEYWORDS),
        ("SQL_LITERALS", SQL_LITERALS),
        ("SQL_TYPES", SQL_TYPES),
        ("CSS_LITERALS", CSS_LITERALS),
    ];
    for &(name, list) in tables {
        assert!(
            list.windows(2).all(|w| w[0] <= w[1]),
            "{name} 未按字典序排列，二分查找会失效"
        );
    }
}

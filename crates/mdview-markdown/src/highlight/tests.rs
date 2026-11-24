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
fn plain_language_words_produce_nothing() {
    for info in ["text", "txt", "plain", "console", "log", "output"] {
        assert!(
            tokens("let x = 1", info).is_empty(),
            "`{info}` 应该按纯文本处理"
        );
    }
}

// ---- 曾把 Kotlin / C# 当成 Java 的两个语言 ---------------------------------

#[test]
fn kotlin_uses_its_own_keywords() {
    let found = tokens(
        "fun main() {\n    val xs = listOf(1)\n    when (xs) { else -> println(\"中\") }\n}",
        "kt",
    );
    for word in ["fun", "val", "when"] {
        assert!(
            found.contains(&(word.into(), TokenKind::Keyword)),
            "`{word}` 未被识别为关键字: {found:?}"
        );
    }
    assert!(
        !found
            .iter()
            .any(|(t, k)| *k == TokenKind::Keyword && t == "listOf"),
        "`listOf` 是函数调用，不该是关键字"
    );
}

#[test]
fn csharp_uses_its_own_keywords_and_verbatim_strings() {
    let found = tokens(
        "namespace N {\n    async Task<string> F() { var s = @\"C:\\中\"; return s; }\n}",
        "cs",
    );
    for word in ["namespace", "async", "var", "return"] {
        assert!(
            found.contains(&(word.into(), TokenKind::Keyword)),
            "`{word}` 未被识别为关键字: {found:?}"
        );
    }
    assert!(
        found.contains(&("Task".into(), TokenKind::Type)),
        "{found:?}"
    );
    assert!(
        found.contains(&("@\"C:\\中\"".into(), TokenKind::String)),
        "逐字字符串未被识别: {found:?}"
    );
}

#[test]
fn extension_aliases_reach_the_same_scanner() {
    assert!(tokens("interface A {}", "mts").contains(&("interface".into(), TokenKind::Keyword)));
    assert!(tokens("interface A {}", "cts").contains(&("interface".into(), TokenKind::Keyword)));
    assert!(tokens("val x = 1", "kts").contains(&("val".into(), TokenKind::Keyword)));
    assert!(tokens("a { color: red }", "sass").contains(&("color".into(), TokenKind::Attribute)));
    assert!(tokens(r#"{"a": 1}"#, "json5").contains(&("\"a\"".into(), TokenKind::Attribute)));
    assert!(tokens("def f(): pass", "pyi").contains(&("def".into(), TokenKind::Keyword)));
}

// ---- 脚本语言 ---------------------------------------------------------

#[test]
fn php_marks_open_tag_and_variables() {
    let found = tokens("<?php\n$name = '中';\necho $name;", "php");
    assert!(
        found.contains(&("<?php".into(), TokenKind::Macro)),
        "{found:?}"
    );
    assert!(
        found.contains(&("$name".into(), TokenKind::Variable)),
        "{found:?}"
    );
    assert!(
        found.contains(&("echo".into(), TokenKind::Keyword)),
        "{found:?}"
    );
}

#[test]
fn ruby_marks_instance_variables() {
    let found = tokens("class User\n  def initialize(@name)\n  end\nend", "ruby");
    assert!(
        found.contains(&("class".into(), TokenKind::Keyword)),
        "{found:?}"
    );
    assert!(
        found.contains(&("def".into(), TokenKind::Keyword)),
        "{found:?}"
    );
    assert!(
        found.contains(&("@name".into(), TokenKind::Variable)),
        "{found:?}"
    );
}

#[test]
fn lua_long_comments_span_lines() {
    let found = tokens("--[[ 说明\n多行 ]]\nlocal x = 1", "lua");
    assert!(
        found
            .iter()
            .any(|(t, k)| *k == TokenKind::Comment && t.starts_with("--[[")),
        "{found:?}"
    );
    assert!(
        found.contains(&("local".into(), TokenKind::Keyword)),
        "块注释不该吞掉后续代码: {found:?}"
    );
}

#[test]
fn powershell_variables_keep_their_scope_prefix() {
    let found = tokens("$env:PATH\nWrite-Host $name # 注", "pwsh");
    assert!(
        found.contains(&("$env:PATH".into(), TokenKind::Variable)),
        "{found:?}"
    );
    assert!(
        found.contains(&("$name".into(), TokenKind::Variable)),
        "{found:?}"
    );
}

#[test]
fn perl_and_r_mark_variables_and_operators() {
    let perl = tokens("my $name = '中'; # 注", "perl");
    assert!(
        perl.contains(&("my".into(), TokenKind::Keyword)),
        "{perl:?}"
    );
    assert!(
        perl.contains(&("$name".into(), TokenKind::Variable)),
        "{perl:?}"
    );
    let r = tokens("x <- c(1, 2)", "r");
    assert!(
        r.iter()
            .any(|(t, k)| t == "<-" && *k == TokenKind::Operator),
        "{r:?}"
    );
}

// ---- 函数式 -----------------------------------------------------------

#[test]
fn elixir_marks_atoms_and_attributes() {
    let found = tokens("@moduledoc false\ndef f, do: :ok", "elixir");
    assert!(
        found.contains(&("@moduledoc".into(), TokenKind::Annotation)),
        "{found:?}"
    );
    assert!(
        found.contains(&("def".into(), TokenKind::Keyword)),
        "{found:?}"
    );
    assert!(
        found.contains(&(":ok".into(), TokenKind::Literal)),
        "原子未被识别: {found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|(t, k)| *k == TokenKind::Literal && t == ":"),
        "`do:` 的冒号不该单独成原子"
    );
}

#[test]
fn haskell_and_clojure_use_their_own_comment_styles() {
    let hs = tokens("f x = x + 1 -- 注", "haskell");
    assert!(
        hs.iter()
            .any(|(t, k)| *k == TokenKind::Comment && t.starts_with("--")),
        "{hs:?}"
    );
    let clj = tokens("(defn f [x] x) ; 注", "clojure");
    assert!(
        clj.iter()
            .any(|(t, k)| *k == TokenKind::Comment && t.starts_with(';')),
        "{clj:?}"
    );
    assert!(
        clj.contains(&("defn".into(), TokenKind::Keyword)),
        "{clj:?}"
    );
}

// ---- 行导向格式 -------------------------------------------------------

#[test]
fn diff_marks_inserted_and_deleted_lines() {
    let code = "diff --git a/x b/x\nindex 111..222 100644\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-old\n+new\n context\n";
    let found = tokens(code, "diff");
    assert!(
        found.contains(&("+new".into(), TokenKind::Inserted)),
        "{found:?}"
    );
    assert!(
        found.contains(&("-old".into(), TokenKind::Deleted)),
        "{found:?}"
    );
    assert!(
        found.contains(&("+++ b/x".into(), TokenKind::Tag)),
        "文件头不该被当成新增行: {found:?}"
    );
    assert!(
        found.contains(&("@@ -1,2 +1,2 @@".into(), TokenKind::Annotation)),
        "{found:?}"
    );
    assert!(
        found.contains(&("diff --git a/x b/x".into(), TokenKind::Macro)),
        "{found:?}"
    );
}

#[test]
fn dockerfile_marks_directives_and_flags() {
    let found = tokens("FROM node:20\nRUN npm ci --omit=dev\n", "dockerfile");
    assert!(
        found.contains(&("FROM".into(), TokenKind::Keyword)),
        "{found:?}"
    );
    assert!(
        found.contains(&("RUN".into(), TokenKind::Keyword)),
        "{found:?}"
    );
    assert!(
        found.contains(&("--omit".into(), TokenKind::Attribute)),
        "{found:?}"
    );
}

#[test]
fn makefile_marks_targets_assignments_and_variables() {
    let found = tokens("BIN := app\nall: $(BIN)\n\t@echo done\n", "makefile");
    assert!(
        found.contains(&("BIN".into(), TokenKind::Attribute)),
        "{found:?}"
    );
    assert!(found.contains(&("all".into(), TokenKind::Tag)), "{found:?}");
    assert!(
        found.contains(&("$(BIN)".into(), TokenKind::Variable)),
        "{found:?}"
    );
    assert!(
        found.contains(&("@".into(), TokenKind::Operator)),
        "{found:?}"
    );
}

#[test]
fn nginx_marks_directives_variables_and_numbers() {
    let found = tokens(
        "server {\n    listen 80;\n    proxy_pass http://$host;\n}\n",
        "nginx",
    );
    assert!(
        found.contains(&("server".into(), TokenKind::Keyword)),
        "{found:?}"
    );
    assert!(
        found.contains(&("listen".into(), TokenKind::Keyword)),
        "{found:?}"
    );
    assert!(
        found.contains(&("80".into(), TokenKind::Number)),
        "{found:?}"
    );
    assert!(
        found.contains(&("$host".into(), TokenKind::Variable)),
        "{found:?}"
    );
    // 单行写多条指令：`;` 之后的词也算指令位置。
    let inline = tokens("server { listen 80; proxy_pass http://$host; }", "nginx");
    assert!(
        inline.contains(&("proxy_pass".into(), TokenKind::Keyword)),
        "分号后的指令未被识别: {inline:?}"
    );
}

#[test]
fn latex_marks_control_sequences() {
    let found = tokens("\\section{引言} % 注", "latex");
    assert!(
        found.contains(&("\\section".into(), TokenKind::Macro)),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(t, k)| *k == TokenKind::Comment && t.starts_with('%')),
        "{found:?}"
    );
}

#[test]
fn declaration_languages_mark_their_keywords() {
    let cases = [
        ("swift", "let x = 1"),
        ("dart", "var x = 1;"),
        ("scala", "val x = 1"),
        ("groovy", "def x = 1"),
        ("zig", "const x = 1;"),
        ("julia", "local x = 1"),
        ("graphql", "type Query { id: ID }"),
        ("protobuf", "message Msg { string a = 1; }"),
        ("hcl", "resource \"a\" \"b\" {}"),
        ("solidity", "contract C { uint x; }"),
        ("vb", "Dim x As Integer"),
    ];
    for (lang, code) in cases {
        let found = tokens(code, lang);
        assert!(
            found
                .iter()
                .any(|(_, k)| matches!(k, TokenKind::Keyword | TokenKind::Type)),
            "语言 {lang} 没有产出关键字: {found:?}"
        );
    }
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
        // 后来补齐的语言：多字节标识符、未闭合引号、块注释都要守住契约。
        ("kotlin", "fun f(x: Int) = when (x) { else -> \"中\" }"),
        ("cs", "string s = @\"C:\\中\";\n/* 未闭合"),
        ("swift", "let s = \"\"\"中\"\"\"\n// 注"),
        ("ruby", "@名 = \"中\" # 注"),
        ("php", "<?php $名 = '中'; ?>"),
        ("lua", "--[[ 未闭合 中文"),
        ("pwsh", "$env:路 = \"中\" # 注"),
        ("diff", "--- a/中\n+++ b/中\n@@ -1 +1 @@\n-中\n+文\n 上下文"),
        ("dockerfile", "FROM 中\nRUN echo \"中\""),
        ("makefile", "名 := 中\n\t@echo $(名)"),
        ("nginx", "server { 中 on; }"),
        ("hcl", "a = \"中\" # 注"),
        ("solidity", "contract 中 { }"),
        ("latex", "\\section{中} % 注"),
        ("haskell", "f = \"中\" -- 注"),
        ("elixir", "def f, do: :中"),
        ("clojure", "(def 中 \"值\")"),
        ("erlang", "% 注\n中() -> \"值\"."),
        ("ocaml", "(* 注 *) let 中 = \"值\""),
        ("fsharp", "let 中 = \"值\" // 注"),
        ("vb", "' 注\nDim 中 = \"值\""),
        ("r", "中 <- \"值\" # 注"),
        ("perl", "my $中 = \"值\"; # 注"),
        ("julia", "中 = \"值\" # 注"),
        ("zig", "const 中 = \"值\";"),
        ("objc", "@interface 中 : NSObject\n@end"),
        ("scala", "val 中 = \"值\""),
        ("groovy", "def 中 = \"值\""),
        ("dart", "var 中 = \"值\";"),
        ("graphql", "type 中 { id: ID }"),
        ("protobuf", "message 中 { string a = 1; }"),
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
        '-', '+', '0', '9', 'e', '_', '\n',
    ];
    const LANGS: &[&str] = &[
        "rust",
        "js",
        "ts",
        "python",
        "json",
        "html",
        "css",
        "yaml",
        "toml",
        "markdown",
        "sql",
        "bash",
        "kotlin",
        "cs",
        "swift",
        "dart",
        "scala",
        "groovy",
        "objc",
        "zig",
        "php",
        "ruby",
        "perl",
        "lua",
        "pwsh",
        "r",
        "elixir",
        "haskell",
        "julia",
        "clojure",
        "erlang",
        "ocaml",
        "fsharp",
        "vb",
        "solidity",
        "graphql",
        "protobuf",
        "hcl",
        "latex",
        "nginx",
        "dockerfile",
        "makefile",
        "diff",
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
    use super::langs::code::ALL_TABLES;
    for &(name, list) in ALL_TABLES {
        assert!(
            list.windows(2).all(|w| w[0] <= w[1]),
            "{name} 未按字典序排列，二分查找会失效"
        );
    }
}

#[test]
fn negative_numbers_advance_in_json_and_toml() {
    // 回归：JSON / TOML 只在 `-` 紧跟数字时才进 `number_end`，而它原先不认前导
    // 符号，原地返回后调用方 `i = end` 不推进，扫描器直接死循环。
    for (lang, code) in [
        ("json", "{\"ratio\": -1.5e-3, \"zero\": -0}"),
        ("toml", "ratio = -1.5e-3\ncount = -42\n"),
    ] {
        let found = tokens(code, lang);
        assert!(
            found
                .iter()
                .any(|(t, k)| *k == TokenKind::Number && t.starts_with('-')),
            "{lang} 的负数没有被当成一个整体：{found:?}"
        );
        assert!(
            found.iter().any(|(t, _)| t == "-1.5e-3"),
            "{lang} 的负浮点没覆盖完整：{found:?}"
        );
    }
}

#[test]
fn fixture_highlights_every_supported_language() {
    // 夹具是逐语言核对着色的人工样张：任何一段没产出 token，都说明该语言的
    // 词表或扫描器已经失效。新增语言时必须同时补夹具，否则这里会报数量不符。
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src-tauri/tests/fixtures/highlight-all.md"
    );
    let src = std::fs::read_to_string(path).expect("缺少 highlight-all.md 夹具");
    let parsed = crate::MarkdownParser::gfm().parse(&src);

    let mut colored = std::collections::BTreeSet::new();
    let mut blank = 0;
    for block in &parsed.blocks {
        let crate::BlockKind::CodeBlock { info } = &block.kind else {
            continue;
        };
        let word = language_word(info.as_deref()).unwrap_or("");
        let body: String = block
            .spans
            .iter()
            .map(|span| &src[span.byte.clone()])
            .collect();
        if body.trim().is_empty() {
            blank += 1;
            continue;
        }
        // 夹具里刻意留的「未知语言」与「显式纯文本」样本，本来就该原样显示。
        if matches!(word, "foobar" | "text") {
            assert!(block.tokens.is_empty(), "{word} 不该产出 token");
            continue;
        }
        assert!(
            !block.tokens.is_empty(),
            "{word} 段没有任何 token，检查词表与分段规则"
        );
        colored.insert(word.to_owned());
    }

    assert_eq!(blank, 2, "期待 python / toml 两个空块");
    assert_eq!(
        colored.len(),
        47,
        "夹具应覆盖 47 种语言，实际只有 {colored:?}"
    );
}

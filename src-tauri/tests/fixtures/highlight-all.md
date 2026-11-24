# 代码高亮语言全覆盖样张

> 用途：逐段核对 `crates/mdview-markdown/src/highlight` 对**全部 47 种**受支持语言 / 格式的着色。
> 每段都是刻意写复杂的真实风格代码，覆盖该语言的注释、字符串、数字、类型、函数、变量与
> 标点路径，用来暴露扫描器漏判、越界与「名义支持但不着色」。
>
> 用法：`mdview src-tauri/tests/fixtures/highlight-all.md`，编辑态与阅读态各看一遍。
> 新增语言时必须在这里补一段，否则等于没验证过。
>
> 编号 1–47 与 `Lang` 枚举一一对应；末尾「边界用例」不追求好看，只求不崩。

**索引**：Rust、JavaScript、TypeScript、Python、JSON、HTML、CSS、Shell、Go、C、C++、Java、
Kotlin、C#、Swift、Dart、Scala、Groovy、Objective-C、Zig、SQL、YAML、TOML、HCL、Protobuf、
GraphQL、Markdown、PHP、Ruby、Perl、Lua、PowerShell、R、Elixir、Haskell、Julia、Clojure、
Erlang、OCaml、F#、VB.NET、Solidity、LaTeX、Nginx、Dockerfile、Makefile、diff。

---

## 1. Rust

生命周期、`where` 子句、`#[derive]`、`println!`、原始字符串、二进制与下划线数字。

```rust
//! 模块级文档注释：与 `///` 同时出现，两种注释都不应吃掉后续代码。
use std::collections::HashMap;
use std::fmt::{self, Display};

/// 生命周期 + 泛型 + where：检查 `<` `>` 不会被误判成比较运算符。
#[derive(Debug, Clone, PartialEq)]
pub struct Cache<'a, K, V>
where
    K: Eq + std::hash::Hash,
{
    name: &'a str,
    inner: HashMap<K, V>,
    hits: u64,
}

impl<'a, K, V> Cache<'a, K, V>
where
    K: Eq + std::hash::Hash,
    V: Clone,
{
    pub fn new(name: &'a str) -> Self {
        Self { name, inner: HashMap::new(), hits: 0 }
    }

    pub fn get_or_insert<F>(&mut self, key: K, make: F) -> &V
    where
        F: FnOnce() -> V,
    {
        self.hits += 1;
        self.inner.entry(key).or_insert_with(make)
    }
}

impl<K, V> Display for Cache<'_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cache({}, hits={})", self.name, self.hits)
    }
}

fn main() {
    let raw = r#"{"path": "C:\tmp\data", "unicode": "中文 🦀"}"#;
    let mask = 0b1010_1010u8;
    let ratio = 1_000.5e-3;
    let msg = match mask {
        0x00..=0x7f => format!("低七位 {mask:>8b}"),
        _ => String::from("high"),
    };
    println!("{msg} {raw} {ratio}");
}
```

## 2. JavaScript

类私有字段、静态成员、生成器、`async`/`await`、可选链、正则字面量与除法、BigInt。

```js
import { readFile } from "node:fs/promises";
import path from "node:path";

const CONFIG = Object.freeze({ retries: 3, timeout: 1_500 });

/** JSDoc 注释里的 `*` 与斜杠不应吞掉下一行。 */
export class Loader {
  #cache = new Map();
  static instances = 0;

  constructor(root = process.cwd()) {
    this.root = path.resolve(root);
    Loader.instances += 1;
  }

  async *walk(dir = this.root) {
    const entries = await readFile(dir, "utf8").catch(() => "");
    for (const line of entries.split(/\r?\n/u)) {
      if (!line || line.startsWith("#")) continue;
      yield line.trim();
    }
  }

  async load(name) {
    const key = name ?? "<anonymous>";
    if (this.#cache.has(key)) return this.#cache.get(key);

    const started = Date.now();
    const body = await readFile(path.join(this.root, `${key}.json`), "utf8");
    const parsed = JSON.parse(body)?.data ?? [];
    const elapsed = (Date.now() - started) / 1_000;

    this.#cache.set(key, { parsed, elapsed, big: 9_007_199_254_740_993n });
    return this.#cache.get(key);
  }
}

const re = /^(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})$/u;
export const parseDate = (text) => re.exec(text)?.groups ?? { year: "1970" };
```

## 3. TypeScript

接口、泛型约束、枚举、装饰器、`satisfies`、联合类型、重载、`as const` 与类型守卫。

```ts
import type { IncomingMessage } from "node:http";

export type Method = "GET" | "POST" | "PATCH" | "DELETE";
export type Result<T, E = Error> = { ok: true; value: T } | { ok: false; error: E };

export enum Status {
  Idle = 0,
  Busy = 1 << 1,
  Failed = 1 << 2,
}

export interface Handler<Req = unknown, Res = unknown> {
  readonly name: string;
  handle(req: Req, ctx: Context): Promise<Res>;
}

export interface Context {
  readonly id: string;
  trace?: Record<string, string | number>;
}

const ROUTES = {
  health: "/healthz",
  users: "/api/v1/users",
} as const satisfies Record<string, `/${string}`>;

function withTiming<A extends unknown[], R>(fn: (...args: A) => Promise<R>) {
  return async (...args: A): Promise<R> => {
    const started = performance.now();
    try {
      return await fn(...args);
    } finally {
      console.debug(`took ${(performance.now() - started).toFixed(2)}ms`);
    }
  };
}

@sealed
export class Router implements Handler<IncomingMessage, Status> {
  public readonly name = "router";
  #table = new Map<string, Handler>();

  constructor(private readonly prefix: string = "/") {}

  mount(path: string, handler: Handler): this {
    this.#table.set(this.prefix + path, handler);
    return this;
  }

  async handle(req: IncomingMessage): Promise<Status> {
    const hit = this.#table.get(req.url ?? ROUTES.health);
    if (!hit) return Status.Failed;
    return (await hit.handle(req, { id: crypto.randomUUID() })) as Status;
  }
}

function sealed(target: Function, _ctx?: ClassDecoratorContext): void {
  Object.seal(target);
}
```

## 4. Python

装饰器带参数、`@dataclass`、f-string 嵌套格式说明符、海象运算符、异步生成器、字节串。

```python
from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, Iterator

CACHE: dict[str, "Record"] = {}
MAX_RETRY: int = 0xFF


def retry(times: int = 3, *, backoff: float = 0.5):
    """装饰器里的引号 ' " 与三引号 """ 都要正确闭合。"""

    def wrap(fn):
        async def inner(*args: Any, **kwargs: Any) -> Any:
            last: Exception | None = None
            for attempt in range(times):
                try:
                    return await fn(*args, **kwargs)
                except (TimeoutError, OSError) as exc:
                    last = exc
                    await asyncio.sleep(backoff * 2**attempt)
            raise RuntimeError(f"{fn.__name__} failed after {times} tries") from last

        return inner

    return wrap


@dataclass(slots=True)
class Record:
    key: str
    raw: bytes = b"\x00\xff"
    tags: list[str] = field(default_factory=list)
    score: float = 1_000.25e-2

    def __post_init__(self) -> None:
        if not self.tags:
            self.tags.append("untagged")

    def render(self, width: int = 12) -> str:
        joined = ",".join(self.tags)
        return f"{self.key:<{width}} {self.score:>8.3f} [{joined}] {self.raw!r}"


async def stream(records: Iterator[Record]) -> "asyncio.Queue[Record]":
    queue: asyncio.Queue[Record] = asyncio.Queue(maxsize=64)
    for item in records:
        if (n := len(item.tags)) > 3:
            item.tags = item.tags[: n - 1]
        await queue.put(item)
    return queue


@retry(times=5, backoff=0.25)
async def fetch(key: str) -> Record:
    if key not in CACHE:
        CACHE[key] = Record(key=key, score=0.5)
    return CACHE[key]
```

## 5. JSON

嵌套对象、数组、转义、Unicode 转义、指数、空容器与深层数组。

```json
{
  "schema": "https://example.com/mdview.schema.json",
  "version": 2,
  "enabled": true,
  "ratio": -1.5e-3,
  "threshold": 0,
  "title": "中文标题 \u4e2d\u6587 \/ slash",
  "empty": {},
  "list": [],
  "matrix": [[1, 2, 3], [4, 5, 6], [7, 8, 9]],
  "nested": {
    "a": { "b": { "c": { "d": null } } },
    "items": [
      { "id": 1, "tags": ["x", "y"], "meta": { "ok": false } },
      { "id": 2, "tags": [], "meta": { "ok": true } }
    ]
  },
  "escapes": "quote:\" backslash:\\ newline:\n tab:\t"
}
```

## 6. HTML

文档骨架、注释、布尔属性、内联 `<script>`/`<style>`、内联 SVG、实体与自闭合标签。

```html
<!DOCTYPE html>
<html lang="zh-CN" data-theme="dark">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>mdview &mdash; 代码高亮</title>
  <style>
    :root { --fg: #1f2328; --bg: #ffffff; }
    body[data-theme="dark"] { --fg: #e6edf3; --bg: #0d1117; }
    .badge::before { content: "\2713"; color: var(--accent, #2da44e); }
  </style>
</head>
<body class="markdown-body" data-line-numbers="false">
  <!-- 注释里的 <tag> 与 & 都不应影响后续属性着色 -->
  <header id="top" class="site-header">
    <h1 tabindex="-1">Hello, <em>Markdown</em></h1>
    <nav aria-label="目录">
      <a href="#syntax" title='单引号属性' rel="noopener noreferrer">语法</a>
      <a href="/download?v=1&amp;os=win">下载</a>
    </nav>
  </header>

  <main>
    <p>行内代码 <code>mdview --watch</code>、实体 &lt;tag&gt; 与表情 🦀。</p>
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M2 8l4 4 8-8" fill="none" stroke="currentColor" stroke-width="2" />
    </svg>
    <input type="checkbox" checked disabled />
  </main>

  <script type="module">
    const nodes = document.querySelectorAll("[data-line-numbers]");
    for (const node of nodes) {
      node.dataset.count = String(node.textContent.split("\n").length);
    }
  </script>
</body>
</html>
```

## 7. CSS

自定义属性、`@media`、`@keyframes`、`calc()`、渐变、伪元素、属性选择器与 `!important`。

```css
:root {
  --accent: #0969da;
  --radius: 6px;
  --font-mono: "JetBrains Mono", ui-monospace, SFMono-Regular, monospace;
  --shadow: 0 1px 3px rgb(0 0 0 / 12%), 0 8px 24px rgb(0 0 0 / 8%);
}

.markdown-body pre.md-code {
  margin: 1rem 0;
  padding: 16px;
  overflow-x: auto;
  border: 1px solid var(--border, #d0d7de);
  border-radius: var(--radius);
  box-shadow: var(--shadow);
  font: 13px/1.6 var(--font-mono);
  tab-size: 4;
}

.markdown-body .tk-keyword { color: var(--tk-keyword); font-weight: 600; }
.markdown-body .tk-inserted { background: rgb(46 160 67 / 15%); color: #1a7f37 !important; }

@media (prefers-color-scheme: dark) and (min-width: 900px) {
  .markdown-body pre.md-code {
    background: linear-gradient(180deg, #161b22 0%, #0d1117 100%);
    color: #e6edf3;
  }
}

@keyframes caret-blink {
  0%, 45% { opacity: 1; }
  50%, 95% { opacity: 0; }
  100% { opacity: 1; }
}

.caret::after {
  content: "";
  width: 2px;
  height: calc(1em - 2px);
  animation: caret-blink 1.1s steps(1, end) infinite;
}

a[href^="https://"]:not([target="_blank"])::after {
  content: " ↗";
  font-size: 0.75em;
}
```

## 8. Shell

`shebang`、`set -euo pipefail`、函数、数组、`case`、here-doc、算术、默认值与重定向。

```bash
#!/usr/bin/env bash
# 严格模式：未定义变量与管道失败都要立即退出
set -euo pipefail
IFS=$'\n\t'

readonly ROOT="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
declare -a TARGETS=("docs" "fixtures" "tests")
declare -A COUNTS=([ok]=0 [fail]=0)

log() {
  local level="$1"; shift
  printf '[%s] %s\n' "$level" "$*" >&2
}

scan() {
  local dir="$1" hits=0
  while IFS= read -r -d '' file; do
    case "$file" in
      *.md) hits=$((hits + 1)) ;;
      *.png|*.jpg) continue ;;
      *) log warn "skip $file" ;;
    esac
  done < <(find "$dir" -type f -print0 2>/dev/null)
  printf '%d\n' "$hits"
}

for name in "${TARGETS[@]}"; do
  count=$(scan "$ROOT/$name")
  COUNTS[ok]=$(( COUNTS[ok] + count ))
  log info "$name: $count files (total ${COUNTS[ok]})"
done

if [[ "${CI:-0}" == "1" ]]; then
  cat <<'REPORT'
CI 模式：跳过交互确认（here-doc 里的 $ 不会被展开）
REPORT
fi

exec 3>&1
trap 'log error "line $LINENO failed"; exit 1' ERR
```

## 9. Go

分组导入、结构体标签、泛型约束、接口、goroutine + channel、`defer`/`recover`、`iota`。

```go
package cache

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"time"
)

const (
	DefaultTTL = 5 * time.Minute
	maxEntries = 1 << 16
)

var ErrMiss = errors.New("cache: miss")

type Entry[V any] struct {
	Value   V         `json:"value" yaml:"value"`
	Expires time.Time `json:"expires_at"`
}

type Store[K comparable, V any] interface {
	Get(ctx context.Context, key K) (V, error)
	Put(ctx context.Context, key K, value V) error
}

type memory[K comparable, V any] struct {
	mu      sync.RWMutex
	ttl     time.Duration
	entries map[K]Entry[V]
}

func New[K comparable, V any](ttl time.Duration) Store[K, V] {
	if ttl <= 0 {
		ttl = DefaultTTL
	}
	return &memory[K, V]{ttl: ttl, entries: make(map[K]Entry[V], 128)}
}

func (m *memory[K, V]) Get(_ context.Context, key K) (V, error) {
	var zero V
	m.mu.RLock()
	entry, ok := m.entries[key]
	m.mu.RUnlock()
	if !ok || time.Now().After(entry.Expires) {
		return zero, fmt.Errorf("%w: %v", ErrMiss, key)
	}
	return entry.Value, nil
}

func (m *memory[K, V]) Put(ctx context.Context, key K, value V) (err error) {
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("put panicked: %v", r)
		}
	}()
	select {
	case <-ctx.Done():
		return ctx.Err()
	default:
		m.mu.Lock()
		m.entries[key] = Entry[V]{Value: value, Expires: time.Now().Add(m.ttl)}
		m.mu.Unlock()
	}
	return nil
}
```

## 10. C

预处理指令、多行宏、`#ifdef`、`typedef struct`、指针与位运算、两种注释、字符与转义。

```c
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define ARRAY_LEN(a) (sizeof(a) / sizeof((a)[0]))
#define CLAMP(x, lo, hi) \
    ((x) < (lo) ? (lo) : (x) > (hi) ? (hi) : (x))

#ifdef NDEBUG
#  define TRACE(fmt, ...) ((void)0)
#else
#  define TRACE(fmt, ...) fprintf(stderr, "[trace] " fmt "\n", __VA_ARGS__)
#endif

typedef struct node {
    char name[32];
    uint32_t flags;
    struct node *next;
} node_t;

/* 块注释：里面的 // 与 " 都不应改变扫描状态 */
static int count_marked(const node_t *head, uint32_t mask)
{
    int total = 0;
    for (const node_t *it = head; it != NULL; it = it->next) {
        if (it->flags & mask) {
            total++;
        } else if (it->name[0] == '\0' || it->name[0] == '\\') {
            TRACE("empty %u", it->flags);
        }
    }
    return total;
}

int main(void) {
    node_t nodes[3] = {
        { .name = "alpha", .flags = 0x01u, .next = &nodes[1] },
        { .name = "beta", .flags = 0b0010u, .next = &nodes[2] },
        { .name = "gamma", .flags = ~0u, .next = NULL },
    };
    uint32_t mask = 1u << 1;
    int n = count_marked(nodes, mask);
    printf("%d of %zu (mask=0x%08X)\n", n, ARRAY_LEN(nodes), mask);
    return 0;
}
```

## 11. C++

模板 + `concept`、lambda、结构化绑定、`unique_ptr`、原始字符串、`constexpr` 与 `noexcept`。

```cpp
#include <algorithm>
#include <memory>
#include <numeric>
#include <string>
#include <vector>

namespace mdview::detail {

template <typename T>
concept Numeric = requires(T v) {
    { v + v } -> std::same_as<T>;
    { v * 2 } -> std::convertible_to<T>;
};

constexpr auto kScale = 1.5;
inline constexpr const char *kBanner = R"json({"ok": true, "path": "C:\tmp"})json";

class Pipeline final {
public:
    explicit Pipeline(std::string name, int threads = 4) noexcept
        : name_(std::move(name)), threads_(threads) {}

    template <Numeric T>
    [[nodiscard]] T sum(const std::vector<T> &values) const {
        const auto op = [scale = kScale](T acc, T v) { return acc + v * scale; };
        return std::accumulate(values.begin(), values.end(), T{0}, op);
    }

    const std::string &name() const noexcept { return name_; }

private:
    std::string name_;
    int threads_;
};

} // namespace mdview::detail

int main() {
    auto pipeline = std::make_unique<mdview::detail::Pipeline>("demo", 8);
    std::vector<double> samples{1.0, 2.5, 3.75};
    double total = pipeline->sum(samples);

    for (const auto &[index, value] : samples | std::views::enumerate) {
        std::printf("#%zu -> %.3f\n", index, value * total);
    }
    return total > 0 ? 0 : 1;
}
```

## 12. Java

注解带参数、`sealed interface`、`record`、泛型、Stream 管道、`try-with-resources`、文本块。

```java
package com.example.mdview;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.stream.Collectors;

@FunctionalInterface
interface Formatter<T> {
    String format(T value);
}

sealed interface Node permits Leaf, Branch {
    int weight();
}

record Leaf(String label, int weight) implements Node {}

record Branch(List<Node> children) implements Node {
    @Override
    public int weight() {
        return children.stream().mapToInt(Node::weight).sum();
    }
}

public final class Report {
    private static final String TEMPLATE = """
            {"nodes": %d, "weight": %d, "path": "%s"}
            """;

    @Deprecated(since = "2.0", forRemoval = false)
    public String render(Node root, Path source) throws IOException {
        int count;
        try (var lines = Files.lines(source)) {
            count = (int) lines.filter(line -> !line.isBlank()).count();
        }

        List<String> labels = switch (root) {
            case Leaf leaf -> List.of(leaf.label());
            case Branch branch -> branch.children().stream()
                    .map(Node::toString)
                    .collect(Collectors.toList());
        };

        return TEMPLATE.formatted(count, root.weight(), String.join("/", labels));
    }
}
```

## 13. Kotlin

`data class`、`sealed interface`、`when`、扩展函数、`suspend`、字符串模板、空安全、三引号。

```kotlin
package com.example.mdview

import kotlinx.coroutines.delay
import kotlin.time.Duration.Companion.milliseconds

data class Block(val id: Int, val kind: String, val text: String)

sealed interface ParseResult {
    data class Ok(val blocks: List<Block>) : ParseResult
    data class Err(val line: Int, val reason: String) : ParseResult
}

const val MAX_BLOCKS = 1_000

fun ParseResult.firstOrNull(): Block? = when (this) {
    is ParseResult.Ok -> blocks.firstOrNull()
    is ParseResult.Err -> null
}

private fun String.normalize(): String =
    replace("\r\n", "\n").trimEnd()

suspend fun parse(source: String): ParseResult {
    val text = source.normalize()
    if (text.isEmpty()) return ParseResult.Err(0, "empty document")

    val blocks = text.lineSequence()
        .mapIndexedNotNull { index, line ->
            if (line.isBlank()) null
            else Block(index, "paragraph", line.trim())
        }
        .take(MAX_BLOCKS)
        .toList()

    val header = """
        |# 解析完成
        |blocks=${blocks.size}, first=${blocks.firstOrNull()?.text}
    """.trimMargin()

    println(header)
    delay(5.milliseconds)
    return ParseResult.Ok(blocks)
}

class Cache<T : Any>(private val capacity: Int = 64) {
    private lateinit var stats: MutableMap<String, Int>

    init {
        require(capacity > 0) { "capacity 必须为正，实际 $capacity" }
        stats = mutableMapOf()
    }

    companion object {
        fun <T : Any> of(items: List<T>): Cache<T> = Cache(items.size.coerceAtLeast(1))
    }
}
```

## 14. C#

命名空间、`#region`、`record`、特性、`async`/`await`、LINQ、`@"..."` 与 `$@"..."`、模式匹配。

```csharp
#region usings
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Threading.Tasks;
#endregion

namespace Mdview.Core;

[Obsolete("改用 Document.LoadAsync")]
public sealed record Document(string Path, int Blocks, string? Title)
{
    public bool IsEmpty => Blocks == 0;
}

public interface IStore<T> where T : class
{
    Task<T?> FindAsync(string key, CancellationToken token = default);
}

public class FileStore : IStore<Document>
{
    private readonly Dictionary<string, Document> _cache = new(StringComparer.OrdinalIgnoreCase);
    private const string Root = @"C:\docs\fixtures";
    private static readonly string[] Extensions = { ".md", ".markdown" };

    public async Task<Document?> FindAsync(string key, CancellationToken token = default)
    {
        if (_cache.TryGetValue(key, out var cached)) return cached;

        var path = Path.Combine(Root, $"{key}.md");
        if (!File.Exists(path)) return null;

        var json = await File.ReadAllTextAsync(path, token);
        var title = JsonDocument.Parse(json).RootElement
            .TryGetProperty("title", out var node) ? node.GetString() : null;

        var doc = new Document(path, Blocks: json.Length, title);
        _cache[key] = doc;
        return doc;
    }

    public IEnumerable<Document> Top(int count) => _cache.Values
        .Where(doc => doc is { IsEmpty: false, Title: not null })
        .OrderByDescending(doc => doc.Blocks)
        .Take(count);

    public static string Describe(object value) => value switch
    {
        Document { Blocks: 0 } => "empty",
        Document(var p, var b, _) when b > 100 => $@"大文件 {p} ({b} 块)",
        _ => "unknown",
    };
}
```

## 15. Swift

协议、带关联值的枚举、扩展、`guard`、尾随闭包、属性包装器、`@objc`、多行字符串插值。

```swift
import Foundation

@objc protocol Renderable {
    func render(width: Int) -> String
}

enum Block: Equatable {
    case heading(level: Int, text: String)
    case code(language: String?, body: String)
    case list(items: [String], ordered: Bool)
}

extension Block: Renderable {
    func render(width: Int) -> String {
        switch self {
        case let .heading(level, text):
            return String(repeating: "#", count: level) + " " + text
        case let .code(language, body):
            return "```\(language ?? "text")\n\(body)\n```"
        case let .list(items, ordered):
            return items.enumerated()
                .map { index, item in
                    ordered ? "\(index + 1). \(item)" : "- \(item)"
                }
                .joined(separator: "\n")
        }
    }
}

final class Document {
    @Published private(set) var blocks: [Block] = []
    let title: String

    init(title: String, blocks: [Block] = []) {
        self.title = title
        self.blocks = blocks
    }

    func firstCode() -> String? {
        guard let block = blocks.first(where: { if case .code = $0 { return true } else { return false } })
        else { return nil }

        if case let .code(_, body) = block {
            return body.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        return nil
    }

    lazy var summary: String = """
        标题：\(title)
        块数：\(blocks.count)
        首行：\(blocks.first.map { "\($0)" } ?? "无")
        """
}

let doc = Document(title: "样例", blocks: [
    .heading(level: 2, text: "代码高亮"),
    .list(items: ["Rust", "Swift"], ordered: false),
])
print(doc.blocks.map { $0.render(width: 80) }.joined(separator: "\n\n"))
```

## 16. Dart

空安全、`late final`、命名参数、mixin、扩展方法、`async`/`await`、级联操作与集合 `if`。

```dart
import 'dart:async';
import 'dart:convert';

const maxBlocks = 1 << 8;

mixin Timestamped {
  DateTime get createdAt;
  bool get isStale => DateTime.now().difference(createdAt).inHours > 24;
}

enum BlockKind { heading, paragraph, code }

class Block with Timestamped {
  Block({
    required this.id,
    required this.kind,
    this.language,
    String? text,
  })  : text = text?.trim() ?? '',
        createdAt = DateTime.now();

  final int id;
  final BlockKind kind;
  final String? language;
  final String text;

  @override
  final DateTime createdAt;

  Map<String, Object?> toJson() => {
        'id': id,
        'kind': kind.name,
        'lang': language ?? 'text',
        'text': text,
      };
}

extension BlockList on List<Block> {
  List<Block> get codeBlocks => where((b) => b.kind == BlockKind.code).toList();

  String describe() => map((b) => '${b.kind.name}#${b.id}').join(', ');
}

Future<String> render(List<Block> blocks) async {
  await Future<void>.delayed(const Duration(milliseconds: 12));
  final buffer = StringBuffer();
  for (final block in blocks) {
    if (block.kind == BlockKind.code) {
      buffer.writeln('```${block.language ?? 'text'}');
      buffer.writeln(block.text.isEmpty ? '<empty>' : block.text);
      buffer.writeln('```');
    } else {
      buffer.writeln(block.text);
    }
  }
  return buffer.toString();
}

Future<void> main() async {
  final blocks = [
    Block(id: 1, kind: BlockKind.heading, text: '  标题  '),
    if (maxBlocks > 0) Block(id: 2, kind: BlockKind.code, language: 'dart', text: 'void main() {}'),
  ];
  final json = jsonEncode(blocks.map((b) => b.toJson()).toList());
  print(await render(blocks));
  print(json.length);
}
```

## 17. Scala

`case class`、`enum`、`given`、扩展方法、for 推导、模式匹配、`s"..."` 插值、`Option` 链。

```scala
package com.example.mdview

import scala.collection.mutable
import scala.util.{Failure, Success, Try}

enum Kind(val label: String):
  case Heading(level: Int) extends Kind(s"h$level")
  case Code(language: String) extends Kind(s"code:$language")
  case Plain extends Kind("p")

case class Block(id: Int, kind: Kind, text: String):
  def isEmpty: Boolean = text.isBlank
  def preview(width: Int = 24): String =
    if text.length <= width then text else s"${text.take(width)}…"

trait Store[F[_]]:
  def put(key: String, block: Block): F[Unit]
  def get(key: String): F[Option[Block]]

given Ordering[Block] = Ordering.by((b: Block) => (b.kind.ordinal, b.id))

extension (blocks: List[Block])
  def codeOnly: List[Block] = blocks.filter(_.kind match
    case Kind.Code(_) => true
    case _            => false
  )

  def summary: String =
    blocks.groupBy(_.kind).view.mapValues(_.size).toMap.mkString(", ")

object Report:
  private val cache = mutable.Map.empty[String, Block]
  val MaxBlocks = 1_000

  def parse(source: String): Try[List[Block]] =
    Try {
      source.linesIterator.zipWithIndex.collect {
        case (line, index) if line.trim.nonEmpty =>
          val kind =
            if line.startsWith("#") then Kind.Heading(line.takeWhile(_ == '#').length)
            else if line.startsWith("```") then Kind.Code("text")
            else Kind.Plain
          Block(index, kind, line.trim)
      }.take(MaxBlocks).toList
    }

  def run(source: String): Unit =
    parse(source) match
      case Success(blocks) =>
        blocks.foreach(b => cache.update(b.id.toString, b))
        println(s"解析 ${blocks.size} 块，摘要 ${blocks.summary}")
      case Failure(err) =>
        System.err.println(s"失败：${err.getMessage}")

@main def entry(): Unit =
  val sample =
    """# 标题
      |```rust
      |let x = 1;
      |```
      |""".stripMargin
  Report.run(sample)
```

## 18. Groovy

`def`、闭包、GString、`@Grab`、集合操作、`switch`、多行字符串与正则斜杠字面量。

```groovy
@Grab(group = 'org.slf4j', module = 'slf4j-simple', version = '2.0.13')
import groovy.transform.Field
import java.util.regex.Pattern

@Field static final String ROOT = '/srv/docs'
@Field int counter = 0

class Scanner {
    String name
    List<String> extensions = ['.md', '.markdown']

    def scan(File dir, Closure onFile) {
        dir.eachFileRecurse { file ->
            if (file.isFile() && extensions.any { file.name.endsWith(it) }) {
                counter++
                onFile(file, file.length())
            }
        }
    }
}

def scanner = new Scanner(name: 'fixtures')
def sizes = []
scanner.scan(new File(ROOT)) { file, size ->
    sizes << size
    println "${file.name} -> ${size} bytes (total ${counter})"
}

def pattern = ~/^(\d{4})-(\d{2})-(\d{2})\.md$/
def matched = pattern.matcher('2026-11-24.md')
switch (true) {
    case matched.matches():
        println "日期：${matched.group(1)} 年"
        break
    default:
        println 'no match'
}

def summary = sizes.inject(0) { acc, value -> acc + value }
def text = """
    文件数：${sizes.size()}
    总字节：${summary}
    平均：${sizes ? summary.intdiv(sizes.size()) : 0}
""".stripIndent()

assert text.contains('总字节')
```

## 19. Objective-C

`#import`、泛型集合、属性特性、`NS_ENUM`、block、消息发送、`@autoreleasepool`、`NSString`。

```objc
#import <Foundation/Foundation.h>

#pragma mark - 类型

typedef NS_ENUM(NSInteger, MDBlockKind) {
    MDBlockKindHeading = 0,
    MDBlockKindCode,
    MDBlockKindPlain,
};

@protocol MDRenderable <NSObject>
- (nonnull NSString *)renderWithWidth:(NSInteger)width;
@end

@interface MDBlock : NSObject <NSCopying, MDRenderable>

@property (nonatomic, copy, readonly, nonnull) NSString *identifier;
@property (nonatomic, assign) MDBlockKind kind;
@property (nonatomic, copy, nullable) NSString *language;
@property (nonatomic, strong, null_resettable) NSMutableArray<NSString *> *lines;

- (instancetype)initWithIdentifier:(nonnull NSString *)identifier
                              kind:(MDBlockKind)kind NS_DESIGNATED_INITIALIZER;
- (void)enumerateLinesUsingBlock:(void (^_Nonnull)(NSString *line, NSUInteger index, BOOL *stop))block;

@end

@implementation MDBlock

- (instancetype)initWithIdentifier:(NSString *)identifier kind:(MDBlockKind)kind {
    if (self = [super init]) {
        _identifier = [identifier copy];
        _kind = kind;
        _lines = [NSMutableArray arrayWithCapacity:8];
    }
    return self;
}

- (NSString *)renderWithWidth:(NSInteger)width {
    NSString *label = self.language ?: @"text";
    NSString *body = [self.lines componentsJoinedByString:@"\n"];
    return [NSString stringWithFormat:@"```%@ (%ld)\n%@\n```", label, (long)width, body];
}

- (void)enumerateLinesUsingBlock:(void (^)(NSString *, NSUInteger, BOOL *))block {
    BOOL stop = NO;
    for (NSUInteger i = 0; i < self.lines.count && !stop; i++) {
        block(self.lines[i], i, &stop);
    }
}

- (id)copyWithZone:(NSZone *)zone {
    MDBlock *copy = [[[self class] allocWithZone:zone] initWithIdentifier:self.identifier kind:self.kind];
    copy.lines = [self.lines mutableCopy];
    return copy;
}

@end

int main(void) {
    @autoreleasepool {
        MDBlock *block = [[MDBlock alloc] initWithIdentifier:@"b-1" kind:MDBlockKindCode];
        block.language = @"objc";
        [block.lines addObjectsFromArray:@[@"int x = 0x2A;", @"printf(\"%d\\n\", x);"]];
        NSLog(@"%@", [block renderWithWidth:72]);
    }
    return EXIT_SUCCESS;
}
```

## 20. Zig

`@import`、错误联合、`try`、`defer`、`switch`、`comptime`、`orelse` 与内建函数。

```zig
const std = @import("std");
const Allocator = std.mem.Allocator;

const ParseError = error{
    EmptyDocument,
    TooManyBlocks,
    OutOfMemory,
};

const Block = struct {
    id: u32,
    kind: enum { heading, code, plain },
    text: []const u8,

    pub fn isEmpty(self: Block) bool {
        return self.text.len == 0;
    }
};

pub const Parser = struct {
    allocator: Allocator,
    max_blocks: usize = 1_000,

    pub fn init(allocator: Allocator) Parser {
        return .{ .allocator = allocator };
    }

    pub fn parse(self: *Parser, source: []const u8) ParseError![]Block {
        if (source.len == 0) return ParseError.EmptyDocument;

        var blocks = std.ArrayList(Block).init(self.allocator);
        defer blocks.deinit();

        var lines = std.mem.tokenizeScalar(u8, source, '\n');
        var index: u32 = 0;
        while (lines.next()) |line| : (index += 1) {
            const trimmed = std.mem.trim(u8, line, " \t\r");
            if (trimmed.len == 0) continue;
            if (index >= self.max_blocks) return ParseError.TooManyBlocks;

            const kind: @TypeOf(@as(Block, undefined).kind) = if (std.mem.startsWith(u8, trimmed, "#"))
                .heading
            else if (std.mem.startsWith(u8, trimmed, "```"))
                .code
            else
                .plain;

            try blocks.append(.{ .id = index, .kind = kind, .text = trimmed });
        }

        return blocks.toOwnedSlice();
    }
};

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var parser = Parser.init(allocator);
    const source =
        \\# 标题
        \\```zig
        \\const x: u8 = 0xFF;
        \\```
    ;
    const blocks = parser.parse(source) catch |err| {
        std.debug.print("parse failed: {s}\n", .{@errorName(err)});
        return;
    };
    defer allocator.free(blocks);

    const first = blocks[0] orelse return;
    std.debug.print("{d} blocks, first={s}\n", .{ blocks.len, first.text });
}
```

## 21. SQL

CTE、窗口函数、多表连接、`CASE`、子查询、两种注释、`::` 转换与参数占位符。

```sql
-- 统计每个文档的语言分布与排名
WITH recent AS (
    SELECT d.id,
           d.path,
           b.language,
           b.byte_len,
           date_trunc('day', d.updated_at) AS day
      FROM documents d
      JOIN blocks b ON b.document_id = d.id
     WHERE d.updated_at >= CURRENT_DATE - INTERVAL '30 days'
       AND b.language IS NOT NULL
),
ranked AS (
    SELECT language,
           day,
           COUNT(*)                          AS blocks,
           SUM(byte_len)::numeric / 1024     AS kib,
           ROW_NUMBER() OVER (
               PARTITION BY day
               ORDER BY COUNT(*) DESC
           )                                 AS rank
      FROM recent
     GROUP BY language, day
)
SELECT r.language,
       r.day,
       r.blocks,
       ROUND(r.kib, 2) AS kib,
       CASE
           WHEN r.rank = 1 THEN 'top'
           WHEN r.rank <= 3 THEN 'popular'
           ELSE 'other'
       END AS bucket,
       (SELECT COUNT(*) FROM blocks b2 WHERE b2.language = r.language) AS total_all_time
  FROM ranked r
 WHERE r.blocks > $1
   AND r.language NOT IN ('text', 'plain', 'log')
 ORDER BY r.day DESC, r.rank
 LIMIT 50;

/* 块注释：里面的 -- 与 ' 不应影响后续语句 */
UPDATE documents
   SET title = replace(title, '''', '’'),
       updated_at = now()
 WHERE path LIKE 'fixtures/%'
   AND title <> '';
```

## 22. YAML

锚点与合并键、块标量 `|` / `>`、缩进指示器、引号变体、标签、文档分隔与注释。

```yaml
%YAML 1.2
---
version: 2
defaults: &defaults
  retries: 3
  timeout: 1.5
  flags: [fast, safe]          # 行内序列
  tags:
    - markdown
    - highlight

service:
  <<: *defaults
  name: "mdview: worker"
  command: >
    多行折叠标量会被合并成一行，
    但这里保留逗号与冒号: 用于测试。
  banner: |
    line one
      indented line
    last line
  script: |-
    #!/bin/sh
    set -eu
    echo "done"
  env:
    MODE: 'production'
    DEBUG: false
    EMPTY: null
    PORT: 8080
    RATIO: 0.75
    HEX: 0x1F
    PATH_LIST: "/usr/local/bin:/usr/bin"
  matrix:
    - - 1
      - 2
    - - 3
      - 4
  typed: !!str 12345
...
---
# 第二个文档：键名含冒号与问号
"weird:key": "value with # hash"
question?: &q yes
alias: *q
```

## 23. TOML

表、点号键、内联表、表数组、三种字符串、日期时间、进制与浮点、注释。

```toml
# 顶层键与点号键
title = "mdview fixture"
version = "0.2.0"
enabled = true
retries = 3
ratio = -1.5e-3
mask = 0xDEAD_BEEF
size = 1_048_576

[owner]
name = "mdview"
email = 'literal@example.com'   # 字面量字符串不处理转义
joined = 2024-03-05T08:30:00Z
local_time = 08:30:00

[database]
server = "192.168.1.1"
ports = [8001, 8001, 8002]
connection_max = 5000
dotted.key = "also valid"

[database.settings]
timeout = 30
description = """
多行基本字符串：支持 \n 转义与 "${placeholder}"。
第二行保持原样。
"""

[servers.alpha]
ip = "10.0.0.1"
role = "frontend"

[servers.beta]
ip = "10.0.0.2"
role = "backend"
metadata = { zone = "cn-north", weight = 1.5 }

[[products]]
name = "Hammer"
sku = 738594937

[[products]]
name = "Nail"
sku = 284758393
color = "gray"
```

## 24. HCL

Terraform 资源、变量与校验、局部值、`for` 表达式、heredoc、动态块与依赖。

```hcl
terraform {
  required_version = ">= 1.6.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

variable "instance_count" {
  type        = number
  default     = 2
  description = "应用实例数量"

  validation {
    condition     = var.instance_count > 0 && var.instance_count <= 20
    error_message = "instance_count 必须在 1..20 之间。"
  }
}

locals {
  name_prefix = "${var.project}-${var.env}"
  common_tags = merge(var.tags, {
    ManagedBy = "terraform"
    Env       = var.env
  })
  azs = slice(data.aws_availability_zones.available.names, 0, 2)
  upper = [for s in var.services : upper(s) if length(s) > 2]
}

data "aws_ami" "base" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["al2023-ami-*-x86_64"]
  }
}

resource "aws_instance" "app" {
  count                  = var.instance_count
  ami                    = data.aws_ami.base.id
  instance_type          = "t3.micro"
  availability_zone      = local.azs[count.index % length(local.azs)]
  vpc_security_group_ids = [aws_security_group.app.id]
  user_data              = <<-EOT
    #!/bin/bash
    set -euo pipefail
    echo "${local.name_prefix}-${count.index}" >> /etc/hostname
  EOT

  dynamic "ebs_block_device" {
    for_each = var.volumes
    content {
      device_name = ebs_block_device.value.name
      volume_size = ebs_block_device.value.size
      encrypted   = true
    }
  }

  tags = merge(local.common_tags, {
    Name = format("%s-%02d", local.name_prefix, count.index + 1)
  })

  depends_on = [aws_security_group.app]
}
```

## 25. Protobuf

`proto3`、嵌套消息、枚举、`oneof`、`map`、`reserved`、选项与双向流 `rpc`。

```protobuf
syntax = "proto3";

package mdview.v1;

import "google/protobuf/timestamp.proto";
import "google/protobuf/duration.proto";

option go_package = "github.com/example/mdview/gen/mdview/v1;mdviewv1";
option java_multiple_files = true;

// 块类型：注释与行内选项都要正确着色
enum BlockKind {
  BLOCK_KIND_UNSPECIFIED = 0;
  BLOCK_KIND_HEADING = 1;
  BLOCK_KIND_CODE = 2;
  BLOCK_KIND_TABLE = 3;
}

message Block {
  reserved 7, 8;
  reserved "legacy_html";

  uint32 id = 1;
  BlockKind kind = 2;
  string text = 3;
  map<string, string> attributes = 4;
  repeated string tags = 5;

  oneof payload {
    CodeBlock code = 6;
    TableBlock table = 9;
  }

  message CodeBlock {
    string language = 1;
    uint32 byte_len = 2;
    bool truncated = 3;
  }
}

message TableBlock {
  repeated string headers = 1;
  repeated Row rows = 2;

  message Row {
    repeated string cells = 1;
  }
}

message ParseRequest {
  string document_id = 1;
  bytes source = 2;
  google.protobuf.Duration timeout = 3;
}

message ParseResponse {
  repeated Block blocks = 1;
  google.protobuf.Timestamp parsed_at = 2;
  uint64 elapsed_micros = 3;
}

service ParserService {
  rpc Parse(ParseRequest) returns (ParseResponse);
  rpc ParseStream(stream ParseRequest) returns (stream ParseResponse) {
    option idempotency_level = NO_SIDE_EFFECTS;
  }
}
```

## 26. GraphQL

SDL 的 `type` / `interface` / `union` / `enum` / `input` / 指令，加一段带变量与片段的查询。

```graphql
# Schema 定义
schema {
  query: Query
  mutation: Mutation
}

interface Node {
  id: ID!
}

type Document implements Node {
  id: ID!
  path: String!
  title: String
  blocks(first: Int = 20, after: String): BlockConnection!
  updatedAt: DateTime!
}

type Block implements Node {
  id: ID!
  kind: BlockKind!
  text: String!
  language: String
  byteLen: Int @deprecated(reason: "改用 size")
}

enum BlockKind {
  HEADING
  CODE
  TABLE
  PARAGRAPH
}

union SearchResult = Document | Block

input DocumentFilter {
  pathPrefix: String
  kinds: [BlockKind!] = [CODE, TABLE]
  updatedAfter: DateTime
}

type Query {
  document(id: ID!): Document
  search(term: String!, filter: DocumentFilter): [SearchResult!]!
}

type Mutation {
  renameDocument(id: ID!, title: String!): Document!
}

directive @auth(requires: Role = VIEWER) on FIELD_DEFINITION | OBJECT

enum Role {
  VIEWER
  EDITOR
  ADMIN
}

# 客户端查询：变量、别名、片段、内联片段与指令
query LoadDocument($id: ID!, $withBlocks: Boolean! = true, $count: Int = 10) {
  doc: document(id: $id) @auth(requires: EDITOR) {
    ...DocumentFields
    blocks(first: $count) @include(if: $withBlocks) {
      edges {
        node {
          ... on Block {
            kind
            language
            text
          }
        }
      }
    }
  }
}

fragment DocumentFields on Document {
  id
  title
  updatedAt
}
```

## 27. Markdown

外层用 4 个反引号，内层演示标题、强调、列表、表格、引用、任务列表、脚注与围栏本身。

````markdown
# 一级标题

段落里有 **加粗**、*斜体*、~~删除线~~、`行内代码` 与 [链接](https://example.com/a_(b))。

> 引用块：第一行
> 第二行，含 [引用式链接][ref] 与图片 ![logo](./logo.png "标题")。

1. 有序列表
   - 嵌套无序
     - 更深一层
2. 第二项

- [x] 已完成的任务
- [ ] 未完成的任务

| 语言 | 行数 | 备注 |
| :--- | ---: | :---: |
| Rust | 64 | 生命周期 |
| SQL  | 32 | 窗口函数 |

---

```rust
fn main() {
    println!("内层围栏不应提前结束外层");
}
```

脚注引用[^1]，以及自动链接 <https://example.com>。

[ref]: https://example.com/ref "引用标题"
[^1]: 脚注内容，含 `code` 与 **strong**。
````

## 28. PHP

`declare`、命名空间、特性、`enum`、构造器提升、`match`、箭头函数、heredoc/nowdoc、空安全。

```php
<?php
declare(strict_types=1);

namespace Mdview\Highlight;

use InvalidArgumentException;
use JsonSerializable;

#[\Attribute(\Attribute::TARGET_CLASS)]
final class Block
{
    public const MAX_BYTES = 1_000_000;

    public function __construct(
        public readonly int $id,
        public string $kind = 'paragraph',
        private ?string $language = null,
    ) {
        if ($id < 0) {
            throw new InvalidArgumentException("id 不能为负：{$id}");
        }
    }

    public function language(): string
    {
        return $this->language ??= 'text';
    }

    public function describe(int $width = 16): string
    {
        $label = str_pad($this->language(), $width, '.', STR_PAD_RIGHT);
        $snippet = mb_substr($this->text ?? '', 0, 24);

        return match (true) {
            $this->kind === 'code' => "{$label} {$snippet}",
            $this->kind === 'heading' => strtoupper($label) . " #{$this->id}",
            default => sprintf('%s (%d bytes)', $label, strlen($snippet)),
        };
    }
}

enum Kind: string
{
    case Heading = 'heading';
    case Code = 'code';

    public function label(): string
    {
        return match ($this) {
            self::Heading => '标题',
            self::Code => '代码',
        };
    }
}

$blocks = array_map(
    fn (array $row, int $i): Block => new Block($i, $row['kind'] ?? 'paragraph', $row['lang'] ?? null),
    $rows ?? [],
    array_keys($rows ?? []),
);

$sql = <<<SQL
    SELECT id, kind FROM blocks
     WHERE language = '{$blocks[0]?->language()}'
SQL;

$raw = <<<'TEXT'
    单引号 heredoc 不做变量插值：$blocks[0]
    TEXT;

echo implode(PHP_EOL, array_map(static fn (Block $b) => $b->describe(), $blocks));
echo json_encode($blocks, JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
```

## 29. Ruby

模块、`attr_accessor`、块与 `do |x|`、符号、插值、`%w[]`、`case/when`、`rescue/ensure`、heredoc。

```ruby
# frozen_string_literal: true

require 'json'
require 'set'

module Mdview
  MAX_BLOCKS = 1_000

  class Block
    attr_accessor :kind, :language
    attr_reader :id, :text

    def initialize(id:, text:, kind: :paragraph, language: nil)
      @id = id
      @text = text.to_s.strip
      @kind = kind
      @language = language
      @tags = Set.new
    end

    def code?
      kind == :code && !language.nil?
    end

    def preview(width = 24)
      text.length > width ? "#{text[0, width]}…" : text
    end

    def to_h
      { id: id, kind: kind, lang: language, text: preview, tags: @tags.to_a }
    end

    def <<(tag)
      @tags << tag.to_sym
      self
    end
  end

  class Parser
    PATTERN = /\A(?<level>#+)\s+(?<title>.+)\z/

    def initialize(source)
      @lines = source.each_line.map(&:chomp)
    end

    def parse
      raise ArgumentError, '文档为空' if @lines.empty?

      blocks = @lines.each_with_index.filter_map do |line, index|
        next if line.strip.empty?

        case line
        when PATTERN
          Block.new(id: index, text: Regexp.last_match[:title], kind: :heading)
        when /\A```(\w*)/
          Block.new(id: index, text: line, kind: :code, language: Regexp.last_match(1))
        else
          Block.new(id: index, text: line)
        end
      end

      blocks.take(MAX_BLOCKS).each_with_object([]) do |block, acc|
        acc << block << :parsed
      end
    end
  end
end

source = <<~MARKDOWN
  # 标题
  ~~~ruby
  puts "hello #{Time.now.year}"
  ~~~
MARKDOWN

begin
  blocks = Mdview::Parser.new(source).parse
  puts blocks.map { |b| b.to_h.to_json }.join("\n")
  puts "总计 #{blocks.size} 块"
rescue ArgumentError => e
  warn "解析失败：#{e.message}"
ensure
  puts 'done'
end
```

## 30. Perl

`use strict`、散列与引用、正则替换、`qw()`、`sub`、`foreach`、`=pod` 文档块与特殊变量。

```perl
#!/usr/bin/env perl
use strict;
use warnings;
use feature qw(say signatures);
use JSON::PP qw(encode_json);

my %CONFIG = (
    root     => $ENV{MARKDOWN_ROOT} // '.',
    exts     => [qw(.md .markdown .mdown)],
    max_size => 1_048_576,
);

sub scan ( $dir, $depth = 0 ) {
    opendir my $dh, $dir or die "无法打开 $dir: $!";
    my @entries = grep { $_ ne '.' && $_ ne '..' } readdir $dh;
    closedir $dh;

    my @found;
    for my $name ( sort @entries ) {
        my $path = "$dir/$name";
        next if $name =~ /^\../;

        if ( -d $path ) {
            push @found, scan( $path, $depth + 1 );
        }
        elsif ( grep { $path =~ /\Q$_\E$/ } @{ $CONFIG{exts} } ) {
            push @found, { path => $path, size => -s $path };
        }
    }
    return @found;
}

my @files = scan( $CONFIG{root} );
my $total = 0;
$total += $_->{size} for @files;

# 正则与替换：斜杠、$1 与 /x 修饰符
for my $file (@files) {
    ( my $slug = $file->{path} ) =~ s{^.*/}{}x;
    $slug =~ s/[^A-Za-z0-9._-]+/-/g;
    $file->{slug} = lc $slug;
}

say encode_json( {
    count => scalar @files,
    bytes => $total,
    files => \@files,
} );

=pod

=head1 说明

块注释式文档：C<=pod> 到 C<=cut> 之间不是代码。

=cut

exit 0;
```

## 31. Lua

`local`、元表、块注释 `--[[ ]]`、长字符串 `[[ ]]`、方法 `:`、变长参数、数值 for 与 `goto`。

```lua
-- 单行注释
local M = {}

--[[
  块注释：里面的 -- 与 " 不应改变扫描状态
  local fake = "still comment"
]]

local DEFAULT_OPTIONS = {
  retries = 3,
  ratio = 0.75,
  hex = 0xFF,
  tags = { "markdown", "highlight" },
}

local Block = {}
Block.__index = Block

function Block.new(id, text, kind)
  local self = setmetatable({}, Block)
  self.id = id
  self.text = text or ""
  self.kind = kind or "paragraph"
  self.tags = {}
  return self
end

function Block:describe(width, ...)
  local extra = table.pack(...)
  local preview = #self.text > width and self.text:sub(1, width) .. "…" or self.text
  return string.format("%s#%d %s (%d extra)", self.kind, self.id, preview, extra.n)
end

function Block:add_tag(tag)
  self.tags[#self.tags + 1] = tag
  return self
end

local function parse(source)
  assert(type(source) == "string", "source 必须是字符串")

  local blocks, index = {}, 0
  for line in source:gmatch("[^\n]+") do
    index = index + 1
    if line:match("^#") then
      blocks[index] = Block.new(index, line, "heading")
    elseif line:match("^```") then
      blocks[index] = Block.new(index, line, "code")
    else
      blocks[index] = Block.new(index, line)
    end
  end

  for i = 1, #blocks do
    if blocks[i].kind == "code" then
      goto continue
    end
    blocks[i]:add_tag("text")
    ::continue::
  end

  return blocks
end

local banner = [[
多行长字符串：\n 不会被转义，$ 也不是变量
]]

local blocks = parse("# 标题\n```lua\nprint('hi')\n```")
for _, block in ipairs(blocks) do
  print(block:describe(24, "a", "b"))
end
print(banner, M.retries, DEFAULT_OPTIONS.hex)
return M
```

## 32. PowerShell

`param` 与校验特性、`$env:` 变量、here-string、管道、`try/catch/finally`、哈希表、反引号续行。

```powershell
<#
.SYNOPSIS
    扫描 Markdown 目录并统计代码块。
.DESCRIPTION
    块注释里出现 # 与 $var 都不应影响后续解析。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Container })]
    [string]$Root,

    [ValidateRange(1, 64)]
    [int]$MaxDepth = 8,

    [switch]$AsJson
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Total = 0
$extensions = @('.md', '.markdown')
$summary = @{ Files = 0; Blocks = 0; Bytes = 0 }

function Get-MarkdownFile {
    param([string]$Path, [int]$Depth = 0)

    if ($Depth -gt $MaxDepth) { return }

    Get-ChildItem -LiteralPath $Path -Force | ForEach-Object {
        if ($_.PSIsContainer) {
            Get-MarkdownFile -Path $_.FullName -Depth ($Depth + 1)
        }
        elseif ($extensions -contains $_.Extension.ToLowerInvariant()) {
            $_
        }
    }
}

$files = Get-MarkdownFile -Path $Root |
    Where-Object { $_.Length -gt 0 } |
    Sort-Object -Property Length -Descending

foreach ($file in $files) {
    $text = Get-Content -LiteralPath $file.FullName -Raw -Encoding utf8
    $count = ([regex]::Matches($text, '(?m)^```')).Count / 2
    $script:Total += [int]$count
    $summary.Files += 1
    $summary.Blocks += [int]$count
    $summary.Bytes += $file.Length
}

$report = @"
路径: $Root
文件: $($summary.Files)
代码块: $($summary.Blocks)
环境: $env:COMPUTERNAME / $env:OS
"@

try {
    if ($AsJson) {
        $summary | ConvertTo-Json -Depth 4 -Compress
    }
    else {
        Write-Host $report -ForegroundColor Cyan
    }
}
catch [System.IO.IOException] {
    Write-Error "读取失败：$($_.Exception.Message)"
}
finally {
    Remove-Variable -Name files -ErrorAction SilentlyContinue
}
```

## 33. R

`function` 默认参数、`<-`、原生管道 `|>`、`ggplot2` 图层、列表、`ifelse`、公式与 `NA`。

```r
library(ggplot2)
library(dplyr)

MAX_ROWS <- 1e5
NA_LEVEL <- NA_character_

summarise_blocks <- function(data, group = "language", min_bytes = 0L, ...) {
  stopifnot(is.data.frame(data), is.character(group))

  data |>
    filter(!is.na(.data[[group]]), bytes >= min_bytes) |>
    group_by(across(all_of(group))) |>
    summarise(
      blocks = n(),
      total = sum(bytes, na.rm = TRUE),
      mean_kib = round(mean(bytes) / 1024, 2),
      .groups = "drop"
    ) |>
    arrange(desc(total)) |>
    mutate(
      share = total / sum(total),
      tier = ifelse(share > 0.25, "hot", ifelse(share > 0.05, "warm", "cold")),
      label = sprintf("%s (%d)", .data[[group]], blocks)
    )
}

set.seed(42)
raw <- data.frame(
  path = sprintf("fixtures/doc-%02d.md", 1:50),
  language = sample(c("rust", "python", "sql", NA), 50, replace = TRUE),
  bytes = round(rlnorm(50, meanlog = 7, sdlog = 1.2))
)

summary_tbl <- summarise_blocks(raw, min_bytes = 100L)
print(head(summary_tbl, 10))

model <- lm(log(bytes) ~ factor(language), data = raw)
coefs <- coef(model)
cat(sprintf("R^2 = %.3f, n = %d\n", summary(model)$r.squared, nrow(raw)))

plot <- ggplot(summary_tbl, aes(x = reorder(language, -total), y = total, fill = tier)) +
  geom_col(width = 0.7, colour = "grey20") +
  geom_text(aes(label = scales::comma(total)), vjust = -0.4, size = 3) +
  scale_y_continuous(labels = scales::comma, expand = expansion(mult = c(0, 0.12))) +
  labs(title = "各语言字节总量", x = NULL, y = "bytes") +
  theme_minimal(base_size = 12)

ggsave("blocks.png", plot, width = 7, height = 4, dpi = 144)
```

## 34. Elixir

`defmodule`、`@moduledoc` heredoc、多子句与守卫、管道、`with`、sigil、结构体与协议实现。

```elixir
defmodule Mdview.Parser do
  @moduledoc """
  解析 Markdown 文档，统计代码块。

  这里的 `@doc` 与 `#{}` 插值都要正确闭合：#{inspect(__MODULE__)}
  """

  @max_blocks 1_000
  @pattern ~r/^```(?<lang>[\w+-]*)\s*$/
  @tags ~w(rust elixir sql)

  defstruct [:id, :kind, :text, language: "text", tags: []]

  @type t :: %__MODULE__{
          id: non_neg_integer(),
          kind: :heading | :code | :paragraph,
          text: String.t(),
          language: String.t(),
          tags: [String.t()]
        }

  @spec parse(String.t(), keyword()) :: {:ok, [t()]} | {:error, atom()}
  def parse(source, opts \\ []) when is_binary(source) do
    limit = Keyword.get(opts, :limit, @max_blocks)

    with true <- String.valid?(source),
         [_ | _] = lines <- String.split(source, ~r/\r?\n/u),
         blocks when length(blocks) <= limit <- build(lines) do
      {:ok, blocks}
    else
      [] -> {:error, :empty}
      false -> {:error, :invalid_utf8}
      _too_many -> {:error, :too_many_blocks}
    end
  end

  defp build(lines) do
    lines
    |> Enum.with_index()
    |> Enum.reduce([], fn {line, index}, acc ->
      case classify(line) do
        :skip -> acc
        {kind, lang} -> [%__MODULE__{id: index, kind: kind, text: String.trim(line), language: lang} | acc]
      end
    end)
    |> Enum.reverse()
  end

  defp classify(""), do: :skip

  defp classify("#" <> rest = line) do
    if String.starts_with?(rest, "#"), do: {:heading, "text"}, else: {kind_for(line), "text"}
  end

  defp classify(line) do
    case Regex.named_captures(@pattern, line) do
      %{"lang" => lang} -> {:code, if(lang == "", do: "text", else: lang)}
      nil -> {:paragraph, "text"}
    end
  end

  defp kind_for(_line), do: :heading

  def summary(blocks) do
    blocks
    |> Enum.filter(&(&1.kind == :code))
    |> Enum.map(& &1.language)
    |> Enum.frequencies()
    |> Enum.sort_by(fn {_lang, count} -> -count end)
    |> Enum.map_join(", ", fn {lang, count} -> "#{lang}=#{count}" end)
  end
end

defimpl Inspect, for: Mdview.Parser do
  def inspect(%{language: lang, kind: kind}, _opts) do
    "#<block #{kind} lang=#{lang}>"
  end
end

source = """
# 标题
~~~elixir
IO.puts("hello #{1 + 1}")
~~~
"""

case Mdview.Parser.parse(source) do
  {:ok, blocks} ->
    IO.puts(Mdview.Parser.summary(blocks))

  {:error, reason} ->
    IO.warn("parse failed: #{reason}", [])
end
```

## 35. Haskell

模块导出列表、导入、带记录的 `data`、类型类与实例、守卫、列表推导、`do` 与 `<$>`。

```haskell
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE RecordWildCards   #-}

module Mdview.Parser
  ( Block(..)
  , Kind(..)
  , parseDocument
  , summarise
  ) where

import qualified Data.Map.Strict as Map
import           Data.List (foldl', sortOn)
import           Data.Maybe (mapMaybe)
import           Text.Read (readMaybe)

data Kind
  = Heading Int
  | Code (Maybe String)
  | Paragraph
  deriving (Eq, Ord, Show)

data Block = Block
  { blockId   :: Int
  , blockKind :: Kind
  , blockText :: String
  } deriving (Eq, Show)

class Describable a where
  describe :: a -> String

instance Describable Kind where
  describe (Heading level) = replicate level '#' <> " heading"
  describe (Code Nothing)   = "code (plain)"
  describe (Code (Just l))  = "code (" <> l <> ")"
  describe Paragraph        = "paragraph"

instance Describable Block where
  describe Block{..} =
    let preview = if length blockText > 24 then take 24 blockText <> "…" else blockText
     in show blockId <> ": " <> describe blockKind <> " — " <> preview

classify :: String -> Maybe Kind
classify line
  | null stripped = Nothing
  | "#" `isPrefixOf` stripped = Just (Heading (length (takeWhile (== '#') stripped)))
  | "```" `isPrefixOf` stripped = Just (Code (parseLang (drop 3 stripped)))
  | otherwise = Just Paragraph
  where
    stripped = dropWhile (== ' ') line
    isPrefixOf prefix value = take (length prefix) value == prefix
    parseLang raw =
      case filter (not . null) (words raw) of
        (lang:_) -> Just lang
        []       -> Nothing

parseDocument :: String -> [Block]
parseDocument src =
  [ Block index kind (trim line)
  | (line, index) <- zip (lines src) [0 ..]
  , Just kind <- [classify line]
  , let trim = reverse . dropWhile (== ' ') . reverse
  ]

summarise :: [Block] -> String
summarise blocks =
  let counts = Map.fromListWith (+) [(lang, 1 :: Int) | Block _ (Code (Just lang)) _ <- blocks]
      ranked = sortOn (negate . snd) (Map.toList counts)
      total  = foldl' (+) 0 (Map.elems counts)
   in unwords ["total =", show total, "|", unwords (map render ranked)]
  where
    render (lang, n) = lang <> "=" <> show n

main :: IO ()
main = do
  let sample = unlines ["# 标题", "```rust", "let x: u8 = 0xFF;", "```"]
  mapM_ (putStrLn . describe) (parseDocument sample)
  putStrLn $ summarise (parseDocument sample)
  print $ fmap (+ 1) (readMaybe "41" :: Maybe Int)
```

## 36. Julia

`module`、带类型参数的结构体、多重派发、`where`、宏、广播、字符串插值、推导与 `do` 块。

```julia
module MdviewParser

using Dates
using Statistics: mean

export Block, parse_document, summarise

const MAX_BLOCKS = 1_000
const PATTERN = r"^```(?<lang>[A-Za-z0-9_+-]*)\s*$"

struct Block{T<:AbstractString}
    id::Int
    kind::Symbol
    text::T
    language::String
    bytes::Int
end

Block(id::Int, kind::Symbol, text::AbstractString; language::String = "text") =
    Block(id, kind, String(text), language, sizeof(text))

function parse_document(source::AbstractString; limit::Int = MAX_BLOCKS)::Vector{<:Block}
    isempty(source) && throw(ArgumentError("文档为空"))

    blocks = Block[]
    for (index, line) in enumerate(split(source, '\n'))
        0 < index <= limit || break
        stripped = strip(line)
        isempty(stripped) && continue

        if startswith(stripped, '#')
            push!(blocks, Block(index, :heading, stripped))
        elseif (m = match(PATTERN, stripped)) !== nothing
            lang = isempty(m[:lang]) ? "text" : m[:lang]
            push!(blocks, Block(index, :code, stripped; language = lang))
        else
            push!(blocks, Block(index, :paragraph, stripped))
        end
    end
    return blocks
end

summarise(blocks::AbstractVector{<:Block}) =
    let codes = filter(b -> b.kind == :code, blocks)
        Dict(lang => count(b -> b.language == lang, codes) for lang in unique(getfield.(codes, :language)))
    end

function describe(block::Block; width::Int = 24)
    preview = length(block.text) > width ? block.text[1:width] * "…" : block.text
    @sprintf("%-10s #%03d %s", string(block.kind), block.id, preview)
end

macro timed(ex)
    quote
        started = time_ns()
        value = $(esc(ex))
        elapsed_ms = (time_ns() - started) / 1e6
        @info "elapsed" elapsed_ms
        value
    end
end

source = """
# 标题
~~~julia
x = 0xFF
~~~
"""

blocks = parse_document(source)
println(join(describe.(blocks), "\n"))
println(summarise(blocks))

values = [1, 2, 3, 4]
@timed begin
    scaled = values .^ 2 .+ 0.5
    println(mean(scaled), " ", sum(abs2, scaled))
end

map(values) do v
    v * 2
end |> println
```

## 37. Clojure

`ns`、`defn` 文档串与解构、`let`、线程宏、`atom`、集合字面量、`loop/recur` 与 `:keyword`。

```clojure
(ns mdview.parser
  (:require [clojure.string :as str]
            [clojure.edn :as edn])
  (:import (java.time Instant)))

(def max-blocks 1000)
(def default-opts {:language "text" :tags #{} :ratio 0.75})

(defn classify
  "返回 [kind lang]，注释里的 `#` 与 \"引号\" 都不能影响解析。"
  [line]
  (let [trimmed (str/trim line)]
    (cond
      (str/blank? trimmed)          nil
      (str/starts-with? trimmed "#") [:heading "text"]
      (re-matches #"^```([\w+-]*)$" trimmed)
      (let [[_ lang] (re-matches #"^```([\w+-]*)$" trimmed)]
        [:code (if (str/blank? lang) "text" lang)])
      :else [:paragraph "text"])))

(defn parse-document
  ([source] (parse-document source {}))
  ([source {:keys [limit] :or {limit max-blocks}}]
   (when (str/blank? source)
     (throw (ex-info "空文档" {:type :empty})))
   (->> (str/split-lines source)
        (map-indexed (fn [index line]
                       (when-let [[kind lang] (classify line)]
                         {:id index :kind kind :text (str/trim line) :language lang})))
        (remove nil?)
        (take limit)
        (vec))))

(defn summarise [blocks]
  (->> blocks
       (filter #(= :code (:kind %)))
       (map :language)
       (frequencies)
       (sort-by (comp - val))
       (map (fn [[lang n]] (str lang "=" n)))
       (str/join ", ")))

;; 线程宏：从右往左读，检查 -> 与 ->> 不混淆
(def stats
  (->> (parse-document "# 标题\n```clojure\n(+ 1 2)\n```")
       (map (juxt :id :kind))
       (into (sorted-map))))

(def counter (atom 0))

(defn bump! []
  (swap! counter (fn [n] (inc n))))

(dotimes [_ 3] (bump!))

(loop [i 0 acc []]
  (if (< i 3)
    (recur (inc i) (conj acc (* i i)))
    (do
      (println "squares" acc)
      acc)))

(try
  (edn/read-string "{:a 1 :b [2 3]}")
  (catch Exception e
    (println "读取失败：" (ex-message e))))

(println stats (summarise (parse-document "# t")) @counter (Instant/now))
```

## 38. Erlang

`-module`/`-export`/`-record`/`-spec`、守卫、多子句、列表推导、`fun`、二进制与 `receive`。

```erlang
-module(mdview_parser).
-behaviour(gen_server).

-export([parse/1, parse/2, summarise/1, start_link/0]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

-define(MAX_BLOCKS, 1000).
-define(IS_FENCE(Line), (binary:part(Line, 0, 3) =:= <<"```">>)).

-record(block, {
    id      :: non_neg_integer(),
    kind    :: heading | code | paragraph,
    text    :: binary(),
    lang    :: binary()
}).

-type block() :: #block{}.
-export_type([block/0]).

-spec parse(binary()) -> {ok, [block()]} | {error, atom()}.
parse(Source) -> parse(Source, ?MAX_BLOCKS).

parse(Source, Limit) when is_binary(Source), is_integer(Limit), Limit > 0 ->
    case string:trim(Source) of
        <<>> -> {error, empty_document};
        _ ->
            Lines = binary:split(Source, <<"\n">>, [global]),
            {ok, build(Lines, 1, Limit, [])}
    end;
parse(_Source, _Limit) ->
    {error, badarg}.

build([], _Index, _Limit, Acc) ->
    lists:reverse(Acc);
build(_Lines, Index, Limit, Acc) when Index > Limit ->
    lists:reverse(Acc);
build([Line | Rest], Index, Limit, Acc) ->
    case classify(Line) of
        skip -> build(Rest, Index + 1, Limit, Acc);
        {Kind, Lang} ->
            Block = #block{id = Index, kind = Kind, text = string:trim(Line), lang = Lang},
            build(Rest, Index + 1, Limit, [Block | Acc])
    end.

classify(<<>>) -> skip;
classify(<<"#", _/binary>> = Line) ->
    case Line of
        <<"##", _/binary>> -> {heading, <<"text">>};
        _ -> {heading, <<"text">>}
    end;
classify(Line) ->
    case ?IS_FENCE(Line) of
        true ->
            Lang = binary:part(Line, 3, byte_size(Line) - 3),
            {code, case string:trim(Lang) of <<>> -> <<"text">>; L -> L end};
        false ->
            {paragraph, <<"text">>}
    end.

-spec summarise([block()]) -> binary().
summarise(Blocks) ->
    Codes = [Lang || #block{kind = code, lang = Lang} <- Blocks],
    Counts = lists:foldl(fun(Lang, Acc) ->
                             maps:update_with(Lang, fun(N) -> N + 1 end, 1, Acc)
                         end, #{}, Codes),
    Sorted = lists:reverse(lists:keysort(2, maps:to_list(Counts))),
    iolist_to_binary(lists:join(", ", [io_lib:format("~s=~p", [L, N]) || {L, N} <- Sorted])).

start_link() ->
    gen_server:start_link({local, ?MODULE}, ?MODULE, [], []).

init([]) ->
    {ok, #{blocks => 0, started => erlang:system_time(millisecond)}}.

handle_call({parse, Source}, _From, #{blocks := N} = State) ->
    case parse(Source) of
        {ok, Blocks} ->
            {reply, {ok, Blocks}, State#{blocks := N + length(Blocks)}};
        Error ->
            {reply, Error, State}
    end;
handle_call(_Req, _From, State) ->
    {reply, ok, State}.

handle_cast(_Msg, State) -> {noreply, State}.

handle_info({'DOWN', _Ref, process, _Pid, Reason}, State) ->
    error_logger:warning_msg("down: ~p~n", [Reason]),
    {noreply, State}.

terminate(_Reason, _State) -> ok.

%% 二进制与位语法：<<Len:32/big, Rest/binary>>
-spec frame(binary()) -> {ok, binary()} | {error, incomplete}.
frame(<<Len:32/big, Payload:Len/binary, Rest/binary>>) ->
    {ok, {Payload, Rest}};
frame(_Short) ->
    {error, incomplete}.

loop_receive() ->
    receive
        {ping, From} when is_pid(From) ->
            From ! pong,
            loop_receive();
        stop ->
            ok
    after 1000 ->
        timeout
    end.
```

## 39. OCaml

`let rec`、变体与记录、模块与函子、`match`、管道、`ref`、`List.map` 与类型标注。

```ocaml
(* 块注释：里面的 (* 嵌套 *) 与 " 字符串都不应打断后续代码 *)
module type STORE = sig
  type t
  val empty : t
  val add : string -> int -> t -> t
  val find : string -> t -> int option
end

module MapStore : STORE = struct
  type t = (string * int) list

  let empty = []

  let add key value store =
    (key, value) :: List.remove_assoc key store

  let find key store = List.assoc_opt key store
end

type kind =
  | Heading of int
  | Code of string option
  | Paragraph

type block = {
  id : int;
  kind : kind;
  text : string;
}

let classify line =
  let stripped = String.trim line in
  if stripped = "" then None
  else if String.length stripped > 0 && stripped.[0] = '#' then
    let level = String.length (String.sub stripped 0 (String.index_from_opt stripped 0 ' ' |> Option.value ~default:1)) in
    Some (Heading level)
  else if String.length stripped >= 3 && String.sub stripped 0 3 = "```" then
    let lang = String.trim (String.sub stripped 3 (String.length stripped - 3)) in
    Some (Code (if lang = "" then None else Some lang))
  else Some Paragraph

let parse_document source =
  source
  |> String.split_on_char '\n'
  |> List.mapi (fun index line ->
         match classify line with
         | None -> None
         | Some kind -> Some { id = index; kind; text = String.trim line })
  |> List.filter_map Fun.id

let describe block =
  let preview =
    if String.length block.text > 24 then String.sub block.text 0 24 ^ "…"
    else block.text
  in
  let kind_label =
    match block.kind with
    | Heading level -> Printf.sprintf "h%d" level
    | Code None -> "code:text"
    | Code (Some lang) -> "code:" ^ lang
    | Paragraph -> "p"
  in
  Printf.sprintf "%s#%d %s" kind_label block.id preview

let summarise blocks =
  blocks
  |> List.filter_map (fun b ->
         match b.kind with Code (Some lang) -> Some lang | _ -> None)
  |> List.sort compare
  |> List.fold_left
       (fun acc lang ->
         match acc with
         | (prev, count) :: rest when prev = lang -> (prev, count + 1) :: rest
         | _ -> (lang, 1) :: acc)
       []
  |> List.rev
  |> List.map (fun (lang, count) -> Printf.sprintf "%s=%d" lang count)
  |> String.concat ", "

let () =
  let sample = "# 标题\n```ocaml\nlet x = 0xFF\n```\n" in
  let blocks = parse_document sample in
  List.iter (fun b -> print_endline (describe b)) blocks;
  Printf.printf "summary: %s\n" (summarise blocks);
  let store = MapStore.(empty |> add "a" 1 |> add "b" 2) in
  match MapStore.find "a" store with
  | Some value -> Printf.printf "found %d\n" value
  | None -> print_endline "missing";
  let counter = ref 0 in
  for i = 1 to 3 do
    counter := !counter + i
  done;
  Printf.printf "counter = %d\n" !counter
```

## 40. F#

模块、可区分联合、记录、`let rec`、管道、`async { }` 计算表达式、模式匹配与特性。

```fsharp
module Mdview.Parser

open System
open System.IO
open System.Text.RegularExpressions

[<Literal>]
let MaxBlocks = 1000

type Kind =
    | Heading of level: int
    | Code of language: string option
    | Paragraph

type Block =
    { Id: int
      Kind: Kind
      Text: string
      Bytes: int }

let private fence = Regex(@"^```(?<lang>[\w+-]*)\s*$", RegexOptions.Compiled)

let classify (line: string) : Kind option =
    let trimmed = line.Trim()
    if String.IsNullOrEmpty trimmed then None
    elif trimmed.StartsWith "#" then
        Some(Heading(trimmed.Length - trimmed.TrimStart('#').Length))
    else
        let m = fence.Match trimmed
        if m.Success then
            let lang = m.Groups.["lang"].Value
            Some(Code(if String.IsNullOrEmpty lang then None else Some lang))
        else Some Paragraph

let parseDocument (source: string) : Result<Block list, string> =
    if String.IsNullOrWhiteSpace source then Error "空文档"
    else
        source.Split('\n')
        |> Array.indexed
        |> Array.choose (fun (index, line) ->
            classify line
            |> Option.map (fun kind ->
                { Id = index
                  Kind = kind
                  Text = line.Trim()
                  Bytes = Text.Encoding.UTF8.GetByteCount line }))
        |> Array.truncate MaxBlocks
        |> List.ofArray
        |> Ok

let describe block =
    let label =
        match block.Kind with
        | Heading level -> sprintf "h%d" level
        | Code None -> "code:text"
        | Code(Some lang) -> sprintf "code:%s" lang
        | Paragraph -> "p"
    let preview =
        if block.Text.Length > 24 then block.Text.Substring(0, 24) + "…" else block.Text
    sprintf "%s#%d %s" label block.Id preview

let summarise (blocks: Block list) =
    blocks
    |> List.choose (fun b ->
        match b.Kind with
        | Code(Some lang) -> Some lang
        | _ -> None)
    |> List.countBy id
    |> List.sortByDescending snd
    |> List.map (fun (lang, n) -> sprintf "%s=%d" lang n)
    |> String.concat ", "

let readAll (path: string) =
    async {
        use stream = File.OpenRead path
        use reader = new StreamReader(stream)
        let! text = reader.ReadToEndAsync() |> Async.AwaitTask
        return text
    }

[<EntryPoint>]
let main argv =
    let sample = "# 标题\n```fsharp\nlet x = 0xFF\n```\n"

    match parseDocument sample with
    | Ok blocks ->
        blocks |> List.iter (describe >> printfn "%s")
        printfn "summary: %s" (summarise blocks)
    | Error reason -> eprintfn "失败: %s" reason

    let total =
        [ for i in 1..10 do
              if i % 2 = 0 then yield i * i ]

    total |> List.sum |> printfn "sum = %d"

    let maybe = Some 21 |> Option.map (fun x -> x * 2) |> Option.defaultValue 0
    printfn "maybe = %d" maybe
    0
```

## 41. VB.NET

`Imports`、`Namespace`、`Module`、`Dim`、`Select Case`、`Try/Catch/Finally`、`Property`、`Async`。

```vb
Imports System
Imports System.Collections.Generic
Imports System.Linq
Imports System.Threading.Tasks

Namespace Mdview.Core

    Public Enum BlockKind
        Heading = 0
        Code = 1
        Paragraph = 2
    End Enum

    Public Class Block
        Public Property Id As Integer
        Public Property Kind As BlockKind = BlockKind.Paragraph
        Public Property Text As String = String.Empty
        Public Property Language As String = "text"

        Public ReadOnly Property IsCode As Boolean
            Get
                Return Kind = BlockKind.Code
            End Get
        End Property

        Public Function Describe(Optional width As Integer = 24) As String
            Dim preview As String =
                If(Text.Length > width, Text.Substring(0, width) & "…", Text)
            Return String.Format("{0}#{1} {2} ({3} bytes)", Kind, Id, preview, Text.Length)
        End Function
    End Class

    Public Module Report

        Private Const MaxBlocks As Integer = 1000

        Public Async Function LoadAsync(path As String) As Task(Of String)
            Using reader As New IO.StreamReader(path, Text.Encoding.UTF8)
                Return Await reader.ReadToEndAsync()
            End Using
        End Function

        Public Function Parse(source As String) As List(Of Block)
            If String.IsNullOrWhiteSpace(source) Then
                Throw New ArgumentException("文档为空", NameOf(source))
            End If

            Dim result As New List(Of Block)
            Dim lines = source.Split({ControlChars.CrLf, ControlChars.Lf}, StringSplitOptions.None)

            For index As Integer = 0 To Math.Min(lines.Length, MaxBlocks) - 1
                Dim line = lines(index).Trim()
                If line = "" Then Continue For

                Dim kind As BlockKind
                Select Case True
                    Case line.StartsWith("#")
                        kind = BlockKind.Heading
                    Case line.StartsWith("```")
                        kind = BlockKind.Code
                    Case Else
                        kind = BlockKind.Paragraph
                End Select

                result.Add(New Block With {
                    .Id = index,
                    .Kind = kind,
                    .Text = line,
                    .Language = If(kind = BlockKind.Code, "vb", "text")
                })
            Next

            Return result
        End Function

        Public Sub Main()
            Try
                Dim blocks = Parse("# 标题" & vbLf & "```vb" & vbLf & "Dim x = &HFF" & vbLf & "```")
                For Each block In blocks.Where(Function(b) b.IsCode)
                    Console.WriteLine(block.Describe())
                Next

                Dim grouped = From b In blocks
                              Group b By b.Kind Into Count()
                              Order By Count Descending
                              Select Kind, Count

                Console.WriteLine($"代码块 {grouped.Count()} 组")
            Catch ex As ArgumentException
                Console.Error.WriteLine($"参数错误：{ex.Message}")
            Catch ex As Exception When ex.Message.Contains("IO")
                Console.Error.WriteLine($"读取失败：{ex.Message}")
            Finally
                Console.WriteLine("done")
            End Try
        End Sub

    End Module

End Namespace
```

## 42. Solidity

`pragma`、继承、`modifier`、`event`/`error`、`mapping`、`struct`、`payable`、`unchecked` 与类型转换。

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20} from "./interfaces/IERC20.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";

/// @title Markdown 内容登记合约
/// @notice 每个块按 keccak256 指纹登记，用于去重
contract BlockRegistry is Ownable {
    error AlreadyRegistered(bytes32 digest);
    error EmptyContent();
    error InsufficientFee(uint256 required, uint256 paid);

    event BlockRegistered(bytes32 indexed digest, address indexed author, uint64 size, string language);
    event FeeUpdated(uint256 previous, uint256 current);

    struct Block {
        bytes32 digest;
        address author;
        uint64 size;
        uint64 createdAt;
        string language;
    }

    mapping(bytes32 => Block) private _blocks;
    mapping(address => uint256) public registeredCount;

    uint256 public fee = 0.001 ether;
    uint256 public constant MAX_SIZE = 1_000_000;

    modifier onlyNonEmpty(bytes memory payload) {
        if (payload.length == 0) revert EmptyContent();
        _;
    }

    modifier paidEnough() {
        if (msg.value < fee) revert InsufficientFee(fee, msg.value);
        _;
    }

    constructor(address initialOwner) Ownable(initialOwner) {}

    function register(bytes calldata payload, string calldata language)
        external
        payable
        onlyNonEmpty(payload)
        paidEnough
        returns (bytes32 digest)
    {
        if (payload.length > MAX_SIZE) revert InsufficientFee(MAX_SIZE, payload.length);

        digest = keccak256(payload);
        if (_blocks[digest].author != address(0)) revert AlreadyRegistered(digest);

        unchecked {
            registeredCount[msg.sender] += 1;
        }

        _blocks[digest] = Block({
            digest: digest,
            author: msg.sender,
            size: uint64(payload.length),
            createdAt: uint64(block.timestamp),
            language: language
        });

        emit BlockRegistered(digest, msg.sender, uint64(payload.length), language);
    }

    function get(bytes32 digest) external view returns (Block memory) {
        return _blocks[digest];
    }

    function setFee(uint256 next) external onlyOwner {
        uint256 previous = fee;
        fee = next;
        emit FeeUpdated(previous, next);
    }

    function withdraw(address payable to) external onlyOwner {
        (bool ok, ) = to.call{value: address(this).balance}("");
        require(ok, "withdraw failed");
    }

    receive() external payable {}
}
```

## 43. LaTeX

导言区、自定义命令、章节、行内与行间公式、`align`、列表、表格、脚注、引用与 `%` 注释。

```latex
\documentclass[11pt,a4paper]{article}

\usepackage[UTF8]{ctex}
\usepackage{amsmath,amssymb}
\usepackage{booktabs}
\usepackage{hyperref}
\usepackage[margin=2.5cm]{geometry}

\newcommand{\mdview}[2][1.0]{%
  \texttt{mdview}%
  \ifx#2\relax\else\ (\textit{#2})\fi
}
\newcommand{\R}{\mathbb{R}}

\title{代码高亮与 \LaTeX{} 排版}
\author{mdview}
\date{\today}

\begin{document}
\maketitle

\section{引言}
\label{sec:intro}

本文讨论 \mdview[0.8]{highlight} 的着色策略，详见第 \ref{sec:model} 节与文献~\cite{knuth1984}。 % 行尾注释
行内公式 $E = mc^2$ 与 $\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$ 混排。

\section{模型}
\label{sec:model}

设映射 $f \colon \R^n \to \R$，目标函数为
\begin{equation}
  \mathcal{L}(\theta) = \frac{1}{N} \sum_{i=1}^{N}
    \left\lVert y_i - f_\theta(x_i) \right\rVert_2^2
    + \lambda \lVert \theta \rVert_1 .
  \label{eq:loss}
\end{equation}

由式~\eqref{eq:loss} 可得梯度：
\begin{align}
  \nabla_\theta \mathcal{L}
    &= \frac{2}{N} \sum_{i=1}^{N} \bigl( f_\theta(x_i) - y_i \bigr) \nabla_\theta f_\theta(x_i) \notag \\
    &\quad + \lambda \, \operatorname{sign}(\theta).
\end{align}

\subsection{超参数}

\begin{itemize}
  \item 学习率 $\eta = 1 \times 10^{-3}$
  \item 批大小 $B = 64$，其中 $B \ll N$
  \item 正则系数 \verb|\lambda = 0.01|
\end{itemize}

\begin{table}[htbp]
  \centering
  \caption{语言支持矩阵}
  \begin{tabular}{@{}llr@{}}
    \toprule
    语言 & 扫描器 & 行数 \\
    \midrule
    Rust   & 词表驱动 & 64 \\
    SQL    & 词表驱动 & 32 \\
    diff   & 专用     & 18 \\
    \bottomrule
  \end{tabular}
\end{table}

\paragraph{脚注} 高亮不改变停靠点\footnote{见 \texttt{mod.rs} 的 \texttt{normalize}。}。

\begin{thebibliography}{9}
\bibitem{knuth1984} D.~E. Knuth. \emph{The \TeX book}. Addison-Wesley, 1984.
\end{thebibliography}

\end{document}
```

## 44. Nginx

`events`/`http` 嵌套、`log_format`、`upstream` 权重、`map` 正则、`server` 与 `location`、变量与 `try_files`。

```nginx
user  www-data;
worker_processes  auto;
pid /run/nginx.pid;

events {
    worker_connections  4096;
    multi_accept        on;
    use                 epoll;
}

http {
    include       /etc/nginx/mime.types;
    default_type  application/octet-stream;
    sendfile      on;
    tcp_nopush    on;
    keepalive_timeout  65s;
    server_tokens off;

    log_format  main  '$remote_addr - $remote_user [$time_local] "$request" '
                      '$status $body_bytes_sent "$http_referer" '
                      '"$http_user_agent" rt=$request_time';

    map $http_upgrade $connection_upgrade {
        default upgrade;
        ''      close;
    }

    map $request_uri $is_markdown {
        ~*\.(md|markdown)$   1;
        default              0;
    }

    upstream mdview_backend {
        least_conn;
        server 127.0.0.1:7001 weight=3 max_fails=2 fail_timeout=10s;
        server 127.0.0.1:7002 weight=1 backup;
        keepalive 32;
    }

    server {
        listen              443 ssl http2;
        listen              [::]:443 ssl http2;
        server_name         mdview.example.com;
        root                /srv/mdview/public;

        ssl_certificate     /etc/letsencrypt/live/example/fullchain.pem;
        ssl_certificate_key /etc/letsencrypt/live/example/privkey.pem;

        add_header X-Frame-Options SAMEORIGIN always;
        add_header X-Content-Type-Options nosniff;

        location / {
            try_files $uri $uri/ /index.html;
            expires 5m;
        }

        location ~* \.(?:css|js|woff2?|png|svg)$ {
            expires 30d;
            access_log off;
            add_header Cache-Control "public, immutable";
        }

        location ~ ^/api/v(?<version>\d+)/ {
            proxy_pass         http://mdview_backend;
            proxy_http_version 1.1;
            proxy_set_header   Host              $host;
            proxy_set_header   X-Real-IP         $remote_addr;
            proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
            proxy_set_header   X-Forwarded-Proto $scheme;
            proxy_set_header   Upgrade           $http_upgrade;
            proxy_set_header   Connection        $connection_upgrade;
            proxy_read_timeout 60s;
            proxy_buffering    off;

            if ($is_markdown = 0) {
                rewrite ^/api/v\d+/(.*)$ /internal/$1 break;
            }
        }

        location = /healthz {
            access_log off;
            return 200 "ok\n";
            add_header Content-Type text/plain;
        }

        error_page 404 /404.html;
        error_page 500 502 503 504 /50x.html;
    }
}
```

## 45. Dockerfile

多阶段构建、`ARG` 前置、`ENV`、`COPY --from`、`RUN` 续行与 `&&`、heredoc、`HEALTHCHECK` 与 exec 形式入口。

```dockerfile
# syntax=docker/dockerfile:1.7
ARG RUST_VERSION=1.83
ARG NODE_VERSION=22

# ---------- 前端构建 ----------
FROM node:${NODE_VERSION}-alpine AS web
WORKDIR /app/web
ENV CI=true \
    PNPM_HOME=/pnpm \
    PATH=/pnpm:$PATH
COPY package.json pnpm-lock.yaml ./
RUN corepack enable \
 && pnpm install --frozen-lockfile --prefer-offline
COPY src ./src
COPY vite.config.ts tsconfig.json index.html ./
RUN pnpm build --mode production

# ---------- Rust 构建 ----------
FROM rust:${RUST_VERSION}-slim AS builder
ARG TARGET=x86_64-unknown-linux-gnu
WORKDIR /app

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /app/web/dist ./dist

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --locked --target ${TARGET} \
 && cp target/${TARGET}/release/mdview /usr/local/bin/mdview

# 生成校验和与版本信息（heredoc 内部按普通文本处理）
RUN <<EOF
set -eu
mdview --version > /app/VERSION
sha256sum /usr/local/bin/mdview > /app/mdview.sha256
EOF

# ---------- 运行镜像 ----------
FROM debian:bookworm-slim AS runtime
LABEL org.opencontainers.image.title="mdview" \
      org.opencontainers.image.source="https://example.com/mdview"

RUN groupadd --gid 10001 mdview \
 && useradd --uid 10001 --gid mdview --create-home mdview

COPY --from=builder --chown=mdview:mdview /usr/local/bin/mdview /usr/local/bin/mdview
COPY --from=builder --chown=mdview:mdview /app/dist /srv/mdview/public
COPY --from=builder /app/VERSION /app/mdview.sha256 /srv/mdview/

ENV MDVIEW_ROOT=/srv/mdview \
    RUST_LOG=info,mdview=debug \
    PORT=7001

EXPOSE 7001
USER mdview:mdview
WORKDIR /srv/mdview

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD mdview healthcheck || exit 1

ENTRYPOINT ["/usr/local/bin/mdview"]
CMD ["serve", "--root", "/srv/mdview", "--port", "7001"]
```

## 46. Makefile

变量四种赋值、`$(shell)` 与替换引用、条件分支、模式规则、自动变量、`.PHONY` 与配方修饰符。

```makefile
SHELL      := /bin/bash
.DEFAULT_GOAL := all

APP        := mdview
VERSION    ?= 0.2.0
PREFIX     ?= /usr/local
RUST_FLAGS += --locked

SRC_DIR    := crates
BUILD_DIR  := target/release
SOURCES    := $(shell find $(SRC_DIR) -name '*.rs' 2>/dev/null)
TEST_BINS  := $(patsubst $(SRC_DIR)/%/src/main.rs,$(BUILD_DIR)/%,$(wildcard $(SRC_DIR)/*/src/main.rs))
COMMIT     := $(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)

ifeq ($(origin CI), undefined)
  JOBS := 4
else
  JOBS := $(shell nproc)
endif

.PHONY: all build test fmt lint install clean help

all: build test

build: $(BUILD_DIR)/$(APP)

$(BUILD_DIR)/$(APP): $(SOURCES) Cargo.toml Cargo.lock
	@echo "==> building $(APP) $(VERSION) ($(COMMIT))"
	@mkdir -p $(BUILD_DIR)
	cargo build --release $(RUST_FLAGS) --jobs $(JOBS)
	@touch $@

$(BUILD_DIR)/%: $(SRC_DIR)/%/src/main.rs
	@echo "==> bin $*"
	cargo build --release -p $* $(RUST_FLAGS)

test:
	cargo test $(RUST_FLAGS) --workspace
	pnpm test

fmt:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets -- $(RUST_FLAGS) -D warnings

install: build
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 0755 $(BUILD_DIR)/$(APP) $(DESTDIR)$(PREFIX)/bin/$(APP)

clean:
	-rm -rf target dist
	@echo "cleaned"

help:
	@printf '%-12s %s\n' \
	  build '编译 release 产物' \
	  test  '跑 Rust 与前端测试' \
	  lint  'clippy 严格校验'

-include .env
```

## 47. diff

多文件、多 hunk、上下文行、文件头与 hunk 头、新增/删除行以及「无换行结尾」标记。

```diff
diff --git a/crates/mdview-markdown/src/highlight/mod.rs b/crates/mdview-markdown/src/highlight/mod.rs
index 3f9c2a1..b47d0e5 100644
--- a/crates/mdview-markdown/src/highlight/mod.rs
+++ b/crates/mdview-markdown/src/highlight/mod.rs
@@ -44,9 +44,11 @@ pub enum TokenKind {
     Macro,
     /// Rust `#[derive]`、`@dataclass` 这类属性与注解。
     Annotation,
+    /// 差异补丁里的新增行。
+    Inserted,
+    /// 差异补丁里的删除行。
+    Deleted,
     Operator,
 }
 
 impl TokenKind {
@@ -58,6 +60,8 @@ impl TokenKind {
             TokenKind::Macro => "tk-macro",
             TokenKind::Annotation => "tk-annotation",
+            TokenKind::Inserted => "tk-inserted",
+            TokenKind::Deleted => "tk-deleted",
             TokenKind::Operator => "tk-operator",
         }
     }
@@ -96,7 +100,7 @@ pub fn highlight(info: Option<&str>, src: &str, content: Range<usize>) -> Vec<Tok
     let lang = Lang::from_info(info);
     let tokens = match lang {
         Lang::Plain => return Vec::new(),
-        Lang::Json => langs::web::json_tokens(code, content.start),
+        Lang::Diff => langs::diff::diff_tokens(code, content.start),
         _ => lexer::code_tokens(lang, code, content.start),
     };
     normalize(tokens, &content)
--- a/README.md
+++ b/README.md
@@ -8,4 +8,7 @@
-- **代码高亮**：16 种语言。
+- **代码高亮**：47 种语言，覆盖 diff / Dockerfile / Makefile 等行导向格式。
+- **测试样张**：`src-tauri/tests/fixtures/highlight-all.md` 逐语言核对。
 
 许可：MIT
+
+维护：见 `CHANGELOG.md`。
\ No newline at end of file
```

---

## 48. 边界用例

不是「第 48 种语言」，而是专门喂给扫描器的坏输入。要求：**不 panic、不越界、不吞掉后续段落**。

未闭合的字符串与块注释（Rust）：

```rust
fn broken() {
    let s = "还没结束的字符串;
    /* 块注释也不闭合
    let after = 1;
}
```

多字节标识符与 emoji（Python，检查 byte/char 边界）：

```python
def 计算(数据: dict[str, int]) -> int:
    總和 = 0
    表情 = "🦀🚀✨"
    for 键, 值 in 数据.items():
        if 值 > 0:
            總和 += 值
    return 總和 + len(表情)
```

超长单行（JS，检查折行与 token 边界）：

```js
const longLine = ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "cccccccccccccccccccccccccccccccccccccccc", "dddddddddddddddddddddddddddddddddddddddd"].map((part) => part.toUpperCase()).join("-");
```

未知语言与显式纯文本（应原样显示、无 `tk-*`）：

```foobar
这不是任何已知语言 -> { "raw": true }
```

```text
显式纯文本：<span> 不应着色，$VAR 也不应着色。
```

空代码块与只含空行的块：

```python
```

```toml

```

波浪号围栏 + info 属性（`rust,ignore`）：

~~~rust,ignore
let escaped = r#"路径 "C:\tmp" 与 $not_a_var"#;
~~~

四个反引号里含三个反引号：

````markdown
外层 4 个反引号：

```rust
fn inner() {}
```
````

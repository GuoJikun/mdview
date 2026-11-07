# PLAN.md — mdview 开发计划与执行进度

> 最后更新：2026-11-07 17:35
> 适用对象：接手本项目的 AI 或开发者。**本文自包含**，读完即可继续，不需要任何历史对话。
> 每次有实质进展请同步更新第 6 节的状态和第 7 节的快照。

---

## 1. 项目目标

做一个 Typora 式的**所见即所得** Markdown 编辑器，解析器必须纯 Rust。

一句话架构：**Rust 持有 rope 文本与解析结果，前端只做投影与输入采集**。

明确不做的路线：`解析 → 富文本 → 序列化回 Markdown`。这条路会在 round-trip 中丢失表格对齐、定界符风格（`*` vs `_`）、转义符和列表缩进。Typora 的历史 bug 大多源于此。

正确的心智模型：**每次编辑都等价于对源码的一次字节区间替换，视图只是源码的投影**。结构可以变化，文本字节保持不变。

---

## 2. 硬约束（违反必出 bug，先读这节）

1. **三段坐标**：`display ↔ byte（解析层/DOM）↔ char（ropey）`。core 的 `Range<usize>` 一律是 char index，markdown 层与 DOM 一律是 byte offset。边界必须走 `Document::byte_to_char` / `char_to_byte`。中文文档的坐标错位全部出在漏换算。
2. **Revision 校验**：异步解析结果回传时必须比对 Revision，不符直接丢弃。
3. **隐藏标记 = 差集**：`hidden = 块区间 − 所有可见片段区间`。**不能**从 `End` 事件读取闭合定界符。
4. **`End` 事件区间覆盖整个容器**（`End(Strong)` 的 range 是 `**加粗**` 全段），不是闭合 token。
5. **`Event::Code` 的区间包含反引号**，必须按定界符长度内缩，否则 `` `x` `` 差集为空、反引号不会隐藏。
6. **`SoftBreak` / `HardBreak` 不能进隐藏集**，否则段落内换行消失、排版塌成一行。
7. **解析必须针对整篇**，不能逐块独立解析——逐块会丢引用式链接（实测整篇 3 个 Link → 逐块 0 个）。切块只用于划分事件归属。
8. **撤销存 Transaction 的逆操作**，不存文档快照。
9. **IME**：`compositionstart` 到 `compositionend` 之间，重解析与 DOM patch 全部挂起，只让浏览器渲染 preedit，`compositionend` 后一次性提交。否则中文输入会被自己的重绘打断。
10. **同步滚动按锚点块插值**，禁止用滚动百分比换算。

---

## 3. 技术选型（已实测，不要重新调研）

判据只有一个：**是否提供每个事件对应的精确源码字节区间**。WYSIWYG 的标记隐藏与光标定位都依赖它。

| 库 | 用途 | 关键数据 |
|---|---|---|
| `pulldown-cmark 0.13.6` | **编辑态主引擎** | 纯 Rust 零依赖，`into_offset_iter()` 给 byte range；165KB / 2000 段全量解析 **1.58ms**（release），200KB 探针 2.30ms |
| `ropey 1.x` | 文本存储 | 2.0 仍是 beta，用 1.x |
| `markdown-syntax 0.8` | 结构操作 / 保真回写（**当前未接入**） | 纯 Rust no_std 零依赖，byte Span；实测未改动文档回写逐字节一致（114→114），改 1 块后其余块保留原字节 |
| `cindermark 0.3` | 可选增量后端（**当前未接入**） | 脏块增量 1.13ms vs 全量 9.6ms（8.5x）。风险：2026 新 crate，不保证完整 CM/GFM 兼容，主坐标 UTF-16。接入前须跑兼容性与模糊测试 |
| `comrak 0.59` | 仅离线导入导出 | 默认 feature 拉 syntect/onig 的 C 依赖，编辑态太慢且不保真 |

**已排除**：`tree-sitter-md`（非纯 Rust，C grammar；release 全量 135ms / 增量 128ms 仅 1.1x，因每次重建全部 inline trees）、`markdown-rs`（73ms 太慢；`Position::offset` 实测是 byte 但文档写 character）、`GPUI`（2026 年仍无内置文本输入组件）。

**性能结论**：分块增量在 Markdown 文档规模下**没有收益**（200KB 整篇 2.30ms vs 逐块 2.36ms）。v1 一律全量重解析。

**渲染宿主已定**：Tauri 2 + WebView，`contenteditable` 仅作输入层，文本真值留在 Rust。不用"contenteditable 当真值"（DOM 成为真值就会 round-trip 失真），也不建议 v1 走原生自绘（复杂度集中在 CJK 断行、IME、选区绘制，不在解析，需要 3–6 个月才到"可用"）。

---

## 4. Crate 结构

```
crates/
  mdview-core/      领域层：Document(ropey) / Transaction / History / Anchor / Selection
                    不依赖 GUI、文件系统、时钟
  mdview-markdown/  解析与投影：split / project / block / sourcemap / layout / preview
  mdview-support/   运行期基础设施：logging（滚动落盘）/ error（AppError）
  mdview-app/       状态机：State / Message / update / Effect + ports trait
  mdview-cli/       无头入口，所有验证都从这里跑
```

依赖方向：`cli → app → markdown → core`，`support` 被 app 与 cli 共用。**core 不得依赖 markdown**。

`mdview-markdown` 内部：
- `split.rs`：按空行切块（围栏感知，去尾部换行）
- `project.rs`：单次全篇解析，事件按 `range.start` 二分归属块，产 `Block { spans, hidden }`
- `block.rs`：`Span { kind, byte, text, style }`、`align()`（投影文本 ↔ 源码字节）、`to_html()`（阅读模式/调试用）
- `sourcemap.rs`：byte ↔ 块
- `layout.rs`：**视觉布局**，产停靠点表（见下节）
- `preview.rs`：生成可交互 HTML，标定用

---

## 5. 核心模型：停靠点表

这是整个编辑器的地基，改动前务必理解。

`Block` 的视觉宽度与字节宽度不等：`**加粗**` 视觉 2 列、字节 8。且 byte 0..2 与 8..10 在视觉上不存在。所以光标不能用 byte 定位。

`Layout` 产出**停靠点表**：

```
源码   ** 加 粗 **
byte   0  2  5  8  10
视觉   0  0  1  2  2
```

**关键性质：同一视觉列可以有多个停靠点**。视觉 0 对应 byte 0（标记内）与 byte 2（文字前）。

- 光标左右移动 = 停靠点序号 ±1。跨过标记时视觉列不变（用户看不到跳动），但 byte 从标记前跳到标记后，标记因此**永远不会被停进**。
- 两种重力 `Gravity::{Upstream, Downstream}`：`Upstream` 贴住左词末尾（点击定位），`Downstream` 越过标记（键盘右移）。块首连续标记左侧无可见文本，`Upstream` 也必须越过，否则光标落到块外。
- 段模型 `Visible`（宽=字符数，边界走 `align()` 反查）/ `Atom`（宽 1：Image、Rule、TaskMarker、Break）/ `Collapsed`（宽 0：hidden）。
- `DocumentLayout` 把各块串联，全局停靠点索引 = 块基址 + 块内序号，跨块移动直接 `+1`，块间空行不产生停靠点。
- `Selection { anchor, head }` 用全局停靠点索引；`byte_range()` 跨块时**包含块间空行**，否则复制粘贴会把两段粘连。

---

## 6. 分期计划与状态

图例：✅ 已完成并验证 · 🔶 部分完成 · ⬜ 未开始

### M0 探针与选型 ✅
- 探针工程在 `.workbuddy/lab/`（`inline-probe` / `comrak-probe` / `ts-md-probe`，均为独立 workspace，不属主项目）。
- 已确认：`End` 事件区间语义、`Code` 含反引号、Break 不可折叠、逐块解析丢引用链接、性能数据。

### M1 内核 ✅（除原子写缺陷）
- ✅ `Document`：ropey 包装、`byte_to_char` / `char_to_byte` / `slice` / `revision`
- ✅ `Transaction` / `History`：逆操作撤销、undo/redo 栈
- ✅ `Anchor` / `Selection`：带 Bias 的位置锚点，可随 Transaction 平移
- ✅ `Revision` 单调递增
- 🔶 原子保存：`StdFileStore::save` 目前是「写临时文件 → 若目标存在先 `remove_file` → `rename`」，中间有窗口期。**生产实现应改用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` 或 `ReplaceFileW`**
- ✅ 外部冲突检测：mtime + size 指纹比对（`Fingerprint`）
- ⬜ `Watcher` 只有 trait，无实现（应基于 `notify`）

### M2 只读渲染 🔶
- ✅ 投影模型：`Block` / `Span` / `hidden`，HTML 合成（`to_html()`）
- ✅ 安全：raw HTML 降级为文本（`ENABLE_RAW_HTML` 不启用 + 事件流显式转义），已测 `<script>` 不会泄漏
- ✅ 样式：标题 / 加粗 / 斜体 / 删除线 / 行内代码 / 链接 / 引用 / 代码块 / 分隔线
- ✅ **可交互验证页**（`mdview <file> preview`）：真实 DOM + 自绘 caret + 状态栏，用于核对停靠点
- ✅ 列表项与表格行断行、列表项目符号逐项渲染
- ⬜ Tauri 2 外壳与 `frontendDist` 接线
- ⬜ 主题系统、大纲侧栏
- ⬜ 同步滚动（锚点插值）

### M3 可编辑 🔶
- ✅ 视觉位置索引（停靠点表）、`Caret { block, stop }`、`next/prev/home/end`
- ✅ 选区模型与按块切高亮
- ⬜ `beforeinput` 翻译器：DOM offset → byte → `EditOp` → rope → 重解析 → DOM patch → 重建 Selection
- ⬜ IME 挂起机制（第 2 节约束 9）
- ⬜ `selectionchange` 隔离，抑制原生选区与自绘 caret 的抖动
- ⬜ 输入格式化：回车拆分块、Tab 缩进列表、输入 `**` 自动配对

### M4 细节 ⬜
- 🔶 表格：目前仅按行断行 + 单元格间距，**无网格渲染**。表格是独立子工程（列对齐、单元格内光标、行列增删），不要塞进 M3
- ✅ 大纲数据层（`ParseResult::outline`）
- ⬜ 代码高亮（建议 `syntect` 或 `tree-sitter-highlight`，仅在阅读态启用）
- ⬜ 链接/图片行内编辑
- ⬜ 粘贴 HTML 规范化（sanitize 后转 Markdown，不能直接注入）
- ⬜ 脚注、数学公式
- ⬜ 大文档虚拟化（5 万字以下不做；DOM 节点数与跨块选区天然冲突）

---

## 7. 当前进度快照

- 日期：2026-11-07
- 测试：**67 项全过**（markdown 49 / support 7 / core 5 / app 6），无编译告警
- 最近提交（Angular 规范）：
  - `feat: 初始化内核、解析投影与日志`
  - `feat(markdown): 新增视觉布局与停靠点表`
  - `feat(markdown): 新增可交互投影页与结构断行`
- 远端：`origin/main` 停在 `feat: 初始化内核…`，本地历史已分叉，同步需 `git push --force-with-lease`

---

## 8. 已知缺陷与技术债

按影响排序，接手时不要误以为是新 bug：

1. **表格没有网格**：markdown 层把 `|` 与 `---` 折叠后，表格退化成普通文本流。当前只做到「行分行、单元格加间隙」。
2. **每字符一个 DOM 元素**：`preview.rs` 为了让停靠点可寻址，把每个字符渲染成独立 `<span>`。副作用是英文单词任意处可断行。生产实现应改为「每片段一个元素 + 内核 `align` 换算 char↔byte」。
3. **Windows 原子写窗口期**：见 M1。
4. **无虚拟化**：全文档一次性建 DOM。5 万字以上会卡，但暂无此需求。
5. **图片只渲染占位符**：不加载真实图片，路径沙箱与尺寸测量未做。
6. **`Watcher` 无实现**：外部修改只在保存时靠指纹被动发现。
7. **列表嵌套未验证**：多级列表的 `Item` 边界换行只测了单层。

---

## 9. 下一步（按推荐顺序）

1. **Tauri 2 外壳**：`crates/mdview-desktop`，`frontendDist` 直接指向 `preview_html` 的输出，先用 `build.rs` 或启动时生成静态文件；接 `AppState` 的 `Message::Click` / `Select`，验证 `Effect::MoveCursor { visual }` 能对上真实像素。
2. **输入翻译器**：`beforeinput` → `EditOp`，先只支持「单字符插入 / Backspace / Enter」，跑通闭环再扩。
3. **IME**：在翻译器之上加 composition 挂起。这是中文体验的生死线，单独排一轮。
4. **表格子工程**：`<table>` 渲染 + 单元格内停靠点，替代当前的文本流占位。

每完成一项，更新第 6 节状态、第 7 节快照，并用 Angular 规范提交。

---

## 10. 命令速查

```bash
cargo test --workspace                      # 全量测试
cargo run -p mdview-cli -- demo             # 端到端演示（edit/click/undo/save）
cargo run -p mdview-cli -- <file> outline   # 大纲
cargo run -p mdview-cli -- <file> stats     # 字符/字节/块/隐藏区间统计
cargo run -p mdview-cli -- <file> blocks    # 块清单
cargo run -p mdview-cli -- <file> spans     # 逐块投影，⟦⟧ 内为折叠标记
cargo run -p mdview-cli -- <file> layout    # 停靠点表：列 / byte / 字符（← 同列）
cargo run -p mdview-cli -- <file> preview   # 生成 <file>.preview.html
cargo run -p mdview-cli -- <file> html      # 阅读态 HTML
```

日志落在可执行文件同级 `logs/`：`mdview.log`（5×1MB 滚动）、`error.log`（error 独占，5×1MB）。只读目录自动回落到 `%LOCALAPPDATA%/mdview/logs`。

`layout` 是最有价值的调试入口：同一列出现两行就说明那里有两个停靠点（中间隔着被折叠的标记）。

---

## 11. 文件地图

| 关注点 | 文件 |
|---|---|
| 文本与撤销 | `crates/mdview-core/src/document.rs`、`transaction.rs` |
| 块切分 | `crates/mdview-markdown/src/split.rs` |
| 投影（差集、图片原子化、内联代码内缩） | `crates/mdview-markdown/src/project.rs` |
| 片段模型、`align`、HTML | `crates/mdview-markdown/src/block.rs` |
| **视觉布局与停靠点** | `crates/mdview-markdown/src/layout.rs` |
| 验证页生成 | `crates/mdview-markdown/src/preview.rs` |
| 状态机与 Effect | `crates/mdview-app/src/state.rs` |
| 端口（FileStore / RenderSink / Watcher） | `crates/mdview-app/src/ports.rs` |
| 日志滚动 | `crates/mdview-support/src/logging.rs` |
| 命令行入口 | `crates/mdview-cli/src/main.rs` |

---

## 12. 协作约定

- 提交信息用 **Angular 规范**（`type(scope): subject`），subject 简洁中文。类型用 `feat` / `fix` / `refactor` / `test` / `docs` / `chore`。
- 分支名不含 `/`。
- 改动解析或布局层时，必须同时补测试；`layout` 的停靠点行为是回归高发区。
- 不引入非纯 Rust 依赖到 `mdview-markdown`。
- 每次实证结论（尤其是与直觉相反的）写进本文或 `.workbuddy/memory/`，避免下一轮重复调研。

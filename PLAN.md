# PLAN.md — mdview 开发计划与执行进度

> 最后更新：2026-11-08 20:42
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
   - 输入法只认真正的可编辑元素，所以 `#doc` 之外必须有一个透明 `textarea` 承接焦点，位置实时贴住 caret（候选窗跟着它）。
   - `compositionend` 之后紧跟的那一下回车要吞掉：浏览器会补发 `keydown`，此时 `isComposing` 已是 false，不拦就多分一段。
   - `compositionend` 的 `data` 可能为空（Safari），必须退回输入框的 `value`。
   - **取消组合也必须给内核发消息**（`CompositionCancel`）。内核的 `composing` 只认结束信号，漏发就永久挂起，之后所有输入被静默吞掉——现象是「编辑器死了」，且日志里什么都没有。失焦同样要自己收尾。
   - 前端资源由 `generate_context!` 在编译期嵌入，`build.rs` 要写 `rerun-if-changed=web`，否则改了 JS 窗口行为不变。
10. **同步滚动按锚点块插值**，禁止用滚动百分比换算。
11. **文档必须永远至少有一个块**：`split_blocks` 在「整篇为空」或「以空行结尾」时补一个零长块。否则空文档没有停靠点、在末尾回车后光标只能贴在上一段行尾。（改动前 `split` 会丢弃空行段，M3 实测踩到）
12. **纵向位置一律问浏览器**：视觉行、行高、折行只有渲染器知道。`ArrowUp/Down`、`Home/End` 这类「上一行/行首」的判断必须基于 `getBoundingClientRect`，不能在核心里用「视觉列 + 块索引」推算——内核不知道一个字有多宽，会把折行当成块边界。反过来，横向移动（左/右、点选）仍然走内核的停靠点表，因为「哪些字节不可见」只有解析层知道。两套坐标的分界线就是这条。
13. **不能让光标停在「视觉上看不见的位置」**：同一视觉列上的多个停靠点，只有在编辑语义确实不同、且用户能通过操作区分时才可以都保留。块首折叠标记（`**`、`# `、`- `、`> `）左侧没有可见文字，它的「标记之前」必须不产生停靠点，否则 Delete 会整对去格式而不是删字，`←` 还会空转一步。判定见 `Layout::build` 的 `has_visible`。
14. **空段落必须被空行夹在中间**：块只能由非空行组成，所以文档中间的「等待输入的空段落」只能写成三行空行——首尾两行当分隔，中间那行才是空段落（`split_blocks` 的 `collect_blank_paragraphs`）。只留一行空行当空段落的话，用户敲下第一个字符时那一行就不再是空行，文字立刻粘到隔壁块上（`- 乙\nX` 是列表项的懒续行，`甲\nX` 是同段落软换行），而且事后再也补不回来。
15. **拖动选区时的落点探针不能压在视口边缘上**：视口最下面那几十像素归固定状态栏 `#bar` 所有，`elementFromPoint` 命中的是状态栏而不是正文，落点一路返回 null——现象是「页面自己在滚，选区一动不动」，日志里什么错都没有。探针点必须先压进「正文可见区」（`probeBox()` = 正文矩形 ∩ 视口 ∩ 状态栏上沿），见 `dragscroll.js` 的 `clampToBox`。

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
  mdview-desktop/   Tauri 2 外壳：session.rs（IPC 契约，可单测）+ web/（只做像素与输入）
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
- ✅ **无序 / 有序列表区分渲染**（2026-11-08 修复）
  - 两个根因：① `classify` 只认块的第一个事件，而松散列表（项间有空行）会被切块拆成多块，后半段块的首个事件是 `Item`，整块退化成段落——序号和项目符号一起消失。现在 `project.rs` 维护「打开中的 List 容器」栈，首事件是 `Item` 的块按栈顶判 `List { ordered }`
  - ② `.li::before` 写死 `content: "•"`。现在每项从**源码**读出自己的标记（`item_marker`：数字 + `.`/`)`）写进 `data-m`，CSS 用 `attr(data-m)` 显示。不用 CSS counter：用户写 `3.` 就显示 `3.`，`1)` 就显示 `1)`，从别处粘来的半截列表不会被悄悄改号
  - 阅读态 `to_html` 顺带修好：以前列表输出 `<ol>甲<br>乙</ol>`，没有 `<li>`，浏览器只给一个序号；现在按项切 `<li>`，有序列表带 `<ol start="N">`。为此 `to_html` 需要 `src` 参数（`<li>` 边界与起始号只能从源码读）
- ✅ **Tauri 2 外壳**（`crates/mdview-desktop`）：`frontendDist` 指向内嵌的 `web/`，`session.rs` 隔离 IPC 契约（前端 `kind` 字符串 → `Message`），窗口创建与 IPC 已用启动探针在日志中验证
- ✅ 前端渲染：按块落地、增量应用 `Patch`、用停靠点 `data-b`/`data-a` 的左右边缘量出 caret 像素、点击/拖动报停靠点
- ⬜ 主题系统、大纲侧栏
- ⬜ 同步滚动（锚点插值）

### M3 可编辑 🔶
- ✅ 视觉位置索引（停靠点表）、`Caret { block, stop }`、`next/prev/home/end`
- ✅ 选区模型与按块切高亮
- ✅ **输入翻译器骨架**（`AppState::update` 内闭环）：`InsertText` / `Backspace` / `DeleteForward` / `Newline` / `HardBreak` / `MoveCaret{delta,extend}` / `CompositionStart` / `CompositionCommit`
  - 一次编辑 = 一次 `Transaction` = 一步撤销；编辑后按 char→byte→停靠点（`Gravity::Upstream`）回填光标，闭合标记右侧不会掉出样式
  - 回车按块类型分派：段落 `\n\n`、代码块/表格 `\n`、列表延续标记（有序序号 +1）、空项回车退出该层、引用延续 `> `
- ✅ **增量重绘帧**：`block_frames` 出每块 HTML + 全局停靠点区间，`diff_frames` 用前后缀比对只替换变化段；编辑只发 `Effect::Patch`，结构变化才发 `Effect::Render`
- ✅ **桌面端输入接线**：`keydown` / `paste` / 鼠标 → `Wire` → `Message`；消费 `Patch` 做块节点替换，用 `MoveCursor { stop, block, visual }` 对齐像素
- ✅ **空白处点击定位**：`elementFromPoint` 没命中字符时（行尾右侧、行间、块内留白），退到 `caretRangeFromPoint` 借浏览器的命中测试定位视觉行，再按 x 取行首/行尾。点在行外直接取端点而不走「最近」——词尾有两个同位置停靠点，取「最近」会落到与 `End` 不一致的那个
- ✅ **输入法输入层**：透明 `textarea#ime` 贴住 caret 承接 IME（候选窗跟着走），`#preedit` 镜像层按光标所在块的字体画 preedit，`web/ime.js` 是纯状态机（不碰 DOM，Node 可直接测）
  - 组合期只发一条 `CompositionStart`，之后不重绘、不碰 `#doc`，避免冲掉浏览器正在渲染的 preedit
  - 提交走 `CompositionCommit`，整段一次事务 = 一步撤销；取消候选走 `CompositionCancel` 复位挂起态（漏了会让内核永久吞输入）
  - 补掉「确认候选那一下回车」在 `compositionend` 之后补发 `keydown` 导致的误分段
  - 失焦时自行收尾，不依赖浏览器补发 `compositionend`
  - `--self-test` 在真实 WebView2 里合成 composition 事件，跑通「事件 → 状态机 → IPC → 事务 → 补丁 → 光标 → 撤销 → 取消后恢复输入」，当前 11/11
- ✅ **标记整删**：`Block.markers` 由容器事件推出成对标记，退格/Delete 落在标记里时整对抽掉并保留中间原文，一次事务、一次撤销
- ✅ **块首不产生不可见停靠点**：`**`／`# `／`- `／`> ` 左侧没有可见文字时只留右端。否则「标记之前」与「文字之前」同列同位置，光标停进去之后 Delete 变成整对去格式（与退格一样），而 ← 还会先空转一步
- ✅ **上下方向键按真实视觉行移动**（前端像素判据，`web/caretline.js`）
  - 放前端的原因：软换行只有浏览器知道。内核只认「停靠点 + 视觉列」，它眼里的下一行只能是下一块，长段落里按一下会整段跳走
  - 判据：同一视觉行 = 矩形纵向重叠超过较矮者一半；目标是下一行里离当前 x 最近的停靠点
  - 期望 x 跨多次按键保持（经过短行不会缩水），任何非上下移动的操作都重置
  - 按键连发用 `pendingStop` 记住「已发出未回包」的落点，否则每下都从同一位置起算、原地不动
  - `--self-test` 里验证真实像素：从标题 ↓ 到正文块后 x 仍为 122（对齐同一列）
- ✅ **`Home` / `End` 按视觉行**：同一模块的 `edge(stop, ±1)`；单行段落里与块首/块尾等价，软换行段落里落到行首/行尾
- ✅ **删空容器一并收起**：删除区间扩到语法边界（`expand_deletion`）
  - 与折叠标记相交就吃到边界，选区切进标记一半也不会留下 `*粗**`
  - 容器内容被吃空就**连标记一起删**，否则 `**粗**` 删掉 `粗` 后剩下 `****`，pulldown 不再当容器，四个星号变成可见文字；同理 `~~~~`、`` `` ``
  - 必须在删的当下做：源码一旦退化成字面文本，配对信息就没了
  - 嵌套容器连锁收起（`**a *b* c**` 删空内容 → 整体消失）
- ✅ **空项回车不留「跳行」**：列表/引用的空项回车时会删掉标记退出该层，光标必须落在**空段落**上
  - 空项正好是文档最后一行（连行尾换行都没有）时：把整行换成空段落（`\n`），光标落进新段落。否则整篇只剩列表本身、没有块能承接光标，`stop_at_byte` 把光标退回上一行末尾——用户看到「回车把光标送回列表最后一行」，而且再也离不开列表（下一次回车又造出一个空项）
  - 后面还有内容时：删标记只留下块间距（空行不属于任何块），光标会吸附到下一块的行首——用户看到「回车没换行，光标跳到了下一行的开头」。现在按需要补空行，凑出「分隔 + 空段落 + 分隔」三行，光标落在中间那行
  - 后面只剩空行时：末尾空段落本来就接得住光标，删掉标记即可，不必再堆空行
  - 空段落里再按回车只往下走一行（`split_paragraph` 在零长块里只插一个换行）：否则一次多出两行空行，像在跳行
- ✅ **标题必须「成形」才升格**：ATX 标题要求 `#` 后有空格。pulldown 把行尾裸 `#` 也当合法标题（内容为空），于是逐字输入会出现两次跳变：敲 `#` 整行先变成一级标题（`#` 被折叠、字号变大），再输入文字又掉回段落（`#` 重新可见）。现在 `heading_is_bare` 把「只有 `#`、后面没空格」的块降级成段落——标记始终可见，敲下空格那一刻才升格，之后不会再变回来
- ✅ **选区拖动到窗口边缘时的自动滚动**（`web/dragscroll.js`）
  - 纯几何不碰 DOM，Node 直接测（`dragscroll.test.js` 16 项）；帧循环、滚动、落点回写全部由 hooks 注入
  - **必须自走帧循环**：拖选靠 `mousemove` 驱动，而指针停在边缘时浏览器不再发新事件，选区就冻结在那一屏。这里每帧「滚一点 + 重新求指针下的落点」，指针不动、页面在动，落点自然一路延伸
  - 速度与「陷进边缘多深」成正比（48px 边缘带，满速 24px/帧），拖出窗口后封顶；视口极矮时边缘带压到高度的一半，免得上下两条带互相吃掉、滚动永远停不下来
  - 探针点先压进 `probeBox()`（正文 ∩ 视口 ∩ 状态栏上沿）。踩过的坑：直接压到视口下沿会命中固定状态栏，`elementFromPoint` 拿不到正文 → 落点全是 null，页面在滚而选区不动（自检抓到的就是这条）
  - 三个收尾口子都要堵：`mouseup`、`blur`（切走窗口收不到 mouseup）、`mousemove` 里 `buttons === 0`（在窗口外松手）
  - 拖动期间抑制 `revealCaret`：光标就在指针底下，本来就在视口里；追视口会每帧多滚 30px、越过指针对应那一行，观感是一顿一顿的
  - `--self-test` 三项：自动滚了 `+384px` / 选区 head 从 0 伸到 348 / 松手后 `384 → 384` 不再动（rAF 被节流的窗口会显式跳过而不是假红）
- ✅ **原生选区隔离**：`selectionchange` 时清掉浏览器自己的选区（Ctrl+A、焦点回到 body 之后的拖动、程序化 `addRange` 都会留下它），页面上只保留自绘的 `.sel`；判据在 `selection.js`，4 项 Node 用例覆盖
- ✅ **全选 / 复制 / 剪切**：`selectAll` / `copy` / `cut` 三条消息，`Ctrl+A` / `Ctrl+C` / `Ctrl+X` 全部走内核
  - 复制出去的是**源码**而不是可见文本（视图里 `**`、`# ` 是折叠的，照 DOM 拼出来再粘贴回去格式就没了）
  - 整篇选中（首尾贴住文档两端，拖选到头也算）时直接取文件全文：块首折叠标记左边没有停靠点（硬约束 13），按停靠点取会漏掉第一行的 `# `
  - 剪切 = 复制 + 删选区；删除仍走 `expand_deletion`（掏空 `**粗**` 时连标记一起收），剪贴板里只放选中的那段，一次事务可整体撤销
  - 内核不碰剪贴板：`Effect::Clipboard { text }` 只产出文本，写入由前端 `clipboard.js` 完成（`navigator.clipboard` → `execCommand` 兜底）
- 🔶 输入格式化：回车拆分与列表/引用延续已做；Tab 缩进、`**` 自动配对、Delete 合并块未做

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

- 日期：2026-11-08
- 测试：**Rust 163 项全过**（markdown 79 / app 58 / desktop 14 / support 7 / core 5），无编译告警；desktop 那一组里 5 项是前端守门（`node --check` 全部脚本（含 `clipboard.js`、`dragscroll.js`）+ 四套 Node 用例：`ime` 13 项、`caretline` 21 项、`selection` 4 项、`dragscroll` 16 项，共 54 项）
- 桌面端已能起窗：`cargo run -p mdview-desktop -- <file>`。日志里出现「前端就绪：N 块 / M 停靠点」才说明页面与 IPC 都活着
- 自检：`cargo run -p mdview-desktop -- crates/mdview-desktop/tests/fixtures/selftest.md --self-test`，日志出现「自检通过：OK 34/34」即链路通（含 IME 11 项、上下移动 7 项、Home/End 2 项、点击空白 2 项、**拖动自动滚动 3 项**、视口 1 项、全选/复制/剪切 8 项）。**失败时报告写进 `logs/error.log` 而不是 `mdview.log`**，排查别只看后一个
  - 样张必须是 `tests/fixtures/selftest.md`：首块是标题、第二块是**单行**段落、第三块是列表，且总长可滚 200px 以上。随手拿长段落文档跑会误报三项——软换行会让 `End` 落在视觉行尾、让 `↓` 停在同块第二行，而用例的假设是「一块一行」
- **真实输入法已人工验收**（2026-11-07）：候选窗贴住光标不跑偏、preedit 覆盖层表现正常
- 最近提交（Angular 规范）：
  - `feat: 初始化内核、解析投影与日志`
  - `feat(markdown): 新增视觉布局与停靠点表`
  - `feat(markdown): 新增可交互投影页与结构断行`
  - `feat(mdview): 打通输入到光标的编辑闭环与增量重绘`
  - `feat(desktop): 新增 Tauri 外壳与前端编辑接线`
  - `feat(desktop): 接入输入法镜像输入层与自检`
  - `fix(desktop): 取消组合时复位内核挂起态，避免输入被永久吞掉`
  - `feat(mdview): 退格与 Delete 按标记对整删，不再啃坏语法`
  - `feat(desktop): 上下方向键按真实视觉行移动`
  - `feat(desktop): Home/End 按视觉行两端定位`
  - `fix(mdview): 删空容器时连标记一起收起，不再留下可见星号`
  - `fix(markdown): 块首折叠标记不再产生不可见停靠点`
  - `feat(desktop): 点击行尾空白也落到这一行的末尾`
  - `fix(markdown): 裸 # 先按段落显示，敲空格才升格为标题`
  - `fix(app): 空项回不到上一行，文末退出列表后光标落进新段落`
  - `fix(markdown): 空项退出列表时留下可停靠的空段落`
  - `feat(app): 全选、复制、剪切走内核，Ctrl+A/C/X 接上`
  - `feat(desktop): 拖动选区到窗口边缘时自动滚动`
  - `fix(markdown): 有序列表显示序号，松散列表不再退化成段落`
- 分支：全部特性/修复分支已按线性顺序快进并入 `main`（`3110c20`），`origin/main` 只差在 `feat: 初始化内核…`，普通 `git push origin main` 即可，不需要 force

---

## 8. 已知缺陷与技术债

按影响排序，接手时不要误以为是新 bug：

1. ~~**删字后留下空标记**~~ **已修（M3）**：删除区间会把「内容被吃空的容器」连标记一起收起。关键是**必须在删的当下做**——`**粗**` 删掉 `粗` 之后源码是 `****`，pulldown 已不把它当容器，事后再看就没有配对了。同一函数还负责「选区切进标记一半时扩到整对」。
2. **表格没有网格**：markdown 层把 `|` 与 `---` 折叠后，表格退化成普通文本流。当前只做到「行分行、单元格加间隙」。
3. **每字符一个 DOM 元素**：`preview.rs` 为了让停靠点可寻址，把每个字符渲染成独立 `<span>`。副作用是英文单词任意处可断行，DOM 节点数也偏高。生产实现应改为「每片段一个元素 + 内核 `align` 换算 char↔byte」。
4. **Windows 原子写窗口期**：见 M1。
5. **无虚拟化**：全文档一次性建 DOM。5 万字以上会卡，但暂无此需求。
6. **图片只渲染占位符**：不加载真实图片，路径沙箱与尺寸测量未做。
7. **`Watcher` 无实现**：外部修改只在保存时靠指纹被动发现。
8. **列表嵌套未验证**：多级列表的 `Item` 边界换行只测了单层。
9. ~~**退格会先吃掉标记字符**~~ **已修（M3）**：`Block.markers` 记录成对标记（`**…**`、`` `…` ``、`[…]() `），配对来自 pulldown 的容器事件而不是猜。退格/Delete 落在标记里时按「整对抽掉、保留中间原文」处理，一次事务完成。副作用是**语义选择**：在加粗词首退格 = 去掉加粗，而不是删掉前一个可见字。想反过来的话改 `marker_edit_at` 的调用点即可，但那样就没有「不删字去格式」的入口了。
   - 由此带出**重合停靠点**的坑，两端已经区别处理：
     - **词首（已修）**：块首 `**`、`# `、`- `、`> ` 的左右两端落在同一视觉列，「标记之前」这个位置用户永远分不清，Delete 会变成「整对去格式」而不是删第一个字。现在**左侧没有可见文字时只产生右端停靠点**（`Layout::build` 的 `has_visible`），← 也不会再空转一步。
     - **词尾（保留）**：闭合标记之前 / 之后仍是两个停靠点——前者退格删字、后者去格式。点选与 `End` 落到后者，`←` 先经过前者。要统一为「删字」也行，但会失去「不删字只去格式」的入口，等手感反馈再定。
10. ~~**原生选区未隔离**~~ **已修**：`selectionchange` 时清空浏览器选区，只留自绘 `.sel`；透明输入框内部的选区放行（清掉会打断输入法）。`#doc ::selection` 留了一条透明兜底，防清理晚一帧时两套高亮叠加。**Ctrl+A 已由内核的 `selectAll` 补上**（见第 9 节），代价是这三个组合键全都不再由浏览器提供：撤销、粘贴仍是原生行为，但复制走剪贴板 API 后**不写 `text/html`**——粘贴到 Word 里只有纯文本。
11. ~~**`Home` / `End` 按块首块尾**~~ **已修（M3）**：改用 `caretline.js` 的 `edge()`，按视觉行两端定位；矩形取不到时回退到块首/块尾。
12. **上下移动依赖当前视口的矩形**：`desiredX` 是视口坐标，窗口缩放或正文容器宽度变化会让它失效，因此 `resize` 时重置。若将来支持水平滚动或分栏，需要一并重置。
13. **空标题显示为字面 `#`**：这是「标题必须成形」的刻意取舍（见第 6 节 M3）。代价是文件里真有 `#` 独自成行的空标题时，打开后会显示成段落文本 `#`。要区分「用户正在输入」与「文件本来如此」，得把光标位置引到投影层；目前不做。
14. **空段落 = 三行空行**：文档中间的空段落（退出列表后等待输入的那一行）只能靠「分隔 + 空段落 + 分隔」三行空行表达（见第 2 节硬约束 14）。代价有两条：① 手写的文件里本来就有三行以上空行时，中间那几行会渲染成空段落，视觉上比之前多一行；② 空段落没被使用时，文件里就留着三行空行。敲下第一个字之后会自动收敛成正常的两段（`- 甲` + 空行 + `X` + 空行 + `正文`），所以只有「退出列表但没写字」的瞬间才看得到。
15. **滚动与光标跟随的边界**：页面滚动时只重画光标像素、不再「保证可见」；落点没变也不追视口（不然任何一次重绘回包都会抵消用户的滚动）。这条无法用 `--self-test` 回归：合成出来的滚动是程序化滚动，Chromium 会抑制滚动期间的脚本滚动，有没有这层修复都测不出差别；能测的只有 `scrollDelta` 的几何（`caretline.test.js`）与「按键后光标重新可见」。
16. **剪贴板写入可能在无焦点窗口里挂起**：`navigator.clipboard.writeText` 在窗口没拿到焦点时，Chromium 会把请求挂住等一个不会来的授权，Promise 永远不 settle。生产里只影响「复制好像没反应」，但**自检会整条卡死**——所以自检里对它的等待必须自带 `Promise.race` 超时。另外 `execCommand('copy')` 兜底会短暂抢走 `#ime` 的焦点，调用方要还回去，否则输入法组合被中断。
17. **其它块级标记的同类抖动未处理**：`>`（引用）与 `***`／`---`（分隔线）也会在敲完最后一个字符的瞬间改变块类型。它们不涉及字号跳变，先不动；若将来统一处理，应复用同一套「成形」判定。

---

## 9. 下一步（按推荐顺序）

1. ~~**`selectAll`**~~ **已完成**：全选 / 复制 / 剪切三条消息 + `Ctrl+A` / `Ctrl+C` / `Ctrl+X`。剩下的相关缺口是 **粘贴**——`paste` 事件直接把 `clipboardData` 的纯文本发给 `InsertText`，还没有 HTML→Markdown 的规范化（见 M4）
2. ~~**选区拖动到窗口边缘的自动滚动**~~ **已完成**：`web/dragscroll.js` + `--self-test` 三项。剩下的相邻缺口只有「拖到窗口左右两侧时横向自动滚动」——正文不横向滚动，当前不需要。
3. **词尾重合停靠点的退格语义**：词首已经收敛为一处（不可见的那端不再产生停靠点），词尾仍保留两个（见第 8 节第 9 条），需要按实际手感定方向。
4. **表格子工程**：`<table>` 渲染 + 单元格内停靠点，替代当前的文本流占位。这是剩余最大的一块。
5. **`Watcher` 落地**（`notify`），把外部修改从「保存时被动发现」变成主动提示。
6. **引用 / 分隔线的成形判定**：见第 8 节第 17 条，与标题同源，可复用判定函数。

每完成一项，更新第 6 节状态、第 7 节快照，并用 Angular 规范提交。

---

## 10. 命令速查

```bash
cargo test --workspace                      # 全量测试
cargo run -p mdview-cli -- demo             # 端到端演示（edit/click/undo/save）
cargo run -p mdview-cli -- demo edit        # 键盘脚本回放：输入 → 光标 → 增量帧
cargo run -p mdview-cli -- <file> outline   # 大纲
cargo run -p mdview-cli -- <file> stats     # 字符/字节/块/隐藏区间统计
cargo run -p mdview-cli -- <file> blocks    # 块清单
cargo run -p mdview-cli -- <file> spans     # 逐块投影，⟦⟧ 内为折叠标记
cargo run -p mdview-cli -- <file> layout    # 停靠点表：列 / byte / 字符（← 同列）
cargo run -p mdview-cli -- <file> preview   # 生成 <file>.preview.html
cargo run -p mdview-cli -- <file> html      # 阅读态 HTML
cargo run -p mdview-desktop -- <file>       # 起窗（Tauri 2）
cargo run -p mdview-desktop -- crates/mdview-desktop/tests/fixtures/selftest.md --self-test
                                            # 端到端自检（含拖选自动滚动），跑完自动退出
node crates/mdview-desktop/web/ime.test.js  # 输入法状态机单测（cargo test 已含）
node crates/mdview-desktop/web/caretline.test.js  # 上下移动几何单测（cargo test 已含）
node crates/mdview-desktop/web/selection.test.js # 原生选区判据单测（cargo test 已含）
node crates/mdview-desktop/web/dragscroll.test.js # 边缘自动滚动几何单测（cargo test 已含）
```

`--self-test` 会在真实 WebView2 里合成 composition 事件与方向键，把结果写进日志并以 0/1 退出，是唯一不需要人打字的端到端验证。它同时是唯一能验证「像素对齐」的地方——纯 Node 用例只能喂合成矩形。

日志落在可执行文件同级 `logs/`：`mdview.log`（5×1MB 滚动）、`error.log`（error 独占，5×1MB）。只读目录自动回落到 `%LOCALAPPDATA%/mdview/logs`。

`layout` 是最有价值的调试入口：同一列出现两行就说明那里有两个停靠点（中间隔着被折叠的标记）。

---

## 11. 文件地图

| 关注点 | 文件 |
|---|---|
| 文本与撤销 | `crates/mdview-core/src/document.rs`、`transaction.rs` |
| 块切分（含末尾空段落） | `crates/mdview-markdown/src/split.rs` |
| 片段模型、`align`、HTML、**标记配对表**（`Block::markers`） | `crates/mdview-markdown/src/block.rs` |
| 投影（差集、图片原子化、内联代码内缩、标记配对推导） | `crates/mdview-markdown/src/project.rs` |
| **视觉布局与停靠点** | `crates/mdview-markdown/src/layout.rs` |
| **分块帧与增量 patch**（`block_frames` / `diff_frames`）+ 验证页 | `crates/mdview-markdown/src/preview.rs` |
| **编辑闭环**（输入 → 事务 → 光标回填 → patch） | `crates/mdview-app/src/state.rs` |
| 端口（FileStore / RenderSink / Watcher） | `crates/mdview-app/src/ports.rs` |
| 日志滚动 | `crates/mdview-support/src/logging.rs` |
| 命令行入口（含 `demo edit` 回放） | `crates/mdview-cli/src/main.rs` |
| **IPC 契约**（前端 `kind` → `Message`、`Patch`/光标回包） | `crates/mdview-desktop/src/session.rs` |
| **编辑页**（停靠点 → 像素、按键 → 消息、增量应用、自检入口） | `crates/mdview-desktop/web/app.js` |
| **输入法状态机**（不碰 DOM，Node 可测） | `crates/mdview-desktop/web/ime.js`、`ime.test.js` |
| **上下移动几何**（视觉行判定 + 期望列，不碰 DOM，Node 可测） | `crates/mdview-desktop/web/caretline.js`、`caretline.test.js` |
| **视口与原生选区**（滚多少才露头、哪些原生选区该清，不碰 DOM） | `crates/mdview-desktop/web/caretline.js`（`scrollDelta`）、`selection.js`、`selection.test.js` |
| **拖选时的边缘自动滚动**（速度曲线、探针点压缩、自走帧循环，不碰 DOM） | `crates/mdview-desktop/web/dragscroll.js`、`dragscroll.test.js` |
| 自检样张（必须是「一块一行」的文档） | `crates/mdview-desktop/tests/fixtures/selftest.md` |
| **剪贴板写入**（内核给文本、前端写系统剪贴板，`execCommand` 兜底） | `crates/mdview-desktop/web/clipboard.js` |
| **键位契约**（`Ctrl+A/C/X` → `selectAll`/`copy`/`cut`，见 `app.js` 的 keydown） | `crates/mdview-desktop/web/app.js`、`crates/mdview-desktop/src/session.rs` |
| 前端守门测试（语法检查 + 跑两套 Node 用例） | `crates/mdview-desktop/tests/web_ime.rs` |

---

## 12. 协作约定

- 提交信息用 **Angular 规范**（`type(scope): subject`），subject 简洁中文。类型用 `feat` / `fix` / `refactor` / `test` / `docs` / `chore`。
- 分支名不含 `/`。
- 改动解析或布局层时，必须同时补测试；`layout` 的停靠点行为是回归高发区。
- 不引入非纯 Rust 依赖到 `mdview-markdown`。
- 每次实证结论（尤其是与直觉相反的）写进本文或 `.workbuddy/memory/`，避免下一轮重复调研。

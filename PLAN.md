# PLAN.md — mdview 开发计划与执行进度

> 最后更新：2026-11-21（前端外壳专项计划并入本文，全仓只保留这一份计划）
> 适用对象：接手本项目的 AI 或开发者。**本文自包含**，读完即可继续，不需要任何历史对话。
> 每次有实质进展请同步更新第 6 节的状态和第 7 节的快照。
> `ui-vue` 专项分支已并入 `main`，其计划内容（留洞式覆盖层、构建链、状态桥、迁移阶段的踩坑）已收进第 4 节与 M5，**不再单独维护分支计划文档**。

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
   - 前端资源由 `generate_context!` 在编译期嵌入，`build.rs` 要写 `rerun-if-changed=../dist`（并对整目录做指纹），否则改了 JS 窗口行为不变。
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

## 4. 工程结构

### 4.1 Rust crate

```
crates/
  mdview-core/      领域层：Document(ropey) / Transaction / History / Anchor / Selection
                    不依赖 GUI、文件系统、时钟
  mdview-markdown/  解析与投影：split / project / block / sourcemap / layout / preview
  mdview-support/   运行期基础设施：logging（滚动落盘）/ error（AppError）
  mdview-app/       状态机：State / Message / update / Effect + ports trait
  mdview-cli/       无头入口，所有验证都从这里跑
src-tauri/          桌面端壳（原 crates/mdview-desktop 整体上提到仓库根）
                    main.rs（窗口/菜单/文件对话框）/ session.rs（IPC 契约，可单测）
```

依赖方向：`cli → app → markdown → core`，`support` 被 app 与 cli 共用。**core 不得依赖 markdown**。

`mdview-markdown` 内部：
- `split.rs`：按空行切块（围栏感知，去尾部换行）
- `project.rs`：单次全篇解析，事件按 `range.start` 二分归属块，产 `Block { spans, hidden }`
- `block.rs`：`Span { kind, byte, text, style }`、`align()`（投影文本 ↔ 源码字节）、`to_html()`（阅读模式/调试用）
- `sourcemap.rs`：byte ↔ 块
- `layout.rs`：**视觉布局**，产停靠点表（见下节）
- `preview.rs`：生成可交互 HTML，标定用

### 4.2 前端目录（标准 Tauri 布局）

前端工程根 = 仓库根：`src-tauri/` 放 Rust，`src/` 是前端源码，`dist/` 是 Vite 产物（不入库）。与 `create-tauri-app` 的标准形态一致，不再有「Rust 占了 `src/` 所以前端只能叫 `web/`」的妥协。

```
<repo-root>/
├─ index.html                 Vite 入口（#vue-app 覆盖层 + 洞）
├─ package.json / vite.config.ts / tsconfig.json / vitest.config.ts / mise.toml
├─ src/
│  ├─ main.ts                 单入口：按序 import 全部脚本 + mount('#vue-app')
│  ├─ platform.ts             首帧 UA 判定（原 index.html 内联脚本，为满足 CSP 挪出）
│  ├─ App.vue / bridge.ts / menu.ts / scrollbars.ts
│  ├─ styles/                 全部样式，四层：app → skin → dialog → vue-overlay
│  ├─ assets/                 前端素材（logo.png，由应用图标降采样得到）
│  ├─ components/             TitleBar / MenuPopup / StatusBar / Toast
│  └─ editor/                 编辑内核，与外壳分家
│     ├─ editor.ts            停靠点 → 像素、按键 → 消息、增量应用、自检入口
│     ├─ ipc.ts               IPC 契约类型（与 session.rs 逐条对齐）
│     ├─ ime / caretline / selection / dragscroll / clipboard / outline / theme（七个 .js）
│     └─ __tests__/           八套用例（六 unit + 两 dom），与实现靠子目录分开
├─ dist/                      Vite 产物（gitignore）
└─ src-tauri/                 Rust 桌面端 + tauri.conf.json + capabilities + icons + tests/fixtures
```

`src/editor/editor.ts` 是编辑内核投影层（全量 TS，无 `@ts-nocheck`），契约类型在 `src/editor/ipc.ts`；七个纯逻辑模块保持 `.js` 原样（vitest 的 node project 直接加载，守门测试用 `node --check` 过一遍语法）。
外壳（标题栏 / 菜单 / 状态栏 / toast）由 `src/components/*.vue` 渲染，经 `src/bridge.ts` 与内核单向通信。大纲与正文区是覆盖层上的「洞」，不进任何组件模板。

### 4.3 前端外壳：留洞式 Vue 覆盖层

**范围**：Vue 3 + Vite（TS）只接管外壳——标题栏、菜单弹层、状态栏、toast。动机是原 `titlebar.js` 464 行菜单树 + 递归渲染 + 子菜单 + 勾选态难维护，组件化能显著收敛。

**红线**：编辑内核一行都不迁、一行都不改——`editor.ts` 的投影 / `reindex` / `applyPatch` / 光标定位 / 点击拖选落点 / `revealCaret` / 视口坐标，七个纯逻辑模块，Rust 侧 `session.rs` 契约与 `main.rs` 窗口逻辑，全部原地保留（只挪目录、改注释）。

```
视口 100vh；window 仍是滚动容器（window.scrollY 必须保持有效）
┌─ #vue-app  position:fixed; inset:0; pointer-events:none ─────────┐
│  TitleBar.vue        AUTO（菜单按钮 / 居中标题 / 窗口按钮 / 拖拽区）│
│  ├─ MenuPopup.vue    AUTO（动态弹层）                             │
│  ├── 洞 1：大纲  #outline #outline-scroll   无 Vue 节点 → 穿透    │
│  ├── 洞 2：正文  #doc #caret #ime #preedit #vscroll → 穿透        │
│  StatusBar.vue       AUTO（统计 / 脏标记 / 主题 / 表格工具条）     │
│  Toast.vue           NONE                                        │
└──────────────────────────────────────────────────────────────────┘
        │ 洞里没有 Vue 节点，鼠标与事件直接落到原有 DOM
        ▼
  原有层全部原地保留：#titlebar / #bar / #toast（占位供 edges() 读几何）
```

**核心**：大纲和正文不是「Vue 画的镜像」，而是**真正的洞**——Vue 层在那两处不放任何节点。所以没有第二套大纲/正文 DOM，「同一状态两套 DOM」的同步难题整块消失。

**命门判据**：`pointer-events:none` 只解决「鼠标命中谁」，不改变「谁在滚动」。`editor.ts` 那套坐标能否零改动，唯一取决于 `window.scrollY` 是否仍有效——所以 `#doc` 必须留在文档流、`min-height:100vh` 撑高 body。

方案演进（别重复踩）：

| 方案 | 结论 | 原因 |
|---|---|---|
| `v-pre` 跳过 `#doc` 子树 | 否决 | 依赖编译器 hoist 行为；`#doc` 误绑 `:class`/`v-if` 即失效 |
| Vue 挂载点物理隔离（只 mount `#ui`） | 采纳为地基 | 边界在 DOM 结构上硬隔离，不依赖编译器 |
| `#doc-placeholder` + `#doc` 绝对定位叠加 | 否决 | `#doc` 脱离文档流后不撑高 body，`scrollY` 恒 0，坐标全要重写 |
| `#doc` 自带滚动的 fixed 工作区 | 否决 | 滚动容器换了，坐标来源整体要换 |
| Vue 整层叠在现有实现上 | 否决 | 同一状态两套 DOM，同步机制别扭 |
| **只穿透大纲和正文（留洞）** | **最终采纳** | 无镜像、滚动不变、坐标零改动，外壳真正由 Vue 接管 |

双层重叠的代价：Vue 的标题栏/状态栏/toast 与同名 DOM 短暂共存，靠「严格像素级复刻尺寸（标题栏 30px、状态栏 29px、大纲 232px）+ 不透明背景 + 上层接管命中」规避，下层按钮收不到点击。macOS 原生标题栏下 `html.native-titlebar` 要同时隐藏 `#titlebar` 与 Vue 的 `TitleBar`，否则多画一条假标题栏。

### 4.4 构建链与路径

- **Vite**：`outDir = 'dist'`、`emptyOutDir = true`（每次清空，防旧脚本残留导致全局符号双份加载——历史上两个脚本顶层 `const` 撞名，`app.js` 整个不执行）、`port = 1450`、`strictPort = true`。`pnpm build` 同时跑 `vue-tsc --noEmit`。
- **Tauri**：`tauri.conf.json` 的 `frontendDist` 相对 conf 所在目录解析，所以是 `../dist`（写成 `./dist` 会指向不存在的 `src-tauri/dist`）；`beforeDevCommand: pnpm dev` / `devUrl: http://localhost:1450` / `beforeBuildCommand: pnpm build` 由 Tauri 在前端根执行。`build.rs` 的 `tauri_build::build()` 自动读同目录 conf，只需保证 `rerun-if-changed=../dist` **加上整目录内容指纹**（`MDVIEW_WEB_STAMP`）：`rerun-if-changed` 只让 build script 重跑，脚本输出没变时目标 crate 仍不重编，「改了 JS 窗口行为不变」就是这么来的（见 §2 第 9 条）。
- **CSP** 保持 `default-src 'self'`，**不开 `unsafe-eval`**：走 runtime-only 构建（`@vitejs/plugin-vue` 预编译模板），运行时不拼 template 字符串。首帧 UA 判定因此从内联 `<script>` 挪进 `platform.ts`。
- **`dist/` 不提交**：`generate_context!()` 编译期就要读产物，所以**自 clone 起、或清空 `dist/` 后必须先 `pnpm build` 再 `cargo`**，`cargo test --workspace` 同样会编译 desktop bin。`tauri dev/build` 有 before 命令兜底，不受此限。
- **相对路径基准**：conf、`build.rs`、守门测试的 CWD 都是 `src-tauri/`；`tauri.conf.json` 的 `../dist`、`build.rs` 的 `../dist`、`web_ime.rs` 的 `../src`、`vite.config.ts` 的 `outDir`、根 `package.json` 的 scripts 互相咬合，改一个就要核对其余。**易漏**：`crates/mdview-app/src/state.rs` 里还有一个跨 crate 读样张的路径（`../../src-tauri/tests/fixtures/selftest.md`），grep `tests/fixtures` 才找得到。
- **根 `Cargo.toml`**：`members` 里 desktop 写 `"src-tauri"`（不能用 `crates/*`，会匹配到已无 manifest 的旧目录），另设 `default-members = ["src-tauri"]`，裸 `cargo run` 默认跑桌面端。
- **脚本必须是 `type="module"`**：`index.html` 现在只留 `<script type="module" src="/src/main.ts">`；漏写会被 Vite 原样输出 `/src/xxx.js`，而 dist 里没有 `src/` 目录，运行时全部 404。旧脚本的顺序依赖与 `window` 全局已由 `main.ts` 的显式 import 取代。
- **全仓 ESM**：根 `package.json` 是 `type: module`，七个 `.js` 模块与用例都按 ESM 写。早期用例用 `require('assert')`，靠 `src/package.json` 的 `{"type":"commonjs"}` 兜住；迁 vitest 后该文件已删除，不要再加回来。
- **vitest 两 project**：`unit`（node 环境，纯逻辑）+ `dom`（真实 Chromium）。三个坑：① `launchOptions` 是 **provider 级**选项（`playwright({ launchOptions })`），写进 `browser.instances[].launch` 会被静默忽略，随后去下载 Playwright 自带 Chromium——本机 `cdn.playwright.dev` 经代理零字节，表现为「卡住不动」，用 `channel: 'chrome'` 复用系统浏览器；② `optimizeDeps.include` 必须显式列 `@tauri-apps/api/core` 与 `@tauri-apps/api/event`，否则首次 import 触发 Vite 中途 reload（`Vite unexpectedly reloaded a test`）；③ `vi.mock` 的工厂会被提升，要引用的数据须放进 `vi.hoisted`。另：`boot()` 末尾会 `invoke('ready')`，断言 IPC 序列时别漏这条。

### 4.5 状态桥契约

`bridge.ts` 是外壳与内核之间唯一通道，**只传状态快照和动作名，不传文档内容、不传 DOM 节点**。

- **状态进 Vue（单向）**：reactive store 字段 `path / title / dirty / blocks / stop / column / revision / inTable / themeMode / theme / mode / outline / activeOutlineIdx / toast`。写入点只有三处：`pushShellState()`（`editor.ts`，每次状态变化推一份快照）、`bridge.setTitle`（标题）、`bridge.notify`（toast 文案与计时）。主题不往 `theme.js` 里塞桥接代码，由 `bridge.ts` 用 `MutationObserver` 跟随 `<html data-theme>`。
- **动作出 Vue（单向）**：菜单项与状态栏按钮 `emit('action', name)` → `bridge.dispatch(name)` → 复用 `window.__mdviewRunAction(name)`，**动作逻辑一行不重写**。窗口最小化/最大化/关闭由 `TitleBar.vue` 直接调 Tauri 窗口 API。
- **全局钩子保留**：`window.__mdviewSelfTest` / `__mdviewRunAction` / `__mdviewSetTitle` / `__mdviewRefreshScrollbar` 继续挂着，Rust 侧 `window.eval` 与自检断言不用动；菜单迁移后自检要多一句 `await window.__mdviewNextTick()`（bridge 暴露 Vue 的 `nextTick`）。
- **异步 DOM 红线**：Vue 渲染是异步的。自检里「派发事件后同步查 DOM」的写法要改成 `nextTick` 之后；尺寸相关测量要 `nextTick` + `requestAnimationFrame` 之后再算，否则 caret / 锚点会在布局稳定前测错。
- **穿透例外**：菜单按钮、`#tb-min/max/close`、`[data-tauri-drag-region]`、状态栏表格工具按钮必须显式 `pointer-events:auto`，漏了拖拽区就拖不动窗口。大纲与自绘滚动条在洞里，天然不受穿透规则影响。
- **组件模板保留原有 `id`/`class`**，让自检断言零改动；确需改断言时同步改 `editor.ts` 的 `__mdviewSelfTest` 与 `tests/` 下的用例，不允许只改一边。

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

### M1 内核 ✅
- ✅ `Document`：ropey 包装、`byte_to_char` / `char_to_byte` / `slice` / `revision`
- ✅ `Transaction` / `History`：逆操作撤销、undo/redo 栈
- ✅ `Anchor` / `Selection`：带 Bias 的位置锚点，可随 Transaction 平移
- ✅ `Revision` 单调递增
- ✅ **原子保存**（2026-11-20 修复）：`StdFileStore::save` 改为「同目录临时文件 → `atomic_replace`」。Windows 走 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`（extern 直连 kernel32，零新依赖），Unix 走 POSIX `rename`。两者都是内核级单步替换，彻底消除旧实现 `remove_file → rename` 之间目标文件消失的窗口期；失败时清理临时文件且不掩盖原错误。4 项单测覆盖覆盖/新建/无残留/指纹变化
- ✅ 外部冲突检测：mtime + size 指纹比对（`Fingerprint`）
- ✅ **Watcher 实现**（2026-11-20）：`NotifyWatcher`（notify 8）监听父目录而非单文件——编辑器保存常走「写临时文件 → rename 覆盖」，watch 单文件会在 rename 瞬间丢监听目标。回调过滤 Modify/Create/Remove（忽略 Access），`poll()` 取走并去重。后台线程 400ms 轮询，命中后用 `AppState::externally_modified` 指纹比对**挡掉自己保存产生的事件**，再 emit `external-change`；前端 `src/editor/editor.ts` 监听后 toast 提示（不自动重载，避免冲掉未保存编辑）。自检模式不开监听。端到端测试 + 去重单测各 1 项

### M2 只读渲染 ✅
- ✅ 投影模型：`Block` / `Span` / `hidden`，HTML 合成（`to_html()`）
- ✅ 安全：raw HTML 降级为文本（`ENABLE_RAW_HTML` 不启用 + 事件流显式转义），已测 `<script>` 不会泄漏
- ✅ 样式：标题 / 加粗 / 斜体 / 删除线 / 行内代码 / 链接 / 引用 / 代码块 / 分隔线
- ✅ **可交互验证页**（`mdview <file> preview`）：真实 DOM + 自绘 caret + 状态栏，用于核对停靠点
- ✅ 列表项与表格行断行、列表项目符号逐项渲染
- ✅ **无序 / 有序列表区分渲染**（2026-11-08 修复）
  - 两个根因：① `classify` 只认块的第一个事件，而松散列表（项间有空行）会被切块拆成多块，后半段块的首个事件是 `Item`，整块退化成段落——序号和项目符号一起消失。现在 `project.rs` 维护「打开中的 List 容器」栈，首事件是 `Item` 的块按栈顶判 `List { ordered }`
  - ② `.li::before` 写死 `content: "•"`。现在每项从**源码**读出自己的标记（`item_marker`：数字 + `.`/`)`）写进 `data-m`，CSS 用 `attr(data-m)` 显示。不用 CSS counter：用户写 `3.` 就显示 `3.`，`1)` 就显示 `1)`，从别处粘来的半截列表不会被悄悄改号
  - 阅读态 `to_html` 顺带修好：以前列表输出 `<ol>甲<br>乙</ol>`，没有 `<li>`，浏览器只给一个序号；现在按项切 `<li>`，有序列表带 `<ol start="N">`。为此 `to_html` 需要 `src` 参数（`<li>` 边界与起始号只能从源码读）
- ✅ **Tauri 2 外壳**（`src-tauri/`）：`frontendDist` 指向 Vite 产物 `../dist`，`session.rs` 隔离 IPC 契约（前端 `kind` 字符串 → `Message`），窗口创建与 IPC 已用启动探针在日志中验证
- ✅ 前端渲染：按块落地、增量应用 `Patch`、用停靠点 `data-b`/`data-a` 的左右边缘量出 caret 像素、点击/拖动报停靠点
- ✅ **主题系统**（2026-11-20）：`app.css` 颜色全部改为 CSS 变量，`[data-theme="dark"]` 覆盖为深色。`theme.js` 默认跟随系统 `prefers-color-scheme`，手动切换存 localStorage、可恢复跟随系统。状态栏按钮一键切换。7 项单测 + 自检验证
- ✅ **大纲侧栏**（2026-11-20）：`Snapshot` / `Reply` 经 `OutlineDto` 下发标题表（`level`/`title`/`block`）；`src/outline.js` 按锚点实际坐标判定当前节（硬约束 10：不用滚动百分比），侧栏点击按锚点位置跳转。10 项单测 + 自检验证。**点击后高亮要钉住点击项**（`outlinePinned`）：文末标题受最大滚动位置限制，永远越不过 80px 基准线，按位置判定会把高亮还给上一个标题；只有滚轮 / 触摸 / 导航键 / 拖自绘滚动条这些真实输入才撤钉，`scroll` 事件本身不能撤（平滑滚动期间一直在发）
  - **图片高度**（2026-11-23 修）：图片是异步加载的，撑开后所有标题下移。`hydrateImages` 的 `load` / `error` 回调里重测一次锚点；`jumpToOutline` 点击时再现场量一次 `getBoundingClientRect()`，不只依赖缓存——两层保险，图片无论何时加载完都不会让跳转差出一张图的高度。dom 用例 `outline-image` 固定住了 0 → 400px 的回归
- ✅ **同步滚动（锚点插值）**（2026-11-20）：滚动正文时按 `activeIndex(锚点, 阅读基准线)` 高亮当前节，侧栏自动把高亮项滚进视野；resize / 编辑后重测锚点。纯函数 `outline.js` 可在 Node 下测，判定依据是标题块真实纵坐标而非序号均分
- ✅ **跨平台标题栏与菜单**（2026-11-20）：
  - Windows/Linux 用 `src-tauri/tauri.windows.conf.json` / `tauri.linux.conf.json` 关掉原生装饰（数组整体替换，须重复完整 window 配置），标题栏由 `TitleBar.vue` + `MenuPopup.vue` 自绘 VSCode 风格：文件/编辑/选择/视图/转到/帮助六个下拉菜单 + 居中文件名标题 + 最小化/最大化/关闭。`data-tauri-drag-region="deep"` 负责拖拽，按钮不参与拖拽
  - macOS 保留原生标题栏；`main.rs` 用 `#[cfg(target_os = "macos")]` 构建系统菜单栏，自定义项以 `menu-action` 事件 emit 给前端，和自绘菜单共用 `editor.ts` 的 `runAction`。原生项不设 accelerator，避免和 `keydown` 重复触发（撤销这类非幂等动作尤其危险）
  - capability 补 `core:window:allow-{minimize,toggle-maximize,is-maximized,close,start-dragging,set-title}`；`use_custom_titlebar` 命令让前端以 Rust 配置为唯一判据（UA 只用于首帧抢跑）
  - 验证：`cargo test -p mdview-desktop` 17 项通过，端到端自检 47/47，真实窗口截图确认菜单/标题/窗口按钮与正文让位均正常
  - **滚动条修正**：原生滚动条属于窗口视口，纵跨整窗会插进标题栏、压住状态栏。改为自绘（现位于 `src/scrollbars.ts`）：`html.custom-titlebar` 隐藏原生滚动条，`#vscroll` 的 top/bottom 按标题栏与状态栏实际高度定位，滑块跟随 `window.scrollY` 并支持拖拽/点轨道；滚动本身仍是 window 的，caret / 大纲 / 拖拽自动滚动的视口坐标逻辑一行未改。截图验证滑块严格落在两栏之间

### M3 可编辑 🔶
- ✅ 视觉位置索引（停靠点表）、`Caret { block, stop }`、`next/prev/home/end`
- ✅ 选区模型与按块切高亮
- ✅ **输入翻译器骨架**（`AppState::update` 内闭环）：`InsertText` / `Backspace` / `DeleteForward` / `Newline` / `HardBreak` / `MoveCaret{delta,extend}` / `CompositionStart` / `CompositionCommit`
  - 一次编辑 = 一次 `Transaction` = 一步撤销；编辑后按 char→byte→停靠点（`Gravity::Upstream`）回填光标，闭合标记右侧不会掉出样式
  - 回车按块类型分派：段落 `\n\n`、代码块/表格 `\n`、列表延续标记（有序序号 +1）、空项回车退出该层、引用延续 `> `
- ✅ **增量重绘帧**：`block_frames` 出每块 HTML + 全局停靠点区间，`diff_frames` 用前后缀比对只替换变化段；编辑只发 `Effect::Patch`，结构变化才发 `Effect::Render`
- ✅ **桌面端输入接线**：`keydown` / `paste` / 鼠标 → `Wire` → `Message`；消费 `Patch` 做块节点替换，用 `MoveCursor { stop, block, visual }` 对齐像素
- ✅ **空白处点击定位**：`elementFromPoint` 没命中字符时（行尾右侧、行间、块内留白），退到 `caretRangeFromPoint` 借浏览器的命中测试定位视觉行，再按 x 取行首/行尾。点在行外直接取端点而不走「最近」——词尾有两个同位置停靠点，取「最近」会落到与 `End` 不一致的那个
- ✅ **输入法输入层**：透明 `textarea#ime` 贴住 caret 承接 IME（候选窗跟着走），`#preedit` 镜像层按光标所在块的字体画 preedit，`src/ime.js` 是纯状态机（不碰 DOM，Node 可直接测）
  - 组合期只发一条 `CompositionStart`，之后不重绘、不碰 `#doc`，避免冲掉浏览器正在渲染的 preedit
  - 提交走 `CompositionCommit`，整段一次事务 = 一步撤销；取消候选走 `CompositionCancel` 复位挂起态（漏了会让内核永久吞输入）
  - 补掉「确认候选那一下回车」在 `compositionend` 之后补发 `keydown` 导致的误分段
  - 失焦时自行收尾，不依赖浏览器补发 `compositionend`
  - `--self-test` 在真实 WebView2 里合成 composition 事件，跑通「事件 → 状态机 → IPC → 事务 → 补丁 → 光标 → 撤销 → 取消后恢复输入」，当前 11/11
- ✅ **标记整删**：`Block.markers` 由容器事件推出成对标记，退格/Delete 落在标记里时整对抽掉并保留中间原文，一次事务、一次撤销
- ✅ **块首不产生不可见停靠点**：`**`／`# `／`- `／`> ` 左侧没有可见文字时只留右端。否则「标记之前」与「文字之前」同列同位置，光标停进去之后 Delete 变成整对去格式（与退格一样），而 ← 还会先空转一步
- ✅ **上下方向键按真实视觉行移动**（前端像素判据，`src/caretline.js`）
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
- ✅ **选区拖动到窗口边缘时的自动滚动**（`src/dragscroll.js`）
  - 纯几何不碰 DOM，Node 直接测（`src/editor/__tests__/dragscroll.test.ts` 16 项）；帧循环、滚动、落点回写全部由 hooks 注入
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
  - 内核不碰剪贴板：`Effect::Clipboard { text }` 只产出文本，写入由前端 `src/clipboard.js` 完成（`navigator.clipboard` → `execCommand` 兜底）
- ✅ **Tab 缩进 / Shift+Tab 反缩进**（2026-11-23）：`Message::Indent` 按列表层级改行首空白，普通块仍在光标处插两个空格（Shift+Tab 删行首空格）。列表顶层第一项没有父级、顶层项不能再外缩，两种情况都直接不动。光标用**字符**偏移回填——不能拿新文本里的字节位置去换旧文本的字符，中文项会在多字节字符中间截断。Rust 单测 4 项 + 自检 2 项
- ✅ **段末 Delete 接成一行**（2026-11-23）：光标停在文本块尾、下一块也是文本块、且中间恰好一个空行时，一次 Delete 吞掉 `\n\n` 把两块接成一行（标题、列表、引用都参与，块类型保留前一块）。下一块是围栏代码块 / 表格 / 分隔线时**拒绝删除**——`甲\n```\nx\n```" 会被解析成一整段，那道空行是语法分隔，删掉等于毁掉代码块。中间多出来的空行（含空段落）只删一个字符，用户留的结构不动
- ⬜ **`**` 自动配对：经实测不可行**。空内容的 `****` 在行首就是分隔线（`***` 起），补全的那一刻语法已经从「粗体起点」变成 thematic break；`** **` 同样被吃成 rule，`~~~~` 会被当成代码围栏。要绕开只能「补占位符再替换」（污染源码、撤销栈里有垃圾）或「选中文字后输入 `**` 直接包围」（改掉「输入替换选区」的语义），两条都没采用


### M4 细节 🔶
- ✅ **表格子工程**（2026-11-20）：仍是源码块，但解析层从 pulldown 事件抽出 `TableGrid / TableRow / TableCell / ColumnAlign`（行列、单元格 byte 区间、对齐），只服务于投影和导航、不替代源码真值。`preview.rs` 把同一组线性停靠点铺进真正的 `<table class="md-table">` 网格，保留全部 `data-b`/`data-a`，行尾换行改零宽标记。应用层新增 `TableMoveCell` / `TableInsertRow` / `TableDeleteRow` / `TableInsertColumn` / `TableDeleteColumn`，所有操作最终转成源码替换事务；表头行插行强制落在分隔行之后，删行禁止删表头/分隔行，删列至少保留一列。前端 Tab 跨格 + 状态栏行列工具条；阅读态 `to_html` 补 `<tr>/<th>/<td>` 与对齐 style（并修掉嵌套 `<table>`）。**表格默认带 1px 边框与 4px 圆角**：根因是桌面端 `app.css` 完全没有表格样式（网格 CSS 只写在 CLI 验证页的 `preview.rs`），已补齐并用主题变量 `--rule`/`--code-bg` 适配深浅色；CLI 验证页与阅读态内联样式同步统一。Rust 单测 + 会话测试 + 端到端自检（新增 8 项）全过
- ✅ 大纲数据层（`ParseResult::outline`）
- ✅ **外链打开**（2026-11-21）：投影里的链接字符只带 `data-href`，**不产出 `<a href>`**——有锚点的话点一下 webview 就把整个应用导航走，且没有后退可言（中键、拖拽、右键菜单都能绕过 click 拦截，所以干脆不给出可导航的锚点）。前端在 `#doc` 上接管 click：编辑态 Ctrl/Cmd+Click、阅读态单击，转成 `open_url` 命令落到系统 shell（Windows 直调 `ShellExecuteW`，macOS `open`，Linux `xdg-open`，不引 opener 插件）。三个必须的细节：① URL 白名单只放行 `http(s)` / `mailto`，因为 `[x](javascript:...)` 是合法 Markdown，`dest_url` 会原样落进属性；② 链接的每个字符各挂一份 `data-href`，点击命中的是单个字符元素、与同链接其它字符是平级兄弟，`closest` 读不到邻居身上的属性；③ 拖选之后浏览器也会补发一次 click，位移超过 4px 视为选字。编辑态不让单击跟随链接是硬要求：正文整块都是可编辑区，抢走单击等于废掉「点哪儿光标落哪儿」
- ✅ **代码高亮**（2026-11-21）：解析层新增 `highlight.rs`，用自带的轻量词法扫描给围栏代码块产出 byte 区间 token（关键字 / 类型 / 字面量 / 字符串 / 注释 / 数字 / 函数 / 变量 / 标签 / 属性 / 运算符），覆盖 Rust、JS/TS、Python、JSON、HTML/XML、CSS、Shell、Go、C/C++、Java、SQL、YAML、TOML、Markdown。**不引 `syntect` / `tree-sitter-highlight`**：前者要背整套语法包，后者是 C grammar，都会破坏 `mdview-markdown` 的纯 Rust 零依赖约束；这里只需要粗粒度着色，自带扫描更可控。token 挂在 `Block::tokens`，渲染时按字符停靠点查表加 `tk-*` class——编辑态与阅读态共用同一套 DOM，**停靠点集合、光标、选区、增量 patch 全不变**；阅读态 `to_html` 另按 token 切段包 `<span class="tk …">`。未知语言不着色，按原文显示
- ✅ **代码高亮补全语言与 diff**（2026-11-24）：语言从 16 种扩到 47 种。三个层次：①**修 bug**——`kt` / `kotlin` / `cs` / `csharp` 原先一律路由到 Java 扫描器，关键字表完全对不上（`fun` / `val` / `when` / `namespace` / `using` / `get` 全不着色），比「不支持」更误导；现在各自独立词表，并补了 C# 的 `@"…"` / `$@"…"` 逐字字符串。②**新 token 语义**——`TokenKind::Inserted` / `Deleted` + `tk-inserted` / `tk-deleted`，diff 的增删行整行染色（含背景），`+++` / `---` 文件头与 `@@` hunk 头另算；这是唯一需要同时改 token 模型、`app.css` 明暗两套变量与 `preview.rs` 验证页样式的语言。③**行导向扫描器**——Dockerfile / Makefile / Nginx 的语义由行首词决定，通用词法器分不清 `RUN` 与普通标识符，放在 `langs/config.rs`；diff 单独 `langs/diff.rs`。词法层顺带抽象出 `Quotes` 规则（`Double` / `Both` / `RawBoth` / `Triple`），`string_end` 不再为每种语言长一个分支；`$var`、Ruby `@ivar`、Elixir `:atom`、LaTeX `\command`、Lua `--[[ ]]` 各自特判。词表按字节序生成（`ALL_TABLES` 统一校验），并补了 `mts` / `cts` / `sass` / `json5` / `pyi` / `kts` / `cu` 等别名，`text` / `console` / `log` 明确按纯文本处理。Rust 单测 152 项（含 43 种语言的 Unicode 契约与随机串用例）
- ✅ **链接/图片行内编辑**（2026-11-23）：编辑态单击链接或图片弹出小面板，可改地址与文字（图片为 alt）、删除或打开。内核 `Message::EditLink { stop, url?, text?, remove }` 按停靠点找到覆盖它的 Link/Image 片段；Link 片段只覆盖 label，必须借 `Block::markers` 的配对把 `[label](url)` 整段圈进来再重写，label 里原有的 `**` 等行内标记原样保留。URL 含空白或圆括号时用 `<...>` 包起来。整段重写是一次事务、一步撤销。Rust 单测 4 项 + 自检 6 项
- ✅ **查找 / 替换**（2026-11-23，UI 对齐 VSCode）：`Ctrl+F` / `Ctrl+H` 唤起浮层，左侧箭头展开/收起替换行，`Esc` 关闭；查找框内联 Aa（区分大小写）/ ab（全词）/ .*（正则）三个开关，`Alt+C` / `Alt+W` / `Alt+R` 也能切；右侧显示 `1/5` 或「无结果」、正则有误时显示「无效正则」，上/下一个与替换按钮在无命中时置灰；替换行有 AB（保留大小写）与替换 / 全部替换，「更多」里可开启「仅在选区内查找」。正则替换支持 `$1` 捕获组，保留大小写按命中串的全大写/全小写/首字母大写三种形态套用（`src/editor/find.js` 的 `buildMatcher` / `findMatches` / `replacementFor` / `replaceMatches`）。搜索对象是**源码**，命中下标先换成 UTF-8 字节，再用每个字符元素的 `data-byte` 二分出停靠点区间交给内核 `Select`；落在折叠标记里的命中（如搜 `**`）选出来是零宽，直接跳过。高亮用一根游标扫 `marks`，不做元素数 × 命中数的包含判断；文档编辑后 60ms 防抖重算，但不抢光标。纯逻辑单测 23 项 + 自检 8 项。`Ctrl+F` 强制收起替换行、`Ctrl+H` 才展开——不能沿用上次的展开状态，否则 Ctrl+H 用过一次后每次 Ctrl+F 都会带出一行用不到的替换框。收起靠 `hidden` 属性，`#find-replace-row[hidden]` 的 `display:none` 必须 `!important`：`.find-row { display:flex }` 与它特异性相同且写在后面，会把收起顶掉（替换行明明 `hidden` 却还在显示，2026-11-23 踩过）
- ✅ **字数统计**（2026-11-23）：`AppState::word_count` 按块统计 `plain_text` 的非空白字符，不含 Markdown 标记；随 `Snapshot` 与每次 `Reply` 下发，状态栏显示「字数」。Rust 单测 3 项 + 自检 2 项
- ✅ **最近打开文件**（2026-11-23）：宿主侧 `recent.json`（应用配置目录，最多 10 条，去重后提到最前）。打开文件、另存为、带文件参数启动都会记录；`--self-test` 不写。Windows / Linux 的自绘菜单由 `bridge.recent` 运行时填充（文件名 + `title` 完整路径 + 清除记录）；macOS 原生菜单在每次记录变化后重建，空表显示「（暂无）」。Rust 单测 6 项 + 自检 1 项
- ✅ **拖放打开文件**（2026-11-23）：`WindowEvent::DragDrop` 在宿主侧判定落进来的是不是存在的 `.md/.markdown/.txt`（大小写不敏感），只发路径给前端复用 `open_file`，未保存改动的确认与菜单打开完全一致；拖入时正文铺一圈虚线，松手/离开撤掉。前端另挡掉 webview 对文件拖放的默认导航。Rust 单测 1 项（拖放本身不可合成，未进自检）
- ✅ **粘贴 HTML 规范化 / 复制富文本**（2026-11-23）：`html-to-md.js` 把富文本转 Markdown——标题、段落、列表（含嵌套）、引用、代码块（保留语言与反引号围栏）、GFM 表格（对齐、竖线转义、无表头补空表头）、链接/图片（协议白名单，`javascript:` 与非图片 `data:` 丢弃）、粗斜体/删除线/行内代码、分隔线；脚本 / 样式 / 表单控件整段丢弃，字面量元字符转义。粘贴时只有出现结构标签才转换：VSCode / 终端复制的纯文本也带 `<div>` HTML，按 HTML 转会拆段、吃缩进，这类仍走 `text/plain`。复制时内核用 `MarkdownParser::gfm().parse(选区源码).to_html()` 附带 HTML，前端一次 `ClipboardItem` 写 `text/html` + `text/plain`，粘到 Word / 飞书保留格式；`ClipboardItem` 不可用时退回纯文本。接线顺带修掉「在查找 / 链接面板输入框里粘贴会被插进正文」。HTML 转换 dom 用例 20 项、剪贴板 4 项，Rust 复制 HTML 1 项
- ✅ **数学公式**（2026-11-21）：`MarkdownParser::gfm()` 开启 `ENABLE_MATH`，`project.rs` 把 `InlineMath` / `DisplayMath` 落成 `SpanKind::Math { display }`，定界符按「恰好 want 个 `$`」内缩进 `hidden` 并登记为 `MarkerPair`（退格可整对抽掉，与行内代码同一套逻辑）。编辑态每个公式字符带 `.m`、保留全部 `data-b/data-a` 停靠点；阅读态 `to_html` 输出 `<code class="m">` / `<code class="m md">`，后者由 CSS 居中排版。`app.css` 与 CLI 验证页同步配色，明暗主题各有变量。Rust 单测 4 项 + 应用层配对删除 1 项全过
- ✅ **脚注完整链路**（2026-11-21）：`project.rs` 收集全篇 `FootnoteReference` / `FootnoteDefinition` 事件，按引用首次出现顺序分配编号并回填到 `SpanKind::FootnoteReference { label, number }` 与 `BlockKind::FootnoteDefinition { label, number, references }`。阅读态 `to_html` 输出上标序号 + `<a href="#fnN">`、编号定义块与逐处回链 `↩`；编辑态定义独立成 `kind-footnote` 块，`id="fnN"` + `data-fn` 供跳转，标签前缀仍折叠（CSS `::before` 显示序号），回链放在块尾且不带 `data-b`、不占停靠点。引用字符带 `data-fnref/data-fnbyte` 与锚点 id，前端 Ctrl/Cmd+Click（阅读态单击）跳定义、回链跳回引用。定义正文照常逐字符可编辑，源码真值不变。Rust 单测 5 项 + 应用层编辑 1 项全过
- ✅ **重新加载入口**（2026-11-23）：文件菜单「重新加载」+ `Ctrl+R`，把磁盘上的最新内容读回来。内核侧复用 `Session::open_file` 的构造路径（解析、指纹、光标归零一致），撤销栈随旧 `AppState` 一起丢弃；未命名文档直接报错（没有磁盘来源），脏文档先走 `ask_discard` 的「重新加载会丢失当前未保存的改动」。macOS 原生菜单同步加项，`menu_action` 表转发 `reload-file`；`Ctrl+R` 必须在 keydown 里 `preventDefault`，否则 WebView 会刷新整个页面
- ⬜ 大文档虚拟化（5 万字以下不做；DOM 节点数与跨块选区天然冲突）

### M5 前端工程化（Vite + Vue 外壳）✅
- ✅ **标准 Tauri 结构**：Rust crate 上提为 `src-tauri/`，前端工程根回到仓库根；`index.html` + `src/` + `dist/`（产物不入库，`cargo` 直跑前先 `pnpm build`）
- ✅ **Vite + Vue 3 + TypeScript**：端口 1450、`outDir=dist`、`emptyOutDir`；`pnpm build` 同时跑 `vue-tsc --noEmit`
- ✅ **`bridge.ts` 状态桥**：只读快照 + `set/notify/dispatch`；主题经 `MutationObserver` 跟随 `<html data-theme>`，不侵入 `theme.js`；`setTitle` / `nextTick` / `close-window` 收进桥
- ✅ **外壳逐组件迁移**（每步单独提交，自检断言零改动）：
  - `StatusBar.vue` 用 `<Teleport to="#bar">` 接管状态栏内容，`editor.ts` 不再拼 `innerHTML`
  - `Toast.vue` 接管 `#toast`，显隐与计时统一由 `bridge.notify` 管
  - `TitleBar.vue` + `MenuPopup.vue` + `menu.ts` 接管标题、菜单、窗口按钮、拖拽区与最大化图标；滚动条抽到 `scrollbars.ts`；删除 `titlebar.js`
- ✅ **图标库 Lucide**（2026-11-23）：`lucide-vue-next@1` 按名导入，包声明 `sideEffects: false`，Vite 生产构建只留用到的图标——实测 4 个 Vue 图标（ChevronRight / Check / TriangleAlert 等）原始体积只增约 2.2 KB、gzip 约 130 B，产物里搜不到未使用的图标名。菜单箭头/勾选、更新弹窗、标题栏窗口按钮（Minus / Square / Copy / X）都已替换；窗口按钮取 14px + `stroke-width: 1.7`，渲染后仍是约 1px 细线，观感不变。查找栏不在 Vue 树里，改用 `index.html` 里的一份 Lucide symbol sprite（8 个图标：chevron / 上下箭头 / 省略号 / 关闭 / 替换 / 全部替换 / 勾选），静态 DOM 用 `<use>` 复用，同样不引字体。刻意保留的非 Lucide 图形：关于弹窗的 `M↓` 品牌记号、`Aa/ab/.*/AB` 这类文字开关、正文列表圆点/箭头（文档内容，不是 UI 图标）
- ✅ **单入口**：`index.html` 只留 `<script type="module" src="/src/main.ts">`，九个旧脚本按序在 `main.ts` 里 `import`；首帧 UA 判定挪到 `platform.ts`，满足 CSP（不再需要内联脚本）
- ✅ **编辑内核收敛**（P3.5）：`src/app.js` → `src/editor/editor.ts`，七个纯逻辑模块与用例一起挪进 `src/editor/`，`web_ime.rs` 的守门清单与 `cwd` 同步
- ✅ **`editor.ts` 全量类型化**（P3.6）：新增 `src/editor/ipc.ts` 承载 `Wire`（23 种 `kind`，与 `session.rs` 逐条对应）/ `Reply` / `Snapshot` / `BlockDto` / `PatchDto` / `CaretDto`（保留 `char_offset` 等 Rust 字段名，不做 camelCase），摘掉 `@ts-nocheck`
- ✅ **用例迁 vitest**（P3.7）：`.test.js` → `.test.ts`，`unit`（75 项）+ `dom`（10 项）两个 project；`web_ime.rs` 守门从 7 项合并为 2 项（`node --check` 七个 `.js` + 跑 unit project）
- ✅ **检查更新**（2026-11-21）：`tauri-plugin-updater` 负责清单拉取与签名校验，桌面端只暴露四个自定义命令（`check_update` / `download_update` / `cancel_update_download` / `install_update`）。`src/update.ts` 管弹窗状态机（检查 / 可下载 / 下载中 / 就绪 / 失败），`UpdateDialog.vue` 只管展示与按钮；帮助菜单加「检查更新…」与「启动时检查更新」（默认关闭，开关存 localStorage，与 image-view 一样不主动联网）。启动检查静默失败，发现新版本只 toast 一句；下载中关窗等同取消。签名公钥写在 `tauri.conf.json`，私钥在 `.tauri/mdview.key`（已 gitignore，发布机用 `TAURI_SIGNING_PRIVATE_KEY` 注入）。更新说明（Release 正文）由 `release_notes::render` 按 Markdown 渲染成安全 HTML（文本转义、链接只给 `data-href`、图片只认 http(s)、原始 HTML 丢弃），弹窗直接 `v-html`，链接走 `open_url`；更新请求走 `reqwest` 默认的系统代理（Windows 读 Internet 设置，PAC 不支持）
- ✅ **关于弹窗**（2026-11-21）：原先「帮助 → 关于」只弹一句 toast，现改成与检查更新同款的模态弹窗。`src/dialog.css` 抽出两份弹窗共用的骨架（遮罩 / 卡片 / 标题 / 正文 / 按钮排），`src/dialog-modal.ts` 收拢锁滚动、吞 Esc、聚焦关闭按钮这三件事，两处不再各写一套；`src/about.ts` 只存 `open` 与版本号（`getVersion`，浏览器里回落到 `package.json`），`AboutDialog.vue` 展示图标、版本、一句话简介与内核/外壳/许可/项目四行元信息。关闭同样只认底部按钮；菜单动作名 `about` 由 `bridge.dispatch` 直接接管，内核里那句 toast 已删
- ✅ **GitHub 发布流水线**（2026-11-21）：新增 `.github/workflows/release.yml`，与 image-view 同构。推 `v*` 标签触发，分两个 job：`validate` 校验标签、`Cargo.toml` 的 workspace 版本、`package.json` 版本三者一致，并要求 `tauri.conf.json` 不写 `version`（版本只留一处来源），再从 `CHANGELOG.md` 抽对应版本段作为 Release 正文；`build` 矩阵覆盖 Windows x64 / arm64、macOS aarch64 / x86_64（`--bundles app,dmg`，少了 app 就没有 `*.app.tar.gz`，latest.json 会缺 darwin 条目）、Ubuntu 22.04，由 `tauri-action` 上传产物并生成 `latest.json`（`updaterJsonPreferNsis` 让 Windows 更新走 setup.exe）。新增 `CHANGELOG.md` 作为发布说明的唯一来源。另加 `.github/workflows/check.yml`：push 到 main 与 PR 时在 windows-latest 上跑 `cargo fmt --check`、`clippy --all-targets -- -D warnings`、`test --locked --workspace`（只搬 Rust 这一半，前端 type-check/build 未纳入）。接门禁前先做了一次 `style(rust)` 清理：全仓 rustfmt（17 文件）+ 6 处 `collapsible_if` 改 let-chain + `mdview-cli` 的 `type_complexity` 提别名 + 删掉 `mdview-markdown` 未使用的 `thiserror`。**已知缺口**：`src-tauri/icons` 目前只有 32×32 占位图（`icon.png` 也是 32px，没有 `.icns`），正式发版前需要用 1024×1024 源图跑一次 `pnpm tauri icon <源图>` 重新生成整套图标
- ✅ **多窗口**（2026-11-23）：从「一个进程一篇文档」改成「一个进程多个窗口」。宿主新增 `Sessions` 注册表（窗口标签 → `Session`）与 `CloseGuards`（标签 → 一次性放行标志），所有 IPC 命令都取**调用窗口**的会话；watcher 改成单线程轮询所有窗口，事件只发给对应窗口；`force_close` 只放行自己。二次启动带文件时不再替换现有文档，而是新开窗口（每个窗口独立打开、独立脏标记、独立外部修改监听）；文件菜单与 macOS 原生菜单新增「新建窗口」，快捷键 `Ctrl+Shift+N`。`new_window` 命令必须是 async 并 `run_on_main_thread` 建窗——同步命令在主线程执行，而 `WebviewWindowBuilder::build` 要等主线程消息循环，直接在命令里建窗会死锁
  - **窗口权限**（2026-11-23 修）：`capabilities/default.json` 的 `windows` 从 `["main"]` 扩成 `["main", "doc-*"]`，否则 `doc-N` 窗口调 `is_maximized` / `set_title` / 最小化 / 关闭全被 ACL 拒掉。另有构建坑：`capabilities/*.json` 改了但 cargo 不会重跑 `tauri-build`，二进制里嵌的还是旧 ACL——`build.rs` 现在显式 `rerun-if-changed` 每个能力文件与四份平台窗口配置
- ✅ **红线保持**：`editor.ts` 的投影/光标/坐标内核与七个纯逻辑模块零逻辑改动；大纲与正文仍是覆盖层上的洞

迁移分阶段（记录在案，方便回溯是哪一步引入的结构变化）：

| 阶段 | 内容 | 过关线（均已达成） |
|---|---|---|
| P0-a | Rust crate 整体挪进 `src-tauri/`，路径临时指向 `../web` | 纯搬家：全量测试绿 + 自检 57/57 |
| P0-b | 搭 Vite、`web/` → `src/`、`dist/` + 端口 1450、路径切最终值 | 行为不变、自检 57/57 |
| P0-c | 桌面 crate 从 `crates/mdview-desktop/` 上提到仓库根 | 自检 61/61 |
| P1–P2 | 铺状态桥与动作桥，按 StatusBar → Toast → TitleBar/MenuPopup 逐组件迁移 | 每步自检全绿，模板复刻原 `id`/`class` |
| P3 / P3.5–P3.7 | 单入口、内核收敛、TS 类型化、测试迁 vitest | 见上；最终 **Rust 205 项 + 前端 91 项 + 自检 65/65** |

- 验证：`pnpm build` 通过、`cargo test --workspace` **205 项全绿**、`pnpm test` **91 项全过**、端到端自检 **OK 65/65**（2026-11-21 11:55 实跑）、真实窗口截图复核标题栏/菜单/状态栏/滚动条与正文让位

### M6 完整度收敛 🔶

M0–M5 解决的是「内核正确」与「工程结构」，M6 解决的是「普通用户打开就能用」。判断标准不是功能数量，而是**不预先读 README 也不会卡住**：想加粗会按 `Ctrl+B`、想插图能找到入口、改不了字号不会去翻代码、断电不丢稿、写完能发出去。

分三组：P0 成品门槛（缺一条就不像成品）、P1 日常可用、P2 发布完备。

#### P0 成品门槛

- ✅ **M6-1 行内格式快捷操作**（2026-11-26）：`Ctrl+B` 加粗 / `Ctrl+I` 斜体 / `Ctrl+Shift+X` 删除线 / `Ctrl+E` 行内代码，自绘菜单与 macOS 原生菜单同步四档。内核新增 `Message::ToggleInline { kind: InlineKind }`，三条规则由内核判定：有选区且已被同格式标记包住 → 摘掉那一对（以「标记串精确相等」区分 `*` 与 `**`，否则 `***x***` 会摘错层）；有选区未包住 → 两端插标记、选区跟着内容平移；无选区 → 插「标记 + 占位文字」并整段选中。**插占位而不是空容器**：`****` 单独成段会被 pulldown 判成分隔线，插入瞬间块类型就变了、光标无处可落。**光标停在别的行内样式里一律不响应**——在 `*斜*` 词首插 `**` 会得到 `***粗体**斜*`，开头三个星被当成 strong+em 共用定界符；这是刻意的「不响应好过写坏」。跨块选区同样拒绝（`**` 跨段落会把两段并成一行），表格跨格走 `table_locked_selection`。前端 `runAction` 与 keydown 都加了模式门：源码视图真值是 textarea、阅读视图只读，误发会与内核文本两套真值打架。Rust 单测 6 项（包/摘/占位/撤销/不串格式/跨块拒绝）+ 自检 5 项
  - 原计划里的 `Ctrl+K` 插入链接移入 M6-2：链接面板依赖 `linkTarget.stop`，插入模式需要一个「没有既有 Link 片段」的分支，与插入菜单同源，一起做更省

- ✅ **M6-2 插入菜单（含 `Ctrl+K` 插入链接）**（2026-11-26）：新增「插入」菜单——图片…（`Ctrl+Shift+I`）、链接…（`Ctrl+K`）、表格、分隔线、代码块、引用、任务列表，自绘菜单与 macOS 原生菜单同步。内核新增三条消息：`InsertLink { url, text }`、`InsertImage { src, alt }`、`InsertBlock { kind: InsertKind }`（`ThematicBreak` / `CodeBlock` / `Quote` / `TaskList` / `Table { rows, cols }`）。
  - **块级插入的三条规则**：当前块是空段落 → 直接用掉它，不留多余空行；否则在块后另起一块——插入点两侧统一写成两个换行，做法是「吃掉」原文已有的换行（最多两个）再补足，这样 `甲\r\n` 这种只有行尾换行的文件不会叠出三个空行，`---` 也不会贴着上一段被读成 setext 标题；空文档直接写。
  - **行尾符取文档级而不是块级**：当前块可能只有一行文字（块范围里根本没有换行），按块判定会把 CRLF 文件的新块写成 LF——这是实现时实测踩到的，已有 CRLF 单测锁住。
  - **光标落点按品类定**：表格落表头首格（与 `cell_caret` 的空单元格规则一致）、代码块落围栏中间、分隔线落到它之后（原子块内部没有可落位置）、`> 引用` 与 `- [ ] 任务` 整段选中占位文字（裸标记不成形，占位顺带给出「这里要填东西」）。
  - **图片复制在宿主侧**（`pick_image` 命令）：`tauri-plugin-dialog` 选图 → 复制到文档同级 `assets/` → 返回 `assets/图.png`。重名改成 `图-1.png` 而不是覆盖——覆盖会悄悄毁掉之前插入的那张，编辑器里完全看不出来。文档未保存时退回绝对路径并 toast 提示。内核不认识除文档本身以外的任何文件，所以 `InsertImage` 只收最终地址。
  - 前端：链接面板加「插入」分支（`linkInserting`，隐藏打开 / 删除，提交走 `insertLink`），`Ctrl+Shift+I` 必须排在 `Ctrl+I`（斜体）之前——两者 `event.key` 都是 `'i'`。插入类动作与行内格式共用同一套模式门（源码 / 阅读视图不响应）。
  - 测试：Rust 单测 8 项（链接三态 / 图片 / 表格解析 / 块间空行 / 代码块落点 / CRLF / 图片存盘重开）+ 宿主复制重名 1 项 + 线缆契约 3 项 + 自检 4 项。**过关线「新建 → 插图 → 保存 → 重开图片仍在」由 `inserted_image_survives_save_and_reopen` 覆盖**（对话框本身无法无人值守驱动，注入点 `pick_image(path)` 留给手动验证）。

- ✅ **M6-3 偏好设置面板**（2026-11-26）：新增「文件 → 偏好设置…」（`Ctrl+,`），五项——正文字体（系统无衬线 / 衬线 / 等宽）、字号（12–20）、正文宽度（560–1200）、Tab 缩进宽度（2–8）、启动视图。改动即时生效，不需要「保存」按钮。
  - `src/prefs.ts` 是唯一读写口：`sanitize` 对 localStorage 里的东西一律不信——枚举值不在白名单就回落、数值夹紧、未知字段丢弃。测试锁的就是这条（坏 JSON、越界字号、`font: 'comic-sans'`）。
  - 生效方式是往 `<html>` 写 `--doc-font` / `--doc-font-size` / `--doc-width`，app.css 里 `#doc` 全读变量，默认值等于改造前的硬编码值，没动过偏好的用户观感零变化。**`#doc` 的 `margin-left` 里那个 390px 也要换成 `var(--doc-width) * .5`**，否则换行宽后正文不居中。
  - **改字号 / 行宽必须重测**：`window.__mdviewRelayout()` 复用 resize 那条路径（`lines.reset()` + `applyCaret(caret, true, true)` + 大纲锚点重测 + 滚动条刷新）。自绘光标是按像素量的，不重测会停在旧位置。
  - 缩进宽度落在内核（`Message::SetIndentWidth`，夹在 1–8），**只作用于普通段落**：列表仍按 `marker_width` 缩进——`- ` 两格、`1. ` 三格，改成 4 会让子项落进标记中间。它是「偏好」不是「文档内容」，所以 `Session::replace_document` 会把它带过新建 / 打开 / 重新加载，否则改过的 Tab 宽度会莫名其妙失效（有单测）。
  - 皮肤层让位：HyperOS 原先给 `#doc` 设 `font-family: var(--font-ui)`，会盖掉用户的字体选择，现在只保留 `body` 的皮肤字体（外壳控件用），正文听偏好。
  - **「是否记住窗口尺寸」这一项本次没做**：window-state 的 flags 在插件构建时就固定了，要做成开关得让 Rust 启动时先读一份配置决定 flags，与前端 localStorage 是两套存储。单独做更干净，留在 P2。
  - 测试：前端 unit 8 项（校验 / 夹紧 / 脏 JSON / 记忆）、Rust 单测 2 项（宽度生效与列表不受影响、越界夹紧）、会话测试 1 项（换文档后仍保留）、契约 1 项、自检 4 项。

- ✅ **M6-4 自动保存与崩溃恢复**（2026-11-26）：两件事分开做，互不依赖。
  - **崩溃副本**（`src-tauri/src/backup.rs`）：脏文档每 5 秒写 `<配置目录>/backup/<id>.md` + `<id>.json`。三条硬约束——不碰原文件、不落在文档目录（`notify` 看不到，不会误报「文件已被外部修改」）、保存成功后自清理（于是「有副本」就等于「上次没保存」）。写入走临时文件 + 改名，写到一半被杀掉不会留下半截正文。
  - **`id` 的算法是有讲究的**：路径的 FNV-1a 十六进制，**不能用 `DefaultHasher`**——它的算法不保证跨版本稳定，升级一次 Rust 就可能换值，用户的副本会集体失联。未命名文档用 `untitled-<窗口标签>`，同一窗口下次启动还能对上。
  - **有效性判定 `is_current`**：只比「备份时记下的磁盘 mtime」与「现在的 mtime」。磁盘变新说明用户已在别处保存，副本只会把旧内容盖回去。两个例外照旧恢复：备份时文件还不存在、以及现在文件被删了。
  - **恢复后必须标脏**（`Session::restore`）：副本不是磁盘内容，关窗前还得再拦一次，不能让人以为已经落盘。
  - **自动保存**（偏好开关，默认关）：每 5 秒静默走现有 `save`，成功不弹「已保存」（`quietSave` 标志，失败时复位，免得吞掉下一次手动保存的提示）。未命名文档没有原文件可写，仍走崩溃副本。真正写盘后的副本清理靠 `applyReply` 的 saved 分支单独发一次 `write_backup`——定时器那条路径只在 dirty 时才发，等不到它。
  - 自检模式一律跳过（`--self-test` 反复换文档，写出来的副本会污染下一次启动）。
  - 测试：`backup.rs` 单测 5 项（id 稳定性 / 未命名 key / 有效性判定 / 落盘读取清理全流程 / 未命名文档）、`Session::restore` 1 项、自检 1 项。
  - **手动验证项（自动化覆盖不到）**：真·任务管理器强杀 → 重启是否弹「恢复 / 丢弃」。副本的落盘、读取、有效性判定都有单测，唯独「进程被强杀时最后 5 秒的改动」只能靠人演一遍。

- ✅ **M6-5 导出与打印**（2026-11-26）：新增「文件 → 导出 HTML…」，把当前文档写成一个自带样式的单文件 HTML。**PDF 不做内置**（原因见下），README 与菜单都不承诺 PDF 导出。
  - 渲染复用解析层：`MarkdownParser::gfm().parse(&text).to_html(&text)` 拿到与阅读视图同一份 HTML，样式表（`export.rs::STYLE`）单独写一份，不引用前端 CSS——导出的文件得能脱离应用单独活着，指望 `dist/assets/index-xxx.css` 是不行的。含 `tk-*` 代码高亮类、表格边框、`<figure>` 图片居中、脚注返回链接样式，以及一段 `@media print`（去掉背景色、避免标题被孤行切开、链接打印出 URL）。
  - **图片内联**：`inline_images(html, resolve)` 把 `<img src="assets/图.png">` 换成 data URL，`resolve` 由宿主提供（`image_data_url`，沿用 32 MiB 上限）。三条边界——`data:` / `http(s)://` / `//` 开头的直接跳过；读不到的文件**保留原路径而不是让整次导出失败**（一张图丢了不该毁掉整篇文章）；HTML 里的 `&amp;` 要先还原再交回读盘，否则 `a&amp;b.png` 一定找不到。
  - **PDF 为什么不做内置**：Tauri 2 没有跨平台打印 / 页面导出的 API。Windows 能借 WebView2 的 `PrintToPdfAsync`，macOS 得走 `NSPrintOperation`，Linux 上 WebKitGTK 又是另一套——三份平台代码，还都拿不到分页控制。现实路径就是导出 HTML 后让浏览器「打印为 PDF」，输出质量由浏览器决定，比自写分页可靠。
  - 宿主侧 `export_html` 命令：默认目录取文档所在目录、文件名 `stem.html`（未命名就叫 `未命名.html`），对话框可改路径；写成后返回实际路径给前端 toast。
  - 测试：`export.rs` 单测 4 项（页面骨架与标题转义 / 图片内联的三种跳过 / 读不到时保留原路径 / `&amp;` 还原）、宿主链路 1 项（`exported_html_inlines_images_and_keeps_highlighting`：真文档 → 导出 → 断言图片变 data URL、`tk-` 高亮类与表格都在）、自检 1 项（文件菜单有导出项）。
  - **手动验证项**：导出的 HTML 在 Chrome / Firefox 里逐项看图片、表格、高亮、脚注——「双击打开是否好看」没法自动化，骨架与内联由单测锁住。

#### P1 日常可用

- ✅ **M6-6 文件侧栏 / 打开文件夹**（2026-11-26）：新增「文件 → 打开文件夹…」（`Ctrl+Shift+O`）与拖入文件夹；侧栏上半段是工作区文件树，下半段仍是大纲——大纲跟「当前文档」，文件树跟「工作区」。
  - **一次只列一层**（`workspace::read_dir`）：递归扫整棵树不可控（网络盘、几万个文件），而单次 `read_dir` 总是很快；子目录点开时再问一次，`read_folder` 命令就是那个入口。单层上限 1000 项，超了在面板底部说明，不假装列全了。
  - **过滤是白名单**：文件只认 `.md` / `.markdown` / `.txt`（与 `open_file` 的对话框过滤器同一份名单），目录一律收（哪怕眼下没有 md，用户随时会往里加）。隐藏项与 `node_modules` / `target` / `dist` / `vendor` / `Library` / `AppData` 跳过。**隐藏文件也跳过**——`.gitignore` 那种不算正文，这条是最初漏了、被单测逮到的。
  - **绝对路径用 `std::path::absolute` 而不是 `canonicalize`**：后者在 Windows 上会加 `\\?\` 前缀（`\\?\E:\笔记`），还会解析符号链接与 8.3 短名，让「看到的路径」与「存的路径」对不上。这个前缀会直接冒到侧栏 `title` 与标题栏里。
  - 排序：目录在前、文件在后，各自按名字**不区分大小写**（`A.md` 在 `b.md` 前），同名再按原字符串定序，与平台无关。
  - 前端拆两层：`src/filetree.ts` 是纯树模型（`fromEntries` / `attachChildren` / `flatten` / `samePath` / `isInside`，8 项单测在 node 里跑）；`src/files.ts` 只做 DOM 与 IPC。**重新展开一个目录会重新读盘**（外部新增的文件立刻可见），但按路径把下一层已展开的状态续回去——收起再展开不用一层层点回来。
  - 点文件走 `openPath` → `open_file`，与「最近打开」「拖放」同一条队列，脏检查在宿主侧，确认文案一字不差；`applySnapshot` 与另存为都会同步侧栏高亮，文档不在工作区里就不高亮任何一行。
  - 布局：**不套 flex 外壳**，而是给文件面板固定高度 `--files-h`（无工作区时为 0），大纲的 `top` 各自加一次这个变量。这样原生皮肤与 HyperOS 的浮卡片样式互不干扰，`阅读视图大纲贴底` 那两条几何断言不动分毫。自绘滚动条也补了一套 `#files-scroll`，`attachScrollbar` 顺手改成「骨架缺元素就不装」，免得测试页少一个节点就整页崩掉。
  - 工作区是纯视图状态：内核与 `Session` 不知道它存在，根路径记在 localStorage，启动时 `read_folder` 试恢复，目录没了就静默清掉。
  - 测试：`workspace.rs` 单测 6 项（只列 Markdown 与目录 / 空目录 / 文件与不存在的路径报错 / 不区分大小写排序 / 同名不同大小写的定序 / 扩展名判定）、`filetree.ts` 单测 8 项、自检 9 项（菜单项、侧栏出现、列子目录、不列非 Markdown、展开前看不到、懒展开后出现、点文件经 `open_file` 打开、高亮当前文档、可收起）。
  - **已知取舍**：源码视图里侧栏整体隐藏（与大纲一致）；工作区没有「刷新」按钮，靠重新展开目录拉取最新列表。

- ⬜ **M6-7 多标签页**
  现状：多窗口，开五篇文档任务栏散成五格。
  注意：`Sessions` 是**窗口级**注册表（`label → Session`），一个窗口只有一份文档。标签页要把键改成 `(label, doc_id)` 并让所有 IPC 命令带文档标识——**架构改动，动手前先出设计**，不要边写边改。
  过关线：一个窗口内多标签独立脏标记与独立外部修改监听；关标签拦截未保存；自检覆盖两个标签互不串状态。

- ✅ **M6-8 非 UTF-8 编码读入**（2026-11-27）：`mdview-support/src/text.rs` 一个 `decode` / `encode` 对，先严格 UTF-8、失败退 GB18030；编码跟着**文件**记，保存按原编码回写。内核内存态仍然只有 UTF-8 `String`，字节偏移与停靠点不受影响。
  - 判定顺序的道理：有 BOM 就一定是 UTF-8（BOM 是自证），剥掉后解码；否则 `str::from_utf8` **严格**校验通过就是 UTF-8——一段纯 ASCII 在 GBK 里是同一个意思，先判 UTF-8 不会误伤；校验失败才退 GB18030（GBK / GB2312 都是它的子集，一套码表通吃）。
  - `TextEncoding` 三态而不是两态：`Utf8` / `Utf8Bom` / `Gb18030`。「有没有 BOM」在回写时是两个不同结果，合并成「UTF-8」就一定会丢 BOM。
  - **编码记在存储上、按路径存**（`StdFileStore::encodings`），不是记在 `AppState` 里：内核不认识编码，它只认识文本；而同一份文件在多个窗口里打开时，编码判定必须一致。这样 `state.rs` 的 `save_into` 一行都不用改——`files.save()` 自己知道该用哪种编码。
  - 崩溃恢复是唯一的坑：副本里只有 UTF-8 文本，恢复时这个路径可能还没被读过，于是保存会写成 UTF-8。`remember_from_disk` 在 `restore` 里补一次编码判定（只读字节、不取内容），把「用户存过的 GBK 文件」的编码找回来。
  - GB18030 编码不了的字符会被写成数字字符引用 `&#..;`——这是 `encoding_rs` 的既定行为，宁可显示成引用，也不丢字或让整次保存失败。
  - `StdFileStore` 从 unit struct 变成带 `Arc<Mutex<HashMap<..>>>` 的结构，因此丢了 `Copy`；`Session::open` 改成 `with_store`，把「刚读过盘的那个 store」交给会话，否则读出来的编码记忆会在构造新 store 时丢掉。
  - 前端：`Snapshot.encoding` → 状态栏一格提示（只有非 UTF-8 才占位，UTF-8 BOM 也标出来）。
  - 测试：`text.rs` 8 项（UTF-8 / BOM 剥与回写 / GBK 回退 / ASCII / 混合文档往返 / detect / 标签 / 空文本）、`ports.rs` 3 项（GBK 回写字节不变 / 新路径按 UTF-8 / `remember_from_disk`）、`session.rs` 2 项（**过关线**：GBK 打开→编辑→保存后字节仍是 GBK、BOM 剥掉但回写保留）、自检 4 项（解码正确 / 快照报 gb18030 / 状态栏提示 / 换回 UTF-8 后提示消失）。
  - 指纹仍按磁盘元数据（长度 + mtime）算，与编码无关。

- ⬜ **M6-9 大文档性能**
  现状：全文档一次性建 DOM（每字符一个 span），第 8 节第 5 条记了「5 万字以上会卡」。
  做法：先试最便宜的 `content-visibility: auto` + `contain-intrinsic-size`（不动结构，可能不够）；不够再做分块渲染（可视区上下各一屏）。必须解决它与「跨块选区」「大纲按真实坐标判定」的冲突——这两处是停靠点体系的既得利益，改坏了比卡更糟。
  过关线：10 万字文档滚动不卡、点击落点准确、跨块选区仍可拖选。

- ⬜ **M6-10 表格剩余能力**
  现状：单元格内换行 / 多段落、跨格复制、排序都缺（第 8 节第 2 条）。
  做法：单元格内换行在 Markdown 里没有标准表达（`<br>` 与「raw HTML 降级」冲突），先按「不支持、不给入口」定；跨格复制直接用「选区源码」即可，不必做 TSV 剪贴板；排序最后做。前两项不要顺手开工。

- ⬜ **M6-11 数学公式排版（可选，先不做）**
  现状：M4 已做「定界符折叠 + 变色源码」（`SpanKind::Math` + `.m` class），**没有 KaTeX 排版**，这是当时的取舍。
  若要做，只能落在**阅读态**：编辑态必须保持源码可编辑，否则停靠点全崩。做法是阅读态 `to_html` 输出 `data-tex`，前端按需加载 KaTeX 只渲染 `#doc.reading` 内的公式节点。是否值得做取决于目标用户（技术文档 / 学术），列此备查。

#### P2 发布完备

- ✅ **M6-12 帮助体系与空状态**（2026-11-27）：三件事——快捷键一览、空状态、首次提示。
  - **快捷键一览由 `src/shortcuts.ts` 从 `MENUS` 生成**，不是另抄一份表。菜单里的 `acc` 才是唯一真值（`editor.ts` 的 keydown 就是照着它写的），面板漏一条就是用户照着按却没反应。生成规则：按 `MENU_ORDER` 走一遍，收 `acc` 非空的项，同名同键只留一条（`Ctrl+A` 在「编辑」与「选择」里各有一条），菜单项尾部的省略号去掉。
  - 菜单表达不了的键位（`Alt+C/W/R`、`Tab` / `Shift+Tab`、`Esc`、`Ctrl+点击`）只能手写，集中在一个 `EXTRA` 常量里，容易看清也容易改。
  - 防漂移的落点写在测试里：`shortcuts.test.ts` 反向枚举 `MENUS` 里所有带 `acc` 的项，要求面板里**一条不少**；面板里也不许出现带省略号的标签。这条测试是「不手写第二份」的强制执行机制，删掉生成逻辑它立刻红。
  - 入口三个：`F1`、`Ctrl+/`、「帮助 → 键盘快捷键」。前两个走 `__mdviewBridge.dispatch('shortcuts')`（与 `Ctrl+,` 开偏好设置同一条路），第三个走菜单动作名，`menu_action` 与原生 macOS 菜单都补了一条。
  - 空状态（`#empty`）：判据是「未命名 + 不脏 + `sourceText.trim()` 为空」——**不能用块数**，新建文档也有一个空段落、块数恒为 1。三个入口 + 最近打开前 5 条，动作名与菜单完全一致（`new-file` / `open-file` / `open-folder` / `recent:N`），不另写一套逻辑。源码视图不显示（textarea 就在左边，盖上去只会挡住它）。
  - 首次提示存 `mdview.seen-welcome`：只提示一次；`localStorage` 被禁用时静默跳过——提示不是功能。
  - 测试：`shortcuts.test.ts` 5 项（分组顺序 / 一条不少 / 关键项在对应组 / 去重 / 去省略号）、自检 9 项（空状态显隐两次、三个入口、F1 打开面板、键位数量与内容、分组数、Esc 关闭、帮助菜单有入口）。

- ⬜ **M6-13 国际化（i18n）**
  现状：界面文案全中文硬编码——前端散在模板与 `editor.ts`，Rust 侧在菜单构建里。
  做法：先抽不翻译——`src/i18n/zh-CN.ts` 集中前端文案，`menu.rs` 的 label 收进常量表；结构就位后再加 `en-US`。不要在半成品状态下引入运行时切换。

- ⬜ **M6-14 可访问性**
  现状：正文是自绘投影层（每字符一个 span、`tabindex=-1`），屏幕阅读器读到的是碎字符；`aria-live` 只在 toast 上有。
  做法：`#doc` 补 `role="textbox"` + `aria-multiline` + `aria-label`；查找结果与保存结果走 `aria-live="polite"`；把「源码视图」标注为无障碍视图并保证 `Tab` 可达；设置面板等新弹窗复用 `dialog-modal.ts` 的焦点陷阱。
  过关线：只用键盘能走完 新建 → 写 → 保存 → 查找 → 退出。

- ✅ **M6-15 CI 补齐**（2026-11-27）：`check.yml` 从「只有 Windows + Rust」扩成两道门禁。
  - `rust` job 加矩阵 `[windows-latest, macos-latest]`，`fail-fast: false`（一个平台红不该把另一个的结论吞掉）。macOS 那一份不是凑数：`#[cfg(not(windows))]` 的 POSIX 原子写、`#[cfg(target_os = "macos")]` 的原生菜单，平时只有这条链在编译。
  - 新增 `frontend` job：`jdx/mise-action` 装 node/pnpm（版本只在 `mise.toml` 写一处）→ `pnpm install --frozen-lockfile` → `pnpm build`（内含 `vue-tsc` 类型检查）→ `pnpm test`（unit + dom）。**前端用例从此进门禁**，之前 `pnpm test` 挂了 CI 照样绿。
  - pnpm store 指到工作区的 `.pnpm-store` 再缓存：Windows 与 Linux 的默认 store 目录不同，写死任一个另一边永远命中不了缓存。
  - `frontend` 固定在 Windows runner：dom 那一批断言量的是像素（光标落点、点击命中、大纲锚点），换平台就换了字体渲染与默认缩放，结论不再可比。GitHub 的 Windows runner 预装 Google Chrome，`vitest.config.ts` 的 `channel: 'chrome'` 直接复用，不用下 Playwright 浏览器。
  - **Linux 不进门禁**：要跑测试得先装 webkit2gtk / libsoup / librsvg 一整套 Tauri 依赖，装一次几分钟且常因源站问题红；`release.yml` 本来就在 ubuntu-22.04 上编译一次，Linux 专属的编译问题发版时必然暴露。
  - 过关线验证：本地把一条用例改坏确认 `pnpm test` 退出码非 0（CI 判红就靠这个码），已养成习惯；真正的「CI 变红」要在首次推送后看 Actions 结果。

- ⬜ **M6-16 发布合规与反馈入口**
  现状：`release.yml` 出包但 macOS 无签名 / 公证（用户要右键绕过 Gatekeeper）；Linux 拖放打开未实机验证；崩溃日志写在 `logs/error.log`，用户不知道怎么给你。
  做法：macOS 配 `APPLE_CERTIFICATE` / `APPLE_ID` 等 secrets 走签名 + 公证；Linux 在虚拟机上实测三条（拖放、原生菜单、更新）；帮助菜单加「打开日志目录」。

- ⬜ **M6-17 发版图标**
  现状：`src-tauri/icons` 仍是 32×32 占位。
  做法：1024×1024 源图跑 `pnpm tauri icon`。三平台的图标清单已在 Unreleased「平台」段分好，只需替换素材。

**M6 不做**：PAC 代理、外部修改自动重载、富文本 round-trip——均为既有决策（见第 8 节与工作记忆），不是缺口。

---

## 7. 当前进度快照

- 日期：2026-11-26（M6-1 行内格式、M6-2 插入菜单、M6-3 偏好设置、M6-4 崩溃恢复与自动保存；上一轮为多窗口、最近打开、字数统计、拖放打开、粘贴 HTML / 复制富文本、查找替换、Tab 缩进、链接/图片行内编辑）
- 测试：**Rust 417 项全过**（app 149 / markdown 172 / desktop bin 74 + 前端守门 2 / core 5 / support 15），`cargo clippy --workspace --all-targets -- -D warnings` 无告警。前端 **197 项**（20 文件，unit + dom 两个 project：`ime` / `caretline` / `selection` / `dragscroll` / `outline` / `theme` / `update-format` / `find` / `clipboard` / `html-to-md` / `skin` / 侧栏树模型 / 快捷键表 / CRLF 视图 / 环境能力 / 滚动条 / 图片大纲 / 内核启动），由 `pnpm test` 一次跑完
- 前端工程：`pnpm build`（含 `vue-tsc --noEmit`）通过；`src/` 为 Vite + Vue 工程根，产物 `dist/` 不入库，`cargo` 直跑前必须先 build
- 桌面端已能起窗：`pnpm tauri dev -- -- <file>`。日志里出现「前端就绪：N 块 / M 停靠点」才说明页面与 IPC 都活着
- 自检：`pnpm tauri dev -- -- tests/fixtures/selftest.md --self-test`，日志出现「自检通过：OK 135/135」即链路通（含 IME 11 项、上下移动 7 项、Home/End 2 项、点击空白 2 项、**拖动自动滚动 3 项**、视口 1 项、全选/复制/剪切 8 项、**M2 大纲/主题 15 项**、**表格 8 项**、**文件/新建/另存为/导出 11 项**、**文件侧栏 9 项**、**编码 4 项**、**字数 2 项**、**查找替换 8 项**、**Tab 缩进 2 项**、**链接/图片编辑 6 项**、**行内格式 5 项**、**插入 4 项**、**偏好设置 5 项**、**帮助 9 项**）。**失败时报告写进 `logs/error.log` 而不是 `mdview.log`**，排查别只看后一个
  - 样张必须是 `tests/fixtures/selftest.md`：首块是标题、第二块是**单行**段落、第三块是列表，且总长可滚 200px 以上。随手拿长段落文档跑会误报三项——软换行会让 `End` 落在视觉行尾、让 `↓` 停在同块第二行，而用例的假设是「一块一行」
  - **必须走 `pnpm tauri dev`**：debug 构建会按 `devUrl` 加载 `http://localhost:1450`，没有 Vite 时窗口标题只有配置里的 `mdview`、页面白屏、日志停在「启动」一行——不是回归。想直接用 `target/debug/mdview-desktop.exe` 跑自检，手工起的 Vite **必须绑 IPv4**（`pnpm dev --host 127.0.0.1`，别再夹一层 `--`：pnpm 会把 `--` 原样传给 vite，host 不生效）：Vite 默认在这台机器上只监听 `[::1]`，WebView2 打不开 `localhost` 时同样白屏，且 CDP 里页面的 `title` 会停在 `about:blank`
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
  - `feat(app): 保存改为内核级原子替换并接入文件监听，补齐 M1`
  - `feat(desktop): 加入主题系统与大纲侧栏，完成 M2 收尾`
  - `test(desktop): 新增标准 Markdown 语法全覆盖测试样张`
  - `feat: 表格子工程落地真实网格渲染与行列编辑`
  - `refactor(desktop): 改用标准 Tauri 结构并接入 Vite 骨架`
  - `feat(desktop): 接入外壳状态桥与只读 Vue 组件`
  - `feat(desktop): 状态栏改由 Vue 渲染`
  - `feat(desktop): Toast 改由 Vue 渲染`
  - `feat(desktop): 标题栏与菜单改由 Vue 渲染`
  - `refactor(desktop): 前端合并为单一入口`
  - `refactor(frontend): 非 Vue 前端改为 ESM 并接入 Tauri API 包`
  - `refactor(frontend): 编辑内核收敛到 src/editor 并改名 editor.ts`
  - `refactor(editor): editor.ts 全量类型化，抽出 IPC 契约类型`
  - `fix(app): 未命名文档保存不再谎称已保存，补另存为`
  - `fix(app): 关窗与换文档前检查未保存改动，并补「新建」`
  - `test(frontend): 前端用例统一迁到 vitest，按是否需要真实布局分环境`
  - `chore(tooling): 提交 mise.toml，锁定 node 与 pnpm 版本`
  - `test(editor): 用例收进 src/editor/__tests__，与实现按目录分开`
  - `feat(markdown): 围栏代码块按语言语法高亮`
- 分支：`feat-code-highlight` 承载代码高亮（基于 `main`）；`ui-vue`（基于 `defbbf7`）已完成并快进并入 `main`，当前 `main` 含表格子工程、Vue 外壳、vitest 迁移

---

## 8. 已知缺陷与技术债

按影响排序，接手时不要误以为是新 bug：

1. ~~**删字后留下空标记**~~ **已修（M3）**：删除区间会把「内容被吃空的容器」连标记一起收起。关键是**必须在删的当下做**——`**粗**` 删掉 `粗` 之后源码是 `****`，pulldown 已不把它当容器，事后再看就没有配对了。同一函数还负责「选区切进标记一半时扩到整对」。
2. ~~**表格没有网格**~~ **已修（M4 表格子工程，2026-11-20）**：表格仍是源码块，但解析层抽出 `TableGrid / TableRow / TableCell / ColumnAlign`（行列、单元格 byte、对齐）；WYSIWYG 把同一组停靠点铺进 `<table class="md-table">`，保留全部 `data-b`/`data-a`；支持 Tab 跨格、行列增删，阅读态输出 `<tr>/<th>/<td>` 与对齐。剩余缺口：单元格内换行与多段落、跨格选区复制、表格排序。
3. **每字符一个 DOM 元素**：`preview.rs` 为了让停靠点可寻址，把每个字符渲染成独立 `<span>`。副作用是英文单词任意处可断行，DOM 节点数也偏高。生产实现应改为「每片段一个元素 + 内核 `align` 换算 char↔byte」。
4. ~~**Windows 原子写窗口期**~~ **已修（M1，2026-11-20）**：见 M1，`atomic_replace` 内核级单步替换。
5. **无虚拟化**：全文档一次性建 DOM。5 万字以上会卡，但暂无此需求。
6. ~~**图片只渲染占位符**~~ **已修（2026-11-20）**：`project.rs` 把 `![alt](url)` 折叠成原子 Span；`preview.rs` 输出带 `data-asset` 的原子节点，保留 `data-b/data-a` 停靠点。桌面端 `load_image` 命令用 `mdview_markdown::resolve_local` 把路径限制在 Markdown 所在目录内，读盘后编码成 data URL；远程/data URL 直接交给浏览器，失败则回退「图」占位符。样式限制 `max-width:100%; max-height:360px`。仍有的边界：单图上限 32 MiB，SVG 由 `<img>` 加载（脚本不执行），无按 DPI/缩放的精细排版。
7. ~~**`Watcher` 无实现**~~ **已修（M1，2026-11-20）**：见 M1，`NotifyWatcher` + 后台轮询 + 指纹二次确认 + 前端 toast。当前提示要用户手动重新打开，自动重载（会冲掉未保存编辑）未做——2026-11-23 补了「文件 → 重新加载」（`Ctrl+R`），把「手动重新打开」简化成一次菜单/快捷键，自动重载仍然不做。
8. **列表嵌套未验证**：多级列表的 `Item` 边界换行只测了单层。
9. ~~**退格会先吃掉标记字符**~~ **已修（M3）**：`Block.markers` 记录成对标记（`**…**`、`` `…` ``、`[…]() `），配对来自 pulldown 的容器事件而不是猜。退格/Delete 落在标记里时按「整对抽掉、保留中间原文」处理，一次事务完成。副作用是**语义选择**：在加粗词首退格 = 去掉加粗，而不是删掉前一个可见字。想反过来的话改 `marker_edit_at` 的调用点即可，但那样就没有「不删字去格式」的入口了。
   - 由此带出**重合停靠点**的坑，两端已经区别处理：
     - **词首（已修）**：块首 `**`、`# `、`- `、`> ` 的左右两端落在同一视觉列，「标记之前」这个位置用户永远分不清，Delete 会变成「整对去格式」而不是删第一个字。现在**左侧没有可见文字时只产生右端停靠点**（`Layout::build` 的 `has_visible`），← 也不会再空转一步。
     - **词尾（保留）**：闭合标记之前 / 之后仍是两个停靠点——前者退格删字、后者去格式。点选与 `End` 落到后者，`←` 先经过前者。要统一为「删字」也行，但会失去「不删字只去格式」的入口，等手感反馈再定。
10. ~~**原生选区未隔离**~~ **已修**：`selectionchange` 时清空浏览器选区，只留自绘 `.sel`；透明输入框内部的选区放行（清掉会打断输入法）。`#doc ::selection` 留了一条透明兜底，防清理晚一帧时两套高亮叠加。**Ctrl+A 已由内核的 `selectAll` 补上**（见第 9 节），代价是这三个组合键全都不再由浏览器提供：撤销、粘贴仍是原生行为；复制现在同时写 `text/html`（见 M4），粘到 Word 能保留格式，`text/plain` 仍是源码。
11. ~~**`Home` / `End` 按块首块尾**~~ **已修（M3）**：改用 `caretline.js` 的 `edge()`，按视觉行两端定位；矩形取不到时回退到块首/块尾。
12. **上下移动依赖当前视口的矩形**：`desiredX` 是视口坐标，窗口缩放或正文容器宽度变化会让它失效，因此 `resize` 时重置。若将来支持水平滚动或分栏，需要一并重置。
13. **空标题显示为字面 `#`**：这是「标题必须成形」的刻意取舍（见第 6 节 M3）。代价是文件里真有 `#` 独自成行的空标题时，打开后会显示成段落文本 `#`。要区分「用户正在输入」与「文件本来如此」，得把光标位置引到投影层；目前不做。
14. **空段落 = 三行空行**：文档中间的空段落（退出列表后等待输入的那一行）只能靠「分隔 + 空段落 + 分隔」三行空行表达（见第 2 节硬约束 14）。代价有两条：① 手写的文件里本来就有三行以上空行时，中间那几行会渲染成空段落，视觉上比之前多一行；② 空段落没被使用时，文件里就留着三行空行。敲下第一个字之后会自动收敛成正常的两段（`- 甲` + 空行 + `X` + 空行 + `正文`），所以只有「退出列表但没写字」的瞬间才看得到。
15. **滚动与光标跟随的边界**：页面滚动时只重画光标像素、不再「保证可见」；落点没变也不追视口（不然任何一次重绘回包都会抵消用户的滚动）。这条无法用 `--self-test` 回归：合成出来的滚动是程序化滚动，Chromium 会抑制滚动期间的脚本滚动，有没有这层修复都测不出差别；能测的只有 `scrollDelta` 的几何（`src/editor/__tests__/caretline.test.ts`）与「按键后光标重新可见」。
16. **剪贴板写入可能在无焦点窗口里挂起**：`navigator.clipboard.writeText` / `write` 在窗口没拿到焦点时，Chromium 会把请求挂住等一个不会来的授权，Promise 永远不 settle。生产里只影响「复制好像没反应」，但**自检会整条卡死**——所以自检里对它的等待必须自带 `Promise.race` 超时。另外 `execCommand('copy')` 兜底会短暂抢走 `#ime` 的焦点，调用方要还回去，否则输入法组合被中断。
17. **其它块级标记的同类抖动未处理**：`>`（引用）与 `***`／`---`（分隔线）也会在敲完最后一个字符的瞬间改变块类型。它们不涉及字号跳变，先不动；若将来统一处理，应复用同一套「成形」判定。
18. ~~**菜单没有「新建」**~~ **已修（2026-11-21）**：文件菜单补齐「新建 / 打开文件… / 保存 / 另存为… / 关闭窗口」，`Ctrl+N` 可用。新建**不在硬盘上落文件**——只在内存里换成未命名空文档（`Session::new_file` → `AppState::open("", None)`），与「双击图标启动」是同一条状态路径；首次 Ctrl+S 才弹保存对话框落盘，这样既不会留下空文件，也不用维护两套入口语义。
    - 同批修掉的两个丢稿缺口：① 未命名文档按 Ctrl+S 不再谎称「已保存」（`Effect::NeedsPath` → 弹框 → `saveTo`），写盘失败从 `Conflict` 拆成 `Effect::WriteFailed`；② 关窗与换文档前检查 dirty——`CloseRequested` 由 `on_window_event` 拦截（`CloseGuard` 放行标志 + `close-requested` 事件），`open_file` / `new_file` 前用 `ask_discard` 问一句。自检模式下确认框一律放行，否则会挂死。
19. **块级公式内含空行时不被识别**：pulldown-cmark 把 math 的定界符跟踪限制在单个块内，`$$\na\n\nb\n$$` 中的 `$$` 会退化成字面文本（mdview 的 `split_blocks` 同样按空行切块，行为一致）。多段公式请把空行去掉，或用多个 `$$…$$`。
20. **脚注里的复杂块级结构不分层**：定义内的列表 / 代码块 / 引用会被拍平成带换行的普通片段（定义块本身仍独立）。脚注以短段落为主，复杂结构请放到正文；另外无定义的 `[^x]` 按 pulldown 行为保留为字面文本。
21. **拖放打开的 Linux 支持未实机验证**：走 `WindowEvent::DragDrop`，能否收到该事件由 tauri 运行时决定。macOS 原生菜单的「最近打开」已补齐：菜单在打开 / 另存为 / 清除后经 `refresh_native_menu` 投递到主线程整棵重建。

---

## 9. 下一步（按推荐顺序）

完整清单见第 6 节 **M6**，这里只给执行顺序。**一次只推一条**，做完更新第 6 节状态与第 7 节快照，并用 Angular 规范提交。

1. ~~**M6-1 行内格式快捷操作**~~ **已完成（2026-11-26）**：`Ctrl+B/I/Shift+X/E` + 编辑菜单四档 + 自检 5 项，见第 6 节
2. ~~**M6-2 插入菜单 + `Ctrl+K` 插入链接**~~ **已完成（2026-11-26）**：插入菜单七项、图片复制到 `assets/`、自检 4 项，见第 6 节
3. ~~**M6-3 偏好设置面板**~~ **已完成（2026-11-26）**：`Ctrl+,` + 五项排版偏好 + 缩进宽度进内核，见第 6 节
4. ~~**M6-4 自动保存与崩溃恢复**~~ **已完成（2026-11-26）**：5 秒崩溃副本 + 可选的静默自动保存，见第 6 节
5. ~~**M6-5 导出 HTML**~~ **已完成（2026-11-26）**：单文件 HTML + 图片内联；PDF 走浏览器打印，见第 6 节
6. ~~**M6-6 文件侧栏**~~ **已完成（2026-11-26）**：`Ctrl+Shift+O` 打开工作区、懒展开目录树、点文件走同一条 `open_file`，见第 6 节
7. ~~**M6-8 非 UTF-8 编码读入**~~ **已完成（2026-11-27）**：GB18030 回退 + 按原编码回写，见第 6 节
8. ~~**M6-15 CI 补齐**~~ **已完成（2026-11-27）**：Rust 双平台矩阵 + 前端 job 进门禁，见第 6 节
9. ~~**M6-12 帮助体系**~~ **已完成（2026-11-27）**：快捷键一览从菜单生成 + 空状态 + 首次提示，见第 6 节
10. **M6-14 可访问性** —— 下一步；随后 M6-9 性能 / M6-10 表格，再往后 P2 剩余的 M6-13 / M6-16 / M6-17
11. **M6-7 多标签页** —— **押后单独一轮**：它是唯一需要改架构的条目（`Sessions` 从窗口级改成 `(label, doc_id)` 双键、所有 IPC 命令带文档标识、脏检查与外部修改监听按标签重做），动手前先在第 8 节写设计再实现

已完成的历史项（`selectAll`、拖边自动滚动、Watcher、M2 收尾、表格子工程、粘贴 HTML、段末 Delete）见 M1–M5 与第 8 节，不再在此记账。

**遗留技术债**（不属 M6 主线，分析或改到相关代码时顺手清）：词尾重合停靠点的退格语义（第 8 节第 9 条）、引用与分隔线的成形判定（第 8 节第 17 条）。

---

## 10. 命令速查

```bash
mise install                                # 首次：按 mise.toml 拉齐 node 26.13.0 / pnpm 12.18.0
pnpm install                                # 首次
pnpm build                                  # 构建前端（含 vue-tsc --noEmit）；dist/ 不入库
pnpm exec vue-tsc --noEmit                  # 只做类型检查
cargo test --workspace                      # 全量测试（依赖上面的 dist）
cargo run -p mdview-cli -- demo             # 端到端演示（edit/click/undo/save）
cargo run -p mdview-cli -- demo edit        # 键盘脚本回放：输入 → 光标 → 增量帧
cargo run -p mdview-cli -- <file> outline   # 大纲
cargo run -p mdview-cli -- <file> stats     # 字符/字节/块/隐藏区间统计
cargo run -p mdview-cli -- <file> blocks    # 块清单
cargo run -p mdview-cli -- <file> spans     # 逐块投影，⟦⟧ 内为折叠标记
cargo run -p mdview-cli -- <file> layout    # 停靠点表：列 / byte / 字符（← 同列）
cargo run -p mdview-cli -- <file> preview   # 生成 <file>.preview.html
cargo run -p mdview-cli -- <file> html      # 阅读态 HTML
pnpm tauri dev -- -- <file>                 # 起窗（Tauri 2 + Vite devUrl）
pnpm tauri dev -- -- tests/fixtures/selftest.md --self-test
                                            # 端到端自检（CWD=src-tauri/），跑完自动退出
cargo run -- src-tauri/tests/fixtures/selftest.md --self-test
                                            # 备选链路（裸 cargo run 即桌面端，default-members 兜底）
pnpm test                                   # 前端全部用例（unit + dom 两个 project）
pnpm test:unit                              # 七个纯逻辑模块（node 环境，cargo test 已含这一支）
pnpm test:dom                               # 真实浏览器里的内核启动用例（需本机有 Chrome/Edge）
pnpm test:watch                             # 改代码时盯着
```

`--self-test` 会在真实 WebView2 里合成 composition 事件与方向键，把结果写进日志并以 0/1 退出，是唯一不需要人打字的端到端验证。它仍然是唯一跑在**真实 WebView2**里的验证——vitest 的 dom project 用的是系统 Chrome，能测真实布局（光标像素、点击落点、视口滚动），但内核最终要跑的那个 WebView2 环境只有自检覆盖得到。

**别念外层退出码**：`pnpm tauri dev -- ... --self-test` 有时会以 `exit code -1` 结束并在终端打一条 `Failed to unregister class Chrome_WidgetWin_0`，但 `mdview.log` 里已经是「自检通过：OK 65/65」（应用侧走的是 `app.exit(0)`）。判定只看日志那一行。

日志落在可执行文件同级 `logs/`：`mdview.log`（5×1MB 滚动）、`error.log`（error 独占，5×1MB）。只读目录自动回落到 `%LOCALAPPDATA%/mdview/logs`。

`layout` 是最有价值的调试入口：同一列出现两行就说明那里有两个停靠点（中间隔着被折叠的标记）。

---

## 11. 文件地图

| 关注点 | 文件 |
|---|---|
| 文本与撤销 | `crates/mdview-core/src/document.rs`、`transaction.rs` |
| 块切分（含末尾空段落） | `crates/mdview-markdown/src/split.rs` |
| 片段模型、`align`、HTML、**标记配对表**（`Block::markers`） | `crates/mdview-markdown/src/block.rs` |
| 投影（差集、图片原子化、内联代码/公式定界符内缩、标记配对推导、**表格结构抽出**） | `crates/mdview-markdown/src/project.rs` |
| **代码块词法着色**（token → `tk-*` class，47 种语言 / 格式） | `crates/mdview-markdown/src/highlight/` |
| **表格结构化模型**（`TableGrid` / 单元格 byte / 对齐 / 跨格导航 / 阅读态 `<tr>/<th>/<td>`） | `crates/mdview-markdown/src/block.rs` |
| **视觉布局与停靠点** | `crates/mdview-markdown/src/layout.rs` |
| **分块帧与增量 patch**（`block_frames` / `diff_frames`）+ 验证页 + **表格网格渲染** + 公式 `.m` 样式 | `crates/mdview-markdown/src/preview.rs` |
| **编辑闭环**（输入 → 事务 → 光标回填 → patch）+ **表格跨格 / 行列增删** | `crates/mdview-app/src/state.rs` |
| 端口（FileStore 原子写 / RenderSink / **NotifyWatcher**） | `crates/mdview-app/src/ports.rs` |
| 外部修改监听线程 + `external-change` emit | `src-tauri/src/main.rs`（setup 钩子） |
| 外部修改提示（前端 listen + toast） | `src/editor/editor.ts`（boot） |
| 日志滚动 | `crates/mdview-support/src/logging.rs` |
| 命令行入口（含 `demo edit` 回放） | `crates/mdview-cli/src/main.rs` |
| **IPC 契约**（前端 `kind` → `Message`、`Patch`/光标回包、大纲下发、**表格操作**、**保存/另存为**） | `src-tauri/src/session.rs` |
| **IPC 契约的前端类型**（`Wire` / `Reply` / `Snapshot` / `CaretDto`，字段名与 serde 输出逐一对齐） | `src/editor/ipc.ts` |
| **保存与另存为**（`Effect::NeedsPath` / `WriteFailed`、保存对话框只给路径不写盘） | `crates/mdview-app/src/state.rs`（`save_into`）、`src-tauri/src/main.rs`（`pick_save_path`）、`src/editor/editor.ts`（`requestSavePath`） |
| **新建 / 关闭 / 换文档的守卫**（`new_file`、`ask_discard`、`CloseGuard` + `close-requested`） | `src-tauri/src/main.rs`、`src-tauri/src/session.rs`（`new_file` / `is_dirty`）、`src/editor/editor.ts`（`adoptSnapshot`、close 监听） |
| **表格交互**（Tab 跨格、状态栏行列工具条、自检 6 项） | `src/editor/editor.ts`、`app.css` |
| **大纲侧栏**（锚点判定 + 跳转，不碰 DOM，跑 unit project） | `src/editor/outline.js`、`src/editor/__tests__/outline.test.ts`、`src/editor/editor.ts`（侧栏渲染/联动） |
| **主题系统**（跟随系统 + 手动切换 + localStorage，不碰 DOM，跑 unit project） | `src/editor/theme.js`、`src/editor/__tests__/theme.test.ts`、`app.css`（CSS 变量） |
| **编辑页**（停靠点 → 像素、按键 → 消息、增量应用、自检入口） | `src/editor/editor.ts` |
| **输入法状态机**（不碰 DOM，跑 unit project） | `src/editor/ime.js`、`src/editor/__tests__/ime.test.ts` |
| **上下移动几何**（视觉行判定 + 期望列，不碰 DOM，跑 unit project） | `src/editor/caretline.js`、`src/editor/__tests__/caretline.test.ts` |
| **视口与原生选区**（滚多少才露头、哪些原生选区该清，不碰 DOM） | `src/editor/caretline.js`（`scrollDelta`）、`src/editor/selection.js`、`src/editor/__tests__/selection.test.ts` |
| **拖选时的边缘自动滚动**（速度曲线、探针点压缩、自走帧循环，不碰 DOM） | `src/editor/dragscroll.js`、`src/editor/__tests__/dragscroll.test.ts` |
| **浏览器里的内核启动用例**（铺 index.html 骨架 + mock IPC + 断言光标像素） | `src/editor/__tests__/editor.dom.test.ts`、`src/editor/__tests__/env.dom.test.ts` |
| 自检样张（必须是「一块一行」的文档） | `src-tauri/tests/fixtures/selftest.md` |
| **剪贴板写入**（内核给文本 + HTML、前端一次 `ClipboardItem` 写双格式，`execCommand` 兜底） | `src/editor/clipboard.js` |
| **HTML → Markdown**（标题 / 列表 / 引用 / 代码块 / 表格 / 链接 / 图片 / 行内格式，协议白名单，纯逻辑） | `src/editor/html-to-md.js`、`src/editor/__tests__/html-to-md.dom.test.ts` |
| **查找替换纯逻辑**（UTF-16→UTF-8 下标、全词 / 正则匹配、`$1` 展开、保留大小写、批量替换，不碰 DOM） | `src/editor/find.js`、`src/editor/__tests__/find.test.ts` |
| **查找替换接线**（VSCode 式浮层、三个内联开关、命中高亮、跳转、替换、选区内查找、防抖重算） | `src/editor/editor.ts`、`index.html`（`#findbar`）、`app.css` |
| **链接 / 图片行内编辑**（面板、`data-href`/`data-asset` 预填、Esc/外部点击收起） | `src/editor/editor.ts`、`index.html`（`#linkedit`）、`app.css` |
| **链接 / 图片改写事务**（标记对圈住整段、URL 转义、删除保留 label） | `crates/mdview-app/src/state.rs`（`edit_link`） |
| **Tab 缩进 / 反缩进**（列表层级、顶层不动、字符偏移回填） | `crates/mdview-app/src/state.rs`（`indent` / `list_position`） |
| **字数统计**（按块 `plain_text` 数非空白字符） | `crates/mdview-app/src/state.rs`（`word_count`）、`src/components/StatusBar.vue` |
| **多窗口会话注册表**（label → `Session` / 关窗守卫，创建、销毁、单例转发、按窗口 watcher） | `src-tauri/src/main.rs`（`Sessions` / `CloseGuards` / `create_window` / `watch_loop`） |
| **最近打开持久化**（`recent.json`、去重、上限 10）与 **macOS 原生菜单重建**（`build_native_menu` / `rebuild_native_menu` / `recent_menu_entries`） | `src-tauri/src/recent.rs` 与 `main.rs` 的 `recent_files` / `clear_recent_files` |
| **拖放打开**（`WindowEvent::DragDrop` → `drop-open` / `drop-hover`） | `src-tauri/src/main.rs`（`first_markdown`）、`src/editor/editor.ts` |
| **键位契约**（`Ctrl+A/C/X` → `selectAll`/`copy`/`cut`，见 `editor.ts` 的 keydown） | `src/editor/editor.ts`、`src-tauri/src/session.rs` |
| 前端守门测试（语法检查七个 `.js` + 跑 vitest 的 unit project；dom project 不进 cargo test） | `src-tauri/tests/web_ime.rs`、`vitest.config.ts` |
| **Vue 外壳入口**（挂载、单入口 import、UA 首帧判定、平台复核） | `src/main.ts`、`src/platform.ts`、`src/App.vue` |
| **状态桥**（只读快照 / `set` / `notify` / `dispatch` / `setTitle` / `nextTick`） | `src/bridge.ts` |
| **标题栏 / 菜单 / 状态栏 / Toast**（模板复刻原 `id`/`class`） | `src/components/TitleBar.vue`、`MenuPopup.vue`、`StatusBar.vue`、`Toast.vue` |
| **菜单数据与展开状态**（六档菜单树、勾选态、键盘切换） | `src/menu.ts` |
| **自绘滚动条**（正文 + 大纲，支持拖拽 / 点轨道） | `src/scrollbars.ts` |
| **覆盖层样式**（整层穿透，外壳显式 `pointer-events:auto`） | `src/vue-overlay.css`、`src/app.css` |

---

## 12. 协作约定

- 提交信息用 **Angular 规范**（`type(scope): subject`），subject 简洁中文。类型用 `feat` / `fix` / `refactor` / `test` / `docs` / `chore`。
- 分支名不含 `/`。
- **计划只有一份 `PLAN.md`**：专项工作（如前端框架化）的架构决策、分阶段记录与踩坑直接并进本文对应章节，不再新开 `PLAN-*.md`，避免两处状态漂移。
- 改动解析或布局层时，必须同时补测试；`layout` 的停靠点行为是回归高发区。
- 不引入非纯 Rust 依赖到 `mdview-markdown`，如有必要必须开发者审批后执行。
- 每次实证结论（尤其是与直觉相反的）写进本文或 `.workbuddy/memory/`，避免下一轮重复调研。

# PLAN-VUE.md — `ui-vue` 分支：Vue 3 + Vite 重构前端外壳

> 最后更新：2026-11-20 17:20（前端工程根提到仓库根，Rust crate 在根目录 `src-tauri/`）
> 分支：`ui-vue`（基于 `main` @ `defbbf7`）
> 性质：`ui-vue` 是**独立专项分支**，本文只管前端外壳的框架化，不改动主 `PLAN.md` 的编辑内核计划。
> 本文自包含：接手人读完即可继续，不需要翻历史对话。硬约束见主 `PLAN.md` 第 2 节，本文不重复。

---

## 1. 目标与范围

### 1.1 要做的

用 **Vue 3 + Vite（TS）** 接管前端的**外壳 UI**：标题栏、菜单弹层、状态栏、toast。
动机是这部分代码（`titlebar.js` 464 行的菜单树 + 递归渲染 + 子菜单 + 勾选态，以及散落在 `app.js` 里的状态栏/toast 写入）难维护，用组件化改写能显著收敛。

同时把目录整理成**标准 Tauri 结构**：前端工程根在仓库根，Rust crate 整体挪进根目录 `src-tauri/`（其余库 crate 仍在 `crates/`）。

### 1.2 明确不做的（红线）

下面这些是项目最难、回归高发的核心逻辑，**一行都不迁、一行都不改**：

- `app.js` → `editor.ts` 的 `#doc` 投影、`reindex`、`applyPatch`、光标定位、点击/拖选落点、`revealCaret`、视口坐标
- `ime.js` / `caretline.js` / `selection.js` / `dragscroll.js` / `clipboard.js` / `outline.js` / `theme.js` 七个纯逻辑模块及其六套 Node 用例（只挪目录、改名文件路径）
- Rust 侧 `session.rs` 的 IPC 契约、`main.rs` 的窗口与菜单逻辑（只挪目录、改注释）

**一句话范围**：外壳进 Vue，编辑内核原地不动，两者之间只有一条传「状态快照 + 动作名」的窄桥。

---

## 2. 选定方案：留洞式 Vue 覆盖层

### 2.1 UI 组合简图

```text
视口 html/body = 100vh；window 仍是滚动容器（关键：window.scrollY 保持有效）
┌──────────────────────────────────────────────────────────────────┐
│ Vue 覆盖层 #vue-app   position:fixed; inset:0;                   │
│                       pointer-events:none; z-index 高于现有层    │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ TitleBar.vue            pointer-events:AUTO               │  │
│  │   ├─ 菜单按钮 ──────> MenuPopup.vue   AUTO（动态弹层）     │  │
│  │   ├─ 居中标题            只读 bridge 的 title              │  │
│  │   └─ 最小化/最大化/关闭   AUTO + data-tauri-drag-region    │  │
│  ├────────────────────────────────────────────────────────────┤  │
│  │ ┌──────── 洞 1：大纲 ────────┐                             │  │
│  │ │   无任何 Vue 节点          │  ← 鼠标穿透                │  │
│  │ │   落到现有 #outline        │    点击跳转/高亮全走原 JS  │  │
│  │ │   + #outline-scroll        │                             │  │
│  │ └────────────────────────────┘                             │  │
│  │ ┌──────── 洞 2：正文 ────────┐                             │  │
│  │ │   无任何 Vue 节点          │  ← 鼠标穿透                │  │
│  │ │   落到现有 #doc            │    caret / IME / 选区 /     │  │
│  │ │   #caret #ime #preedit     │    拖选自动滚动全走原 JS    │  │
│  │ │   + #vscroll               │                             │  │
│  │ └────────────────────────────┘                             │  │
│  ├────────────────────────────────────────────────────────────┤  │
│  │ StatusBar.vue           pointer-events:AUTO               │  │
│  │   块/行统计 · 脏标记 · 主题切换 · 表格行列工具条           │  │
│  │ Toast.vue               pointer-events:NONE               │  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
                              │ 穿透（洞里没有 Vue 节点）
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ 现有层（DOM 与 JS 全部原地保留）                                 │
│   #titlebar / #bar / #toast  ← 保留占位供 edges() 读几何，被上层盖住│
│   #outline / #doc / #caret / #preedit / #ime / #vscroll          │
│   editor.ts · seven 纯逻辑模块 · 六套 Node 用例                   │
└──────────────────────────────────────────────────────────────────┘
```

**核心**：大纲和正文不是「Vue 画的镜像」，而是**真正的洞**——Vue 层在那两处不放任何节点。所以没有第二套大纲/正文 DOM，上一版「覆盖层镜像」的同步难题整块消失。

### 2.2 为什么是这一版（方案演进记录，别重复踩）

按讨论顺序，否决理由如实记录：

| 方案 | 结论 | 原因 |
|---|---|---|
| `v-pre` 跳过 `#doc` 子树 | 否决 | Vue 3 确实会把 `v-pre` 子树 hoist、patch 不深入，但依赖编译器优化行为；一旦 `#doc` 误绑 `:class`/`v-if`/`:key` 就失效。物理隔离更稳 |
| Vue 挂载点物理隔离（Vue 只 mount `#ui`，渲染层在挂载点外） | **采纳为地基** | 边界在 DOM 结构上硬隔离，不依赖编译器行为 |
| `#doc-placeholder` 占位 + `#doc` 绝对定位叠加 | 否决 | `#doc` 脱离文档流后不再撑高 body，`window.scrollY` 恒 0；caret/锚点/拖选/自绘滚动条的坐标全要重写 |
| `#doc` 自带滚动的 fixed 工作区 | 否决 | 同上，滚动容器换成内部后坐标来源整体要换 |
| `#doc` 与 body 同高 + 外壳 `pointer-events:none` | 部分成立 | 穿透确实让 `elementFromPoint` 命中正文，但「穿透」与「谁在滚动」是两件事；只要滚动归 `#doc`，坐标仍要改 |
| Vue 放到现有实现上层、对应区域穿透 | 暴露镜像问题 | 同一份状态两套 DOM（现有 `#bar` 与 Vue 状态栏、现有 `#tb-title` 与 Vue 标题），同步机制别扭 |
| **只穿透大纲和正文（留洞）** | **最终采纳** | Vue 根本不画这两处 → 无镜像；window 滚动不变 → 坐标零改动；外壳由 Vue 真正接管 |

**必须记住的判据**：`pointer-events:none` 只解决「鼠标命中谁」，不改变「谁在滚动」。app.js 那套坐标能否不动，唯一取决于 `window.scrollY` 是否仍有效。留洞方案里 `#doc` 留在文档流、滚动归 window，所以有效。

### 2.3 双层重叠的代价与 P0 规避

Vue 画的标题栏/状态栏/toast 与现有同名 DOM 会**短暂共存**（两层重叠）。这是留洞方案唯一新增的代价，P0 这样规避：

- 现有 `#titlebar` / `#bar` / `#toast` **不删**，继续在文档流里占位——`titlebar.js` 的 `edges()` 要读它们的 `getBoundingClientRect()`。
- Vue 层严格像素级复刻其尺寸（标题栏 30px、状态栏 29px、大纲 232px），背景设为不透明，完整盖住下层。
- 鼠标命中由 Vue 层的 `pointer-events:auto` 接管，下层按钮收不到点击，不会双触发。
- P3 可评估删除下层 DOM、把 `edges()` 的数据源换成 bridge——但这是可选优化，不影响 P0 正确性。

---

## 3. 目标目录结构（标准 Tauri 布局）

前端工程根 = **仓库根**（与根 `Cargo.toml` 同级），Rust crate 整体挪进根目录 `src-tauri/`。这就是 `create-tauri-app` 的标准形态：前端在根、Rust 在 `src-tauri/`；其余库 crate 因 workspace 布局仍留在 `crates/`。

```text
<repo-root>/                            ← 前端工程根（与根 Cargo.toml 同级）
├─ Cargo.toml / Cargo.lock              workspace 清单（members 含 "src-tauri"）
├─ index.html                           Vite 入口；#vue-app 覆盖层 + 洞
├─ package.json / pnpm-lock.yaml        Vue 3 + Vite + @vitejs/plugin-vue + TS
├─ vite.config.ts                       build.outDir = 'dist'，emptyOutDir = true
├─ tsconfig.json
├─ src/                                 ← 前端源码（原 web/ 改名；不再与 Rust 混淆）
│  ├─ main.ts                           createApp → mount('#vue-app') → editor.init()
│  ├─ App.vue                           组合 TitleBar / StatusBar / Toast（大纲与正文是洞）
│  ├─ components/
│  │  ├─ TitleBar.vue
│  │  ├─ MenuPopup.vue                  菜单树数据 + 子菜单 + 勾选态
│  │  ├─ StatusBar.vue
│  │  └─ Toast.vue
│  ├─ bridge.ts                         reactive 状态总线 + 动作分发
│  ├─ editor.ts                         由原 app.js 移入：#doc 投影、光标、IME、选区、滚动、自检
│  ├─ styles/                           外壳组件样式（从 app.css 拆出对应规则）
│  └─ ime.js / caretline.js / selection.js / dragscroll.js / clipboard.js
│     outline.js / theme.js + 六套 *.test.js   ← 七个纯逻辑模块原样保留
├─ dist/                                ← Vite 产物（gitignore）；frontendDist 指向它
├─ src-tauri/                           ← Rust crate（原 crates/mdview-desktop 内容整体挪入）
│  ├─ Cargo.toml / build.rs
│  ├─ tauri.conf.json + tauri.{windows,linux}.conf.json
│  ├─ capabilities/default.json
│  ├─ icons/
│  ├─ src/{main.rs, session.rs}
│  ├─ tests/{web_ime.rs, fixtures/}
│  └─ gen/                              （gitignore，Tauri 自动生成）
└─ crates/                              ← 其余五个库/CLI crate
   └─ mdview-{core,support,markdown,app,cli}/
```

**代价**：这一步不是纯改名，Rust 侧与跨 crate 的若干路径要跟着改（详见 4.4）。收益是结构彻底标准，前端 `src/` 与 Rust `src-tauri/src/` 各归其位，不再有「Rust 占了 src/ 所以前端只能叫 web/」的妥协。

---

## 4. 构建链与路径配置

### 4.1 Vite

- `build.outDir = 'dist'`、`emptyOutDir = true`——产物落在前端根的 `dist/`，每次构建清空，杜绝旧脚本残留（历史上两个脚本顶层 `const` 撞名导致 `app.js` 整个不执行）。
- `server.port = 1450`、`strictPort = true`。
- 包管理器沿用参考项目 `image-view/app` 的 `pnpm`。

### 4.2 Tauri

`src-tauri/tauri.conf.json` 的 `build` 段由：

```json
{ "build": { "frontendDist": "./web" } }
```

改为：

```json
{
  "build": {
    "frontendDist": "../dist",
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1450",
    "beforeBuildCommand": "pnpm build"
  }
}
```

**关键**：`frontendDist` 相对 conf 文件所在目录解析。conf 现在在 `src-tauri/`，产物在前端根的 `dist/`，所以是 `../dist`（写成 `./dist` 会指向不存在的 `src-tauri/dist`）。`beforeDevCommand` 由 Tauri 在前端根执行，正好命中那里的 `package.json`。平台 conf 无 `build` 段，无需改。

### 4.3 CSP

CSP 保持 `default-src 'self'`，**不要**为 Vue 开 `unsafe-eval`：走 runtime-only 构建（`@vitejs/plugin-vue` 预编译模板），运行时不拼 template 字符串。

`index.html` 首帧的 UA 平台判定是内联 `<script>`，迁移时复核它是否被 CSP 拦；若拦，挪进 `main.ts` 首行（代价是首帧可能闪一下，随后由 Rust 的 `use_custom_titlebar` 纠正）。

### 4.4 Rust 侧路径改动清单（迁移时逐条核对）

**分两步走**：P0-a 只挪 Rust，此时前端仍在 `web/`、尚无 `dist/`，所以所有路径先指向 `../web`（现有 `web/index.html` 可直接当产物，自检能过）；P0-b 搭好 Vite、`web/` 改名 `src/`、产物落到 `dist/` 后，再把路径切成最终值。写死最终值会让 P0-a 的自检找不到资源。

| 文件 | P0-a（纯搬家） | P0-b（Vite 就位） | 说明 |
|---|---|---|---|
| 根 `Cargo.toml` | `members = ["crates/*"]` → **显式列出六个 crate**，desktop 写 `crates/mdview-desktop/src-tauri` | 不变 | `crates/*` 会匹配到已无 `Cargo.toml` 的 `crates/mdview-desktop`，必须排除；`workspace.dependencies` 的 path 相对 workspace 根，不受影响 |
| `src-tauri/build.rs` | `web` → `../web` | `../web` → `../dist` | build script 的 CWD 是 `src-tauri/`；`tauri_build::build()` 自动读同目录 conf，不用改 |
| `src-tauri/tauri.conf.json` | `frontendDist: "./web"` → `../web` | `../web` → `../dist`，并补 dev 命令 | conf 相对自身目录解析；P0-a 阶段 `web/index.html` 就是产物 |
| `src-tauri/tests/web_ime.rs` | `join("web")` → `join("../web")` | `../web` → `../src` | 守门测试 CWD 变成 `src-tauri/`；语法检查清单里的文件名不变 |
| `.gitignore` | `crates/mdview-desktop/gen` → `crates/mdview-desktop/src-tauri/gen` | 不变 | `node_modules` 忽略规则不变（仍在前端根） |
| `src/main.rs` | `web/app.js` → `web/index.html` | `web/index.html` → `src/editor.js`；`web/ 里的内容变了` → `dist/ 里的内容变了` | 仅注释，`generate_context!()` 调用不变（它读同目录 conf） |
| `src/session.rs` | 注释 `与 web/app.js 里发的一致` 保持（文件仍在 `web/`） | → `与 src/editor.js 里发的一致` | 仅注释，契约字面量不变 |
| **`crates/mdview-app/src/state.rs`** | `table_ops_on_selftest_fixture` 里的 `read_to_string("../mdview-desktop/tests/fixtures/selftest.md")` → 插入 `src-tauri/` | 不变 | **易漏**：跨 crate 读样张的测试，不在 desktop crate 内，grep `tests/fixtures` 才会发现 |
| 自检命令 | 参数路径 → `.../src-tauri/tests/fixtures/selftest.md` | 不变 | 见第 8 节；主 `PLAN.md` 里的旧路径本次不改 |

**P0-c（本次）：桌面 crate 从 `crates/mdview-desktop/` 整体上提到仓库根。** 上表中带 `crates/mdview-desktop/` 前缀的都是历史中间态，**当前最终值**是：

| 文件 | 当前值 |
|---|---|
| 根 `Cargo.toml` | `members` 中 desktop 写 `"src-tauri"`（不再是 `crates/mdview-desktop/src-tauri`）；另设 `default-members = ["src-tauri"]`，让裸 `cargo run` 默认跑桌面端（要 `--workspace` 才覆盖全部） |
| `.gitignore` | `/src-tauri/gen`、`/dist` |
| `crates/mdview-app/src/state.rs` | `read_to_string("../../src-tauri/tests/fixtures/selftest.md")` |
| `src/*.test.js`（4 个） | 注释里的守门测试路径改为 `src-tauri/tests/web_ime.rs` |

**不受影响**（相对 `src-tauri/` 的关系没变）：`tauri.conf.json` 的 `frontendDist: "../dist"`、`build.rs` 的 `../dist`、`web_ime.rs` 的 `../src`、`vite.config.ts` 的 `outDir: 'dist'`、`package.json` 的 scripts、`index.html` 的 `/src/xxx.js` 引用。

`icons/` 与 `capabilities/` 随 crate 一起挪即可：Tauri 相对 conf 目录自动查找，配置里无需改路径。

**`dist/` 不提交（已定）**：与参考项目 `image-view` 一致，`dist` 写进 `.gitignore`，构建产物不入仓库。代价是 `generate_context!()` 在 `cargo build` 时就要读产物——**自 clone 起、或清空 `dist/` 后，必须先 `pnpm build` 再 `cargo build`**，直接 `cargo build` 会因找不到前端资源而编译失败。`tauri dev/build` 走 `beforeDevCommand` / `beforeBuildCommand` 会自动先构建前端，不受此限；只有直接跑 `cargo` 命令时才需要手动先 build。

---

## 5. 桥接契约（本重构最重要的一段）

`bridge.ts` 是两侧唯一通道，**只传状态快照和动作名，不传文档内容、不传 DOM 节点**。

### 5.1 状态进 Vue（单向）

reactive store 字段：`meta.title / stats.blocks / stats.lines / dirty / themeMode / outline / activeOutlineIdx / toast`。

写入点在现有逻辑里各加一句：

| 现有写入点 | 加一句 |
|---|---|
| `titlebar.js` 的 `setTitle` | `bridge.set({ title })` |
| `editor.ts`（原 app.js）的 `renderStatus` | `bridge.set({ blocks, lines, dirty })` |
| `editor.ts` 的 `runAction` / 主题切换 / 大纲联动 | `bridge.set(...)` |

Vue 组件只读 store，反向不写。

### 5.2 动作出 Vue（单向）

Vue 侧的菜单项、状态栏按钮点击后：`emit('action', name)` → `bridge.dispatch(name)` → 复用现有 `window.__mdviewRunAction(name)`。**动作逻辑一行不重写**。

### 5.3 全局钩子保留

`window.__mdviewSelfTest` / `__mdviewRunAction` / `__mdviewSetTitle` / `__mdviewRefreshScrollbar` 继续由 `editor.ts` 挂在 `window` 上，Rust 侧 `window.eval` 和自检断言不用动。

### 5.4 几何对齐

标题栏/状态栏实测高度写进 CSS 变量，`#doc` 的 `padding-top` / 底部留白与 `#vscroll` 的 edges 继续读现有 DOM（见 2.3）。macOS 原生标题栏下 `#titlebar` 由 CSS 隐藏，`#vue-app` 里的 TitleBar 也要同步隐藏（同一套 `html.native-titlebar` 选择器），避免画出一个多余的假标题栏。

---

## 6. 分阶段计划与过关线

基线（迁移前，已实测）：**端到端自检 57/57** · **Rust 194 项全过** · **六套 Node 用例 71 项全过**。P0 后实测为 **自检 61/61**、**Rust 全量测试绿**。每阶段结束都必须回到这条线。

| 阶段 | 内容 | 过关线 |
|---|---|---|
| **P0-a** | Rust crate 整体挪进 `src-tauri/`；按 4.4 的 P0-a 列改路径（临时指向 `../web`）；不碰任何前端代码 | **纯目录搬家**：`cargo test --workspace` 全绿 + 自检 57/57（用新 fixtures 路径） |
| **P0-b** | 搭 Vite；`web/` 改名 `src/`；产物 `dist/`、端口 1450；把 4.4 的路径切成最终值；`#vue-app` 覆盖层留洞；`bridge.ts` / `editor.ts` 拆出 | 行为不变；自检 57/57 + 全量测试绿 |
| **P1** | Vue 组件挂载但仍是空壳/只读展示；把 5.1 的状态写入点与 5.2 的动作桥接铺好 | 自检全绿；鼠标穿透、拖拽区、滚动条均无回归 |
| **P2** | 逐组件迁移，每组件单独提交：`StatusBar` → `Toast` → `TitleBar`/`MenuPopup` | 每步自检全绿；模板复刻现有 `id`/`class`，让断言零改动 |
| **P3** | 删旧脚本引用；`web_ime.rs` 守门清单改造（Vue 侧交 `vue-tsc --noEmit`）；CSP 复核；更新主 `PLAN.md` 与本文件 | 全量测试 + 自检 + 真机截图三对齐 |

P0-a 独立成段是因为它和 Vue 无关、纯路径调整，先做能单独验证，避免把目录问题和前端问题混在一起排查。P0-b 再一次性完成 Vite 就位与路径切换。`MenuPopup` 在 P2 放最后：它有键盘导航、子菜单 `pointerenter`、打开瞬间求值勾选态三个易碎交互，且自检覆盖最多。

---

## 7. 风险红线

1. **大纲和正文必须是洞**：`#outline` / `#doc` / `#caret` / `#preedit` / `#ime` / `#vscroll` / `#outline-scroll` 不进任何 Vue 模板，不写进 `#vue-app`。这是方案命门。
2. **window 滚动不变**：`#doc` 留在文档流、`min-height:100vh` 撑高 body。任何把它改成 fixed/内部滚动的改动都会让坐标体系失效。
3. **Vue 异步 DOM**：现有自检大量是「派发事件后同步查 DOM」。组件化后要 `nextTick`；尺寸相关测量要 `nextTick` + `requestAnimationFrame` 之后再做，否则 caret/锚点会在布局稳定前算错。
4. **穿透例外要显式打开 pointer-events:auto**：菜单按钮、`#tb-min/max/close`、`[data-tauri-drag-region]`、状态栏表格工具按钮。漏了拖拽区就拖不动窗口。大纲与自绘滚动条在洞里，天然不受穿透规则影响。
5. **正文底部让位**：状态栏 fixed 贴底，正文要留 `padding-bottom`，否则最后一行滚到底仍被盖住（当前就有的老问题，顺手修）。
6. **`emptyOutDir` 必须开**：产物目录是 `dist/`，每次构建清空，防旧脚本残留导致全局符号双份加载。
7. **Vue 化不改自检断言的 DOM 契约**：迁移时组件模板保留现有 `id`/`class`；确需改断言时，同步改 `editor.ts` 的 `__mdviewSelfTest` 与 `tests/` 下的用例，不允许只改一边。
8. **`frontendDist` / `build.rs` / workspace members 必须同改**：三者任一遗漏都会出现「跑的是旧产物」「编译期嵌入空目录」或「workspace 解析失败」。P0-a 后用自检和全量测试验证，不要靠肉眼。
9. **相对路径基准变了**：conf、`build.rs`、守门测试的 CWD/基准目录都从 crate 根变成 `src-tauri/`，所有 `web` 相关路径都要重新核对；`generate_context!()` 和 capabilities 因随 conf 同目录而免疫。
10. **UMD 脚本必须带 `type="module"` 才会被 Vite 打包**：`index.html` 里那 9 个 `<script src="/src/xxx.js">` 若不写 `type="module"`，Vite 会警告「can't be bundled」并原样输出 `/src/` 路径，而 dist 里没有 `src/` 目录 → 运行时全部 404。加上后 Vite 把它们打进 bundle，ES module 按文档顺序执行，恰好保住 UMD 脚本间的依赖顺序，且模块作用域避免了顶层 `const` 撞名。**P2 迁移某脚本时要同步把它从 index.html 移除**，否则会重复执行。
11. **`"type": "module"` 与 CommonJS 测试的冲突**：前端根 `package.json` 必须是 `type: module`（Vite 要求），但六套 `*.test.js` 用 `require('assert')`。解法是在 `src/` 下放一个局部 `package.json` 写 `{"type":"commonjs"}`，让 Node 按 CommonJS 跑测试，Vite 不受影响（P0-b 实测：加了之后 `pnpm build` 与守门测试同时通过）。**不要**把测试改成 `.cjs`——那会牵动守门测试里的文件名清单。
12. **`dist/` 不入库，直接 `cargo build` 会失败**：`generate_context!()` 编译期读 `dist/index.html`，新 clone 或清理后它是空的。任何 `cargo` 直跑前先 `pnpm build`；`tauri dev/build` 有 before 命令兜底。`cargo test --workspace` 也会编译 desktop bin，所以首次跑测试前同样要先 build 前端。

---

## 8. 验证命令

```bash
# 构建前端（工程根 = 仓库根；dist 不入库，自 clone 起或清空 dist 后必须先跑）
pnpm install
pnpm build
pnpm exec vue-tsc --noEmit

# 全量测试（含守门测试；cargo 直接跑依赖上面的 dist）
cargo test --workspace

# 端到端自检 —— 推荐用 tauri dev（走 Vite devUrl 链路，窗口能拿到焦点）
# 注意 CWD：tauri dev 把应用参数转给 cargo run 时，cwd 是 src-tauri/，
# 所以样张路径写 tests/fixtures/selftest.md，不是 src-tauri/tests/...
pnpm tauri dev -- -- tests/fixtures/selftest.md --self-test

# 备选：cargo run 从仓库根跑，路径要带 src-tauri/ 前缀。
# 根 Cargo.toml 设了 default-members = ["src-tauri"]，所以裸 cargo run 就是桌面端
# （不需要 -p mdview-desktop；要跑 CLI 得显式 -p mdview-cli）。
# 但这条链路窗口拿不到焦点时「系统剪贴板可写」会偶发 false（60/61），
# 不是回归——判定过关以 tauri dev 的 61/61 为准。
cargo run -- src-tauri/tests/fixtures/selftest.md --self-test

# 起窗人工验收（不含 --self-test）
pnpm tauri dev -- -- tests/fixtures/selftest.md
```

失败报告写在可执行文件同级 `logs/error.log`，排查别只看 `mdview.log`。

---

## 9. 进度记录

| 日期 | 事项 |
|---|---|
| 2026-11-20 | 建 `ui-vue` 分支（基于 `main` @ `defbbf7`）；写本计划；尚未改动任何前端文件 |
| 2026-11-20 | 用户调整：产物 `web` → `dist`、端口 1420 → 1450、前端工程根从 `frontend/` 提到 crate 根 |
| 2026-11-20 | 用户再调整：`web/` 改名 `src/`；为此把 Rust crate 整体挪进 `src-tauri/`，采用标准 Tauri 结构；新增 P0-a 阶段与第 4.4 节路径清单 |
| 2026-11-20 | **P0-a 完成**：`git mv` 搬迁 9 项 Rust 文件 + gen；根 Cargo.toml 改显式 members；build.rs/conf/web_ime.rs/.gitignore 路径改 `../web`（中间态）；补改跨 crate 的 `mdview-app/state.rs` fixtures 路径（原清单漏项，已补进 4.4）。验证：`cargo test --workspace` 204 项 + 守门 7 项全绿，端到端自检 **OK 61/61** |
| 2026-11-20 | **P0-b 完成**：`web/` → `src/`；`index.html` 移到 crate 根；建 package.json / vite.config.ts（outDir=dist, port=1450）/ tsconfig.json；Vue 空壳挂载 `#vue-app`（覆盖层留洞、整层穿透）；路径切最终值（frontendDist `../dist`、build.rs `../dist`、web_ime.rs `../src`）；两处 Rust 注释更新。踩坑两则见 7.10/7.11。验证：`pnpm build` 通过、`cargo test --workspace` 全绿、端到端自检 **OK 61/61**（17:11:39 本次实跑）、`pnpm dev` 在 1450 起服正常 |
| 2026-11-20 | P0 合并提交 `65ab110 refactor(desktop): 改用标准 Tauri 结构并接入 Vite 骨架`（P0-a/P0-b 中间态无法独立编译，故合一）|
| 2026-11-20 | 按用户要求改为不提交 `dist/`：加入 `.gitignore` 并从索引移除，本地产物保留；文档明确「cargo 直跑前先 pnpm build」|
| 2026-11-20 | **P0-c 完成**：`crates/mdview-desktop/` 整体上提到仓库根（前端工程根 = 仓库根）；改根 Cargo.toml members、.gitignore、跨 crate 的 state.rs fixtures 路径、4 个测试文件注释；`src-tauri/` 内部相对路径与 Vite/Tauri 配置因基准未变而免疫。验证：`pnpm build` + `cargo test --workspace` 全绿；`pnpm tauri dev -- -- tests/fixtures/selftest.md --self-test` 端到端自检 **OK 61/61**。注意 `cargo run` 链路同参数得到 60/61（仅「系统剪贴板可写」因窗口焦点为 false），已确认是链路差异非回归，判定以 tauri dev 为准 |

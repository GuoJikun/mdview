# mdview

一个 Rust 写内核的 Markdown 编辑器：文本、解析与排版在 Rust 侧，窗口与界面交给 Tauri 2 + Vue 3。

编辑始终等价于对源码的一次字节区间替换，视图只是源码的投影——不经过「富文本 → 再序列化」的 round-trip，因此不会丢表格对齐、定界符风格或转义符。

## 主要功能

- **三种视图**：所见即所得 / 阅读 / 源码。`**`、`# `、`` ` `` 等标记在光标离开时折叠，进入时展开；退格落在标记上按整对处理，一次撤销。
- **行内格式**：`Ctrl+B` 加粗、`Ctrl+I` 斜体、`Ctrl+Shift+X` 删除线、`Ctrl+E` 行内代码。没选中时插入「标记 + 占位文字」并整段选中，接着打字即替换；光标停在该格式的词里再按一次就取消，一次撤销回到原样。
- **插入**：「插入」菜单可加图片（`Ctrl+Shift+I`）、链接（`Ctrl+K`）、表格、分隔线、代码块、引用与任务列表。块级元素插入即成形，光标落在能接着打字的位置；图片会复制到文档旁的 `assets/` 再写相对路径，重名自动改名而不覆盖。
- **粘贴与复制**：从浏览器 / Word 粘贴富文本会转成 Markdown（标题、列表、表格、链接、粗斜体…）；复制时同时写 `text/html`，粘到 Word 保留格式，`text/plain` 仍是源码。
- **查找替换**：`Ctrl+F` / `Ctrl+H`，界面参照 VSCode——可折叠替换行、区分大小写 / 全词 / 正则、保留大小写、只在选区内查找；全部命中高亮，`Enter` / `Shift+Enter` 前后跳。
- **中文输入法**：透明输入框承接 IME，候选窗与预编辑串按像素跟随光标；组合期间不重解析、不重绘。
- **表格**：解析层抽出行列与对齐，渲染成真正的 `<table>` 网格；Tab 跨格，状态栏可增删行列。`Tab` / `Shift+Tab` 同时负责列表层级的缩进与反缩进。
- **代码高亮**：围栏代码块按语言着色，覆盖 47 种语言与格式（Rust / JS / TS / Python / Go / C / C++ / Java / Kotlin / C# / Swift / PHP / Ruby / Lua / PowerShell / SQL / HTML / CSS / Shell / YAML / TOML / Dockerfile / Makefile / Nginx / diff…），不引第三方语法库，编辑态与阅读态共用同一套停靠点。
- **编码**：UTF-8（含 BOM）与 GBK / GB18030 都能打开；内核内存态统一 UTF-8，保存时按原编码写回，不会悄悄改掉文件的编码。
- **文件侧栏**：`Ctrl+Shift+O` 打开一个文件夹（也可直接拖进窗口），左侧列出其中的 Markdown 与子目录，点一下即切换文档，未保存改动的确认与菜单打开完全一致；目录按需展开，隐藏项与 `node_modules` 之类工程目录自动跳过，上次的文件夹会记住。
- **大纲侧栏**：按标题的真实坐标判定当前节并高亮，点击跳转，不用滚动百分比估算。
- **文件安全**：原子写入（Windows `MoveFileEx` / POSIX `rename`）、监听外部修改（只提示不自动重载）、关窗与换文档前拦截未保存改动；未保存的改动每 5 秒备份到配置目录，崩溃或强制结束后重启会问是否恢复，也可在偏好里打开「自动保存」静默写回原文件。
- **导出**：「文件 → 导出 HTML…」把当前文档存成自带样式的单文件 HTML，图片就地内联，双击即开；要 PDF 就在浏览器里「打印为 PDF」，不内置打印接口。
- **多窗口**：`Ctrl+Shift+N` 新建窗口；双击关联文件或再次启动会新开窗口，而不是顶掉正在编辑的文档。每个窗口一份独立文档与外部修改监听。
- **打开方式**：菜单、`Ctrl+O`、把文件拖进窗口、文件菜单里的「最近打开」（最多 10 条）都走同一条打开路径；打开动作默认替换当前窗口的文档。
- **图片与链接**：相对路径限制在文档所在目录内解析并内联渲染；编辑态单击链接或图片可改地址/文字，Ctrl/Cmd+Click 在系统浏览器打开；raw HTML 一律降级为文本。
- **状态栏**：块数、字数、停靠点与修订号。
- **偏好设置**：`Ctrl+,` 可调正文字体、字号、正文宽度、Tab 缩进宽度与启动视图，改完即时生效、记在本机；样式走 CSS 变量，改字号后光标与大纲自动重新对位。
- **帮助**：`F1` / `Ctrl+/` 打开快捷键一览（条目从菜单表生成，不会与实际键位脱节）；空文档给出新建 / 打开 / 最近打开的入口。
- **外壳**：明暗主题跟随系统或手动切换；Windows / Linux 自绘标题栏与菜单，macOS 用原生菜单栏。
- **更新**：应用内检查、下载、安装更新（签名校验），启动检查默认关闭。

## 开发

### 环境

| 依赖 | 版本 |
| --- | --- |
| Rust | 1.90+（edition 2024） |
| Node | 26.13.0 |
| pnpm | 12.18.0 |

Node 与 pnpm 由 `mise.toml` 锁定，`mise install` 即可拉齐；也可自行安装同版本。

平台还需 Tauri 的系统依赖：Windows 的 WebView2（Win10/11 自带）、macOS 系统 WebView、Linux 的 `webkit2gtk`。

### 运行

```bash
mise install        # 首次：安装锁定的 node / pnpm
pnpm install        # 首次：安装前端依赖
pnpm tauri dev      # 开发模式：Vite + 原生窗口，前端热更新
```

打开指定文件：

```bash
pnpm tauri dev -- -- path/to/file.md
```

只想调界面（纯浏览器，没有 Rust 侧能力与文件读写）：

```bash
pnpm dev            # http://localhost:1450
```

### 常用命令

```bash
pnpm build                  # 前端类型检查（vue-tsc）+ 打包到 dist/
pnpm test                   # 前端全部用例（unit + dom 两个 project）
cargo test --workspace      # Rust 全量测试
pnpm tauri build            # 出安装包（需 TAURI_SIGNING_PRIVATE_KEY）

# 无头调试，<file> 换成任意 md
cargo run -p mdview-cli -- <file> layout   # 停靠点表：列 / byte / 字符
cargo run -p mdview-cli -- <file> spans    # 逐块投影，⟦⟧ 内为折叠标记
cargo run -p mdview-cli -- <file> preview  # 生成可交互的验证页
```

`dist/` 不入库，但 `generate_context!` 在编译期就要读它，所以**直接跑 `cargo test` / `cargo run` 前先 `pnpm build`**；`pnpm tauri dev` 有前置命令兜底，不受此限。

### 目录

```
src/          前端：Vue 外壳（components/）+ 编辑内核（editor/）
src-tauri/    桌面端：窗口、菜单、IPC 契约、端到端自检
crates/       Rust 库：core / markdown / app / support / cli
```

## 文档

- `PLAN.md` — 架构、硬约束与开发进度，接手前先读
- `CHANGELOG.md` — 发布说明，也是 Release 正文的来源

## 许可

MIT OR Apache-2.0，任选其一。全文见 [LICENSE-MIT](LICENSE-MIT) 与 [LICENSE-APACHE](LICENSE-APACHE)。

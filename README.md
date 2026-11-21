# mdview

一个 Rust 写内核的 Markdown 编辑器：文本、解析与排版在 Rust 侧，窗口与界面交给 Tauri 2 + Vue 3。

编辑始终等价于对源码的一次字节区间替换，视图只是源码的投影——不经过「富文本 → 再序列化」的 round-trip，因此不会丢表格对齐、定界符风格或转义符。

## 主要功能

- **三种视图**：所见即所得 / 阅读 / 源码。`**`、`# `、`` ` `` 等标记在光标离开时折叠，进入时展开；退格落在标记上按整对处理，一次撤销。
- **中文输入法**：透明输入框承接 IME，候选窗与预编辑串按像素跟随光标；组合期间不重解析、不重绘。
- **表格**：解析层抽出行列与对齐，渲染成真正的 `<table>` 网格；Tab 跨格，状态栏可增删行列。
- **大纲侧栏**：按标题的真实坐标判定当前节并高亮，点击跳转，不用滚动百分比估算。
- **文件安全**：原子写入（Windows `MoveFileEx` / POSIX `rename`）、监听外部修改（只提示不自动重载）、关窗与换文档前拦截未保存改动。
- **图片**：相对路径限制在文档所在目录内解析并内联渲染；raw HTML 一律降级为文本。
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

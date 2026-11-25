// Vue 入口。外壳组件全部挂在这里；编辑内核仍由 editor/editor.ts 驱动。
//
// 所有前端脚本都从这一处引入，index.html 只留一个入口。纯逻辑模块已是 ES Module，
// 由 editor.ts 显式 import，不再依赖挂 window 全局或副作用 import 的顺序。
import './platform';
import { invoke } from '@tauri-apps/api/core';
import { createApp, nextTick } from 'vue';
import App from './App.vue';
// 样式都在 src/styles/。app.css 与 skin.css 由 index.html 的 <link> 先加载
// （要赶在首帧前），这里只补 Vue 之外的两层：覆盖层与弹窗骨架。
import './styles/vue-overlay.css';
// 弹窗外壳（关于 / 检查更新）的共用骨架。
import './styles/dialog.css';

// bridge.ts 在模块求值时就把 window.__mdviewBridge 挂好，并主动拉一次快照。
import { notify } from './bridge';
// 「检查更新」的外壳状态：版本号、下载进度与启动时自动检查。
import { checkSilently, initUpdate, update } from './update';
// 自绘滚动条从原 titlebar.js 抽出；它惰性读取 Vue 渲染的标题栏/状态栏。
import './scrollbars';

// 编辑内核（`editor/editor.ts`）最后引入：boot() 在模块求值时启动，依赖由它自己 import。
import './editor/editor';

createApp(App).mount('#vue-app');

// 启动时自动检查：默认关闭，只有用户勾选过「帮助 → 启动时检查更新」才会联网。
// 静默失败；发现新版本只提示一句，下不下载由用户决定。
void initUpdate().then(async () => {
  if (!update.autoUpdate) return;
  const info = await checkSilently();
  if (info) notify(`发现新版本 ${info.version}：帮助 → 检查更新`);
});

// 平台判据以 Rust 为准（与窗口 decorations 一致）；platform.ts 的 UA 只负责首帧。
invoke<boolean>('use_custom_titlebar').then((custom) => {
  document.documentElement.classList.toggle('custom-titlebar', !!custom);
  document.documentElement.classList.toggle('native-titlebar', !custom);
  nextTick(() => window.__mdviewRefreshScrollbar?.());
}).catch(() => {});

// 首帧后量一次：标题栏/状态栏此刻才由 Vue 渲染完成。
nextTick(() => window.__mdviewRefreshScrollbar?.());

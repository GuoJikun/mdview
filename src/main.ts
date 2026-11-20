// Vue 入口。外壳组件全部挂在这里；编辑内核仍由 app.js 驱动。
//
// 所有前端脚本都从这一处引入，index.html 只留一个入口。七个纯逻辑模块保持
// .js 原样（它们把自己挂到 window），顺序与原 <script> 标签一致。
import './platform';
import { createApp, nextTick } from 'vue';
import App from './App.vue';
import './vue-overlay.css';

// bridge.ts 在模块求值时就把 window.__mdviewBridge 挂好，并主动拉一次快照。
import './bridge';
// 自绘滚动条从原 titlebar.js 抽出；它惰性读取 Vue 渲染的标题栏/状态栏。
import './scrollbars';

import './ime.js';
import './caretline.js';
import './dragscroll.js';
import './selection.js';
import './clipboard.js';
import './outline.js';
import './theme.js';
// 编辑内核（原 app.js）最后引入：boot() 在模块求值时启动，此时上面的全局都已就位。
import './app.js';

createApp(App).mount('#vue-app');

// 平台判据以 Rust 为准（与窗口 decorations 一致）；platform.ts 的 UA 只负责首帧。
const tauri = window.__TAURI__;
tauri?.core?.invoke?.('use_custom_titlebar').then((custom) => {
  document.documentElement.classList.toggle('custom-titlebar', !!custom);
  document.documentElement.classList.toggle('native-titlebar', !custom);
  nextTick(() => window.__mdviewRefreshScrollbar?.());
}).catch(() => {});

// 首帧后量一次：标题栏/状态栏此刻才由 Vue 渲染完成。
nextTick(() => window.__mdviewRefreshScrollbar?.());

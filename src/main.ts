// Vue 入口。外壳组件全部挂在这里；编辑内核仍由 app.js 驱动。
import { createApp, nextTick } from 'vue';
import App from './App.vue';
import './vue-overlay.css';

// bridge.ts 在模块求值时就把 window.__mdviewBridge 挂好，并主动拉一次快照。
import './bridge';
// 自绘滚动条从原 titlebar.js 抽出；它惰性读取 Vue 渲染的标题栏/状态栏。
import './scrollbars';

createApp(App).mount('#vue-app');

// 平台判据以 Rust 为准（与窗口 decorations 一致）；<head> 的 UA 判定只负责首帧。
const tauri = window.__TAURI__;
tauri?.core?.invoke?.('use_custom_titlebar').then((custom) => {
  document.documentElement.classList.toggle('custom-titlebar', !!custom);
  document.documentElement.classList.toggle('native-titlebar', !custom);
  nextTick(() => window.__mdviewRefreshScrollbar?.());
}).catch(() => {});

// 首帧后量一次：标题栏/状态栏此刻才由 Vue 渲染完成。
nextTick(() => window.__mdviewRefreshScrollbar?.());

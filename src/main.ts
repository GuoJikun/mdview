// Vue 入口。P0 验证 Vite 产物与现有脚本共存；P1 挂载只读外壳并接上 bridge。
// 外壳默认隐藏，P2 逐组件接管后再打开；调试时给 <html> 加 vue-shell-preview。
import { createApp } from 'vue';
import App from './App.vue';
import './vue-overlay.css';

// bridge.ts 在模块求值时就把 window.__mdviewBridge 挂好，并主动拉一次快照。
import './bridge';

createApp(App).mount('#vue-app');

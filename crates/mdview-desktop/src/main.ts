// Vue 入口。P0-b 阶段先挂一个空的覆盖层：只验证 Vite 产物与现有 UMD 脚本
// 能共存、自检不变。外壳组件在 P2 逐块迁入。
import { createApp } from 'vue';
import App from './App.vue';
import './vue-overlay.css';

createApp(App).mount('#vue-app');

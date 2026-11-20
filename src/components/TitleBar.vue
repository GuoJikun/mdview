<template>
  <!-- 只读外壳：P1 只从桥上读标题，不接管任何点击。
       真正的按钮、拖拽区、菜单弹层在 P2 迁入；模板届时复刻现有 id/class。 -->
  <header class="vue-titlebar" aria-hidden="true">
    <div class="tb-left">
      <span class="tb-logo">
        <img alt="" :src="LOGO" />
      </span>
      <nav class="tb-menus">
        <span v-for="name in MENUS" :key="name" class="tb-menu">{{ name }}</span>
      </nav>
    </div>
    <div class="tb-center">
      <span class="tb-title">{{ state.title }}</span>
    </div>
    <div class="tb-right">
      <span class="tb-ctl" title="最小化">
        <svg viewBox="0 0 12 12" width="12" height="12"><path d="M2 6h8" stroke="currentColor" stroke-width="1" /></svg>
      </span>
      <span class="tb-ctl" title="最大化 / 还原">
        <svg viewBox="0 0 12 12" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1">
          <rect x="2.5" y="2.5" width="7" height="7" />
        </svg>
      </span>
      <span class="tb-ctl tb-close" title="关闭">
        <svg viewBox="0 0 12 12" width="12" height="12"><path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1" /></svg>
      </span>
    </div>
  </header>
</template>

<script setup lang="ts">
import { state } from '../bridge';

const MENUS = ['文件', '编辑', '选择', '视图', '转到', '帮助'];

// 与 index.html 里的图标同一张图，避免预览时视觉不一致。
const LOGO =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAASUlEQVR42mNgGAVQIK+s8Z+eeNQBow4YdQDRDui8+I0iPPQdMJoGRh1w4uwlsvDwccBoGhh1ALUS3dB1wGgaGHXAqAPo5oARCwCme+39B8DaVAAAAABJRU5ErkJggg==';
</script>

<style scoped>
.vue-titlebar {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  z-index: 30;
  display: flex;
  align-items: center;
  height: 30px;
  background: var(--chrome);
  color: var(--text);
  border-bottom: 1px solid var(--hairline);
  user-select: none;
  font-family: var(--font-ui);
  pointer-events: none;
}
/* macOS 用系统标题栏时，Vue 也同步隐藏，别画出多余的假标题栏 */
:global(html.native-titlebar) .vue-titlebar {
  display: none;
}
.tb-left,
.tb-center,
.tb-right {
  display: flex;
  align-items: center;
  height: 100%;
}
.tb-left {
  min-width: 0;
}
.tb-logo {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  margin: 0 4px 0 10px;
  flex: none;
}
.tb-logo img {
  display: block;
  width: 16px;
  height: 16px;
}
.tb-menus {
  display: flex;
  align-items: center;
  height: 100%;
  gap: 2px;
}
.tb-menu {
  display: inline-flex;
  align-items: center;
  height: 100%;
  padding: 0 8px;
  font-size: 12px;
  border-radius: 4px;
}
.tb-center {
  flex: 1;
  justify-content: center;
  min-width: 0;
}
.tb-title {
  max-width: 50%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text);
  font-size: 14px;
  font-weight: 600;
}
.tb-right {
  height: 100%;
}
.tb-ctl {
  width: 46px;
  height: 100%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
</style>

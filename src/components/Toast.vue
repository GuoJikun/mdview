<template>
  <!-- 只读外壳：文字与显隐都来自桥；真正的 toast 仍由现有 #toast 承担。 -->
  <div class="vue-toast" :class="{ show: state.toast.visible }" aria-hidden="true">
    {{ state.toast.text }}
  </div>
</template>

<script setup lang="ts">
import { state } from '../bridge';
</script>

<style scoped>
.vue-toast {
  position: fixed;
  left: 50%;
  bottom: 64px;
  transform: translateX(-50%);
  padding: 6px 14px;
  border-radius: 6px;
  font-size: 13px;
  background: var(--toast-bg);
  color: var(--toast-fg);
  opacity: 0;
  transition: opacity 0.2s ease;
  pointer-events: none;
  z-index: 20;
}
.vue-toast.show {
  opacity: 1;
}
</style>

<template>
  <!-- Toast 已由 Vue 接管：文案与显隐都来自 bridge，宿主 #toast 也由这里渲染。
       role=status + aria-live 让看不见它的人也能收到同一条反馈（保存、导出、出错）。 -->
  <div
    id="toast"
    role="status"
    aria-live="polite"
    aria-atomic="true"
    :class="{ show: state.toast.visible }"
  >{{ state.toast.text }}</div>
</template>

<script setup lang="ts">
import { state } from '../bridge';
</script>

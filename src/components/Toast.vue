<template>
  <!-- Toast 已由 Vue 接管：文案与显隐都来自 bridge，宿主 #toast 也由这里渲染。 -->
  <div id="toast" :class="{ show: state.toast.visible }">{{ state.toast.text }}</div>
</template>

<script setup lang="ts">
import { state } from '../bridge';
</script>

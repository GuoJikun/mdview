<template>
  <!-- P2 进行中：StatusBar 已接管（Teleport 到 #bar），TitleBar / Toast 仍是只读空壳。 -->
  <div class="vue-app-root">
    <TitleBar />
    <Toast />
  </div>
  <StatusBar />
</template>

<script setup lang="ts">
import TitleBar from './components/TitleBar.vue';
import StatusBar from './components/StatusBar.vue';
import Toast from './components/Toast.vue';
</script>

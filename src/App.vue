<template>
  <!-- P2 进行中：StatusBar / Toast 已接管，TitleBar 仍是只读空壳。 -->
  <div class="vue-app-root">
    <TitleBar />
  </div>
  <StatusBar />
  <Toast />
</template>

<script setup lang="ts">
import TitleBar from './components/TitleBar.vue';
import StatusBar from './components/StatusBar.vue';
import Toast from './components/Toast.vue';
</script>

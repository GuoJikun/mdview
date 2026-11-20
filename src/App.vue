<template>
  <!-- 外壳全部由 Vue 接管；大纲与正文仍是覆盖层上的洞。 -->
  <TitleBar />
  <MenuPopup />
  <StatusBar />
  <Toast />
</template>

<script setup lang="ts">
import TitleBar from './components/TitleBar.vue';
import MenuPopup from './components/MenuPopup.vue';
import StatusBar from './components/StatusBar.vue';
import Toast from './components/Toast.vue';
</script>

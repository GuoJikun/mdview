<template>
  <!-- 外壳全部由 Vue 接管；大纲与正文仍是覆盖层上的洞。 -->
  <TitleBar />
  <MenuPopup />
  <StatusBar />
  <Toast />
  <UpdateDialog v-if="updateOpen" />
  <AboutDialog v-if="aboutOpen" />
</template>

<script setup lang="ts">
import { computed } from 'vue';
import TitleBar from './components/TitleBar.vue';
import MenuPopup from './components/MenuPopup.vue';
import StatusBar from './components/StatusBar.vue';
import Toast from './components/Toast.vue';
import UpdateDialog from './components/UpdateDialog.vue';
import AboutDialog from './components/AboutDialog.vue';
import { about } from './about';
import { update } from './update';

const updateOpen = computed(() => update.open);
const aboutOpen = computed(() => about.open);
</script>

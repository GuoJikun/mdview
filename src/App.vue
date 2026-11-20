<template>
  <!-- P1：三个外壳组件都已挂载并只读订阅 bridge，但整层默认不显示。
       P2 逐组件接管时，只把对应组件移出隐藏状态并补上 pointer-events/动作。 -->
  <div class="vue-app-root">
    <TitleBar />
    <StatusBar />
    <Toast />
  </div>
</template>

<script setup lang="ts">
import TitleBar from './components/TitleBar.vue';
import StatusBar from './components/StatusBar.vue';
import Toast from './components/Toast.vue';
</script>

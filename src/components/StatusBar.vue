<template>
  <!-- 状态栏内容由 Vue 渲染，但宿主仍是 index.html 里的 #bar：
       这样 titlebar.js 的 edges() 与自检里的 barEl 引用都不用改。
       点击委托仍在 app.js 的 #bar 监听器上，动作逻辑零重写。 -->
  <Teleport to="#bar">
    <span>块 <b>{{ blockText }}</b></span>
    <span>停靠点 <b>{{ dash(state.stop) }}</b></span>
    <span>列 <b>{{ dash(state.column) }}</b></span>
    <span>r<b>{{ state.revision }}</b></span>
    <span v-if="state.inTable" class="table-tools">
      <button type="button" data-table="row+" title="在下方插入行">行+</button>
      <button type="button" data-table="row-" title="删除当前行">行−</button>
      <button type="button" data-table="col+" title="在右侧插入列">列+</button>
      <button type="button" data-table="col-" title="删除当前列">列−</button>
    </span>
    <span class="hint">← → 移动 · Shift 扩选 · Enter 分段 · Tab 跨格 · Ctrl+S 保存</span>
  </Teleport>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { state } from '../bridge';

const dash = (v: number | null) => (v === null || v === undefined ? '-' : String(v));
const blockText = computed(() =>
  (state.stop === null ? '-' : state.stop + 1) + '/' + state.blocks,
);
</script>

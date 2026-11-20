<template>
  <!-- 只读外壳：P1 只展示桥上推上来的统计，按钮与点击仍归现有 #bar。
       P2 接管时模板复刻现有 id/class，自检断言零改动。 -->
  <footer class="vue-status" aria-hidden="true">
    <span>块 <b>{{ state.stop === null ? '-' : state.stop + 1 }}/{{ state.blocks }}</b></span>
    <span>停靠点 <b>{{ dash(state.stop) }}</b></span>
    <span>列 <b>{{ dash(state.column) }}</b></span>
    <span>r<b>{{ state.revision }}</b></span>
    <span v-if="state.inTable" class="table-tools">
      <span data-table="row+" title="在下方插入行">行+</span>
      <span data-table="row-" title="删除当前行">行−</span>
      <span data-table="col+" title="在右侧插入列">列+</span>
      <span data-table="col-" title="删除当前列">列−</span>
    </span>
    <span class="hint">← → 移动 · Shift 扩选 · Enter 分段 · Tab 跨格 · Ctrl+S 保存</span>
  </footer>
</template>

<script setup lang="ts">
import { state } from '../bridge';

const dash = (v: number | null) => (v === null || v === undefined ? '-' : String(v));
</script>

<style scoped>
.vue-status {
  position: fixed;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 10;
  display: flex;
  flex-wrap: wrap;
  gap: 18px;
  align-items: center;
  padding: 8px 18px;
  font: 12px/1 ui-monospace, Consolas, monospace;
  color: var(--muted);
  background: var(--bar-bg);
  border-top: 1px solid var(--border);
  backdrop-filter: blur(6px);
  pointer-events: none;
}
.vue-status b {
  color: var(--fg);
  font-weight: 600;
}
.hint {
  margin-left: auto;
  color: var(--faint);
}
.table-tools {
  display: inline-flex;
  gap: 4px;
}
.table-tools span {
  padding: 2px 6px;
  border-radius: 4px;
}
</style>

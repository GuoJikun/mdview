<template>
  <!-- 状态栏内容由 Vue 渲染，但宿主仍是 index.html 里的 #bar：
       这样 scrollbars.ts 的 edges() 与自检里的 barEl 引用都不用改。
       点击委托仍在 editor.ts 的 #bar 监听器上，动作逻辑零重写。 -->
  <Teleport to="#bar">
    <span>块 <b>{{ blockText }}</b></span>
    <span>字数 <b>{{ state.words ?? 0 }}</b></span>
    <span>停靠点 <b>{{ dash(state.stop) }}</b></span>
    <span>列 <b>{{ dash(state.column) }}</b></span>
    <span>r<b>{{ state.revision }}</b></span>
    <!-- 非 UTF-8 文件给一格提示：保存会按原编码写回，用户不用猜为什么别的
         编辑器里看着还是旧编码。UTF-8 是常态，不占位置。 -->
    <span v-if="encodingLabel" class="enc" :title="'文件编码：' + encodingLabel + '（保存时按它写回）'">
      {{ encodingLabel }}
    </span>
    <span v-if="state.inTable" class="table-tools">
      <button type="button" data-table="row+" title="在下方插入行">行+</button>
      <button type="button" data-table="row-" title="删除当前行">行−</button>
      <button type="button" data-table="col+" title="在右侧插入列">列+</button>
      <button type="button" data-table="col-" title="删除当前列">列−</button>
      <!-- 排序只动数据行：表头与分隔行原地不动，一次事务、一步撤销 -->
      <button type="button" data-table="sort-asc" title="按本列升序排列" aria-label="按本列升序排列">↑</button>
      <button type="button" data-table="sort-desc" title="按本列降序排列" aria-label="按本列降序排列">↓</button>
    </span>
    <span class="hint">← → 移动 · Shift 扩选 · Enter 分段 · Tab 跨格 · Ctrl+S 保存</span>
  </Teleport>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { state } from '../bridge';

const dash = (v: number | null) => (v === null || v === undefined ? '-' : String(v));
// 块号从内核的 caret.block 取：`stop` 是停靠点序号（整篇的字符位置），
// 拿它当块号会显示「块 124/9」这种不可能的数字。
const blockText = computed(() =>
  state.caretBlock === null ? '-' : state.caretBlock + 1 + '/' + state.blocks,
);
// 只有「不是 UTF-8」才需要说明；UTF-8 带不带 BOM 都值得标一下。
const encodingLabel = computed(() => {
  const value = state.encoding || 'utf-8';
  if (value === 'utf-8') return '';
  return value === 'utf-8-bom' ? 'UTF-8 BOM' : 'GB18030';
});
</script>

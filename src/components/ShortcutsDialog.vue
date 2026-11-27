<template>
  <!-- 快捷键一览：分组与条目全部由 src/shortcuts.ts 从菜单生成，
       这里只管排版。与其它弹窗共用 dialog.css 的骨架。 -->
  <div class="dialog-backdrop">
    <section
      ref="panel"
      class="dialog shortcuts-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="shortcuts-title"
      tabindex="-1"
      @keydown.esc.prevent.stop="closeShortcuts"
    >
      <header class="dialog-header">
        <h2 id="shortcuts-title">键盘快捷键</h2>
      </header>

      <div class="dialog-body shortcuts-body">
        <div v-for="group in groups" :key="group.title" class="shortcut-group">
          <h3>{{ group.title }}</h3>
          <dl>
            <template v-for="item in group.items" :key="item.keys + item.label">
              <dt><kbd>{{ item.keys }}</kbd></dt>
              <dd>{{ item.label }}</dd>
            </template>
          </dl>
        </div>
      </div>

      <footer class="dialog-actions">
        <span class="hint">macOS 上 Ctrl 换成 ⌘</span>
        <button type="button" class="btn primary" @click="closeShortcuts">知道了</button>
      </footer>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { closeShortcuts } from '../help';
import { shortcutGroups } from '../shortcuts';

const panel = ref<HTMLElement | null>(null);
const groups = shortcutGroups();

onMounted(() => panel.value?.focus());
// 与「关于」「偏好设置」一致：Esc 在面板里已经处理，这里兜住「焦点不在弹窗上」
// 的情况（焦点跑到正文时按 Esc 也该关）。
const onKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') closeShortcuts();
};
document.addEventListener('keydown', onKeydown);
onBeforeUnmount(() => document.removeEventListener('keydown', onKeydown));
</script>

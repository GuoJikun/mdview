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
      aria-describedby="shortcuts-hint"
      tabindex="-1"
    >
      <header class="dialog-header">
        <h2 id="shortcuts-title">{{ zh.shortcuts.title }}</h2>
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
        <span class="hint" id="shortcuts-hint">{{ zh.shortcuts.hint }}</span>
        <button type="button" class="btn primary" @click="closeShortcuts">{{ zh.common.gotIt }}</button>
      </footer>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { useModalDialog } from '../dialog-modal';
import { closeShortcuts } from '../help';
import { zh } from '../i18n/zh-CN';
import { shortcutGroups } from '../shortcuts';

const panel = ref<HTMLElement | null>(null);
const groups = shortcutGroups();

// 焦点落在面板本身（tabindex=-1），Tab 之后进第一个 kbd 前的按钮；
// 焦点陷阱与 Esc 关闭都由 dialog-modal 统一处理，关闭后焦点还给打开它的地方。
useModalDialog(panel, { closeOnEscape: true, onEscape: closeShortcuts });
</script>

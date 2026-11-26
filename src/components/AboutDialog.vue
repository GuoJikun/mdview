<template>
  <!-- 关于弹窗：与「检查更新」共用 src/styles/dialog.css 的骨架，只有内容不同。
       关闭同样只走底部按钮，遮罩与 Esc 都不关。 -->
  <div class="dialog-backdrop">
    <section
      class="dialog about-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="about-title"
      tabindex="-1"
    >
      <header class="dialog-header">
        <h2 id="about-title">关于 mdview</h2>
      </header>

      <div class="dialog-body">
        <div class="about-hero">
          <span class="about-mark" aria-hidden="true">M↓</span>
          <div class="about-id">
            <strong>mdview</strong>
            <span>版本 {{ about.version || '—' }}</span>
          </div>
        </div>

        <p class="about-desc">
          一个用 Rust 写内核的 Markdown 编辑器：文本、解析与排版都在 Rust 侧完成，
          窗口与界面交给 Tauri + Vue。
        </p>

        <dl class="about-meta">
          <div><dt>内核</dt><dd>Rust — ropey · pulldown-cmark</dd></div>
          <div><dt>外壳</dt><dd>Tauri 2 · Vue 3</dd></div>
          <div><dt>许可</dt><dd>MIT OR Apache-2.0</dd></div>
          <div><dt>项目</dt><dd class="about-link">github.com/GuoJikun/mdview</dd></div>
        </dl>
      </div>

      <footer class="dialog-actions">
        <button ref="closeButton" type="button" class="btn primary" @click="closeAbout">
          关闭
        </button>
      </footer>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { about, closeAbout } from '../about';
import { useModalDialog } from '../dialog-modal';

const closeButton = ref<HTMLButtonElement | null>(null);

useModalDialog(closeButton);
</script>

<style scoped>
.about-dialog {
  width: 420px;
}

.about-hero {
  display: flex;
  align-items: center;
  gap: 12px;
}

/* Markdown 的「M↓」记号，用强调色的软底方块托住，不用位图图标。 */
.about-mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 44px;
  flex: none;
  border-radius: 12px;
  background: var(--accent-soft);
  color: var(--accent);
  font-family: var(--font-ui);
  font-size: 17px;
  font-weight: 700;
  letter-spacing: -0.5px;
}

.about-id {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.about-id strong {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
}

.about-id span {
  font-size: 12px;
  color: var(--text3);
  font-variant-numeric: tabular-nums;
}

.about-desc {
  margin: 12px 0 0;
  font-size: 12px;
  line-height: 1.65;
  color: var(--text2);
  user-select: text;
}

.about-meta {
  display: grid;
  gap: 6px;
  margin: 14px 0 0;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--code-bg);
  font-size: 12px;
  user-select: text;
}

.about-meta > div {
  display: flex;
  gap: 10px;
}

.about-meta dt {
  flex: none;
  width: 32px;
  color: var(--text3);
}

.about-meta dd {
  margin: 0;
  color: var(--text2);
  overflow-wrap: anywhere;
}

/* 项目地址是给人抄的，不伪装成可点链接（没有 opener 插件，点了也打不开）。 */
.about-link {
  color: var(--text3);
}
</style>

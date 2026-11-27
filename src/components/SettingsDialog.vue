<template>
  <!-- 偏好设置：与「关于」「检查更新」共用 dialog.css 的骨架。
       改动即时生效（写 CSS 变量），关掉弹窗不需要「保存」——这是编辑器，
       不是设置向导。 -->
  <div class="dialog-backdrop">
    <section
      ref="panel"
      class="dialog settings-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
      tabindex="-1"
    >
      <header class="dialog-header">
        <h2 id="settings-title">偏好设置</h2>
      </header>

      <div class="dialog-body">
        <div class="row">
          <label for="pref-font">正文字体</label>
          <select
            id="pref-font"
            :value="prefs.font"
            @change="update({ font: valueOf($event) as FontChoice })"
          >
            <option v-for="(label, key) in FONT_LABELS" :key="key" :value="key">{{ label }}</option>
          </select>
        </div>

        <div class="row">
          <label for="pref-size">字号</label>
          <input
            id="pref-size"
            v-bind="LIMITS.fontSize"
            type="range"
            :value="prefs.fontSize"
            @input="update({ fontSize: Number(valueOf($event)) })"
          >
          <span class="value">{{ prefs.fontSize }} px</span>
        </div>

        <div class="row">
          <label for="pref-width">正文宽度</label>
          <input
            id="pref-width"
            v-bind="LIMITS.width"
            type="range"
            :value="prefs.width"
            @input="update({ width: Number(valueOf($event)) })"
          >
          <span class="value">{{ prefs.width }} px</span>
        </div>

        <div class="row">
          <label for="pref-indent">Tab 缩进</label>
          <input
            id="pref-indent"
            v-bind="LIMITS.indent"
            type="range"
            :value="prefs.indent"
            @input="update({ indent: Number(valueOf($event)) })"
          >
          <span class="value">{{ prefs.indent }} 空格</span>
        </div>

        <div class="row">
          <label for="pref-view">启动视图</label>
          <select
            id="pref-view"
            :value="prefs.startupView"
            @change="update({ startupView: valueOf($event) as StartupView })"
          >
            <option v-for="(label, key) in VIEW_LABELS" :key="key" :value="key">{{ label }}</option>
          </select>
        </div>

        <div class="row check">
          <label for="pref-autosave">自动保存</label>
          <input
            id="pref-autosave"
            type="checkbox"
            :checked="prefs.autoSave"
            @change="update({ autoSave: ($event.target as HTMLInputElement).checked })"
          >
          <span class="value wide">有改动时静默写回原文件</span>
        </div>

        <p class="hint">
          偏好存在本机，重启后保留。列表的缩进仍按标记宽度（<code>- </code> 两格、<code>1. </code> 三格）——
          改这里不会把子项推进标记中间。
        </p>
        <p class="hint">
          无论是否开启自动保存，未保存的改动都会每 5 秒备份到配置目录：
          崩溃或强制结束后重启，会问你要不要恢复。
        </p>
      </div>

      <footer class="dialog-actions">
        <button type="button" class="btn" @click="restoreDefaults">恢复默认</button>
        <button type="button" class="btn primary" @click="closeSettings">完成</button>
      </footer>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { useModalDialog } from '../dialog-modal';
import { closeSettings } from '../settings';
import {
  FONT_LABELS,
  LIMITS,
  VIEW_LABELS,
  prefs,
  reset,
  set,
  type FontChoice,
  type Prefs,
  type StartupView,
} from '../prefs';

const panel = ref<HTMLElement | null>(null);

function valueOf(event: Event): string {
  return (event.target as HTMLInputElement | HTMLSelectElement).value;
}

/**
 * 改一项：写进 prefs（顺带记忆）→ 通知编辑器重测。
 *
 * 字号与行宽会改变折行，自绘光标是按像素量出来的，不重测就会停在旧位置；
 * 大纲锚点同理。
 */
function update(patch: Partial<Prefs>) {
  set(patch);
  window.__mdviewRelayout?.();
}

function restoreDefaults() {
  reset();
  window.__mdviewRelayout?.();
}

// 弹窗是模态的：锁页面滚动、Tab 关在弹窗里、Esc 关闭、关掉后焦点还给打开它的地方。
useModalDialog(panel, { closeOnEscape: true, onEscape: closeSettings });
</script>

<style scoped>
.settings-dialog {
  width: 440px;
}

.row {
  display: grid;
  grid-template-columns: 76px 1fr 62px;
  align-items: center;
  gap: 10px;
  padding: 7px 0;
}

.row label {
  color: var(--text2);
  font-size: 12px;
}

.row select {
  grid-column: 2 / 4;
  height: 26px;
  padding: 0 6px;
  font: inherit;
  font-size: 12px;
  color: var(--text);
  background: var(--input, var(--control));
  border: 1px solid var(--border);
  border-radius: 5px;
}

.row input[type='range'] {
  accent-color: var(--accent);
  width: 100%;
}

.value {
  color: var(--text3);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  text-align: right;
}

/* 复选框一行：勾选框紧贴标签，说明文字占右侧剩余宽度 */
.row.check {
  grid-template-columns: 76px auto 1fr;
}

.row.check input[type='checkbox'] {
  justify-self: start;
  accent-color: var(--accent);
}

.row.check .value.wide {
  grid-column: 3;
  text-align: left;
}

.hint {
  margin: 12px 0 0;
  padding-top: 10px;
  border-top: 1px solid var(--border);
  color: var(--text3);
  font-size: 11.5px;
  line-height: 1.6;
}

.hint code {
  padding: 1px 3px;
  border-radius: 3px;
  background: var(--inline-code-bg);
  font-size: 11px;
}
</style>

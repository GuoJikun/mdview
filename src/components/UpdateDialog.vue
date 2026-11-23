<template>
  <!-- 检查更新弹窗：状态机与 image-view 一致
       idle → checking → [up-to-date | available | error]
       available → downloading → [ready | error | available(取消)]
       ready → install（安装后应用退出） -->
  <!-- 关闭只走底部按钮：遮罩与 Esc 都不关闭，避免误触丢掉正在进行的下载。
       骨架样式在 src/dialog.css，与「关于」弹窗共用。 -->
  <div class="dialog-backdrop">
    <section
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="update-title"
      tabindex="-1"
    >
      <header class="dialog-header">
        <h2 id="update-title">检查更新</h2>
      </header>

      <div class="dialog-body">
        <p class="update-version">
          <span>当前版本</span>
          <strong>{{ update.version || '—' }}</strong>
        </p>

        <!-- 检查中 -->
        <div v-if="update.phase === 'checking'" class="update-status">
          <span class="spinner" aria-hidden="true"></span>
          <span>正在检查更新…</span>
        </div>

        <!-- 已是最新 -->
        <div v-else-if="update.phase === 'up-to-date'" class="update-status">
          <span class="check-mark" aria-hidden="true"><Check :size="14" :stroke-width="3" /></span>
          <span>当前已是最新版本。</span>
        </div>

        <!-- 发现新版本 -->
        <template v-else-if="update.phase === 'available' && update.info">
          <div class="update-status">
            <strong class="update-new">发现新版本 — {{ update.info.version }}</strong>
          </div>
          <div v-if="notes" class="update-notes-block">
            <h3>更新内容</h3>
            <!-- Release 正文是 Markdown；这里按纯文本排版，不注入 HTML。 -->
            <div class="update-notes" @wheel.stop>{{ notes }}</div>
          </div>
        </template>

        <!-- 下载中 -->
        <template v-else-if="update.phase === 'downloading'">
          <div class="update-status">
            <span class="spinner" aria-hidden="true"></span>
            <span>正在下载…</span>
            <span v-if="update.total" class="percent">{{ percent }}%</span>
          </div>
          <div class="progress-track">
            <div class="progress-fill" :style="{ width: percent + '%' }"></div>
          </div>
          <p class="progress-detail">
            {{ formatBytes(update.received) }}
            <template v-if="update.total"> / {{ formatBytes(update.total) }}</template>
          </p>
        </template>

        <!-- 下载完成 -->
        <div v-else-if="update.phase === 'ready'" class="update-status">
          <span class="check-mark" aria-hidden="true"><Check :size="14" :stroke-width="3" /></span>
          <div>
            <strong>更新已就绪</strong>
            <p class="ready-hint">点击「安装并重启」完成更新。</p>
          </div>
        </div>

        <!-- 失败 -->
        <div v-else-if="update.phase === 'error'" class="update-status update-error">
          <span class="error-mark" aria-hidden="true"><TriangleAlert :size="14" :stroke-width="2.4" /></span>
          <div>
            <strong>检查更新失败</strong>
            <p>{{ update.error }}</p>
          </div>
        </div>
      </div>

      <footer class="dialog-actions">
        <!-- 失败：重试 + 关闭 -->
        <template v-if="update.phase === 'error'">
          <button type="button" class="btn" @click="check">重试</button>
          <button ref="closeButton" type="button" class="btn primary" @click="requestClose">关闭</button>
        </template>

        <!-- 下载中：只能取消 -->
        <template v-else-if="update.phase === 'downloading'">
          <button type="button" class="btn" @click="cancelDownload">取消</button>
        </template>

        <!-- 就绪：关闭 / 安装 -->
        <template v-else-if="update.phase === 'ready'">
          <button type="button" class="btn" @click="requestClose">关闭</button>
          <button ref="closeButton" type="button" class="btn primary" @click="install">
            安装并重启
          </button>
        </template>

        <!-- 有新版本：关闭 / 下载 -->
        <template v-else-if="update.phase === 'available'">
          <button type="button" class="btn" @click="requestClose">关闭</button>
          <button ref="closeButton" type="button" class="btn primary" @click="download">
            下载更新
          </button>
        </template>

        <!-- idle / checking / up-to-date -->
        <template v-else>
          <button ref="closeButton" type="button" class="btn primary" @click="requestClose">关闭</button>
        </template>
      </footer>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { Check, TriangleAlert } from 'lucide-vue-next';
import {
  cancelDownload,
  check,
  closeUpdateDialog,
  download,
  install,
  update,
} from '../update';
import { formatBytes, progressPercent } from '../update-format';
import { useModalDialog } from '../dialog-modal';

const closeButton = ref<HTMLButtonElement | null>(null);

const notes = computed(() => (update.info?.body ?? '').trim());
const percent = computed(() => progressPercent(update.received, update.total));

/** 下载中不允许直接关窗：要么等它完成，要么显式点「取消」。 */
function requestClose(): void {
  if (update.phase === 'downloading') return;
  closeUpdateDialog();
}

// 锁滚动、吞掉 Esc、焦点落到关闭按钮——与「关于」弹窗同一份实现。
useModalDialog(closeButton);
</script>

<style scoped>
/* 骨架（遮罩 / 卡片 / 标题 / 正文 / 按钮排）在 src/dialog.css，这里只留更新专用样式。 */
.update-version {
  display: flex;
  gap: 8px;
  margin: 0 0 10px;
  font-size: 12px;
  color: var(--text3);
}

.update-version strong {
  color: var(--text);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.update-status {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 36px;
  line-height: 1.6;
}

.update-new {
  color: var(--accent);
  font-weight: 600;
}

.check-mark,
.error-mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  flex: none;
  border-radius: 50%;
  font-size: 13px;
  font-weight: 700;
}

.check-mark {
  background: var(--accent-soft);
  color: var(--accent);
}

.error-mark {
  background: color-mix(in srgb, var(--danger) 16%, transparent);
  color: var(--danger);
}

.update-error strong {
  color: var(--danger);
}

.update-error p {
  margin: 4px 0 0;
  color: var(--danger);
  font-size: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.ready-hint {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text3);
}

.spinner {
  width: 16px;
  height: 16px;
  flex: none;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: update-spin 0.7s linear infinite;
}

@keyframes update-spin {
  to {
    transform: rotate(360deg);
  }
}

.percent {
  margin-left: auto;
  font-size: 12px;
  color: var(--text);
  font-variant-numeric: tabular-nums;
}

.progress-track {
  height: 6px;
  margin-top: 10px;
  border-radius: 3px;
  background: var(--border);
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  border-radius: 3px;
  background: var(--accent);
  transition: width 200ms ease;
}

.progress-detail {
  margin: 6px 0 0;
  font-size: 12px;
  color: var(--text3);
  font-variant-numeric: tabular-nums;
}

.update-notes-block {
  margin-top: 10px;
}

.update-notes-block h3 {
  margin: 0 0 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
}

/* 说明较长时在弹窗内部滚动，不把对话框撑高。 */
.update-notes {
  max-height: min(240px, 40vh);
  overflow-y: auto;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--code-bg);
  color: var(--text2);
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

</style>

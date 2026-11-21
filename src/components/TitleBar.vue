<template>
  <!-- 自绘标题栏（Windows / Linux）。macOS 下由 CSS 隐藏，系统标题栏接管。 -->
  <header id="titlebar" data-tauri-drag-region="deep" @dblclick="onDblclick">
    <div class="tb-left" data-tauri-drag-region>
      <span class="tb-logo" data-tauri-drag-region aria-hidden="true">
        <img alt="" :src="LOGO" />
      </span>
      <nav id="tb-menus" class="tb-menus" aria-label="应用菜单">
        <button
          v-for="key in MENU_ORDER"
          :key="key"
          type="button"
          class="tb-menu"
          :class="{ open: menuState.open === key }"
          :data-menu="key"
          @click="onMenuClick(key, $event)"
          @mouseover="onMenuHover(key, $event)"
        >{{ MENU_LABELS[key] }}</button>
      </nav>
    </div>
    <div class="tb-center" data-tauri-drag-region>
      <span id="tb-title" class="tb-title">{{ state.title }}</span>
    </div>
    <div class="tb-right" data-tauri-drag-region>
      <button type="button" class="tb-ctl" id="tb-min" title="最小化" aria-label="最小化" @click="minimize">
        <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2 6h8" stroke="currentColor" stroke-width="1" /></svg>
      </button>
      <button type="button" class="tb-ctl" id="tb-max" title="最大化 / 还原" aria-label="最大化" @click="toggleMaximize">
        <svg viewBox="0 0 12 12" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1" aria-hidden="true">
          <rect class="tb-max-rect" x="2.5" y="2.5" width="7" height="7" :style="{ display: maximized ? 'none' : '' }" />
          <g class="tb-restore-icon" :style="{ display: maximized ? '' : 'none' }">
            <rect x="1.5" y="3.5" width="6" height="6" />
            <path d="M4 3.5V2h6v6H8.5" />
          </g>
        </svg>
      </button>
      <button type="button" class="tb-ctl tb-close" id="tb-close" title="关闭" aria-label="关闭" @click="closeWindow">
        <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1" /></svg>
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { onMounted, ref } from 'vue';
import { state } from '../bridge';
import { MENU_LABELS, MENU_ORDER, menuState, showMenu, toggleMenu, type MenuKey } from '../menu';

// 与 index.html 里的图标同一张图，避免视觉不一致。
const LOGO =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAASUlEQVR42mNgGAVQIK+s8Z+eeNQBow4YdQDRDui8+I0iPPQdMJoGRh1w4uwlsvDwccBoGhh1ALUS3dB1wGgaGHXAqAPo5oARCwCme+39B8DaVAAAAABJRU5ErkJggg==';

// Tauri 之外（浏览器直接打开 dev server）按原来的可选行为降级。
const appWindow = (() => {
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
})();
const maximized = ref(false);

function onMenuClick(key: MenuKey, event: MouseEvent): void {
  toggleMenu(key, event.currentTarget as HTMLElement);
}

function onMenuHover(key: MenuKey, event: MouseEvent): void {
  if (menuState.open && menuState.open !== key) {
    showMenu(key, event.currentTarget as HTMLElement);
  }
}

function onDblclick(event: MouseEvent): void {
  const target = event.target as HTMLElement;
  if (target === event.currentTarget || target.hasAttribute('data-tauri-drag-region')) {
    toggleMaximize();
  }
}

function minimize(): void {
  appWindow?.minimize();
}

function toggleMaximize(): void {
  appWindow?.toggleMaximize();
}

function closeWindow(): void {
  appWindow?.close();
}

async function syncMaximizeIcon(): Promise<void> {
  if (!appWindow) return;
  try {
    maximized.value = await appWindow.isMaximized();
  } catch {
    maximized.value = false;
  }
}

onMounted(() => {
  syncMaximizeIcon();
  listen('tauri://resize', syncMaximizeIcon).catch(() => {});
});
</script>

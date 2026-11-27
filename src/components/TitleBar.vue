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
          aria-haspopup="menu"
          :aria-expanded="menuState.open === key"
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
        <Minus :size="14" :stroke-width="1.7" aria-hidden="true" />
      </button>
      <button type="button" class="tb-ctl" id="tb-max" title="最大化 / 还原" aria-label="最大化" @click="toggleMaximize">
        <Square v-if="!maximized" :size="14" :stroke-width="1.7" aria-hidden="true" />
        <Copy v-else :size="14" :stroke-width="1.7" aria-hidden="true" />
      </button>
      <button type="button" class="tb-ctl tb-close" id="tb-close" title="关闭" aria-label="关闭" @click="closeWindow">
        <X :size="14" :stroke-width="1.7" aria-hidden="true" />
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Copy, Minus, Square, X } from 'lucide-vue-next';
import { onMounted, ref } from 'vue';
import { state } from '../bridge';
import { MENU_LABELS, MENU_ORDER, menuState, showMenu, toggleMenu, type MenuKey } from '../menu';

// 标题栏标识：由 src-tauri/icons/icon.png（512²）降采样到 64² 得到，
// 16px 显示在 4x 屏上也不会糊。换应用图标后重新导出这一张即可。
import LOGO from '../assets/logo.png';

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

<template>
  <!-- 菜单弹层：DOM 契约与原 titlebar.js 一致（#tb-menu-popup / .tb-sep / .tb-submenu），
       动作统一经 bridge.dispatch 回到 editor.ts 的 runAction。 -->
  <div
    id="tb-menu-popup"
    ref="popupEl"
    class="tb-menu-popup"
    role="menu"
    :hidden="menuState.open === null"
    :style="{ left: left + 'px', top: top + 'px' }"
  >
    <template v-for="(entry, i) in entries" :key="i">
      <div v-if="entry.sep" class="tb-sep" role="separator"></div>

      <div v-else-if="entry.sub" class="submenu-host">
        <button
          type="button"
          class="submenu-trigger"
          :class="{ open: openSubIndex === i }"
          role="menuitem"
          aria-haspopup="true"
          @pointerenter="openSubIndex = i"
          @click.stop="toggleSub(i)"
        >
          <span class="gutter"></span>
          <span class="label">{{ entry.sub.label }}</span>
          <span class="chev">›</span>
        </button>
        <div v-if="openSubIndex === i" class="tb-submenu" role="menu">
          <button
            v-for="(item, j) in entry.sub.items"
            :key="j"
            type="button"
            role="menuitemradio"
            :data-action="item.action"
            :aria-checked="isChecked(item)"
            :class="{ danger: item.danger }"
            @pointerenter.stop
            @click="run(item.action)"
          >
            <span class="gutter">{{ isChecked(item) ? (item.mark === 'dot' ? '•' : '✓') : '' }}</span>
            <span class="label">{{ item.label }}</span>
            <span v-if="item.acc" class="acc">{{ item.acc }}</span>
          </button>
        </div>
      </div>

      <button
        v-else
        type="button"
        :role="entry.mark === 'dot' ? 'menuitemradio' : 'menuitem'"
        :data-action="entry.action"
        :aria-checked="entry.mark === 'dot' ? isChecked(entry) : undefined"
        :class="{ danger: entry.danger }"
        @pointerenter="openSubIndex = null"
        @click="run(entry.action)"
      >
        <span class="gutter">{{ isChecked(entry) ? (entry.mark === 'dot' ? '•' : '✓') : '' }}</span>
        <span class="label">{{ entry.label }}</span>
        <span v-if="entry.acc" class="acc">{{ entry.acc }}</span>
      </button>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { dispatch, state } from '../bridge';
import { MENUS, anchorOf, closeMenu, menuState, switchMenu, type MenuEntry } from '../menu';
import { update } from '../update';

const popupEl = ref<HTMLElement | null>(null);
const openSubIndex = ref<number | null>(null);
const left = ref(4);
const top = ref(30);

const entries = computed<MenuEntry[]>(() =>
  (menuState.open ? MENUS[menuState.open] : []),
);

function isChecked(entry: MenuEntry): boolean {
  if (!entry.checked) return false;
  // 勾选态来自两处：编辑内核推上来的主题/模式，以及纯外壳的自动检查开关。
  return !!entry.checked({
    themeMode: state.themeMode,
    mode: state.mode,
    autoUpdate: update.autoUpdate,
  });
}

function toggleSub(index: number): void {
  openSubIndex.value = openSubIndex.value === index ? null : index;
}

function run(action?: string): void {
  if (!action) return;
  closeMenu();
  dispatch(action);
}

function popupButtons(): HTMLButtonElement[] {
  return Array.from(popupEl.value?.querySelectorAll<HTMLButtonElement>('button[data-action]') ?? []);
}

function highlight(index: number): void {
  const buttons = popupButtons();
  if (!buttons.length) return;
  menuState.activeIndex = (index + buttons.length) % buttons.length;
  buttons.forEach((btn, i) => btn.classList.toggle('focused', i === menuState.activeIndex));
}

function onDocMousedown(event: MouseEvent): void {
  if (!menuState.open) return;
  const target = event.target as HTMLElement | null;
  if (!target) return;
  if (popupEl.value?.contains(target)) return;
  if (target.closest('.tb-menu')) return;
  closeMenu();
}

function onKeydown(event: KeyboardEvent): void {
  if (!menuState.open) return;
  const NAV = ['Escape', 'ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight', 'Enter'];
  if (NAV.indexOf(event.key) < 0) return;
  // 菜单开着时这些键归菜单：不能让 editor.ts 的 window 监听器同时挪动光标
  event.preventDefault();
  event.stopPropagation();
  if (event.key === 'Escape') closeMenu();
  else if (event.key === 'ArrowDown') highlight(menuState.activeIndex + 1);
  else if (event.key === 'ArrowUp') highlight(menuState.activeIndex - 1);
  else if (event.key === 'ArrowLeft') switchMenu(-1);
  else if (event.key === 'ArrowRight') switchMenu(1);
  else if (menuState.activeIndex >= 0) {
    const btn = popupButtons()[menuState.activeIndex];
    if (btn?.dataset.action) run(btn.dataset.action);
  }
}

// 打开后量一次宽度再定位；右侧放不下时整体左移（与原实现一致）。
watch(
  () => menuState.open,
  async (open) => {
    openSubIndex.value = null;
    if (!open) return;
    await nextTick();
    const el = popupEl.value;
    const anchor = anchorOf();
    if (!el || !anchor) return;
    const rect = anchor.getBoundingClientRect();
    const width = el.offsetWidth;
    left.value = Math.max(4, Math.min(rect.left, window.innerWidth - width - 4));
    const titlebar = document.getElementById('titlebar');
    top.value = titlebar ? titlebar.getBoundingClientRect().bottom : 30;
  },
);

onMounted(() => {
  document.addEventListener('mousedown', onDocMousedown);
  document.addEventListener('keydown', onKeydown);
});

onUnmounted(() => {
  document.removeEventListener('mousedown', onDocMousedown);
  document.removeEventListener('keydown', onKeydown);
});
</script>

<style scoped>
/* 覆盖层整层 pointer-events:none，弹层必须自己打开才能点 */
#tb-menu-popup {
  pointer-events: auto;
}
</style>

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
          :data-sub-index="i"
          @pointerenter="openSubIndex = i"
          @click.stop="toggleSub(i)"
        >
          <span class="gutter"></span>
          <span class="label">{{ entry.sub.label }}</span>
          <span class="chev" aria-hidden="true">
            <ChevronRight :size="15" :stroke-width="2.25" />
          </span>
        </button>
        <div v-if="openSubIndex === i" class="tb-submenu" role="menu">
          <template v-for="(item, j) in entry.sub.items" :key="j">
            <div v-if="item.sep" class="tb-sep" role="separator"></div>
            <button
              v-else
              type="button"
              role="menuitemradio"
              :data-action="item.action"
              :title="item.title"
              :aria-checked="isChecked(item)"
              :class="{ danger: item.danger }"
              @pointerenter.stop
              @click="run(item.action)"
            >
              <span
                class="gutter"
                :data-mark="isChecked(item) ? (item.mark ?? 'check') : null"
              >
                <Check class="mark-check" :size="14" :stroke-width="2.4" />
                <span class="mark-dot"></span>
              </span>
              <span class="label">{{ item.label }}</span>
              <span v-if="item.acc" class="acc">{{ item.acc }}</span>
            </button>
          </template>
        </div>
      </div>

      <button
        v-else
        type="button"
        :role="entry.mark === 'dot' ? 'menuitemradio' : 'menuitem'"
        :data-action="entry.action"
        :title="entry.title"
        :aria-checked="entry.mark === 'dot' ? isChecked(entry) : undefined"
        :class="{ danger: entry.danger }"
        @pointerenter="openSubIndex = null"
        @click="run(entry.action)"
      >
        <span
          class="gutter"
          :data-mark="isChecked(entry) ? (entry.mark ?? 'check') : null"
        >
          <Check class="mark-check" :size="14" :stroke-width="2.4" />
          <span class="mark-dot"></span>
        </span>
        <span class="label">{{ entry.label }}</span>
        <span v-if="entry.acc" class="acc">{{ entry.acc }}</span>
      </button>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { Check, ChevronRight } from 'lucide-vue-next';
import { dispatch, state } from '../bridge';
import { zh } from '../i18n/zh-CN';
import { MENUS, anchorOf, closeMenu, menuState, switchMenu, type MenuEntry } from '../menu';
import { update } from '../update';

const popupEl = ref<HTMLElement | null>(null);
const openSubIndex = ref<number | null>(null);
const left = ref(4);
const top = ref(30);

const entries = computed<MenuEntry[]>(() => {
  const base = menuState.open ? MENUS[menuState.open] : [];
  // 「最近打开」的条目来自运行时状态，菜单树本身保持静态。
  return base.map((entry) =>
    entry.sub?.dynamic === 'recent'
      ? { ...entry, sub: { ...entry.sub, items: recentEntries() } }
      : entry,
  );
});

function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

function recentEntries(): MenuEntry[] {
  if (!state.recent.length) return [{ label: zh.common.none }];
  return [
    ...state.recent.map((path, i) => ({
      action: 'recent:' + i,
      label: baseName(path) || path,
      title: path,
    })),
    { sep: true },
    { action: 'recent-clear', label: zh.menu.file.recentClear },
  ];
}

function isChecked(entry: MenuEntry): boolean {
  if (!entry.checked) return false;
  // 勾选态来自两处：编辑内核推上来的主题/皮肤/模式，以及纯外壳的自动检查开关。
  return !!entry.checked({
    themeMode: state.themeMode,
    skin: state.skin,
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

/**
 * 可导航的菜单项。
 *
 * 不能只取 `button[data-action]`：子菜单触发器（最近打开 / 外观 / 主题）没有
 * action，按方向键永远跳不过去。折叠的子菜单是 `v-if`，不在 DOM 里；展开之后
 * 它的条目紧跟触发器，正好接在导航序列里。
 */
function popupButtons(): HTMLButtonElement[] {
  return Array.from(popupEl.value?.querySelectorAll<HTMLButtonElement>('button') ?? []);
}

/** 方向键移动高亮。还没有高亮时：下键从头、上键从尾。 */
function stepHighlight(delta: number): void {
  const buttons = popupButtons();
  if (!buttons.length) return;
  const count = buttons.length;
  const from = menuState.activeIndex;
  const next = from < 0
    ? (delta > 0 ? 0 : count - 1)
    : (from + delta + count) % count;
  menuState.activeIndex = next;
  buttons.forEach((btn, i) => btn.classList.toggle('focused', i === next));
}

/** Enter：普通项直接执行，子菜单触发器展开 / 收起。 */
function activate(index: number): void {
  const btn = popupButtons()[index];
  if (!btn) return;
  if (btn.dataset.action) {
    run(btn.dataset.action);
    return;
  }
  const sub = btn.dataset.subIndex;
  if (sub !== undefined) toggleSub(Number(sub));
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
  else if (event.key === 'ArrowDown') stepHighlight(1);
  else if (event.key === 'ArrowUp') stepHighlight(-1);
  else if (event.key === 'ArrowLeft') switchMenu(-1);
  else if (event.key === 'ArrowRight') switchMenu(1);
  else activate(menuState.activeIndex);
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

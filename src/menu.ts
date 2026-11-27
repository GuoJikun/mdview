// 标题栏菜单树与展开状态。数据从原 titlebar.js 原样搬来，动作名不变。
import { shallowReactive } from 'vue';
import { zh } from './i18n/zh-CN';

/** 菜单文案的短别名：下面每条都写 `m.file.save`，不写 `zh.menu.file.save`。 */
const m = zh.menu;

export type MenuKey = 'file' | 'edit' | 'insert' | 'selection' | 'view' | 'go' | 'help';

/** 勾选态只依赖这几个字段，避免把整个只读快照类型带进菜单数据。 */
export interface MenuCheckState {
  themeMode: string;
  /** 当前皮肤：macos / hyperos。与明暗正交，各自一条子菜单。 */
  skin: string;
  mode: string;
  autoUpdate: boolean;
}

export interface MenuEntry {
  action?: string;
  label?: string;
  acc?: string;
  /** 悬停提示。最近打开这类路径很长的项用它显示完整路径。 */
  title?: string;
  mark?: 'check' | 'dot';
  danger?: boolean;
  sep?: boolean;
  /** `dynamic: 'recent'` 表示条目由运行时状态填充（见 MenuPopup.vue）。 */
  sub?: { label: string; items: MenuEntry[]; dynamic?: 'recent' };
  checked?: (state: MenuCheckState) => boolean;
}

export const MENU_ORDER: MenuKey[] = ['file', 'edit', 'insert', 'selection', 'view', 'go', 'help'];

export const MENU_LABELS: Record<MenuKey, string> = {
  file: m.file.title,
  edit: m.edit.title,
  insert: m.insert.title,
  selection: m.selection.title,
  view: m.view.title,
  go: m.go.title,
  help: m.help.title,
};

export const MENUS: Record<MenuKey, MenuEntry[]> = {
  file: [
    { action: 'new-file', label: m.file.newFile, acc: 'Ctrl+N' },
    { action: 'new-window', label: m.file.newWindow, acc: 'Ctrl+Shift+N' },
    { action: 'open-file', label: m.file.openFile, acc: 'Ctrl+O' },
    { action: 'open-folder', label: m.file.openFolder, acc: 'Ctrl+Shift+O' },
    { sub: { label: m.file.recent, items: [], dynamic: 'recent' } },
    { sep: true },
    { action: 'reload-file', label: m.file.reloadFile, acc: 'Ctrl+R' },
    { sep: true },
    { action: 'save', label: m.file.save, acc: 'Ctrl+S' },
    { action: 'save-as', label: m.file.saveAs, acc: 'Ctrl+Shift+S' },
    { action: 'export-html', label: m.file.exportHtml },
    { sep: true },
    { action: 'settings', label: m.file.settings, acc: 'Ctrl+,' },
    { sep: true },
    { action: 'close-window', label: m.file.closeWindow, acc: 'Alt+F4', danger: true },
  ],
  edit: [
    { action: 'undo', label: m.edit.undo, acc: 'Ctrl+Z' },
    { action: 'redo', label: m.edit.redo, acc: 'Ctrl+Y' },
    { sep: true },
    { action: 'bold', label: m.edit.bold, acc: 'Ctrl+B' },
    { action: 'italic', label: m.edit.italic, acc: 'Ctrl+I' },
    { action: 'strike', label: m.edit.strike, acc: 'Ctrl+Shift+X' },
    { action: 'inline-code', label: m.edit.inlineCode, acc: 'Ctrl+E' },
    { sep: true },
    { action: 'cut', label: m.edit.cut, acc: 'Ctrl+X' },
    { action: 'copy', label: m.edit.copy, acc: 'Ctrl+C' },
    { action: 'paste', label: m.edit.paste, acc: 'Ctrl+V' },
    { sep: true },
    { action: 'select-all', label: m.edit.selectAll, acc: 'Ctrl+A' },
    { sep: true },
    { action: 'find', label: m.edit.find, acc: 'Ctrl+F' },
    { action: 'replace', label: m.edit.replace, acc: 'Ctrl+H' },
  ],
  insert: [
    { action: 'insert-image', label: m.insert.image, acc: 'Ctrl+Shift+I' },
    { action: 'insert-link', label: m.insert.link, acc: 'Ctrl+K' },
    { sep: true },
    { action: 'insert-table', label: m.insert.table },
    { action: 'insert-hr', label: m.insert.hr },
    { sep: true },
    { action: 'insert-code', label: m.insert.code },
    { action: 'insert-quote', label: m.insert.quote },
    { action: 'insert-task', label: m.insert.task },
  ],
  selection: [
    { action: 'select-all', label: m.selection.selectAll, acc: 'Ctrl+A' },
    { sep: true },
    { action: 'line-home', label: m.selection.lineHome, acc: 'Home' },
    { action: 'line-end', label: m.selection.lineEnd, acc: 'End' },
  ],
  view: [
    {
      sub: {
        label: m.view.appearance,
        items: [
          {
            action: 'skin-hyperos', label: m.view.skinHyperos, mark: 'dot',
            checked: (s) => s.skin === 'hyperos',
          },
          {
            action: 'skin-macos', label: m.view.skinMacos, mark: 'dot',
            checked: (s) => s.skin === 'macos',
          },
        ],
      },
    },
    {
      sub: {
        label: m.view.theme,
        items: [
          {
            action: 'theme-system', label: m.view.themeSystem, mark: 'dot',
            checked: (s) => s.themeMode === 'system',
          },
          {
            action: 'theme-light', label: m.view.themeLight, mark: 'dot',
            checked: (s) => s.themeMode === 'light',
          },
          {
            action: 'theme-dark', label: m.view.themeDark, mark: 'dot',
            checked: (s) => s.themeMode === 'dark',
          },
        ],
      },
    },
    { sep: true },
    {
      action: 'mode-wysiwyg', label: m.view.wysiwyg, acc: 'Ctrl+1', mark: 'dot',
      checked: (s) => s.mode === 'wysiwyg',
    },
    {
      action: 'mode-reading', label: m.view.reading, acc: 'Ctrl+2', mark: 'dot',
      checked: (s) => s.mode === 'reading',
    },
    {
      action: 'mode-source', label: m.view.source, acc: 'Ctrl+3', mark: 'dot',
      checked: (s) => s.mode === 'source',
    },
  ],
  go: [
    { action: 'outline-prev', label: m.go.prevHeading },
    { action: 'outline-next', label: m.go.nextHeading },
    { sep: true },
    { action: 'doc-start', label: m.go.docStart, acc: 'Ctrl+Home' },
    { action: 'doc-end', label: m.go.docEnd, acc: 'Ctrl+End' },
  ],
  help: [
    { action: 'shortcuts', label: m.help.shortcuts, acc: 'Ctrl+/' },
    { sep: true },
    // 反馈入口：日志与崩溃副本都在配置目录里，不让用户自己去翻 AppData
    { action: 'open-logs', label: m.help.openLogs },
    { action: 'open-backups', label: m.help.openBackups },
    { sep: true },
    { action: 'update', label: m.help.update },
    {
      action: 'auto-update', label: m.help.autoUpdate, mark: 'check',
      checked: (s) => s.autoUpdate,
    },
    { sep: true },
    { action: 'about', label: m.help.about },
  ],
};

/**
 * 展开状态放模块级，TitleBar 与 MenuPopup 共用。
 * 用 shallowReactive：anchor 是真实 DOM 节点，不能被 Vue 代理。
 */
export const menuState = shallowReactive({
  open: null as MenuKey | null,
  activeIndex: -1,
});

let anchorEl: HTMLElement | null = null;

export function anchorOf(): HTMLElement | null {
  return anchorEl;
}

/** 点击顶级菜单：同一个再点一次收起，换一个则直接切换。 */
export function toggleMenu(name: MenuKey, anchor: HTMLElement): void {
  if (menuState.open === name) {
    closeMenu();
    return;
  }
  showMenu(name, anchor);
}

/** 强制展开某个菜单（鼠标横向扫过时用）。 */
export function showMenu(name: MenuKey, anchor: HTMLElement): void {
  menuState.open = name;
  menuState.activeIndex = -1;
  anchorEl = anchor;
}

export function closeMenu(): void {
  menuState.open = null;
  menuState.activeIndex = -1;
  anchorEl = null;
}

/** 菜单展开时左右方向键切换顶级菜单。 */
export function switchMenu(dir: number): void {
  if (!menuState.open) return;
  const current = MENU_ORDER.indexOf(menuState.open);
  const next = MENU_ORDER[(current + dir + MENU_ORDER.length) % MENU_ORDER.length];
  const anchor = document.querySelector<HTMLElement>(`#tb-menus [data-menu="${next}"]`);
  if (anchor) showMenu(next, anchor);
}

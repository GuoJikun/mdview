// 标题栏菜单树与展开状态。数据从原 titlebar.js 原样搬来，动作名不变。
import { shallowReactive } from 'vue';

export type MenuKey = 'file' | 'edit' | 'selection' | 'view' | 'go' | 'help';

/** 勾选态只依赖这几个字段，避免把整个只读快照类型带进菜单数据。 */
export interface MenuCheckState {
  themeMode: string;
  mode: string;
  autoUpdate: boolean;
}

export interface MenuEntry {
  action?: string;
  label?: string;
  acc?: string;
  mark?: 'check' | 'dot';
  danger?: boolean;
  sep?: boolean;
  sub?: { label: string; items: MenuEntry[] };
  checked?: (state: MenuCheckState) => boolean;
}

export const MENU_ORDER: MenuKey[] = ['file', 'edit', 'selection', 'view', 'go', 'help'];

export const MENU_LABELS: Record<MenuKey, string> = {
  file: '文件',
  edit: '编辑',
  selection: '选择',
  view: '视图',
  go: '转到',
  help: '帮助',
};

export const MENUS: Record<MenuKey, MenuEntry[]> = {
  file: [
    { action: 'new-file', label: '新建', acc: 'Ctrl+N' },
    { action: 'open-file', label: '打开文件…', acc: 'Ctrl+O' },
    { action: 'save', label: '保存', acc: 'Ctrl+S' },
    { action: 'save-as', label: '另存为…', acc: 'Ctrl+Shift+S' },
    { sep: true },
    { action: 'close-window', label: '关闭窗口', acc: 'Alt+F4', danger: true },
  ],
  edit: [
    { action: 'undo', label: '撤销', acc: 'Ctrl+Z' },
    { action: 'redo', label: '重做', acc: 'Ctrl+Y' },
    { sep: true },
    { action: 'cut', label: '剪切', acc: 'Ctrl+X' },
    { action: 'copy', label: '复制', acc: 'Ctrl+C' },
    { action: 'paste', label: '粘贴', acc: 'Ctrl+V' },
    { sep: true },
    { action: 'select-all', label: '全选', acc: 'Ctrl+A' },
  ],
  selection: [
    { action: 'select-all', label: '全选', acc: 'Ctrl+A' },
    { sep: true },
    { action: 'line-home', label: '行首', acc: 'Home' },
    { action: 'line-end', label: '行尾', acc: 'End' },
  ],
  view: [
    {
      sub: {
        label: '主题',
        items: [
          {
            action: 'theme-system', label: '跟随系统', mark: 'dot',
            checked: (s) => s.themeMode === 'system',
          },
          {
            action: 'theme-light', label: '亮色', mark: 'dot',
            checked: (s) => s.themeMode === 'light',
          },
          {
            action: 'theme-dark', label: '暗色', mark: 'dot',
            checked: (s) => s.themeMode === 'dark',
          },
        ],
      },
    },
    { sep: true },
    {
      action: 'mode-wysiwyg', label: '所见即所得', mark: 'dot',
      checked: (s) => s.mode === 'wysiwyg',
    },
    {
      action: 'mode-reading', label: '阅读视图', mark: 'dot',
      checked: (s) => s.mode === 'reading',
    },
    {
      action: 'mode-source', label: '源码视图', mark: 'dot',
      checked: (s) => s.mode === 'source',
    },
  ],
  go: [
    { action: 'outline-prev', label: '上一个标题' },
    { action: 'outline-next', label: '下一个标题' },
    { sep: true },
    { action: 'doc-start', label: '文档开头', acc: 'Ctrl+Home' },
    { action: 'doc-end', label: '文档结尾', acc: 'Ctrl+End' },
  ],
  help: [
    { action: 'update', label: '检查更新…' },
    {
      action: 'auto-update', label: '启动时检查更新', mark: 'check',
      checked: (s) => s.autoUpdate,
    },
    { sep: true },
    { action: 'about', label: '关于' },
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

// 标题栏菜单树与展开状态。数据从原 titlebar.js 原样搬来，动作名不变。
import { shallowReactive } from 'vue';

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
  file: '文件',
  edit: '编辑',
  insert: '插入',
  selection: '选择',
  view: '视图',
  go: '转到',
  help: '帮助',
};

export const MENUS: Record<MenuKey, MenuEntry[]> = {
  file: [
    { action: 'new-file', label: '新建', acc: 'Ctrl+N' },
    { action: 'new-window', label: '新建窗口', acc: 'Ctrl+Shift+N' },
    { action: 'open-file', label: '打开文件…', acc: 'Ctrl+O' },
    { action: 'open-folder', label: '打开文件夹…', acc: 'Ctrl+Shift+O' },
    { sub: { label: '最近打开', items: [], dynamic: 'recent' } },
    { sep: true },
    { action: 'reload-file', label: '重新加载', acc: 'Ctrl+R' },
    { sep: true },
    { action: 'save', label: '保存', acc: 'Ctrl+S' },
    { action: 'save-as', label: '另存为…', acc: 'Ctrl+Shift+S' },
    { action: 'export-html', label: '导出 HTML…' },
    { sep: true },
    { action: 'settings', label: '偏好设置…', acc: 'Ctrl+,' },
    { sep: true },
    { action: 'close-window', label: '关闭窗口', acc: 'Alt+F4', danger: true },
  ],
  edit: [
    { action: 'undo', label: '撤销', acc: 'Ctrl+Z' },
    { action: 'redo', label: '重做', acc: 'Ctrl+Y' },
    { sep: true },
    { action: 'bold', label: '加粗', acc: 'Ctrl+B' },
    { action: 'italic', label: '斜体', acc: 'Ctrl+I' },
    { action: 'strike', label: '删除线', acc: 'Ctrl+Shift+X' },
    { action: 'inline-code', label: '行内代码', acc: 'Ctrl+E' },
    { sep: true },
    { action: 'cut', label: '剪切', acc: 'Ctrl+X' },
    { action: 'copy', label: '复制', acc: 'Ctrl+C' },
    { action: 'paste', label: '粘贴', acc: 'Ctrl+V' },
    { sep: true },
    { action: 'select-all', label: '全选', acc: 'Ctrl+A' },
    { sep: true },
    { action: 'find', label: '查找…', acc: 'Ctrl+F' },
    { action: 'replace', label: '替换…', acc: 'Ctrl+H' },
  ],
  insert: [
    { action: 'insert-image', label: '图片…', acc: 'Ctrl+Shift+I' },
    { action: 'insert-link', label: '链接…', acc: 'Ctrl+K' },
    { sep: true },
    { action: 'insert-table', label: '表格' },
    { action: 'insert-hr', label: '分隔线' },
    { sep: true },
    { action: 'insert-code', label: '代码块' },
    { action: 'insert-quote', label: '引用' },
    { action: 'insert-task', label: '任务列表' },
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
        label: '外观',
        items: [
          {
            action: 'skin-hyperos', label: 'HyperOS 4.0', mark: 'dot',
            checked: (s) => s.skin === 'hyperos',
          },
          {
            action: 'skin-macos', label: 'macOS · Mica', mark: 'dot',
            checked: (s) => s.skin === 'macos',
          },
        ],
      },
    },
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
      action: 'mode-wysiwyg', label: '所见即所得', acc: 'Ctrl+1', mark: 'dot',
      checked: (s) => s.mode === 'wysiwyg',
    },
    {
      action: 'mode-reading', label: '阅读视图', acc: 'Ctrl+2', mark: 'dot',
      checked: (s) => s.mode === 'reading',
    },
    {
      action: 'mode-source', label: '源码视图', acc: 'Ctrl+3', mark: 'dot',
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
    { action: 'shortcuts', label: '键盘快捷键', acc: 'Ctrl+/' },
    { sep: true },
    // 反馈入口：日志与崩溃副本都在配置目录里，不让用户自己去翻 AppData
    { action: 'open-logs', label: '打开日志目录' },
    { action: 'open-backups', label: '打开备份目录' },
    { sep: true },
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

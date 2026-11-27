// 快捷键一览的数据来源。
//
// **不手写第二份键位表**：菜单里的 `acc` 才是唯一真值（`editor.ts` 的 keydown
// 就是照着它写的），这里沿 `MENUS` 走一遍把带键位的项收集出来。手工维护一份
// 「帮助面板里的快捷键表」必然与菜单漂移，而漂移的后果是用户照着面板按键却
// 什么也没发生。
//
// 菜单表达不了的键位（查找栏里的开关、Tab 缩进、Esc 取消）只能手写，集中在
// 下面的 `EXTRA`，一行一条，改动时容易看清楚。
import { MENUS, MENU_LABELS, MENU_ORDER, type MenuEntry } from './menu';

export interface ShortcutItem {
  /** 键位写法，如 `Ctrl+Shift+S`。 */
  keys: string;
  /** 这条键位做什么，取菜单项的名字。 */
  label: string;
}

export interface ShortcutGroup {
  title: string;
  items: ShortcutItem[];
}

/** 菜单里看不到、但确实生效的键位。 */
const EXTRA: ShortcutGroup[] = [
  {
    title: '查找栏',
    items: [
      { keys: 'Enter ／ Shift+Enter', label: '下一个 ／ 上一个' },
      { keys: 'Alt+C', label: '区分大小写' },
      { keys: 'Alt+W', label: '全词匹配' },
      { keys: 'Alt+R', label: '正则表达式' },
      { keys: 'Esc', label: '关闭查找栏' },
    ],
  },
  {
    title: '编辑区',
    items: [
      { keys: 'Tab', label: '缩进（表格里跳到下一格）' },
      { keys: 'Shift+Tab', label: '反缩进（表格里跳到上一格）' },
      { keys: 'Esc', label: '取消选区 ／ 关闭弹层' },
      { keys: 'Ctrl+点击', label: '在浏览器里打开链接' },
    ],
  },
];

/** 把菜单项里带键位的一条转成列表项。子菜单（最近打开 / 主题 / 外观）跳过。 */
function shortcutOf(entry: MenuEntry): ShortcutItem | null {
  if (!entry.acc || !entry.label) return null;
  // 菜单里带省略号的表示「会再问一句」，面板上看着多余，去掉
  return { keys: entry.acc, label: entry.label.replace(/…$/, '') };
}

/**
 * 全部快捷键，按菜单分组。
 *
 * 顺序跟菜单一致（文件 → 编辑 → 插入 → 选择 → 转到），末尾接上手写的那两组。
 * 同一个键位在多个菜单里出现时只留第一条（`Ctrl+A` 在「编辑」与「选择」里都有）。
 */
export function shortcutGroups(): ShortcutGroup[] {
  const seen = new Set<string>();
  const groups: ShortcutGroup[] = [];
  for (const key of MENU_ORDER) {
    const items: ShortcutItem[] = [];
    for (const entry of MENUS[key]) {
      const item = shortcutOf(entry);
      if (!item) continue;
      const id = item.keys + '|' + item.label;
      if (seen.has(id)) continue;
      seen.add(id);
      items.push(item);
    }
    if (items.length) groups.push({ title: MENU_LABELS[key], items });
  }
  return [...groups, ...EXTRA];
}

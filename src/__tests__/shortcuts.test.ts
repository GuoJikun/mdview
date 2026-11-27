// 快捷键一览是「从菜单生成」的，这几条测的正是生成规则本身：
// 分组顺序跟菜单一致、带键位的项一条不落、重复键位只留一条。
import { describe, expect, it } from 'vitest';
import { MENUS, MENU_LABELS, MENU_ORDER } from '../menu';
import { shortcutGroups } from '../shortcuts';

describe('shortcuts.shortcutGroups', () => {
  it('按菜单顺序给出分组，菜单里没有键位的组不出现', () => {
    const groups = shortcutGroups();
    expect(groups.length).toBeGreaterThan(0);
    const titles = groups.map((group) => group.title);
    // 视图 / 帮助 里的动作没有键位，不该凭空造出空分组
    expect(titles).not.toContain(MENU_LABELS.view);
    // 分组顺序与 MENU_ORDER 的前后关系一致
    const menuTitles = MENU_ORDER.map((key) => MENU_LABELS[key]).filter((t) => titles.includes(t));
    expect(titles.slice(0, menuTitles.length)).toEqual(menuTitles);
  });

  it('菜单里每一个带键位的动作都出现在面板里（防止漏项）', () => {
    const listed = new Set(
      shortcutGroups().flatMap((group) => group.items.map((item) => item.keys + '|' + item.label)),
    );
    const missing: string[] = [];
    for (const key of MENU_ORDER) {
      for (const entry of MENUS[key]) {
        if (!entry.acc || !entry.label) continue;
        const id = entry.acc + '|' + entry.label.replace(/…$/, '');
        if (!listed.has(id)) missing.push(id);
      }
    }
    expect(missing).toEqual([]);
  });

  it('行内格式与常用快捷键在各自的组里', () => {
    const byTitle = new Map(shortcutGroups().map((group) => [group.title, group.items]));
    const edit = byTitle.get(MENU_LABELS.edit) ?? [];
    expect(edit).toContainEqual({ keys: 'Ctrl+B', label: '加粗' });
    expect(edit).toContainEqual({ keys: 'Ctrl+Shift+X', label: '删除线' });
    const insert = byTitle.get(MENU_LABELS.insert) ?? [];
    expect(insert).toContainEqual({ keys: 'Ctrl+K', label: '链接' });
    const file = byTitle.get(MENU_LABELS.file) ?? [];
    expect(file).toContainEqual({ keys: 'Ctrl+Shift+O', label: '打开文件夹' });
  });

  it('同一个键位＋名字只列一次（全选在两个菜单里都有）', () => {
    const keys = shortcutGroups().flatMap((group) =>
      group.items.map((item) => item.keys + '|' + item.label),
    );
    expect(new Set(keys).size).toBe(keys.length);
    // 手写的补充键位（查找栏 / Tab 缩进）确实带上了
    expect(keys.some((id) => id.startsWith('Alt+R'))).toBe(true);
    expect(keys.some((id) => id.startsWith('Shift+Tab'))).toBe(true);
  });

  it('菜单项里的省略号不进面板', () => {
    const labels = shortcutGroups().flatMap((g) => g.items.map((item) => item.label));
    expect(labels.some((label) => label.endsWith('…'))).toBe(false);
  });
});

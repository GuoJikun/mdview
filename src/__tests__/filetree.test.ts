// 侧栏树模型是纯逻辑（不碰 DOM 也不碰 IPC），放 unit project。
// 重点测三件事：懒展开的层级拍平、重新展开时保留已展开的子目录、路径比较在
// Windows 反斜杠与大小写上的宽容度。
import { describe, expect, it } from 'vitest';
import {
  attachChildren,
  baseName,
  dirName,
  findNode,
  flatten,
  fromEntries,
  isInside,
  samePath,
  type Entry,
  type FileNode,
} from '../filetree';

const listing = (entries: Entry[]): FileNode[] => fromEntries(entries);

const entries: Entry[] = [
  { name: '子目录', path: 'C:\\笔记\\子目录', dir: true },
  { name: '甲.md', path: 'C:\\笔记\\甲.md', dir: false },
];

describe('filetree.fromEntries', () => {
  it('新拉回来的目录一律收起且未加载', () => {
    const nodes = listing(entries);
    expect(nodes.map((n) => n.name)).toEqual(['子目录', '甲.md']);
    expect(nodes[0].expanded).toBe(false);
    expect(nodes[0].children).toBeNull();
  });
});

describe('filetree.flatten', () => {
  it('收起的目录不展开子项，展开后按层级跟在后面', () => {
    const nodes = listing(entries);
    expect(flatten(nodes).map((r) => r.node.name)).toEqual(['子目录', '甲.md']);

    attachChildren(nodes, 'C:\\笔记\\子目录', [
      { name: '乙.md', path: 'C:\\笔记\\子目录\\乙.md', dir: false },
    ]);
    nodes[0].expanded = true;
    const rows = flatten(nodes);
    expect(rows.map((r) => r.node.name)).toEqual(['子目录', '乙.md', '甲.md']);
    expect(rows.map((r) => r.depth)).toEqual([0, 1, 0]);
  });

  it('展开但没有子项（空目录）时不额外产生行', () => {
    const nodes = listing(entries);
    attachChildren(nodes, 'C:\\笔记\\子目录', []);
    nodes[0].expanded = true;
    expect(flatten(nodes).map((r) => r.node.name)).toEqual(['子目录', '甲.md']);
  });
});

describe('filetree.attachChildren', () => {
  it('重新展开时保留下一层已经展开的状态', () => {
    const nodes = listing(entries);
    attachChildren(nodes, 'C:\\笔记\\子目录', [
      { name: '深层', path: 'C:\\笔记\\子目录\\深层', dir: true },
    ]);
    const inner = findNode(nodes, 'C:\\笔记\\子目录\\深层')!;
    attachChildren(nodes, inner.path, [
      { name: '丙.md', path: 'C:\\笔记\\子目录\\深层\\丙.md', dir: false },
    ]);
    inner.expanded = true;

    // 父目录重新拉一次列表（文件可能被外部改动）
    attachChildren(nodes, 'C:\\笔记\\子目录', [
      { name: '深层', path: 'C:\\笔记\\子目录\\深层', dir: true },
    ]);
    const again = findNode(nodes, 'C:\\笔记\\子目录\\深层')!;
    expect(again.expanded).toBe(true);
    expect(again.children?.map((c) => c.name)).toEqual(['丙.md']);
  });

  it('目标路径不在树里时返回 false，不抛异常', () => {
    const nodes = listing(entries);
    expect(attachChildren(nodes, 'C:\\别处', [])).toBe(false);
    expect(findNode(nodes, 'C:\\别处')).toBeNull();
  });
});

describe('filetree 路径工具', () => {
  it('baseName / dirName 两种斜杠都认', () => {
    expect(baseName('C:\\笔记\\甲.md')).toBe('甲.md');
    expect(baseName('/home/notes/a.md')).toBe('a.md');
    expect(baseName('/home/notes/')).toBe('notes');
    expect(dirName('C:\\笔记\\甲.md')).toBe('C:\\笔记');
    expect(dirName('/home/notes/a.md')).toBe('/home/notes');
  });

  it('samePath 忽略大小写与斜杠方向', () => {
    expect(samePath('C:\\笔记\\甲.md', 'c:/笔记/甲.md')).toBe(true);
    expect(samePath('C:\\笔记\\甲.md', 'C:\\笔记\\乙.md')).toBe(false);
    expect(samePath('', '')).toBe(false);
  });

  it('isInside 认工作区前缀，但不把「笔记2」当成「笔记」的子项', () => {
    expect(isInside('C:\\笔记\\2026\\a.md', 'C:\\笔记')).toBe(true);
    expect(isInside('C:\\笔记', 'C:\\笔记')).toBe(true);
    expect(isInside('C:\\笔记2\\a.md', 'C:\\笔记')).toBe(false);
    expect(isInside('C:\\别处\\a.md', '')).toBe(false);
  });
});

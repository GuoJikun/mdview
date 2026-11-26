// 文件侧栏的树模型：纯数据结构，不碰 DOM、不碰 IPC，Node 里可直接测。
//
// 为什么要有这一层：树是「懒展开」的——目录节点第一次展开才去问后端要子项，
// 之后再展开就用缓存（保留用户已经点开的层级）。这套「按路径找节点、把子项
// 挂上去、按展开状态拍平成一列」的逻辑与界面无关，放在这里才能写单测。
'use strict';

/** 后端一次目录查询回来的单项（与 `src-tauri/src/workspace.rs` 的 Entry 同构）。 */
export interface Entry {
  name: string;
  path: string;
  dir: boolean;
}

/** 侧栏里的一棵树节点。 */
export interface FileNode {
  name: string;
  path: string;
  dir: boolean;
  /** 目录是否展开；文件恒为 false。 */
  expanded: boolean;
  /** 目录的子项；`null` 表示还没展开过（缓存未加载）。 */
  children: FileNode[] | null;
}

/** 拍平后的一行，`depth` 决定缩进层级。 */
export interface Row {
  node: FileNode;
  depth: number;
}

/** 后端列表 → 树节点。新建的目录都是收起且未加载的。 */
export function fromEntries(entries: Entry[]): FileNode[] {
  return entries.map((entry) => ({
    name: entry.name,
    path: entry.path,
    dir: entry.dir,
    expanded: false,
    children: null,
  }));
}

/** 按路径深度优先查找。路径是后端给的绝对路径，同一次会话里唯一。 */
export function findNode(nodes: FileNode[], path: string): FileNode | null {
  for (const node of nodes) {
    if (samePath(node.path, path)) return node;
    if (node.children) {
      const hit = findNode(node.children, path);
      if (hit) return hit;
    }
  }
  return null;
}

/**
 * 把一层子项挂到 `path` 指向的目录下。
 *
 * **保留已经展开过的子目录**：用户点开 `笔记/2026/`、收起、再展开 `笔记/`，不该
 * 让他把层级重新点一遍。所以按路径把旧的 `expanded` / `children` 续到新节点上。
 *
 * @returns 是否找到了目标节点（找不到说明树已经被换掉，调用方刷新即可）。
 */
export function attachChildren(nodes: FileNode[], path: string, entries: Entry[]): boolean {
  const target = findNode(nodes, path);
  if (!target) return false;
  const previous = new Map((target.children ?? []).map((child) => [child.path, child]));
  target.children = fromEntries(entries).map((child) => {
    const old = previous.get(child.path);
    if (old && old.dir && child.dir) {
      return { ...child, expanded: old.expanded, children: old.children };
    }
    return child;
  });
  return true;
}

/** 按展开状态把树拍平成一列渲染行。收起的目录不展开它的子项。 */
export function flatten(nodes: FileNode[], depth = 0, out: Row[] = []): Row[] {
  for (const node of nodes) {
    out.push({ node, depth });
    if (node.dir && node.expanded && node.children) {
      flatten(node.children, depth + 1, out);
    }
  }
  return out;
}

/** 路径末段。Windows 的反斜杠与 POSIX 的斜杠都认。 */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  const cut = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return cut >= 0 ? trimmed.slice(cut + 1) : trimmed;
}

/** 去掉末段后的目录（用于「文件是否在这个工作区里」这类包含判断）。 */
export function dirName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  const cut = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return cut > 0 ? trimmed.slice(0, cut) : '';
}

/**
 * 两个路径是否指同一个文件。
 *
 * Windows / macOS 的文件系统默认大小写不敏感，`C:\a\B.md` 与 `c:\A\b.md` 是同一
 * 个文件；把分隔符也统一掉，免得前后端各写一种斜杠时高亮对不上。
 */
export function samePath(a: string, b: string): boolean {
  if (!a || !b) return false;
  return a.replace(/\\/g, '/').toLowerCase() === b.replace(/\\/g, '/').toLowerCase();
}

/** `path` 是否在 `root` 目录之下（含 root 自身）。 */
export function isInside(path: string, root: string): boolean {
  const norm = (p: string) => p.replace(/\\/g, '/').toLowerCase().replace(/\/+$/, '');
  const child = norm(path);
  const parent = norm(root);
  return !!parent && (child === parent || child.startsWith(parent + '/'));
}

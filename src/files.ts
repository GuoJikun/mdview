// 文件侧栏（工作区）。左侧那一列在「大纲」之上再叠一块：上面跟进工作区里的
// Markdown 目录树，下面仍是大纲——大纲跟当前文档，文件树跟工作区。
//
// 三条约束：
// 1. **打开文件仍走 `open_file`**：树里点一个文件与「最近打开」「拖放」用的是
//    同一条路径（`initFileTree` 传进来的 `open`），未保存改动的确认完全一致。
// 2. **懒展开**：只有第一次点开某个目录时才向后端要子项，见 `workspace.rs`。
// 3. **工作区只是视图状态**：内核与 `Session` 不知道它的存在，所以根路径记在
//    localStorage，启动时尝试恢复；目录不在了就静静忘掉，不打扰用户。
import { invoke } from '@tauri-apps/api/core';
import { zh } from './i18n/zh-CN';
import {
  attachChildren,
  findNode,
  flatten,
  fromEntries,
  samePath,
  type Entry,
  type FileNode,
} from './filetree';

/** 后端 `read_folder` / `open_folder` 的返回体。 */
interface Listing {
  path: string;
  name: string;
  entries: Entry[];
  truncated: boolean;
}

export interface FileTreeOptions {
  /** 点开一个文件时怎么打开（editor.ts 传 `openPath`，走同一条 IPC）。 */
  open: (path: string) => void;
  /** 出错时的提示（editor.ts 传 toast）。 */
  toast: (text: string) => void;
}

const STORAGE_KEY = 'mdview.workspace';

const panel = document.getElementById('files') as HTMLElement | null;

let root: FileNode | null = null;
let activePath: string | null = null;
let options: FileTreeOptions = { open: () => {}, toast: () => {} };
/** 有目录因为条目太多被截断：面板底部提一句，不让用户以为文件丢了。 */
let truncated = false;
/** 展开 / 收起时防止连点造成重复请求。 */
const loading = new Set<string>();

function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;   // 隐私模式下 localStorage 可能直接抛，侧栏不该因此挂掉
  }
}

function rememberRoot(path: string | null): void {
  const store = storage();
  if (!store) return;
  if (path) store.setItem(STORAGE_KEY, path);
  else store.removeItem(STORAGE_KEY);
}

function note(text: string): HTMLElement {
  const el = document.createElement('div');
  el.className = 'ft-note';
  el.textContent = text;
  return el;
}

function rowOf(node: FileNode, depth: number): HTMLElement {
  const row = document.createElement('div');
  row.className = node.dir ? 'ft-row ft-dir' : 'ft-row';
  if (node.expanded) row.classList.add('open');
  if (!node.dir && activePath && samePath(node.path, activePath)) row.classList.add('active');
  row.dataset.path = node.path;
  row.title = node.path;
  row.style.paddingLeft = String(6 + depth * 12) + 'px';

  const caret = document.createElement('span');
  caret.className = 'ft-caret';
  // 文件没有展开箭头，但留出同样的占位，文件名才能与目录名左对齐
  if (node.dir) {
    caret.innerHTML = '<svg class="i" aria-hidden="true"><use href="#i-chevron-right" /></svg>';
  }
  const label = document.createElement('span');
  label.className = 'ft-label';
  label.textContent = node.name;
  row.append(caret, label);
  return row;
}

/** 把树画出来。整棵重绘：一屏几十行，比做增量 diff 简单也更不容易错。 */
function render(): void {
  if (!panel) return;
  if (!root) {
    panel.replaceChildren();
    document.documentElement.classList.remove('has-workspace');
    window.__mdviewRefreshScrollbar?.();
    return;
  }
  document.documentElement.classList.add('has-workspace');

  const head = document.createElement('div');
  head.className = 'ft-head';
  const title = document.createElement('span');
  title.className = 'ft-root';
  title.textContent = root.name;
  title.title = root.path;
  const close = document.createElement('button');
  close.type = 'button';
  close.className = 'ft-close';
  close.dataset.ft = 'close';
  close.title = zh.files.collapse;
  close.setAttribute('aria-label', zh.files.collapse);
  close.innerHTML = '<svg class="i" aria-hidden="true"><use href="#i-x" /></svg>';
  head.append(title, close);

  const list = document.createElement('div');
  list.className = 'ft-list';
  const rows = flatten(root.children ?? []);
  if (!rows.length) list.appendChild(note(zh.files.emptyDir));
  for (const { node, depth } of rows) list.appendChild(rowOf(node, depth));
  if (truncated) list.appendChild(note(zh.files.truncated));

  panel.replaceChildren(head, list);
  window.__mdviewRefreshScrollbar?.();
}

/** 展开 / 收起一个目录。展开时重新拉一遍列表，外部新增的文件能立刻看到。 */
async function toggle(node: FileNode): Promise<void> {
  if (!root) return;
  if (node.expanded) {
    node.expanded = false;
    render();
    return;
  }
  if (loading.has(node.path)) return;
  loading.add(node.path);
  try {
    const listing = await invoke<Listing>('read_folder', { path: node.path });
    attachChildren([root], node.path, listing.entries);
    const fresh = findNode([root], node.path);
    if (fresh) fresh.expanded = true;
    if (listing.truncated) truncated = true;
    render();
  } catch (err) {
    options.toast(zh.files.readFailed(err));
  } finally {
    loading.delete(node.path);
  }
}

function handleClick(event: MouseEvent): void {
  const target = event.target as HTMLElement | null;
  if (!target) return;
  if (target.closest('[data-ft="close"]')) {
    closeWorkspace();
    return;
  }
  const row = target.closest<HTMLElement>('.ft-row');
  if (!row || !root) return;
  const node = findNode([root], row.dataset.path ?? '');
  if (!node) return;
  if (node.dir) {
    void toggle(node);
    return;
  }
  activePath = node.path;
  render();
  options.open(node.path);
}

function adopt(listing: Listing): void {
  root = {
    name: listing.name,
    path: listing.path,
    dir: true,
    expanded: true,
    children: fromEntries(listing.entries),
  };
  activePath = null;
  truncated = listing.truncated;
  render();
}

/**
 * 打开一个工作区。
 *
 * @param path 传了就打开这个目录（拖放 / 启动恢复 / 自检），不传则弹选择框。
 */
export async function openWorkspace(path?: string): Promise<void> {
  let listing: Listing | null;
  try {
    listing = await invoke<Listing | null>('open_folder', { path: path ?? null });
  } catch (err) {
    options.toast(zh.files.openFailed(err));
    return;
  }
  if (!listing) return;   // 用户取消
  adopt(listing);
  rememberRoot(listing.path);
}

/** 收起侧栏并忘掉这个工作区（下次启动不再自动打开）。 */
export function closeWorkspace(): void {
  root = null;
  activePath = null;
  truncated = false;
  rememberRoot(null);
  render();
}

/** 启动时恢复上次的工作区。目录不在了就静默清理，不弹任何提示。 */
export async function restoreWorkspace(): Promise<void> {
  const saved = storage()?.getItem(STORAGE_KEY);
  if (!saved) return;
  try {
    adopt(await invoke<Listing>('read_folder', { path: saved }));
  } catch {
    rememberRoot(null);
  }
}

/**
 * 标记当前文档，让树里对应的那一行高亮。
 *
 * 文档不在工作区里（另存到别处、单独打开的文件）时不高亮任何一行——比高亮错的
 * 那行强。
 */
export function markActive(path: string | null): void {
  if (activePath === path) return;
  activePath = path;
  if (root) render();
}

/** 当前是否已有工作区。 */
export function hasWorkspace(): boolean {
  return !!root;
}

export function initFileTree(opts: FileTreeOptions): void {
  options = opts;
  panel?.addEventListener('click', handleClick);
}

declare global {
  interface Window {
    /** 自检入口：按路径打开一个工作区（不弹对话框）。 */
    __mdviewOpenWorkspace?: (path?: string) => Promise<void>;
  }
}

window.__mdviewOpenWorkspace = openWorkspace;

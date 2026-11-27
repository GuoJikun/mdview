// 前端外壳与编辑内核之间唯一的窄桥。
//
// 只传「状态快照」和「动作名」：状态由现有逻辑推上来（editor/editor.ts / bridge.setTitle），
// 动作由 Vue 组件发回去（复用 window.__mdviewRunAction），两侧都不碰对方的 DOM。
// 文档内容、DOM 节点、编辑器坐标一概不过桥。
import { getCurrentWindow } from '@tauri-apps/api/window';
import { nextTick, reactive, readonly } from 'vue';
import { openAbout } from './about';
import { openSettings } from './settings';
import { openShortcuts } from './help';
import { openUpdateDialog, toggleAutoUpdate } from './update';
import type { SkinName } from './skin';

export interface OutlineItem {
  level: number;
  title: string;
  block: number;
}

export type ThemeMode = 'system' | 'light' | 'dark';
export type ThemeName = 'light' | 'dark';
export type ViewMode = 'wysiwyg' | 'reading' | 'source';

export interface BridgeState {
  /** 当前文件路径；未命名文档为 null。 */
  path: string | null;
  /** 标题栏文案（文件名 + 脏标记 + 应用名）。 */
  title: string;
  dirty: boolean;
  /** 正文块数。 */
  blocks: number;
  /** 正文字数（可见文本，不含 Markdown 标记与空白）。 */
  words: number;
  /** 光标停靠点序号；无光标为 null。 */
  stop: number | null;
  /** 光标所在块的索引（0 起）；无光标为 null。状态栏「块 x/y」用。 */
  caretBlock: number | null;
  /** 视觉列；无光标为 null。 */
  column: number | null;
  /** 内核 revision，状态栏右侧的 r 值。 */
  revision: number;
  /** 磁盘编码（`utf-8` / `utf-8-bom` / `gb18030`）。非 UTF-8 时状态栏给个提示。 */
  encoding: string;
  /** 光标是否落在表格单元格里，决定状态栏是否显示行列工具条。 */
  inTable: boolean;
  /** 主题偏好档位：跟随系统 / 亮色 / 暗色。 */
  themeMode: ThemeMode;
  /** 当前生效主题。 */
  theme: ThemeName;
  /** 外观皮肤：macOS · Mica / HyperOS 4.0。与明暗正交。 */
  skin: SkinName;
  /** 当前视图模式。 */
  mode: ViewMode;
  /** 大纲（标题项），供侧栏渲染与滚动定位 */
  outline: OutlineItem[];
  /** 最近打开的文件路径，最近的在前；文件菜单据此渲染子菜单。 */
  recent: string[];
  activeOutlineIdx: number;
  toast: { text: string; seq: number; visible: boolean };
}

export interface BridgeApi {
  set(patch: Partial<BridgeState>): void;
  notify(text: string): void;
  dispatch(action: string): void;
}

declare global {
  interface Window {
    __mdviewBridge?: BridgeApi;
    __mdviewPushShellState?: () => void;
    __mdviewRunAction?: (action: string) => void;
    __mdviewSetTitle?: (path: string | null, dirty: boolean) => void;
    __mdviewNextTick?: () => Promise<void>;
    __mdviewMode?: ViewMode;
    /**
     * 偏好改动后重测几何：字号 / 行宽会改变折行，自绘光标与大纲锚点
     * 都是按像素量的，不重测就会停在旧位置。
     */
    __mdviewRelayout?: () => void;
    mdviewTheme?: { mode?: () => string };
  }
}

const TOAST_MS = 1400;

const store = reactive<BridgeState>({
  path: null,
  title: document.title || 'mdview',
  dirty: false,
  blocks: 0,
  words: 0,
  stop: null,
  caretBlock: null,
  column: null,
  revision: 0,
  encoding: 'utf-8',
  inTable: false,
  themeMode: 'system',
  theme: document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light',
  // skin-boot.js 已在首帧前写好属性；没写就是默认皮肤
  skin: document.documentElement.getAttribute('data-skin') === 'hyperos' ? 'hyperos' : 'macos',
  mode: window.__mdviewMode || 'wysiwyg',
  outline: [],
  recent: [],
  activeOutlineIdx: -1,
  toast: { text: '', seq: 0, visible: false },
});

/** 组件只读这一份；写入统一走 set / notify。 */
export const state = readonly(store);

export function set(patch: Partial<BridgeState>): void {
  Object.assign(store, patch);
}

let toastTimer = 0;

/** 与现有 #toast 并行：Vue 只负责显示，计时也在这里，避免两套时钟。 */
export function notify(text: string): void {
  store.toast = { text, seq: store.toast.seq + 1, visible: true };
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    store.toast.visible = false;
  }, TOAST_MS);
}

/** 动作出口：一行都不重写，直接复用现有 runAction。 */
export function dispatch(action: string): void {
  // 关闭窗口原先由 titlebar.js 的 run() 拦截，现在收进桥里。
  if (action === 'close-window') {
    tauriWindow()?.close();
    return;
  }
  // 纯外壳动作：编辑内核不关心更新与关于，也不该为了弹窗去 import UI 模块。
  if (action === 'about') {
    openAbout();
    return;
  }
  if (action === 'settings') {
    openSettings();
    return;
  }
  if (action === 'shortcuts') {
    openShortcuts();
    return;
  }
  if (action === 'update') {
    openUpdateDialog();
    return;
  }
  if (action === 'auto-update') {
    const enabled = toggleAutoUpdate();
    notify(enabled ? '已开启启动时检查更新' : '已关闭启动时检查更新');
    return;
  }
  const run = window.__mdviewRunAction;
  if (typeof run === 'function') {
    run(action);
  } else {
    console.warn('[bridge] 动作无人接管：' + action);
  }
}

function tauriWindow() {
  // 直接用浏览器打开 dev server 时没有 Tauri 内部对象，保持原来的可选行为。
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
}

/**
 * 标题栏文案：文件名 + 脏标记 + 应用名，与 VSCode 的写法一致。
 * editor.ts 仍通过 window.__mdviewSetTitle 调用，签名不变。
 */
export function setTitle(path: string | null, dirty: boolean): void {
  const name = path ? String(path).split(/[\\/]/).pop() : '未命名';
  const text = name + (dirty ? ' ●' : '') + ' — mdview';
  set({ path: path || null, dirty: !!dirty, title: text });
  document.title = text;
  tauriWindow()?.setTitle(text)?.catch?.(() => {});
}
window.__mdviewSetTitle = setTitle;
window.__mdviewNextTick = nextTick;

const api: BridgeApi = { set, notify, dispatch };
window.__mdviewBridge = api;

// 主题与皮肤都由各自的纯逻辑模块直接写 <html>（那七个纯逻辑模块一行不改），
// 所以这里用观察器跟随，而不是往 theme.js / skin.ts 里塞桥接代码。
new MutationObserver(() => {
  window.__mdviewPushShellState?.();
}).observe(document.documentElement, {
  attributes: true,
  attributeFilter: ['data-theme', 'data-skin'],
});

// 桥建好时主动拉一次全量快照，避免首帧依赖下一次 renderStatus 才对齐。
window.__mdviewPushShellState?.();

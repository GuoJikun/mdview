// 前端外壳与编辑内核之间唯一的窄桥。
//
// 只传「状态快照」和「动作名」：状态由现有逻辑推上来（app.js / bridge.setTitle），
// 动作由 Vue 组件发回去（复用 window.__mdviewRunAction），两侧都不碰对方的 DOM。
// 文档内容、DOM 节点、编辑器坐标一概不过桥。
import { nextTick, reactive, readonly } from 'vue';

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
  /** 光标停靠点序号；无光标为 null。 */
  stop: number | null;
  /** 视觉列；无光标为 null。 */
  column: number | null;
  /** 内核 revision，状态栏右侧的 r 值。 */
  revision: number;
  /** 光标是否落在表格单元格里，决定状态栏是否显示行列工具条。 */
  inTable: boolean;
  /** 主题偏好档位：跟随系统 / 亮色 / 暗色。 */
  themeMode: ThemeMode;
  /** 当前生效主题。 */
  theme: ThemeName;
  /** 当前视图模式。 */
  mode: ViewMode;
  outline: OutlineItem[];
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
    mdviewTheme?: { mode?: () => string };
    __TAURI__?: {
      core?: { invoke?: (cmd: string, args?: unknown) => Promise<unknown> };
      event?: { listen?: (event: string, handler: (...args: unknown[]) => void) => Promise<() => void> };
      window?: {
        getCurrentWindow?: () => {
          minimize: () => void;
          toggleMaximize: () => void;
          close: () => void;
          isMaximized: () => Promise<boolean>;
          setTitle: (title: string) => Promise<void>;
        };
      };
    };
  }
}

const TOAST_MS = 1400;

const store = reactive<BridgeState>({
  path: null,
  title: document.title || 'mdview',
  dirty: false,
  blocks: 0,
  stop: null,
  column: null,
  revision: 0,
  inTable: false,
  themeMode: 'system',
  theme: document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light',
  mode: window.__mdviewMode || 'wysiwyg',
  outline: [],
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
  const run = window.__mdviewRunAction;
  if (typeof run === 'function') {
    run(action);
  } else {
    console.warn('[bridge] 动作无人接管：' + action);
  }
}

function tauriWindow() {
  return window.__TAURI__?.window?.getCurrentWindow?.();
}

/**
 * 标题栏文案：文件名 + 脏标记 + 应用名，与 VSCode 的写法一致。
 * app.js 仍通过 window.__mdviewSetTitle 调用，签名不变。
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

// 主题由 theme.js 直接写 <html data-theme>（那七个纯逻辑模块一行不改），
// 所以这里用观察器跟随，而不是往 theme.js 里塞桥接代码。
new MutationObserver(() => {
  window.__mdviewPushShellState?.();
}).observe(document.documentElement, {
  attributes: true,
  attributeFilter: ['data-theme'],
});

// 桥建好时主动拉一次全量快照，避免首帧依赖下一次 renderStatus 才对齐。
window.__mdviewPushShellState?.();

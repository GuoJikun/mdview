// Windows 11 系统底材的前端配合层。
// Rust 负责调 DWM；这里只做三件事：
//   1. 查询当前材质，写 <html data-backdrop> 给 CSS 判断；
//   2. 主题变化时把生效明暗推回 Rust——DWM 深色模式没有事件，只能主动同步；
//   3. 暴露一个临时切换函数，方便开发时对照 Mica / Acrylic / 纯色。
// 浏览器直开 dev server 时没有 Tauri IPC，全部静默降级为纯色。
import { invoke } from '@tauri-apps/api/core';

type BackdropKind = 'mica' | 'acrylic' | 'none';
type Theme = 'light' | 'dark';

declare global {
  interface Window {
    /** 临时切换材质，仅当前进程有效；可从 DevTools 控制台调用。 */
    __mdviewSetBackdrop?: (mode: BackdropKind) => Promise<BackdropKind>;
  }
}

function currentTheme(): Theme {
  return document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light';
}

function writeKind(kind: BackdropKind): void {
  document.documentElement.setAttribute('data-backdrop', kind);
}

async function pushTheme(): Promise<void> {
  try {
    await invoke('sync_backdrop', { theme: currentTheme() });
  } catch {
    /* 非 Tauri 环境：没有 DWM 材质要同步 */
  }
}

async function setBackdrop(mode: BackdropKind): Promise<BackdropKind> {
  const kind = await invoke<BackdropKind>('set_backdrop_mode', { mode });
  writeKind(kind);
  return kind;
}
window.__mdviewSetBackdrop = setBackdrop;

export function initBackdrop(): void {
  void invoke<BackdropKind>('backdrop_kind')
    .then(writeKind)
    .catch(() => writeKind('none'))
    .then(pushTheme);

  // theme.js 只写 data-theme，不通知 Rust；借同一个观察口把明暗续上。
  new MutationObserver(() => {
    void pushTheme();
  }).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme'],
  });
}

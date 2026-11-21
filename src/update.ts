// 「检查更新」的外壳状态与 IPC。
//
// 只碰 Tauri API 和 Vue 响应式状态，**不引用 bridge**：bridge 反过来要拦截
// `update` / `auto-update` 两个菜单动作，两边互相 import 会形成模块环。
// 弹窗组件只读这里的 `update`，所有写操作都走本文件的函数。
import { getVersion } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { reactive } from 'vue';

export type UpdatePhase =
  | 'idle'
  | 'checking'
  | 'up-to-date'
  | 'available'
  | 'downloading'
  | 'ready'
  | 'error';

/** 与 `src-tauri/src/update.rs` 的 `UpdateInfo` 对应。 */
export interface UpdateInfo {
  version: string;
  body: string;
  date: string | null;
}

/** 与 `update:progress` 事件负载对应。 */
export interface UpdateProgress {
  received: number;
  total: number | null;
}

interface UpdateErrorPayload {
  code?: string;
  message?: string;
}

/** 启动时自动检查的开关；默认关闭，和 image-view 一样不主动联网。 */
const AUTO_KEY = 'mdview-update-on-launch';

function readAutoFlag(): boolean {
  try {
    return localStorage.getItem(AUTO_KEY) === '1';
  } catch {
    // 浏览器预览或存储被禁用时按关闭处理。
    return false;
  }
}

function writeAutoFlag(enabled: boolean): void {
  try {
    if (enabled) localStorage.setItem(AUTO_KEY, '1');
    else localStorage.removeItem(AUTO_KEY);
  } catch {
    // 只在本次会话生效。
  }
}

export const update = reactive({
  /** 弹窗是否展开。 */
  open: false,
  phase: 'idle' as UpdatePhase,
  info: null as UpdateInfo | null,
  error: '',
  received: 0,
  total: null as number | null,
  /** 当前二进制版本，弹窗里展示；取不到时留空。 */
  version: '',
  /** 「启动时检查更新」开关。 */
  autoUpdate: readAutoFlag(),
});

let initialized = false;

/**
 * 取当前版本并订阅下载进度。只做一次，Tauri 之外（浏览器直接开 dev server）
 * 静默跳过。
 */
export async function initUpdate(): Promise<void> {
  if (initialized) return;
  initialized = true;
  try {
    update.version = await getVersion();
  } catch {
    // 没有 Tauri runtime。
  }
  try {
    await listen<UpdateProgress>('update:progress', ({ payload }) => {
      update.received = payload.received;
      update.total = payload.total;
    });
  } catch {
    // 同上。
  }
}

/** 打开弹窗并立即检查。 */
export function openUpdateDialog(): void {
  update.open = true;
  void check();
}

export function closeUpdateDialog(): void {
  // 下载中关窗等同于取消，否则后端会继续拉完整个安装包。
  if (update.phase === 'downloading') void cancelDownload();
  update.open = false;
}

/** 检查更新（弹窗用）：结果写进 `phase`。 */
export async function check(): Promise<void> {
  update.phase = 'checking';
  update.error = '';
  update.info = null;
  update.received = 0;
  update.total = null;
  try {
    const info = await invoke<UpdateInfo | null>('check_update');
    if (info) {
      update.info = info;
      update.phase = 'available';
    } else {
      update.phase = 'up-to-date';
    }
  } catch (error) {
    update.error = normalizeError(error).message;
    update.phase = 'error';
  }
}

/**
 * 启动时的静默检查：不碰弹窗状态，失败也不提示。
 * 发现新版本由调用方决定怎么告诉用户。
 */
export async function checkSilently(): Promise<UpdateInfo | null> {
  try {
    return await invoke<UpdateInfo | null>('check_update');
  } catch {
    return null;
  }
}

export async function download(): Promise<void> {
  update.phase = 'downloading';
  update.received = 0;
  update.total = null;
  try {
    await invoke<void>('download_update');
    if (update.phase === 'downloading') update.phase = 'ready';
  } catch (error) {
    // 弹窗已关闭或已重新检查时，旧请求的回包没有意义。
    if (update.phase !== 'downloading') return;
    const failure = normalizeError(error);
    if (failure.code === 'update_cancelled') {
      update.phase = 'available';
    } else {
      update.error = failure.message;
      update.phase = 'error';
    }
  }
}

export async function cancelDownload(): Promise<void> {
  try {
    await invoke<void>('cancel_update_download');
  } catch {
    // 取消失败时下载命令会自行收尾。
  }
}

export async function install(): Promise<void> {
  try {
    await invoke<void>('install_update');
  } catch (error) {
    update.error = normalizeError(error).message;
    update.phase = 'error';
  }
}

/** 切换「启动时检查更新」，返回切换后的值。 */
export function toggleAutoUpdate(): boolean {
  update.autoUpdate = !update.autoUpdate;
  writeAutoFlag(update.autoUpdate);
  return update.autoUpdate;
}

function normalizeError(error: unknown): { code: string; message: string } {
  if (typeof error === 'object' && error !== null) {
    const payload = error as UpdateErrorPayload;
    return {
      code: typeof payload.code === 'string' ? payload.code : 'unknown',
      message: typeof payload.message === 'string' ? payload.message : String(error),
    };
  }
  return { code: 'unknown', message: String(error) };
}

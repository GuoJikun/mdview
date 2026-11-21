// 「关于」弹窗的外壳状态。
//
// 与 update.ts 同构：只碰 Tauri API 与 Vue 响应式状态，**不引用 bridge**——
// bridge 要调 openAbout，反向 import 会形成模块环。弹窗组件只读 `about`。
import { getVersion } from '@tauri-apps/api/app';
import { reactive } from 'vue';
import pkg from '../package.json';

/** 拿不到 Tauri 版本时的兜底：浏览器里直接开 dev server 的场景。 */
const FALLBACK_VERSION = typeof pkg.version === 'string' ? pkg.version : '';

export const about = reactive({
  /** 弹窗是否展开。 */
  open: false,
  /** 当前版本，优先用打包进二进制里的那份。 */
  version: FALLBACK_VERSION,
});

let loading: Promise<void> | null = null;

/** 取版本号，只请求一次；失败保持兜底值，不打扰用户。 */
function loadVersion(): Promise<void> {
  loading ??= getVersion()
    .then((version) => {
      about.version = version;
    })
    .catch(() => {});
  return loading;
}

export function openAbout(): void {
  about.open = true;
  void loadVersion();
}

export function closeAbout(): void {
  about.open = false;
}

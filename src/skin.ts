// 外观皮肤。与 theme.js 同构、但互不依赖：
//   data-theme  管明暗（light / dark，跟随系统）
//   data-skin   管风格（macos / hyperos，用户显式选择）
// 两者正交，CSS 里各自一套变量，组合出四种外观。
//
// 默认是 macos —— 也就是改造前的观感，没选过的用户升级后视觉不变。
// 纯逻辑，Node 可直接测；只有 apply() 会碰 document。

const STORAGE_KEY = 'mdview-skin';

export type SkinName = 'hyperos' | 'macos';

export const DEFAULT_SKIN: SkinName = 'macos';

/** 菜单里按这个顺序排，和设计稿的 A / B 两套对应。 */
export const SKINS: readonly SkinName[] = ['hyperos', 'macos'];

export const SKIN_LABELS: Record<SkinName, string> = {
  hyperos: 'HyperOS 4.0',
  macos: 'macOS · Mica',
};

function isSkin(value: unknown): value is SkinName {
  return value === 'hyperos' || value === 'macos';
}

/**
 * 决定当前生效皮肤。
 * 只认白名单里的值，其余（含 null / 脏数据）一律回落到默认，
 * 避免 localStorage 被手改成乱码后界面变成无皮肤状态。
 * @param stored localStorage 里的值
 */
export function resolve(stored: string | null | undefined): SkinName {
  return isSkin(stored) ? stored : DEFAULT_SKIN;
}

function readStore(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function writeStore(value: SkinName): void {
  try {
    localStorage.setItem(STORAGE_KEY, value);
  } catch {
    /* 隐私模式下写不进去，忽略：本次会话内仍然生效 */
  }
}

/** 把皮肤写到 <html data-skin>，触发 CSS 变量切换。 */
export function apply(skin: SkinName): void {
  if (typeof document !== 'undefined') {
    document.documentElement.setAttribute('data-skin', skin);
  }
}

/** 启动时调用：读偏好 → 应用，返回生效皮肤。 */
export function init(): SkinName {
  const skin = resolve(readStore());
  apply(skin);
  return skin;
}

/** 当前选择的皮肤。 */
export function mode(): SkinName {
  return resolve(readStore());
}

/** 选择皮肤：记住 → 应用。非法值原样返回当前皮肤，不写脏数据。 */
export function set(skin: SkinName): SkinName {
  if (!isSkin(skin)) return mode();
  writeStore(skin);
  apply(skin);
  return skin;
}

/** 在两套之间来回切。 */
export function toggle(): SkinName {
  return set(mode() === 'hyperos' ? 'macos' : 'hyperos');
}

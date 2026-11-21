// 主题切换。默认跟随系统 prefers-color-scheme，用户手动切一次后记住选择
// （localStorage），并可用「跟随系统」清除覆盖。纯逻辑，Node 可直接测。
'use strict';

const STORAGE_KEY = 'mdview-theme';

  /** 从系统偏好取默认主题：深色返回 'dark'，否则 'light'。 */
  function systemTheme() {
    if (typeof window !== 'undefined' && window.matchMedia &&
        window.matchMedia('(prefers-color-scheme: dark)').matches) {
      return 'dark';
    }
    return 'light';
  }

  /**
   * 决定当前生效主题。
   * @param {string|null} stored localStorage 里的值（'light'/'dark'/null）
   * @param {string} sys 系统主题
   * @returns {string} 'light' 或 'dark'
   */
  function resolve(stored, sys) {
    if (stored === 'light' || stored === 'dark') return stored;
    return sys === 'dark' ? 'dark' : 'light';
  }

  /**
   * 点主题按钮后的下一个存储值：在浅/深之间循环切换。
   * 从「跟随系统」（null）点一下，先切到与当前相反的那一档。
   * @param {string} current 当前生效主题
   * @returns {string} 新的存储值
   */
  function toggleNext(current) {
    return current === 'dark' ? 'light' : 'dark';
  }

  function readStore() {
    try {
      return localStorage.getItem(STORAGE_KEY);
    } catch (_) {
      return null;
    }
  }

  function writeStore(value) {
    try {
      if (value === null) localStorage.removeItem(STORAGE_KEY);
      else localStorage.setItem(STORAGE_KEY, value);
    } catch (_) { /* 隐私模式下写不进去，忽略 */ }
  }

  /** 把生效主题应用到 <html data-theme>，触发 CSS 变量切换。 */
  function apply(theme) {
    const el = document.documentElement;
    el.setAttribute('data-theme', theme);
  }

  /** 启动时调用：读偏好 → 解析 → 应用，返回生效主题。 */
  function init() {
    const theme = resolve(readStore(), systemTheme());
    apply(theme);
    return theme;
  }

  /** 偏好模式：无覆盖是 'system'，否则是手动选中的 'light' / 'dark'。 */
  function modeOf(stored) {
    return stored === 'light' || stored === 'dark' ? stored : 'system';
  }

  function mode() {
    return modeOf(readStore());
  }

  /** 点击主题按钮：切到相反档 → 记住 → 应用，返回新主题。 */
  function toggle() {
    const current = resolve(readStore(), systemTheme());
    const next = toggleNext(current);
    writeStore(next);
    apply(next);
    return next;
  }

  /** 恢复跟随系统：清掉偏好 → 按系统重新应用，返回生效主题。 */
  function followSystem() {
    writeStore(null);
    const theme = resolve(null, systemTheme());
    apply(theme);
    return theme;
  }

  /** 菜单里的三档选择：system 清掉覆盖，light/dark 记住并应用。 */
  function set(value) {
    if (value === 'system') return followSystem();
    if (value === 'light' || value === 'dark') {
      writeStore(value);
      apply(value);
      return value;
    }
    return resolve(readStore(), systemTheme());
  }

export {
  systemTheme,
  resolve,
  toggleNext,
  modeOf,
  mode,
  set,
  init,
  toggle,
  followSystem,
};

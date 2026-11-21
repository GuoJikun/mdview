// 主题解析：纯函数，不碰 DOM（localStorage 的读写由调用方负责），跑 node 环境。
import { describe, expect, it } from 'vitest';
import { modeOf, resolve, toggleNext } from './theme.js';

describe('resolve：偏好与系统主题合成生效主题', () => {
  it('无偏好 + 系统深色 → 深色', () => {
    expect(resolve(null, 'dark')).toBe('dark');
  });

  it('无偏好 + 系统浅色 → 浅色', () => {
    expect(resolve(null, 'light')).toBe('light');
  });

  it('手动浅色覆盖系统深色', () => {
    expect(resolve('light', 'dark')).toBe('light');
  });

  it('手动深色覆盖系统浅色', () => {
    expect(resolve('dark', 'light')).toBe('dark');
  });

  it('非法存储值回落系统深色', () => {
    expect(resolve('purple', 'dark')).toBe('dark');
  });
});

describe('toggleNext：手动切换取反', () => {
  it('深色切到浅色', () => {
    expect(toggleNext('dark')).toBe('light');
  });

  it('浅色切到深色', () => {
    expect(toggleNext('light')).toBe('dark');
  });
});

describe('modeOf：菜单三档的勾选依据', () => {
  it('无偏好是跟随系统', () => {
    expect(modeOf(null)).toBe('system');
  });

  it('亮色偏好是亮色', () => {
    expect(modeOf('light')).toBe('light');
  });

  it('暗色偏好是暗色', () => {
    expect(modeOf('dark')).toBe('dark');
  });

  it('非法偏好回落跟随系统', () => {
    expect(modeOf('purple')).toBe('system');
  });
});

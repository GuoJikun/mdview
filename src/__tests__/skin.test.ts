// 皮肤是纯逻辑（除 apply 写一个属性），放在 unit project 跑 node，不需要浏览器。
// 用一张内存表冒充 localStorage：Node 环境没有它，而 set/toggle 的语义
// 恰恰是「记住选择」，不验证持久化就等于没测。
import { beforeEach, describe, expect, it } from 'vitest';
import * as skin from '../skin';

const table = new Map<string, string>();

beforeEach(() => {
  table.clear();
  (globalThis as unknown as { localStorage: Storage }).localStorage = {
    getItem: (k: string) => (table.has(k) ? table.get(k)! : null),
    setItem: (k: string, v: string) => void table.set(k, v),
    removeItem: (k: string) => void table.delete(k),
    clear: () => table.clear(),
    key: (i: number) => Array.from(table.keys())[i] ?? null,
    get length() {
      return table.size;
    },
  } as Storage;
});

describe('skin.resolve', () => {
  it('只认白名单里的两个值', () => {
    expect(skin.resolve('hyperos')).toBe('hyperos');
    expect(skin.resolve('macos')).toBe('macos');
  });

  it('空值与脏数据回落到默认皮肤', () => {
    expect(skin.resolve(null)).toBe(skin.DEFAULT_SKIN);
    expect(skin.resolve('')).toBe(skin.DEFAULT_SKIN);
    expect(skin.resolve('solarized')).toBe(skin.DEFAULT_SKIN);
    // 大小写不同也算脏数据：属性值是逐字比对的
    expect(skin.resolve('HyperOS')).toBe(skin.DEFAULT_SKIN);
  });

  it('默认皮肤是 macOS，升级后观感不变', () => {
    expect(skin.DEFAULT_SKIN).toBe('macos');
  });
});

describe('skin.set / toggle', () => {
  it('没选过时是默认皮肤', () => {
    expect(skin.mode()).toBe('macos');
  });

  it('选择后记住，重新读还是它', () => {
    skin.set('hyperos');
    expect(table.get('mdview-skin')).toBe('hyperos');
    expect(skin.mode()).toBe('hyperos');
  });

  it('在两套之间来回切', () => {
    skin.set('macos');
    expect(skin.toggle()).toBe('hyperos');
    expect(skin.toggle()).toBe('macos');
    expect(skin.mode()).toBe('macos');
  });

  it('非法值不写入，维持当前皮肤', () => {
    skin.set('hyperos');
    // @ts-expect-error 故意传非法值，验证运行期守门
    expect(skin.set('vista')).toBe('hyperos');
    expect(table.get('mdview-skin')).toBe('hyperos');
  });
});

describe('skin.apply', () => {
  it('node 环境下没有 document 也不抛错', () => {
    expect(() => skin.apply('hyperos')).not.toThrow();
  });
});

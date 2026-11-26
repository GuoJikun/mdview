// 偏好设置是纯逻辑（apply 只写 CSS 变量，在 node 里跳过），放 unit project。
// 重点测「localStorage 里的东西一律不可信」：脏数据必须回落默认值，
// 而不是让界面带着越界字号或未知字体渲染。
import { beforeEach, describe, expect, it } from 'vitest';

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

/** 每个用例重新 import，避开模块级 reactive 的状态残留。 */
async function fresh() {
  const mod = await import('../prefs');
  return mod;
}

describe('prefs.sanitize', () => {
  it('完全空输入回落默认值', async () => {
    const { sanitize, DEFAULTS } = await fresh();
    expect(sanitize(null)).toEqual(DEFAULTS);
    expect(sanitize(undefined)).toEqual(DEFAULTS);
    expect(sanitize({})).toEqual(DEFAULTS);
    expect(sanitize('nonsense')).toEqual(DEFAULTS);
  });

  it('字号与宽度越界时夹到边界，而不是照单全收', async () => {
    const { sanitize, LIMITS, DEFAULTS } = await fresh();
    expect(sanitize({ fontSize: 400 }).fontSize).toBe(LIMITS.fontSize.max);
    expect(sanitize({ fontSize: 1 }).fontSize).toBe(LIMITS.fontSize.min);
    expect(sanitize({ width: 99 }).width).toBe(LIMITS.width.min);
    expect(sanitize({ width: 99999 }).width).toBe(LIMITS.width.max);
    // 非数字回落默认值（不是夹紧到 min）
    expect(sanitize({ fontSize: 'big' }).fontSize).toBe(DEFAULTS.fontSize);
  });

  it('未知枚举值回落默认，未知字段丢弃', async () => {
    const { sanitize } = await fresh();
    const got = sanitize({ font: 'comic-sans', startupView: 'preview', extra: 1 });
    expect(got.font).toBe('system');
    expect(got.startupView).toBe('wysiwyg');
    expect('extra' in got).toBe(false);
  });

  it('合法值原样保留', async () => {
    const { sanitize } = await fresh();
    const got = sanitize({
      font: 'serif',
      fontSize: 16,
      width: 900,
      indent: 4,
      startupView: 'reading',
    });
    expect(got).toEqual({
      font: 'serif',
      fontSize: 16,
      width: 900,
      indent: 4,
      startupView: 'reading',
    });
  });

  it('缩进宽度夹在边界内（列表缩进另按标记宽度算，不受它影响）', async () => {
    const { sanitize, LIMITS } = await fresh();
    expect(sanitize({ indent: 3 }).indent).toBe(3);
    expect(sanitize({ indent: 0 }).indent).toBe(LIMITS.indent.min);
    expect(sanitize({ indent: 100 }).indent).toBe(LIMITS.indent.max);
  });
});

describe('prefs 读写', () => {
  it('load 解析坏 JSON 时回落默认值，不抛异常', async () => {
    table.set('mdview-prefs', '{ this is not json');
    const { load, DEFAULTS } = await fresh();
    expect(load()).toEqual(DEFAULTS);
  });

  it('set 会记住并夹紧，load 再读回来是同一份', async () => {
    const { set, load } = await fresh();
    set({ fontSize: 18, font: 'mono' });
    expect(load().fontSize).toBe(18);
    expect(load().font).toBe('mono');
    // 越界输入被收敛后再存
    set({ width: 5000 });
    expect(load().width).toBe(1200);
  });

  it('reset 回到默认值', async () => {
    const { set, reset, load, DEFAULTS } = await fresh();
    set({ fontSize: 20, indent: 8 });
    reset();
    expect(load()).toEqual(DEFAULTS);
  });
});

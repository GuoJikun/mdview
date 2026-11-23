// 查找替换的纯逻辑用例：UTF-16 → UTF-8 映射、命中列表、批量替换。
// 不碰 DOM，跑 unit project（node 环境）。
import { describe, expect, it } from 'vitest';
import { escapeRegExp, findAll, replaceAllText, utf8Offsets } from '../find.js';

describe('utf8Offsets', () => {
  it('中文每字 3 字节', () => {
    const map = utf8Offsets('甲乙');
    expect(Array.from(map)).toEqual([0, 3, 6]);
  });

  it('ASCII 每字 1 字节', () => {
    expect(Array.from(utf8Offsets('abc'))).toEqual([0, 1, 2, 3]);
  });

  it('代理对按 4 字节计，两个 code unit 映射到同一起点', () => {
    // 😀 = U+1F600，UTF-16 里占两个 code unit
    const map = utf8Offsets('a😀b');
    expect(map.length).toBe(5);
    expect(map[0]).toBe(0);
    expect(map[1]).toBe(1);
    expect(map[2]).toBe(1); // 低位代理仍指向同一字符起点
    expect(map[3]).toBe(5);
    expect(map[4]).toBe(6);
  });
});

describe('findAll', () => {
  const src = 'Alpha beta ALPHA 中文中文';

  it('默认大小写不敏感', () => {
    expect(findAll(src, 'alpha')).toEqual([
      { start: 0, end: 5 },
      { start: 11, end: 16 },
    ]);
  });

  it('区分大小写时只命中原样', () => {
    expect(findAll(src, 'ALPHA', true)).toEqual([{ start: 11, end: 16 }]);
  });

  it('中文按字符匹配且互不重叠', () => {
    expect(findAll(src, '中文')).toEqual([
      { start: 17, end: 19 },
      { start: 19, end: 21 },
    ]);
  });

  it('空查询不产生命中', () => {
    expect(findAll(src, '')).toEqual([]);
  });

  it('正则元字符按字面量处理', () => {
    expect(findAll('a.b **c** (d)', '**c**')).toEqual([{ start: 4, end: 9 }]);
    expect(findAll('a.b', '.')).toEqual([{ start: 1, end: 2 }]);
  });
});

describe('replaceAllText', () => {
  it('从后往前拼，长度变化不影响前面的命中', () => {
    const src = '甲甲甲';
    const hits = findAll(src, '甲');
    expect(replaceAllText(src, hits, '乙乙')).toBe('乙乙乙乙乙乙');
  });

  it('替换文本可以更长或更短', () => {
    expect(replaceAllText('aXbXc', findAll('aXbXc', 'X'), '')).toBe('abc');
  });

  it('没有命中时原样返回', () => {
    expect(replaceAllText('abc', [], 'x')).toBe('abc');
  });
});

describe('escapeRegExp', () => {
  it('转义所有元字符', () => {
    expect(escapeRegExp('a.b*c(d)[e]{f}^$|\\')).toBe(
      'a\\.b\\*c\\(d\\)\\[e\\]\\{f\\}\\^\\$\\|\\\\',
    );
  });
});

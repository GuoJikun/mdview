// 查找替换的纯逻辑用例：UTF-16 → UTF-8 映射、命中列表、批量替换。
// 不碰 DOM，跑 unit project（node 环境）。
import { describe, expect, it } from 'vitest';
import {
  buildMatcher,
  escapeRegExp,
  findAll,
  findMatches,
  matchCase,
  replaceAllText,
  replaceMatches,
  replacementFor,
  utf8Offsets,
} from '../find.js';

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

describe('全词与正则', () => {
  it('全词匹配不会命中子串', () => {
    const { re } = buildMatcher('port', { wholeWord: true });
    const hits = findMatches('port transport viewport port', { re });
    expect(hits).toEqual([
      { start: 0, end: 4 },
      { start: 24, end: 28 },
    ]);
  });

  it('全词匹配对中文按字边界处理', () => {
    const { re } = buildMatcher('标题', { wholeWord: true });
    expect(findMatches('加标题了 标题 副标题', { re })).toEqual([
      { start: 5, end: 7 },
    ]);
  });

  it('正则模式按模式匹配', () => {
    const { re } = buildMatcher('\\d+', { regex: true });
    expect(findMatches('甲 12 乙 345', { re })).toEqual([
      { start: 2, end: 4 },
      { start: 7, end: 10 },
    ]);
  });

  it('正则写错时返回 error 而不是抛异常', () => {
    expect(buildMatcher('([', { regex: true })).toEqual({ error: true });
  });

  it('未开正则时元字符按字面量处理', () => {
    const { re } = buildMatcher('a.b');
    expect(findMatches('a.b axb', { re })).toEqual([{ start: 0, end: 3 }]);
  });

  it('正则模式的 ^ / $ 按行锚定', () => {
    const src = '# 一\n\n## 二\n\n## 三\n';
    const { re } = buildMatcher('^## ', { regex: true });
    expect(findMatches(src, { re })).toEqual([
      { start: 5, end: 8 },
      { start: 11, end: 14 },
    ]);
    // 行尾锚点同理：三行都以换行结束
    const tail = buildMatcher('。$', { regex: true });
    expect(findMatches('甲。\n乙。\n', { re: tail.re })).toEqual([
      { start: 1, end: 2 },
      { start: 4, end: 5 },
    ]);
  });

  it('未开正则时 ^ 只是字面量', () => {
    const { re } = buildMatcher('^a');
    expect(findMatches('a ^a', { re })).toEqual([{ start: 2, end: 4 }]);
  });
});

describe('替换展开与保留大小写', () => {
  const src = 'name: alice, name: bob';

  it('正则替换展开捕获组', () => {
    const { re } = buildMatcher('name: (\\w+)', { regex: true });
    const hits = findMatches(src, { re });
    expect(replaceMatches(src, hits, '$1', { matcher: { re } }))
      .toBe('alice, bob');
  });

  it('非正则替换把 $1 当普通文本', () => {
    expect(replacementFor(src, { start: 0, end: 4 }, '$1', {})).toBe('$1');
  });

  it('保留大小写：全大写跟着大写', () => {
    expect(matchCase('new', 'OLD')).toBe('NEW');
  });

  it('保留大小写：全小写跟着小写', () => {
    expect(matchCase('NEW', 'old')).toBe('new');
  });

  it('保留大小写：首字母大写沿用替换词的首字母大写', () => {
    expect(matchCase('new', 'Old')).toBe('New');
  });

  it('保留大小写不影响中文', () => {
    expect(matchCase('新词', '旧词')).toBe('新词');
  });

  it('命中里没有大小写概念时，替换文本原样保留', () => {
    // 数字和 CJK 的 upper/lower 与自身相同，之前会被当成「全小写」而强行小写化
    expect(matchCase('Hello', '123')).toBe('Hello');
    expect(matchCase('Hello', '中文')).toBe('Hello');
    expect(matchCase('Hello', '1,000')).toBe('Hello');
  });

  it('带上下文断言的正则也能替换', () => {
    const src = 'foobar foobar';
    const { re } = buildMatcher('foo(?=bar)', { regex: true });
    expect(replaceMatches(src, findMatches(src, { re }), 'X', { matcher: { re } })).toBe(
      'Xbar Xbar',
    );
  });

  it('后顾断言取前文而不是脱离上下文重放', () => {
    const src = '## 标题';
    const { re } = buildMatcher('(?<=## )标题', { regex: true });
    expect(replacementFor(src, { start: 5, end: 7 }, '小标题', { matcher: { re } })).toBe(
      '小标题',
    );
  });

  it('$& / $` / $\' / $$ 按规范展开', () => {
    const src = 'abc';
    const { re } = buildMatcher('b', { regex: true });
    const hit = { start: 1, end: 2 };
    expect(replacementFor(src, hit, '[$&]', { matcher: { re } })).toBe('[b]');
    expect(replacementFor(src, hit, '<$`|$\'>', { matcher: { re } })).toBe('<a|c>');
    expect(replacementFor(src, hit, '$$', { matcher: { re } })).toBe('$');
  });

  it('$nn 没有两位分组时退一位', () => {
    const src = 'abc';
    const { re } = buildMatcher('(a)', { regex: true });
    expect(replacementFor(src, { start: 0, end: 1 }, '$12', { matcher: { re } })).toBe('a2');
    // $0 不指向任何分组
    expect(replacementFor(src, { start: 0, end: 1 }, '$0', { matcher: { re } })).toBe('$0');
  });
});

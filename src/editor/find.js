// 查找替换的纯逻辑：不碰 DOM，Node 环境直接测。
//
// 搜索对象是 Markdown **源码**（`sourceText`），因为视图里的 `**`、`# ` 是折叠的，
// 按可见文本搜会漏掉「搜标记本身」这类需求；但命中之后要交给内核的 `select`
// 消息（byte 区间）才能画选区，所以这里同时提供 UTF-16 下标 → UTF-8 字节的映射。
//
// 只算下标、不算停靠点：停靠点属于 DOM，映射由 editor.ts 用 data-b / data-a 完成。

/**
 * UTF-16 下标 → UTF-8 字节偏移。返回值长度是 `text.length + 1`。
 *
 * 代理对（emoji 等）的两个 code unit 都映射到同一个起始字节，pair 之后的
 * 下标才前进 4 字节——`String.prototype.slice` 用的正是这套下标。
 */
export function utf8Offsets(text) {
  const map = new Uint32Array(text.length + 1);
  let byte = 0;
  for (let i = 0; i < text.length; ) {
    const cp = text.codePointAt(i);
    const units = cp > 0xffff ? 2 : 1;
    map[i] = byte;
    if (units === 2) map[i + 1] = byte;
    byte += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
    i += units;
    map[i] = byte;
  }
  return map;
}

/** 正则元字符转义。用户输入的就是字面量，不该被当成模式。 */
export function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** 全词匹配的边界：中英文都按「字」算，避免 `\b` 把汉字判成非单词字符。 */
const WORD = '[A-Za-z0-9_\\u4e00-\\u9fff]';

/**
 * 按开关编译出匹配器。
 *
 * 返回 `{ re }`、`{ re: null }`（空查询）或 `{ error: true }`（正则写错）。
 * 把「编译失败」当成一种结果而不是抛异常，UI 才能把「无效正则」显示在计数位。
 */
export function buildMatcher(query, options = {}) {
  if (!query) return { re: null };
  const body = options.regex ? query : escapeRegExp(query);
  const source = options.wholeWord
    ? `(?<!${WORD})(?:${body})(?!${WORD})`
    : body;
  try {
    // 正则模式下补 `m`：查找对象是整篇源码，`^` / `$` 应当匹配每一行的首尾，
    // 否则「搜 `^## `」只在文档第一个字符命中，行首锚点基本没法用。
    const flags = options.caseSensitive ? 'g' : 'gi';
    return { re: new RegExp(source, options.regex ? `${flags}m` : flags) };
  } catch {
    return { error: true };
  }
}

/** 用编译好的匹配器扫一遍。`re` 必须是带 `g` 的。 */
export function findMatches(text, matcher) {
  if (!matcher || !matcher.re) return [];
  matcher.re.lastIndex = 0;
  const out = [];
  let hit;
  while ((hit = matcher.re.exec(text)) !== null) {
    out.push({ start: hit.index, end: hit.index + hit[0].length });
    // 空匹配理论上不会出现（查询非空），但防御一下防止死循环
    if (hit[0].length === 0) matcher.re.lastIndex += 1;
  }
  return out;
}

/**
 * 单体命中的替换文本。
 *
 * 正则模式下按 ECMAScript 的 `$` 语义展开捕获组，与 VSCode 一致。
 *
 * 注意不能拿「孤立的命中文本」重放正则：`foo(?=bar)`、`(?<=# )title` 这类带
 * 上下文断言的模式脱离前后文必然失配，`String.replace` 会原样返回，替换就成了
 * 静默的空操作。所以这里在原文上从命中位置重新 exec 一次，拿到分组数组再手工展开。
 */
export function replacementFor(text, match, replacement, options = {}) {
  const matched = text.slice(match.start, match.end);
  let piece = replacement;
  if (options.matcher && options.matcher.re) {
    const hit = execAt(text, options.matcher.re, match.start);
    if (hit) piece = expandReplacement(replacement, text, hit);
  }
  if (options.preserveCase) piece = matchCase(piece, matched);
  return piece;
}

/** 在原文的指定位置重跑一次匹配，拿到带捕获组的 `exec` 结果。 */
function execAt(text, re, index) {
  re.lastIndex = index;
  const hit = re.exec(text);
  return hit && hit.index === index ? hit : null;
}

/**
 * 展开替换模板里的 `$$`、`$&`、`` $` ``、`$'`、`$1..$99`、`$<name>`。
 *
 * 照规范的边界处理：`$0` 不指向任何分组（原样保留），`$nn` 没有两位分组时
 * 退一位（`$12` 在只有 1 个分组时展开成「分组 1 + '2'」）。
 */
function expandReplacement(template, text, hit) {
  const matched = hit[0];
  const groups = hit.groups ?? {};
  return template.replace(/\$(\$|&|`|'|<[^>]*>|\d{1,2})/g, (whole, token) => {
    if (token === '$') return '$';
    if (token === '&') return matched;
    if (token === '`') return text.slice(0, hit.index);
    if (token === "'") return text.slice(hit.index + matched.length);
    if (token[0] === '<') {
      const name = token.slice(1, -1);
      return name in groups ? groups[name] : whole;
    }
    const n = Number(token);
    if (!n) return whole;
    const group = hit[n];
    if (group !== undefined) return group;
    if (token.length === 2) {
      const first = hit[Number(token[0])];
      if (first !== undefined) return first + token[1];
    }
    return whole;
  });
}

/** 保留大小写：全大写/全小写跟着命中走，首字母大写沿用替换文本的「首字母大写」。 */
export function matchCase(replacement, matched) {
  if (!replacement) return replacement;
  const upper = matched.toUpperCase();
  const lower = matched.toLowerCase();
  // 数字、CJK、符号没有大小写概念，upper 与 lower 相同，替换文本原样保留
  if (upper === lower) return replacement;
  if (matched === upper) return replacement.toUpperCase();
  if (matched === lower) return replacement.toLowerCase();
  if (matched[0] !== matched[0].toLowerCase()) {
    return replacement[0].toUpperCase() + replacement.slice(1);
  }
  return replacement;
}

/** 按命中列表替换。每个命中各自展开捕获组、各自套保留大小写。 */
export function replaceMatches(text, matches, replacement, options = {}) {
  let out = '';
  let last = 0;
  for (const hit of matches) {
    out += text.slice(last, hit.start);
    out += replacementFor(text, hit, replacement, options);
    last = hit.end;
  }
  return out + text.slice(last);
}

/**
 * 找出全部命中，返回 `{ start, end }`（UTF-16 下标，左闭右开），按位置升序。
 *
 * 用正则而不是 `indexOf` 循环：大小写不敏感的比较交给引擎，避免自己
 * `toLowerCase()` 之后长度变化导致下标错位（例如 `İ`）。
 */
export function findAll(text, query, caseSensitive = false) {
  const { re } = buildMatcher(query, { caseSensitive });
  return re ? findMatches(text, { re }) : [];
}

/**
 * 按命中列表一次性替换。从后往前拼，前面的下标就不会被替换文本的长度影响。
 * 传入的 `matches` 必须已按位置升序且互不重叠。
 */
export function replaceAllText(text, matches, replacement) {
  let out = '';
  let last = 0;
  for (const hit of matches) {
    out += text.slice(last, hit.start) + replacement;
    last = hit.end;
  }
  return out + text.slice(last);
}

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

/**
 * 找出全部命中，返回 `{ start, end }`（UTF-16 下标，左闭右开），按位置升序。
 *
 * 用正则而不是 `indexOf` 循环：大小写不敏感的比较交给引擎，避免自己
 * `toLowerCase()` 之后长度变化导致下标错位（例如 `İ`）。
 */
export function findAll(text, query, caseSensitive = false) {
  if (!query) return [];
  const re = new RegExp(escapeRegExp(query), caseSensitive ? 'g' : 'gi');
  const out = [];
  let hit;
  while ((hit = re.exec(text)) !== null) {
    out.push({ start: hit.index, end: hit.index + hit[0].length });
    // 空匹配理论上不会出现（query 非空），但防御一下防止死循环
    if (hit[0].length === 0) re.lastIndex += 1;
  }
  return out;
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

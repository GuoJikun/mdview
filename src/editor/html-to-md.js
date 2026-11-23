/* HTML → Markdown（粘贴用）。
 *
 * 只做「浏览器 / Word / 飞书复制出来的富文本」到 Markdown 的常规映射：
 * 标题、段落、列表、引用、代码块、表格、链接、图片、粗斜体、删除线、分隔线。
 * 不认识的行内标签保留其中的文字，脚本 / 样式 / 表单控件整段丢掉。
 *
 * 不是通用 HTML 清洗器：输出只由本文件的模板拼出来，属性不会原样透传，
 * 所以不存在把 `onclick` 之类带进文档的路径。链接与图片地址仍过一遍协议白名单。
 *
 * 依赖 `DOMParser`，只能在浏览器/WebView 里跑；用例放在 dom project。
 */

const BLOCK_TAGS = new Set([
  'ADDRESS', 'ARTICLE', 'ASIDE', 'BLOCKQUOTE', 'DD', 'DIV', 'DL', 'DT',
  'FIELDSET', 'FIGCAPTION', 'FIGURE', 'FOOTER', 'FORM', 'H1', 'H2', 'H3',
  'H4', 'H5', 'H6', 'HEADER', 'HR', 'LI', 'MAIN', 'NAV', 'OL', 'P', 'PRE',
  'SECTION', 'TABLE', 'UL',
]);

const SKIP_TAGS = new Set([
  'AUDIO', 'BUTTON', 'CANVAS', 'EMBED', 'HEAD', 'IFRAME', 'INPUT', 'LINK',
  'META', 'NOSCRIPT', 'OBJECT', 'SCRIPT', 'SELECT', 'STYLE', 'SVG',
  'TEMPLATE', 'TEXTAREA', 'VIDEO',
]);

const HEADINGS = { H1: 1, H2: 2, H3: 3, H4: 4, H5: 5, H6: 6 };

/** 把一段 HTML 片段转成 Markdown；空输入或无有效内容返回空串。 */
export function htmlToMarkdown(html) {
  if (typeof html !== 'string' || !html.trim()) return '';
  const doc = new DOMParser().parseFromString(html, 'text/html');
  const out = blocks(doc.body, 0).join('\n\n').trim();
  return out ? out : '';
}

/**
 * 判断一段 HTML 是否值得走转换。
 *
 * 从 VSCode / 终端复制纯文本时，剪贴板里也带一份 `<div>` 包起来的 HTML——
 * 按 HTML 转会把每一行拆成段落、把缩进吃掉。只有出现真正的结构标签才转换；
 * 没有纯文本兜底时才无条件转。
 */
export function looksLikeRichHtml(html, plainText) {
  if (!html) return false;
  if (!plainText) return true;
  return /<\/?(h[1-6]|p|ul|ol|table|blockquote|pre|strong|b|em|i|a|img|code|del|s)\b/i.test(
    html,
  );
}

function blocks(node, depth) {
  const out = [];
  let inline = '';
  const flush = () => {
    const text = normalizeParagraph(inline);
    if (text) out.push(text);
    inline = '';
  };

  for (const child of node.childNodes) {
    if (child.nodeType === Node.TEXT_NODE) {
      inline += escapeText(collapse(child.nodeValue ?? ''));
      continue;
    }
    if (child.nodeType !== Node.ELEMENT_NODE) continue;
    const tag = child.tagName;
    if (SKIP_TAGS.has(tag)) continue;
    if (tag === 'BR') {
      inline += '\\\n';
      continue;
    }
    if (BLOCK_TAGS.has(tag)) {
      flush();
      out.push(...blockOf(child, depth));
    } else {
      inline += inlineOf(child);
    }
  }
  flush();
  return out.filter(Boolean);
}

function blockOf(el, depth) {
  const tag = el.tagName;
  if (HEADINGS[tag]) {
    const text = normalizeParagraph(inlineOfChildren(el));
    return text ? [`${'#'.repeat(HEADINGS[tag])} ${text}`] : [];
  }
  if (tag === 'HR') return ['---'];
  if (tag === 'PRE') {
    const code = codeBlock(el);
    return code ? [code] : [];
  }
  if (tag === 'BLOCKQUOTE') {
    const inner = blocks(el, depth);
    if (!inner.length) return [];
    const quoted = inner
      .join('\n\n')
      .split('\n')
      .map((line) => (line ? `> ${line}` : '>'))
      .join('\n');
    return [quoted];
  }
  if (tag === 'UL' || tag === 'OL') return [listBlock(el, tag === 'OL', depth)];
  if (tag === 'TABLE') {
    const table = tableBlock(el);
    return table ? [table] : [];
  }
  if (tag === 'DL') return [definitionList(el)];
  // DIV / SECTION / P / LI 等容器：按内部块继续拆
  return blocks(el, depth);
}

function inlineOf(el) {
  const tag = el.tagName;
  if (HEADINGS[tag] || (BLOCK_TAGS.has(tag) && tag !== 'LI')) {
    // 行内位置遇到块级元素（浏览器修复 HTML 时可能出现）：退化成文字
    return blocks(el, 0).join(' ');
  }
  if (tag === 'BR') return '\\\n';
  if (tag === 'STRONG' || tag === 'B') return wrap('**', inlineOfChildren(el));
  if (tag === 'EM' || tag === 'I') return wrap('*', inlineOfChildren(el));
  if (tag === 'DEL' || tag === 'S' || tag === 'STRIKE') {
    return wrap('~~', inlineOfChildren(el));
  }
  if (tag === 'CODE') return codeSpan(el.textContent ?? '');
  if (tag === 'A') {
    const href = safeUrl(el.getAttribute('href') ?? '');
    const text = normalizeInline(inlineOfChildren(el));
    if (!href) return text;
    return `[${text || escapeText(href)}](${escapeDestination(href)})`;
  }
  if (tag === 'IMG') {
    const src = safeUrl(el.getAttribute('src') ?? '', true);
    if (!src) return '';
    const alt = escapeText(el.getAttribute('alt') ?? '');
    return `![${alt}](${escapeDestination(src)})`;
  }
  return inlineOfChildren(el);
}

function inlineOfChildren(el) {
  let out = '';
  for (const child of el.childNodes) {
    if (child.nodeType === Node.TEXT_NODE) {
      out += escapeText(collapse(child.nodeValue ?? ''));
    } else if (child.nodeType === Node.ELEMENT_NODE) {
      if (SKIP_TAGS.has(child.tagName)) continue;
      out += inlineOf(child);
    }
  }
  return out;
}

function listBlock(list, ordered, depth) {
  const items = [...list.children].filter((child) => child.tagName === 'LI');
  if (!items.length) return '';
  const start = ordered ? Number(list.getAttribute('start')) || 1 : 1;
  const pad = '  '.repeat(depth);
  return items
    .map((item, index) => {
      const marker = ordered ? `${start + index}. ` : '- ';
      const parts = blocks(item, depth + 1);
      if (!parts.length) return `${pad}${marker}`.trimEnd();
      const [first, ...rest] = parts;
      const lines = first.split('\n');
      const out = [`${pad}${marker}${lines[0]}`];
      for (const line of lines.slice(1)) out.push(`${pad}  ${line}`);
      for (const part of rest) {
        // 嵌套列表在 blocks 里已经按 depth 缩进好了；段落再补空行 + 缩进
        if (isListText(part)) out.push(part);
        else {
          out.push('');
          out.push(`${pad}  ${part.replace(/\n/g, `\n${pad}  `)}`);
        }
      }
      return out.join('\n');
    })
    .join('\n');
}

function isListText(text) {
  return /^\s*(?:[-*+]|\d+\.)\s/.test(text);
}

function tableBlock(table) {
  const rows = [...table.querySelectorAll('tr')].filter(
    (row) => row.closest('table') === table,
  );
  if (!rows.length) return '';
  const cellsOf = (row) => [...row.children].filter((cell) =>
    cell.tagName === 'TH' || cell.tagName === 'TD',
  );
  if (!cellsOf(rows[0]).length) return '';

  const hasHeader = cellsOf(rows[0]).some((cell) => cell.tagName === 'TH');
  const header = hasHeader ? cellsOf(rows[0]) : null;
  const body = hasHeader ? rows.slice(1) : rows;
  const width = Math.max(...rows.map((row) => cellsOf(row).length));

  const aligns = Array.from({ length: width }, (_, index) => {
    const cell = (header ?? cellsOf(rows[0]))[index];
    return cellAlign(cell);
  });

  const line = (cells) => {
    const padded = Array.from({ length: width }, (_, index) => {
      const cell = cells[index];
      const text = cell ? cellText(cell) : '';
      return text.replace(/\|/g, '\\|');
    });
    return `| ${padded.join(' | ')} |`;
  };

  const out = [
    line(header ?? []),
    `| ${aligns.map((align) => align || '---').join(' | ')} |`,
    ...body.map((row) => line(cellsOf(row))),
  ];
  return out.join('\n');
}

function cellText(cell) {
  const text = blocks(cell, 0).join(' ');
  return normalizeParagraph(text).replace(/\n+/g, ' ');
}

function cellAlign(cell) {
  if (!cell) return '';
  const style = (cell.getAttribute('style') ?? '').toLowerCase();
  const align =
    (cell.getAttribute('align') ?? '').toLowerCase() ||
    /text-align\s*:\s*(left|right|center)/.exec(style)?.[1] ||
    '';
  if (align === 'center') return ':---:';
  if (align === 'right') return '---:';
  return '';
}

function definitionList(list) {
  const lines = [];
  for (const child of list.children) {
    if (child.tagName === 'DT') {
      const term = normalizeParagraph(inlineOfChildren(child));
      if (term) lines.push(`**${term}**`);
    } else if (child.tagName === 'DD') {
      const text = normalizeParagraph(blocks(child, 0).join(' '));
      if (text) lines.push(text);
    }
  }
  return lines.join('\n');
}

function codeBlock(pre) {
  const code = pre.querySelector('code') ?? pre;
  const text = (code.textContent ?? '').replace(/\r\n?/g, '\n').replace(/\n+$/, '');
  if (!text) return '';
  const language = languageOf(code);
  const fence = '`'.repeat(Math.max(3, longestRun(text, '`') + 1));
  return `${fence}${language}\n${text}\n${fence}`;
}

function languageOf(code) {
  const match = /(?:^|\s)(?:language|lang)-([\w+#.-]+)/.exec(code.className ?? '');
  return match ? match[1] : '';
}

function codeSpan(text) {
  if (!text) return '';
  const fence = '`'.repeat(Math.max(1, longestRun(text, '`') + 1));
  const padded = text.startsWith('`') || text.endsWith('`') ? ` ${text} ` : text;
  return `${fence}${padded}${fence}`;
}

function longestRun(text, char) {
  let max = 0;
  let run = 0;
  for (const current of text) {
    if (current === char) {
      run += 1;
      max = Math.max(max, run);
    } else {
      run = 0;
    }
  }
  return max;
}

function wrap(marker, text) {
  const inner = normalizeInline(text);
  return inner ? `${marker}${inner}${marker}` : '';
}

function normalizeInline(text) {
  return text.replace(/[ \t]+/g, ' ').replace(/\s*\n\s*/g, '\n').trim();
}

function normalizeParagraph(text) {
  return text
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .replace(/[ \t]{2,}/g, ' ')
    .trim();
}

function collapse(text) {
  return text.replace(/\s+/g, ' ');
}

/** 字面量里的 Markdown 元字符转义，避免粘贴进来的文字被重新解释。 */
function escapeText(text) {
  return text.replace(/([\\`*[\]<>])/g, '\\$1');
}

function escapeDestination(url) {
  const needsAngle = /[\s()<>]/.test(url);
  return needsAngle ? `<${url.replace(/[<>]/g, encodeURIComponent)}>` : url;
}

/**
 * 协议白名单：脚本伪协议一律丢掉；`data:` 只放行常见图片。
 * 相对路径 / 锚点 / mailto / http(s) 原样保留——Markdown 本身允许它们。
 */
function safeUrl(raw, image = false) {
  const url = raw.trim();
  if (!url) return '';
  const lower = url.toLowerCase();
  if (lower.startsWith('javascript:') || lower.startsWith('vbscript:')) return '';
  if (lower.startsWith('data:')) {
    if (!image) return '';
    return /^data:image\/(?:png|jpe?g|gif|webp);/i.test(url) ? url : '';
  }
  return url;
}

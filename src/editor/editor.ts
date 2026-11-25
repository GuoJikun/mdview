// 编辑内核：把「停靠点」映射成像素，把用户操作翻译成消息。
// 文本真值与解析结果都在 Rust 侧，这里不留任何文档状态。
//
// 类型分两层：IPC 契约在 `ipc.ts`（逐一对应 `src-tauri/src/session.rs`），
// 本文件只写「DOM 与几何」的类型。七个纯逻辑模块仍是带 JSDoc 的 .js，
// 由 TS 从注释里推断签名——它们要能被 `node --test` 直接跑，不引入编译期语法。
// 守门：`vue-tsc --noEmit` 管类型，`--self-test` 的 61 项不变量管真实 DOM 行为。
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ThemeMode } from '../bridge';
import { createLineMotion, scrollDelta } from './caretline.js';
import { copyText } from './clipboard.js';
import { htmlToMarkdown, looksLikeRichHtml } from './html-to-md.js';
import { clampToBox, createDragAutoScroll, edgeSpeed } from './dragscroll.js';
import {
  buildMatcher,
  findMatches as scanMatches,
  replaceMatches,
  replacementFor,
  utf8Offsets,
} from './find.js';
import { createIme } from './ime.js';
import type {
  BlockDto,
  Box,
  CaretDto,
  CaretPos,
  Check,
  OutlineDto,
  PatchDto,
  Point,
  Reply,
  Snapshot,
  Wire,
} from './ipc';
import {
  activeIndex as outlineActiveIndex,
  scrollToAnchor as outlineScrollToAnchor,
} from './outline.js';
import { shouldClearSelection } from './selection.js';
import * as mdviewTheme from './theme.js';
// 外观皮肤（macOS / HyperOS）。与 theme.js 正交：一个管明暗，一个管风格。
import * as mdviewSkin from '../skin';

// 这几个节点在 index.html 里是写死的骨架，缺任何一个后续都会立刻炸。
// 断言只影响类型，不改运行时行为（真拿不到时，仍是首次调用处的 TypeError）。
const docEl = document.getElementById('doc') as HTMLElement;
const sourceEl = document.getElementById('source') as HTMLTextAreaElement;
const caretEl = document.getElementById('caret') as HTMLElement;
const preeditEl = document.getElementById('preedit') as HTMLElement;
const imeEl = document.getElementById('ime') as HTMLTextAreaElement;
const barEl = document.getElementById('bar') as HTMLElement;
const outlineEl = document.getElementById('outline') as HTMLElement;
const findEl = document.getElementById('findbar') as HTMLElement;
const findInput = document.getElementById('find-input') as HTMLInputElement;
const findCount = document.getElementById('find-count') as HTMLElement;
const replaceRow = document.getElementById('find-replace-row') as HTMLElement;
const replaceInput = document.getElementById('replace-input') as HTMLInputElement;
const findMenu = document.getElementById('find-menu') as HTMLElement;
const linkEditEl = document.getElementById('linkedit') as HTMLElement;
const linkUrlInput = document.getElementById('link-url') as HTMLInputElement;
const linkTextInput = document.getElementById('link-text') as HTMLInputElement;

/** 与整篇文档有关的标量。前端只读，写入点只有 applySnapshot / applyReply。 */
interface Meta {
  revision: number;
  dirty: boolean;
  path: string | null;
  stops: number;
  /** 正文字数，由内核随快照 / 回包下发。 */
  words: number;
}

let byBefore: Map<number, HTMLElement> = new Map(); // 停靠点 → 从它开始的元素
let byAfter: Map<number, HTMLElement> = new Map(); // 停靠点 → 在它结束的元素
let marks: HTMLElement[] = [];                      // 全部带 data-b 的元素
let maxStop = -1;                                   // 最大停靠点序号，供上下移动取上界
let caret: CaretDto | null = null;                  // 最近一次内核回报的光标
let pendingStop: number | null = null;              // 已发出、尚未回包的垂直移动落点
let meta: Meta = { revision: 0, dirty: false, path: null, stops: 0, words: 0 };
// 当前视图模式。它不只画菜单圆点，还真正驱动 <html data-view> 切 DOM：
// wysiwyg 显示富文本投影；reading 只读；source 显示并编辑 Markdown 源文本。
// 与内核默认值一致；macOS 原生菜单切模式也走 runAction，会同步更新。
window.__mdviewMode = 'wysiwyg';
let sourceText = '';                        // 内核下发的 Markdown 源文本真值（不是 HTML）
let sourceSyncTimer = 0;                    // textarea → 内核的整篇同步去抖
let dragging = false;
let dragPoint: Point | null = null;  // 拖动期间指针的视口坐标（自动滚动要按它反复求落点）
let dragStop: number | null = null;  // 拖动中已发出的落点，避免同一落点每帧都过一趟 IPC
let imeAnchorEl: HTMLElement | null = null; // preedit 复制字号用的参照元素（光标所在的字）
let revealedStop: number | null = null;     // 上一次把光标滚进视口时的停靠点
let outline: OutlineDto[] = [];             // 大纲项 [{ level, title, block }]
let outlineAnchors: { top: number }[] = []; // 与 outline 同序的锚点，文档坐标系
let activeOutlineIdx = -1;                  // 当前高亮的大纲项
// 点击大纲后「钉住」的高亮项：文末标题滚不到基准线，按滚动位置判定会落回
// 上一个标题，所以点击后先按点击项高亮，等用户自己滚动再交还给滚动跟随。
let outlinePinned: number | null = null;
let lastTitleKey: string | null = null;     // 标题栏去重：path + 脏标记
/** 最近一次绘制的选区（停靠点区间）。查找的「仅在选区内」按它换算字节范围。 */
let selectionStops: [number, number] | null = null;

/**
 * 源码 byte → 停靠点。查找命中来自源码文本，选中却要停靠点，
 * 这张表按 byte 升序，起点要求精确命中，终点取「最后一个 byte 小于它的元素」的 data-a。
 * 折叠标记也在表里（带 `hidden` 标记），但命中起点落在标记里时会被跳过。
 */
interface ByteStop {
  byte: number;
  b: number;
  a: number;
  hidden: boolean;
}
let byteStops: ByteStop[] = [];

/** 光标进出视口时上下各留的余量，避免贴着边缘打字。 */
const REVEAL_MARGIN = 32;
/** 拖动时指针进入视口上下多深就开始自动滚动，以及每帧最多滚多少。 */
const DRAG_EDGE = 48;
const DRAG_MAX_SPEED = 24;

// ---- 渲染 ------------------------------------------------------------

function fragments(html: string): Element[] {
  const tpl = document.createElement('template');
  tpl.innerHTML = html;
  return Array.from(tpl.content.children);
}

/** 重建「停靠点 → 元素」索引。任何 DOM 变更之后都必须调用。 */
function reindex() {
  byBefore = new Map();
  byAfter = new Map();
  marks = [];
  maxStop = -1;
  byteStops = [];
  for (const el of docEl.querySelectorAll<HTMLElement>('[data-b]')) {
    const before = Number(el.dataset.b);
    if (!byBefore.has(before)) byBefore.set(before, el);
    const after = el.hasAttribute('data-a') ? Number(el.dataset.a) : before;
    if (el.hasAttribute('data-a')) byAfter.set(after, el);
    if (before > maxStop) maxStop = before;
    if (after > maxStop) maxStop = after;
    marks.push(el);
    // data-byte 是这一格的源码起点；查找命中靠它对齐停靠点。
    const byte = Number(el.dataset.byte);
    if (Number.isFinite(byte)) {
      byteStops.push({ byte, b: before, a: after, hidden: el.classList.contains('mk') });
    }
  }
  byteStops.sort((x, y) => x.byte - y.byte);
}

/**
 * 当前模式该把焦点放到哪个输入元素。源码视图用 textarea 自己编辑，
 * 其余模式仍靠透明的 #ime 承接输入法。统一收口，避免各调用点写死 imeEl。
 */
function focusActiveEditor(): void {
  const target = window.__mdviewMode === 'source' ? sourceEl : imeEl;
  if (document.activeElement !== target) target.focus({ preventScroll: true });
}

/**
 * 比较 textarea 的值与内核源文本是否等价。
 *
 * textarea.value 会把 CRLF 规范化成 LF（HTML 规范），而内核 read_to_string
 * 原样保留文件的 \r\n。直接 === 比较会让 CRLF 文档在「只是切了个视图」
 * 时被误判成用户编辑，发出 replaceSource 把全文换行改写并置脏。
 * 比较时统一归一成 LF；写回内核的文本也用 textarea 的值（本就是 LF）。
 */
const lf = (s: string) => s.replace(/\r\n/g, '\n');
const sameSource = (a: string, b: string) => lf(a) === lf(b);

/** textarea 里的内容还没同步给内核时，立即整篇写回（切视图 / 保存前调用）。 */
function flushSource(): void {
  if (window.clearTimeout) window.clearTimeout(sourceSyncTimer);
  sourceSyncTimer = 0;
  if (window.__mdviewMode !== 'source') return;
  if (sameSource(sourceEl.value, sourceText)) return;
  sourceText = sourceEl.value;
  send({ kind: 'replaceSource', text: sourceText });
}

/**
 * 应用视图模式：写 <html data-view> 让 CSS 真正切换显示的 DOM 层。
 * source 额外把 sourceText 灌进 textarea 并聚焦它，其它模式把焦点还给 #ime。
 */
function applyViewMode(mode: 'wysiwyg' | 'reading' | 'source'): void {
  if (mode === 'source') {
    // 进入源码视图：把内核的源文本真值灌进 textarea（离开时的未同步编辑
    // 由调用方的 flushSource() 先入队，回包会把 sourceText 刷新成最新）
    sourceEl.value = sourceText;
  }
  window.__mdviewMode = mode;
  document.documentElement.dataset.view = mode;
  focusActiveEditor();
  pushShellState();
  if (window.__mdviewRefreshScrollbar) window.__mdviewRefreshScrollbar();
}

function renderAll(blocks: BlockDto[]) {
  docEl.replaceChildren(...fragments(blocks.map((b) => b.html).join('')));
  reindex();
  hydrateImages();
}

/**
 * 增量重绘。前后缀比对保证 [0,start) 与尾部原样不动，
 * 只有 [start,end) 是新的块，需要换掉的旧块数由 old_len - new_len 推出。
 */
function applyPatch(patch: PatchDto) {
  const children = Array.from(docEl.children);
  const incoming = patch.end - patch.start;
  const outgoing = patch.old_len - patch.new_len + incoming;
  const anchor = children[patch.start + outgoing] || null;
  for (let i = 0; i < outgoing; i++) {
    const node = children[patch.start + i];
    if (node) node.remove();
  }
  const nodes = fragments(patch.html);
  if (anchor) {
    for (const node of nodes) docEl.insertBefore(node, anchor);
  } else {
    for (const node of nodes) docEl.appendChild(node);
  }
  reindex();
  hydrateImages();
}

/**
 * 图片仍是原子停靠点，只有内层 img 需要装载。
 * 本地路径交给内核在文档目录内解析并读成 data URL；远程/data URL 直接交给浏览器。
 * 失败就退回「图」占位符，不打断编辑与光标定位。
 */
function hydrateImages() {
  docEl.querySelectorAll<HTMLElement>('.atom.img[data-asset]').forEach((holder) => {
    if (holder.dataset.imgState) return;
    const img = holder.querySelector<HTMLImageElement>('img');
    if (!img) return;

    const url = holder.dataset.asset || '';
    holder.dataset.imgState = 'loading';
    const done = () => {
      if (!holder.isConnected) return;
      holder.dataset.imgState = 'loaded';
      holder.classList.add('loaded');
      // 图片加载完会撑开正文，所有标题的纵坐标随之下移。不重测锚点的话，
      // 点大纲跳到图片之后的标题会落在图片上方，偏差正好是一张图的高度。
      refreshOutlineAnchors();
    };
    const fail = () => {
      if (!holder.isConnected) return;
      holder.dataset.imgState = 'failed';
      holder.classList.add('failed');
      img.remove();
      // 占位高度同样会变，锚点也要重测
      refreshOutlineAnchors();
    };
    img.addEventListener('load', done);
    img.addEventListener('error', fail);

    if (/^(data:|https?:\/\/)/i.test(url)) {
      if (url) img.src = url;
      else fail();
      return;
    }
    invoke<string | null>('load_image', { url })
      .then((dataUrl) => {
        if (!holder.isConnected) return;
        if (!dataUrl) fail();
        else img.src = dataUrl;
      })
      .catch(fail);
  });
}

// ---- 大纲侧栏 --------------------------------------------------------

/** 阅读基准线距视口顶部的距离：标题滚到这里就算「当前节」。 */
const OUTLINE_LINE = 80;

/**
 * 重新渲染侧栏并测量锚点。
 *
 * 锚点 = 每个标题块在文档坐标系里的纵坐标（rect.top + scrollY），与大纲项
 * 同序。判定当前节时用「最后一个越过基准线的锚点」，按真实位置而非滚动
 * 百分比（PLAN 硬约束 10）——标题疏密不均时才对得上。
 */
function rebuildOutline(items: OutlineDto[]) {
  outline = items || [];
  // 大纲整棵重建（换文档 / 结构变化）时点击意图已经失效
  outlinePinned = null;
  if (!outline.length) {
    outlineEl.innerHTML = '<div class="oc-title">大纲</div><div class="oc-empty">（无标题）</div>';
    outlineAnchors = [];
    activeOutlineIdx = -1;
    pushShellState();
    return;
  }
  outlineEl.replaceChildren(...[
    Object.assign(document.createElement('div'), { className: 'oc-title', textContent: '大纲' }),
    ...outline.map((item, i) => {
      const a = document.createElement('a');
      a.className = 'level-' + Math.min(Math.max(item.level, 1), 6);
      a.textContent = item.title || '（无标题）';
      a.dataset.idx = String(i);
      a.title = item.title;
      return a;
    }),
  ]);
  refreshOutlineAnchors();
  pushShellState();
}

/** 重新测量锚点（DOM 变化或窗口缩放后调用）。 */
function refreshOutlineAnchors() {
  const scrollY = window.scrollY || window.pageYOffset || 0;
  outlineAnchors = outline.map((item) => {
    const el = docEl.children[item.block];
    const top = el ? el.getBoundingClientRect().top + scrollY : Number.POSITIVE_INFINITY;
    return { top };
  });
  updateActiveOutline();
}

/** 按滚动位置高亮当前节。 */
function updateActiveOutline() {
  if (!outline.length) return;
  // 点击大纲后高亮钉在点击项上：文末标题受最大滚动位置限制，永远越不过
  // 基准线，按位置判定会把高亮还给上一个标题。
  if (outlinePinned !== null) return;
  const scrollY = window.scrollY || window.pageYOffset || 0;
  setActiveOutline(outlineActiveIndex(outlineAnchors, scrollY + OUTLINE_LINE));
}

/** 把高亮设到第 `idx` 项（-1 表示不高亮），并把侧栏里的该项滚进可见区。 */
function setActiveOutline(idx: number) {
  if (idx === activeOutlineIdx) return;
  activeOutlineIdx = idx;
  const links = outlineEl.querySelectorAll<HTMLElement>('a[data-idx]');
  links.forEach((a) => a.classList.toggle('active', Number(a.dataset.idx) === idx));
  // 让高亮项在侧栏里保持可见（侧栏自己滚动）
  const active = links[idx];
  if (active) {
    const ar = active.getBoundingClientRect();
    const nr = outlineEl.getBoundingClientRect();
    if (ar.top < nr.top + 8) outlineEl.scrollTop -= (nr.top + 8 - ar.top);
    else if (ar.bottom > nr.bottom - 8) outlineEl.scrollTop += (ar.bottom - nr.bottom + 8);
  }
  pushShellState();
}

/**
 * 用户自己滚动了：撤掉点击大纲时钉住的高亮，交还给「按滚动位置判定」。
 *
 * 只认真正的滚动输入（滚轮、触摸、导航键、拖自绘滚动条），不认 `scroll`
 * 事件本身——平滑滚动过程中也一直在发 `scroll`，按它撤钉会立刻打回原形。
 */
function releaseOutlinePin() {
  if (outlinePinned === null) return;
  outlinePinned = null;
  updateActiveOutline();
}

/** 点击大纲项：按锚点实际位置把该标题滚到阅读基准线处。 */
function jumpToOutline(idx: number) {
  // 现场量一次而不是用缓存：图片可能刚加载完、字体可能刚替换，
  // 缓存里的 top 是上一次布局的旧值，按它滚会差出图片/字体的高度。
  const item = outline[idx];
  const el = item ? docEl.children[item.block] : null;
  const top = el
    ? el.getBoundingClientRect().top + (window.scrollY || window.pageYOffset || 0)
    : outlineAnchors[idx]?.top;
  if (top === undefined || !isFinite(top)) return;
  outlinePinned = idx;
  setActiveOutline(idx);
  const target = outlineScrollToAnchor(top, OUTLINE_LINE);
  window.scrollTo({ top: target, behavior: 'smooth' });
}

// ---- 查找替换 --------------------------------------------------------

/**
 * 一次命中。源码下标（UTF-16）给源码视图选字，字节区间给内核的 `select`，
 * 停靠点区间用来画高亮与滚动定位——三者必须一起带上，缺一个就会错位。
 */
interface FindMatch {
  start: number;
  end: number;
  startByte: number;
  endByte: number;
  startStop: number;
  endStop: number;
}

let findOpen = false;
let findCaseSensitive = false;
let findWholeWord = false;
let findRegex = false;
let findPreserveCase = false;
let findInSelection = false;
let findBadPattern = false;
let findMatcher: { re: RegExp } | null = null;
let findMatches: FindMatch[] = [];
let findIndex = -1;
let findRefreshTimer = 0;

/** 搜索对象：源码视图直接读 textarea，避免去抖未落盘时搜到旧文本。 */
function findHaystack(): string {
  return window.__mdviewMode === 'source' ? sourceEl.value : sourceText;
}

/**
 * 「仅在选区内查找」的字节范围。
 *
 * 选区端点是停靠点，元素身上有 `data-byte`：起点元素的 byte 是选区首字符，
 * 终点元素（选区末端之后的那个停靠点）的 byte 正好是选区末字节。取不到就
 * 返回 null，调用方按全篇搜。
 */
function selectionByteRange(): { start: number; end: number } | null {
  if (!selectionStops) return null;
  const [lo, hi] = selectionStops;
  const first = byBefore.get(lo);
  const last = byBefore.get(hi);
  if (!first || !last) return null;
  const start = Number(first.dataset.byte);
  const end = Number(last.dataset.byte);
  return end > start ? { start, end } : null;
}

/**
 * 源码字节区间 → 停靠点区间。
 *
 * 起点必须正好是一个可见字符的起点：命中落在折叠标记里（比如搜 `**`）时
 * 选区是零宽，选出来什么也看不见，直接跳过。终点取「最后一个起点小于它的
 * 元素」的 `data-a`，这样多字节字符、图片原子都按元素边界算，不必自己数字节。
 */
function stopRangeFor(
  startByte: number,
  endByte: number,
): { startStop: number; endStop: number } | null {
  let lo = 0;
  let hi = byteStops.length - 1;
  let first = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (byteStops[mid].byte === startByte) { first = mid; break; }
    if (byteStops[mid].byte < startByte) lo = mid + 1;
    else hi = mid - 1;
  }
  if (first < 0 || byteStops[first].hidden) return null;

  lo = 0;
  hi = byteStops.length - 1;
  let last = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (byteStops[mid].byte < endByte) { last = mid; lo = mid + 1; }
    else hi = mid - 1;
  }
  if (last < 0) return null;

  const startStop = byteStops[first].b;
  const endStop = byteStops[last].a;
  return endStop > startStop ? { startStop, endStop } : null;
}

function runFind() {
  const query = findInput.value;
  findMatches = [];
  findIndex = -1;
  findMatcher = null;
  findBadPattern = false;
  if (!query) {
    paintFind();
    return;
  }
  const built = buildMatcher(query, {
    caseSensitive: findCaseSensitive,
    wholeWord: findWholeWord,
    regex: findRegex,
  });
  if (built.error) {
    findBadPattern = true;
    paintFind();
    return;
  }
  if (!built.re) {
    paintFind();
    return;
  }
  findMatcher = { re: built.re };
  const text = findHaystack();
  const offsets = utf8Offsets(text);
  const scope = findInSelection ? selectionByteRange() : null;
  for (const hit of scanMatches(text, findMatcher)) {
    const startByte = offsets[hit.start];
    const endByte = offsets[hit.end];
    if (scope && (startByte < scope.start || endByte > scope.end)) continue;
    const range = stopRangeFor(startByte, endByte);
    if (!range) continue;
    findMatches.push({ start: hit.start, end: hit.end, startByte, endByte, ...range });
  }
  paintFind();
}

function clearFindHighlight() {
  for (const el of marks) el.classList.remove('find-hit', 'find-current');
}

/** 命中为 0 时上/下一个与两个替换按钮都置灰。 */
function setFindNavDisabled(disabled: boolean) {
  for (const name of ['prev', 'next', 'replace', 'replace-all']) {
    const btn = findEl.querySelector<HTMLButtonElement>(`[data-find="${name}"]`);
    if (btn) btn.disabled = disabled;
  }
}

/**
 * 重画命中高亮。`marks` 与 `findMatches` 都按位置升序，用一根游标扫一遍即可，
 * 不做 O(元素数 × 命中数) 的包含判断。
 */
function paintFind() {
  clearFindHighlight();
  findCount.classList.toggle('bad', findBadPattern);
  if (findBadPattern) {
    findCount.textContent = '无效正则';
    setFindNavDisabled(true);
    return;
  }
  if (!findMatches.length) {
    // VSCode 在没有命中时显示「无结果」；查询为空则什么都不显示
    findCount.textContent = findInput.value ? '无结果' : '';
    setFindNavDisabled(true);
    return;
  }
  setFindNavDisabled(false);
  findCount.textContent =
    (findIndex >= 0 ? findIndex + 1 : 0) + '/' + findMatches.length;
  const current = findIndex >= 0 ? findMatches[findIndex] : null;
  let mi = 0;
  for (const el of marks) {
    if (el.classList.contains('mk')) continue;
    const b = Number(el.dataset.b);
    const a = el.hasAttribute('data-a') ? Number(el.dataset.a) : b;
    while (mi < findMatches.length && findMatches[mi].endStop <= b) mi++;
    if (mi >= findMatches.length) break;
    const m = findMatches[mi];
    if (b >= m.startStop && a <= m.endStop) {
      el.classList.add('find-hit');
      if (current && b >= current.startStop && a <= current.endStop) {
        el.classList.add('find-current');
      }
    }
  }
}

/** 源码视图的滚动估算：按命中前的换行数乘行高，居中显示。 */
function scrollSourceTo(index: number) {
  const line = sourceEl.value.slice(0, index).split('\n').length - 1;
  const lineHeight = parseFloat(getComputedStyle(sourceEl).lineHeight) || 22;
  sourceEl.scrollTop = Math.max(0, line * lineHeight - sourceEl.clientHeight / 2);
}

function goToMatch(index: number, scroll = true) {
  if (!findMatches.length) {
    findIndex = -1;
    paintFind();
    return;
  }
  findIndex = ((index % findMatches.length) + findMatches.length) % findMatches.length;
  paintFind();
  const m = findMatches[findIndex];
  if (window.__mdviewMode === 'source') {
    sourceEl.focus({ preventScroll: true });
    sourceEl.setSelectionRange(m.start, m.end);
    if (scroll) scrollSourceTo(m.start);
    return;
  }
  // 内核的 Select 收 byte，自己会吸附到停靠点；不必在前端换算。
  send({ kind: 'select', anchor: m.startByte, head: m.endByte });
  if (scroll) {
    queue.then(() => {
      const el = byBefore.get(m.startStop) || byAfter.get(m.startStop);
      el?.scrollIntoView({ block: 'center', behavior: 'smooth' });
    });
  }
}

function stepFind(delta: number) {
  if (!findOpen) { openFind(false); return; }
  if (!findMatches.length) runFind();
  if (!findMatches.length) return;
  const from = findIndex < 0
    ? (delta > 0 ? 0 : findMatches.length - 1)
    : findIndex + delta;
  goToMatch(from);
}

function openFind(withReplace: boolean) {
  findOpen = true;
  findEl.hidden = false;
  // Ctrl+F 只看查找、Ctrl+H 才展开替换行。不能沿用上一次的展开状态：
  // 否则「Ctrl+H 用过一次」之后每次 Ctrl+F 都带出一行用不到的替换框。
  // 替换框里的内容不清，再按 Ctrl+H 或点左侧箭头还能接着用。
  setReplaceVisible(withReplace);
  // 源码视图有原生选区，直接拿它当查询词；编辑态没有可借的选区，保持原查询。
  if (window.__mdviewMode === 'source') {
    const picked = sourceEl.value.slice(sourceEl.selectionStart, sourceEl.selectionEnd);
    if (picked && !picked.includes('\n')) findInput.value = picked;
  }
  findInput.focus({ preventScroll: true });
  findInput.select();
  runFind();
  if (findMatches.length) goToMatch(0);
}

function closeFind() {
  if (!findOpen) return;
  findOpen = false;
  findEl.hidden = true;
  findMenu.hidden = true;
  findMatches = [];
  findIndex = -1;
  findBadPattern = false;
  findCount.classList.remove('bad');
  clearFindHighlight();
  findCount.textContent = '';
  focusActiveEditor();
}

/** 展开 / 收起替换行，同时把左侧箭头转成「展开」朝向。 */
function setReplaceVisible(show: boolean) {
  replaceRow.hidden = !show;
  findEl.querySelector('.find-expand')?.classList.toggle('open', show);
}

function markFindToggle(name: string, on: boolean) {
  findEl.querySelector(`[data-find="${name}"]`)?.classList.toggle('on', on);
}

/** 任一开关变了都重跑一次，并把光标带到第一个命中。 */
function refreshFind() {
  runFind();
  if (findMatches.length) goToMatch(0);
}

function toggleFindCase() {
  findCaseSensitive = !findCaseSensitive;
  markFindToggle('case', findCaseSensitive);
  refreshFind();
}

function toggleFindWord() {
  findWholeWord = !findWholeWord;
  markFindToggle('word', findWholeWord);
  refreshFind();
}

function toggleFindRegex() {
  findRegex = !findRegex;
  markFindToggle('regex', findRegex);
  refreshFind();
}

function toggleFindPreserve() {
  findPreserveCase = !findPreserveCase;
  markFindToggle('preserve', findPreserveCase);
}

function toggleFindScope() {
  if (!findInSelection && !selectionByteRange()) {
    toast('先在正文里选中一段，才能只在选区内查找');
    return;
  }
  findInSelection = !findInSelection;
  findEl
    .querySelector('[data-find="in-selection"]')
    ?.setAttribute('aria-pressed', String(findInSelection));
  refreshFind();
}

function replaceCurrent() {
  if (!findOpen || findIndex < 0 || !findMatches.length) return;
  const match = findMatches[findIndex];
  // 正则模式下 $1 之类的捕获组引用由 find.js 展开；保留大小写也在这里套
  const piece = replacementFor(findHaystack(), match, replaceInput.value, {
    matcher: findMatcher,
    preserveCase: findPreserveCase,
  });
  if (window.__mdviewMode === 'source') {
    sourceEl.value =
      sourceEl.value.slice(0, match.start) + piece + sourceEl.value.slice(match.end);
    flushSource();
  } else {
    // 先选后插：insertText 有选区时就是替换，一次事务、一步撤销。
    send({ kind: 'select', anchor: match.startByte, head: match.endByte });
    send({ kind: 'insertText', text: piece });
  }
  const at = findIndex;
  queue.then(() => {
    runFind();
    if (findMatches.length) goToMatch(Math.min(at, findMatches.length - 1));
  });
}

function replaceAllMatches() {
  if (!findOpen || !findMatches.length) return;
  const text = findHaystack();
  const next = replaceMatches(text, findMatches, replaceInput.value, {
    matcher: findMatcher,
    preserveCase: findPreserveCase,
  });
  if (next === text) return;
  if (window.__mdviewMode === 'source') {
    sourceEl.value = next;
    flushSource();
  } else {
    send({ kind: 'replaceSource', text: next });
  }
  queue.then(() => {
    runFind();
    if (findMatches.length) goToMatch(0);
  });
}

/** 文档被编辑后重算命中，但不抢光标：用户可能正在正文里打字。 */
function refreshFindAfterEdit() {
  if (!findOpen) return;
  if (findRefreshTimer) window.clearTimeout(findRefreshTimer);
  findRefreshTimer = window.setTimeout(() => {
    findRefreshTimer = 0;
    if (!findOpen) return;
    const keep = findIndex;
    runFind();
    if (findMatches.length) {
      findIndex = Math.min(Math.max(keep, 0), findMatches.length - 1);
      paintFind();
    }
  }, 60);
}

findEl.addEventListener('click', (event) => {
  const btn = (event.target as Element | null)?.closest<HTMLElement>('[data-find]');
  if (!btn) return;
  const action = btn.dataset.find;
  // 除了「更多」与菜单里的项，点任何按钮都收起下拉
  if (action !== 'more' && action !== 'in-selection') findMenu.hidden = true;
  switch (action) {
    case 'prev': stepFind(-1); break;
    case 'next': stepFind(1); break;
    case 'case': toggleFindCase(); break;
    case 'word': toggleFindWord(); break;
    case 'regex': toggleFindRegex(); break;
    case 'preserve': toggleFindPreserve(); break;
    case 'toggle-replace':
      setReplaceVisible(replaceRow.hidden);
      if (!replaceRow.hidden) replaceInput.focus({ preventScroll: true });
      break;
    case 'more': findMenu.hidden = !findMenu.hidden; break;
    case 'in-selection':
      findMenu.hidden = true;
      toggleFindScope();
      break;
    case 'close': closeFind(); break;
    case 'replace': replaceCurrent(); break;
    case 'replace-all': replaceAllMatches(); break;
    default: break;
  }
});

findInput.addEventListener('input', () => {
  runFind();
  if (findMatches.length) goToMatch(0);
});

// 替换框里的回车只作用于替换；查找框的回车由 window 的 keydown 统一分派。
replaceInput.addEventListener('keydown', (event) => {
  if (event.key !== 'Enter') return;
  event.preventDefault();
  // 阻止冒泡：否则 window 上的查找快捷键还会再走一次「下一个」
  event.stopPropagation();
  const all = event.shiftKey || (event.ctrlKey && event.altKey);
  if (all) replaceAllMatches();
  else replaceCurrent();
});

// 点「更多」菜单外面收起它。
document.addEventListener(
  'mousedown',
  (event) => {
    if (findMenu.hidden) return;
    const target = event.target as Element | null;
    if (target && (findMenu.contains(target) || target.closest('[data-find="more"]'))) return;
    findMenu.hidden = true;
  },
  true,
);

// ---- 链接 / 图片行内编辑 ----------------------------------------------

interface LinkTarget {
  stop: number;
  url: string;
  text: string;
  image: boolean;
}

let linkTarget: LinkTarget | null = null;

/** 同一个链接的字符是平级兄弟：label 要把左右连续同 href 的字符都拼起来。 */
function linkLabelAround(el: HTMLElement): string {
  const href = el.dataset.href;
  let text = '';
  let node: Element | null = el;
  while (node && (node as HTMLElement).dataset?.href === href) {
    text = (node.textContent || '') + text;
    node = node.previousElementSibling;
  }
  node = el.nextElementSibling;
  while (node && (node as HTMLElement).dataset?.href === href) {
    text += node.textContent || '';
    node = node.nextElementSibling;
  }
  return text;
}

function positionLinkEditor(el: HTMLElement) {
  const rect = el.getBoundingClientRect();
  const width = linkEditEl.offsetWidth || 340;
  const height = linkEditEl.offsetHeight || 96;
  const left = Math.min(Math.max(8, rect.left), window.innerWidth - width - 8);
  let top = rect.bottom + 6;
  // 下方放不下就翻到上面，别被状态栏或视口下沿裁掉
  if (top + height > window.innerHeight - 8) top = Math.max(8, rect.top - height - 6);
  linkEditEl.style.left = left + 'px';
  linkEditEl.style.top = top + 'px';
}

function openLinkEditor(el: HTMLElement) {
  const image = el.classList.contains('img');
  const stop = Number(el.dataset.b);
  if (!Number.isFinite(stop)) return;
  const url = image ? el.dataset.asset || '' : el.dataset.href || '';
  linkTarget = {
    stop,
    image,
    url,
    text: image ? el.getAttribute('title') || '' : linkLabelAround(el),
  };
  linkUrlInput.value = url;
  linkTextInput.value = linkTarget.text;
  // 图片没有「打开」这个动作
  const openBtn = linkEditEl.querySelector<HTMLElement>('[data-link="open"]');
  if (openBtn) openBtn.hidden = image;
  linkEditEl.hidden = false;
  positionLinkEditor(el);
  linkUrlInput.focus({ preventScroll: true });
  linkUrlInput.select();
}

function closeLinkEditor() {
  if (linkEditEl.hidden) return;
  linkEditEl.hidden = true;
  linkTarget = null;
  focusActiveEditor();
}

/**
 * 保存 / 删除。整段重写在核心里做：前端只给 stop 与两个字段，
 * 片段边界、label 里的行内标记都由内核自己处理，一次事务、一步撤销。
 */
function submitLinkEdit(remove: boolean) {
  if (!linkTarget) return;
  const { stop } = linkTarget;
  const url = linkUrlInput.value;
  const text = linkTextInput.value;
  linkEditEl.hidden = true;
  linkTarget = null;
  send({ kind: 'editLink', stop, url, text, remove });
  focusActiveEditor();
}

linkEditEl.addEventListener('click', (event) => {
  const btn = (event.target as Element | null)?.closest<HTMLElement>('[data-link]');
  if (!btn) return;
  switch (btn.dataset.link) {
    case 'save': submitLinkEdit(false); break;
    case 'remove': submitLinkEdit(true); break;
    case 'close': closeLinkEditor(); break;
    case 'open': {
      const url = linkUrlInput.value.trim();
      if (url) invoke('open_url', { url }).catch((err) => toast('打不开链接：' + err));
      break;
    }
    default: break;
  }
});

for (const input of [linkUrlInput, linkTextInput]) {
  input.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    submitLinkEdit(false);
  });
}

// 点面板外面就收起；点另一个链接/图片时先收起，交给 click 重新打开。
document.addEventListener(
  'mousedown',
  (event) => {
    if (linkEditEl.hidden) return;
    const target = event.target as Element | null;
    if (target && linkEditEl.contains(target)) return;
    if (target && target.closest('.atom.img[data-asset], [data-href]')) return;
    closeLinkEditor();
  },
  true,
);



// ---- 光标 ------------------------------------------------------------

/**
 * 停靠点 → 像素。停靠点的 x 就是「以它结尾的元素的右边缘」，
 * 没有这个元素时取「以它开始的元素的左边缘」。全部由浏览器测量，内核不参与。
 *
 * `mk` 用于上下移动时的并列判决：折叠标记零宽，它的右端与相邻文字共享同一列，
 * 落到标记里会让下一次退格先碰标记。`self-test` 里对这一点有断言。
 */
function rectOf(stop: number): CaretPos | null {
  const probes: [HTMLElement | undefined, 'right' | 'left'][] = [
    [byAfter.get(stop), 'right'],
    [byBefore.get(stop), 'left'],
  ];
  for (const [el, edge] of probes) {
    if (!el) continue;
    const rect = el.getBoundingClientRect();
    if (rect.height > 0) {
      const mk = el.classList.contains('mk');
      return {
        x: edge === 'right' ? rect.right : rect.left,
        top: rect.top, height: rect.height, el, mk,
      };
    }
  }
  return null;
}

/**
 * 上下移动的几何。放在前端而不是内核：软换行只有浏览器知道，
 * 内核只认「停靠点 + 视觉列」，它眼里的下一行只能是下一块。
 */
const lines = createLineMotion({
  count: () => maxStop + 1,
  rect: (stop) => rectOf(stop),
});

/**
 * 让光标回到视口内，只滚到「刚好露出来 + 一点余量」。
 *
 * 不用 `scrollIntoView`：它对齐的是 `pos.el` 这个元素的边，
 * 元素可能是一个 600px 高的代码块，也可能是零宽的折叠标记；
 * 我们要的是**光标那一行**的位置。`margin` 同时是上下留白。
 */
function revealCaret(pos: CaretPos) {
  const delta = scrollDelta(
    { top: pos.top, height: Math.max(pos.height, 18) },
    window.innerHeight,
    REVEAL_MARGIN,
  );
  if (delta !== 0) window.scrollBy(0, delta);
}

/**
 * `reveal = false` 时只把光标画到新位置，绝不滚动页面。
 *
 * 滚动事件里必须关掉它：光标是 `position: absolute` 的文档坐标，
 * 页面滚动时它自己会跟着走，不需要任何重画。若这里还去「保证可见」，
 * 用户每往下滚一点、光标被推出视口上沿，就被拉回原位——
 * 表现为「光标在页面顶端时完全滚不动」。
 *
 * `force` 用于视口本身变了（窗口缩放）：那时光标没动，但仍要重新露出来。
 */
function applyCaret(next: CaretDto | null, reveal = true, force = false) {
  caret = next;
  // 只有回包落点与预期一致才作废：连按时可能还在路上（见 moveVertical）
  if (pendingStop !== null && next && next.stop === pendingStop) pendingStop = null;
  if (!next) {
    caretEl.style.display = 'none';
    return;
  }
  const pos = rectOf(next.stop);
  if (!pos) {
    caretEl.style.display = 'none';
    return;
  }
  caretEl.style.display = 'block';
  caretEl.classList.add('blink');
  caretEl.style.left = pos.x + window.scrollX + 'px';
  caretEl.style.top = pos.top + window.scrollY + 'px';
  caretEl.style.height = Math.max(pos.height, 18) + 'px';

  // 只有落点真的变了才追视口。滚动是用户自己的意图，不该被「光标还停在那儿」
  // 这件事打断；同一落点重复回包（重绘、状态刷新）也同理。
  //
  // 拖动期间也不追：光标就在指针底下，本来就在视口里；而拖到边缘时
  // `revealCaret` 会额外多滚一段余量，把自动滚动的节奏顶乱（每帧多滚 30px、
  // 越过指针对应的行，观感是「滚起来一顿一顿的」）。
  if (reveal && (force || (!dragging && revealedStop !== next.stop))) {
    revealCaret(pos);
    revealedStop = next.stop;
  }
  imeAnchorEl = pos.el;
  placeIme();
  renderStatus();
}

function paintSelection(range: [number, number] | null) {
  selectionStops = range ? [range[0], range[1]] : null;
  for (const el of marks) el.classList.remove('sel');
  if (!range) return;
  const [lo, hi] = range;
  for (const el of marks) {
    const before = Number(el.dataset.b);
    const after = el.hasAttribute('data-a') ? Number(el.dataset.a) : before + 1;
    if (before >= lo && after <= hi && !el.classList.contains('mk') && !el.classList.contains('eob')) {
      el.classList.add('sel');
    }
  }
}

/**
 * 原生选区一律清掉，选区只由内核的 anchor/head 决定（判据见 `selection.js`）。
 *
 * `mousedown` 的 `preventDefault` 挡不住所有路径：Ctrl+A、焦点回到 body 之后
 * 的拖动、程序化 `addRange`，都会在本体上留下一片浏览器蓝底，和自绘的 `.sel`
 * 叠在一起——颜色不对，边界也不对。
 *
 * 透明输入框内部的选区要放行，否则输入法组合会被打断。
 */
let clearingSelection = false;
document.addEventListener('selectionchange', () => {
  if (clearingSelection) return;
  const sel = document.getSelection();
  if (!sel || !shouldClearSelection(sel, imeEl)) return;
  clearingSelection = true;
  sel.removeAllRanges();
  clearingSelection = false;
});

/** 光标是否落在表格单元格里。工具条与 Tab 行为都据此切换。 */
function inTable() {
  if (!caret) return false;
  const el = byBefore.get(caret.stop) || byAfter.get(caret.stop);
  return !!(el && el.closest('.md-table'));
}

function renderStatus() {
  // 状态栏内容由 Vue 渲染（见 StatusBar.vue），这里只负责标题栏 IPC 去重与状态快照。
  // 标题栏只在文件名或脏标记变化时才走一次 IPC，别在每次输入后都设
  const titleKey = (meta.path || '') + '|' + !!meta.dirty;
  if (titleKey !== lastTitleKey && window.__mdviewSetTitle) {
    lastTitleKey = titleKey;
    window.__mdviewSetTitle(meta.path, meta.dirty);
  }
  pushShellState();
}

/**
 * 把外壳需要的一份状态快照交给 Vue 桥。
 * 只传显示用的数字/字符串与大纲数组，不传文档内容、不传 DOM 节点。
 */
function pushShellState() {
  const bridge = window.__mdviewBridge;
  if (!bridge) return;
  bridge.set({
    path: meta.path,
    dirty: !!meta.dirty,
    blocks: docEl.children.length,
    words: meta.words,
    stop: caret ? caret.stop : null,
    caretBlock: caret ? caret.block : null,
    column: caret && caret.visual !== null ? caret.visual : null,
    revision: meta.revision,
    inTable: inTable(),
    mode: window.__mdviewMode ?? 'wysiwyg',
    themeMode: mdviewTheme.mode() as ThemeMode,
    theme: document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light',
    skin: mdviewSkin.mode(),
    outline,
    activeOutlineIdx,
  });
}
window.__mdviewPushShellState = pushShellState;

/** toast 的 DOM 与计时都归 Vue（Toast.vue / bridge.notify），这里只投递文案。 */
function toast(text: string) {
  if (window.__mdviewBridge) window.__mdviewBridge.notify(text);
}

/** 路径里的文件名部分（Windows 与 POSIX 两种分隔符都要认）。 */
function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

/** 最近打开列表的本地副本。菜单动作只带序号，路径要从这里取。 */
let recentPaths: string[] = [];

/** 从宿主拉一次最近列表，同步给菜单。失败时保持原样，不影响编辑。 */
function refreshRecent(): Promise<void> {
  return invoke<string[]>('recent_files')
    .then((list) => {
      recentPaths = Array.isArray(list) ? list : [];
      window.__mdviewBridge?.set({ recent: recentPaths });
    })
    .catch(() => {});
}

/** 按路径打开。最近列表、拖放、二次启动三条入口共用同一条队列。 */
function openPath(path: string) {
  queue = queue.then(async () => {
    const snap = await invoke<Snapshot | null>('open_file', { path });
    if (!snap) return;
    adoptSnapshot(snap);
    toast('已打开文件');
    refreshRecent();
  }).catch((err) => toast('打开文件失败：' + err));
}

/** 按序号打开最近文件。未保存改动仍由宿主的 open_file 先问一句。 */
function openRecent(index: number) {
  const path = recentPaths[index];
  if (path) openPath(path);
}

/**
 * 要一个保存路径，再让内核写到那里。
 *
 * 两个入口共用：Ctrl+S 时内核回 `needs_path`（未命名文档还没有路径），
 * 或用户主动点「另存为…」。**写盘仍然在内核里完成**——这里只负责弹框，
 * 因为「写哪个文件」是唯一一件内核自己决定不了的事。
 */
let saveDialogOpen = false;

function requestSavePath() {
  if (saveDialogOpen) return;   // 连按 Ctrl+S 不该叠出多个对话框
  saveDialogOpen = true;
  const suggested = meta.path ? baseName(meta.path) : '未命名.md';
  invoke<string | null>('pick_save_path', { suggested })
    .then((path) => {
      // 取消：内核那边什么都没动（dirty 还在、路径还是空），不用收尾
      if (path) send({ kind: 'saveTo', path });
    })
    .catch((err) => toast('保存对话框打不开：' + err))
    .finally(() => {
      saveDialogOpen = false;
      // 对话框关掉后焦点可能停在系统窗口上，不还回去就再也打不进字
      focusActiveEditor();
    });
}

/** 菜单动作统一入口：Windows/Linux 的自绘菜单、macOS 的原生菜单栏共用。 */
function runAction(action: string) {
  // 源码视图下 textarea 自己就是编辑器：编辑类菜单动作必须作用于它，
  // 不能误发到内核（内核那侧是富文本投影，两套真值会打架）。
  if (window.__mdviewMode === 'source' && document.activeElement === sourceEl) {
    const native: Record<string, string> = {
      undo: 'undo', redo: 'redo', cut: 'cut', copy: 'copy', paste: 'paste',
      'select-all': 'selectAll',
    };
    const cmd = native[action];
    if (cmd) {
      sourceEl.focus({ preventScroll: true });
      document.execCommand(cmd);
      // paste/输入类命令会触发 input 事件，由 input 监听整篇同步；
      // undo/redo 也触发 input，统一走同一条同步路径。
      return;
    }
  }
  // 最近打开：菜单条目是运行时生成的，动作名带序号（`recent:0`）。
  if (action.startsWith('recent:')) {
    openRecent(Number(action.slice('recent:'.length)));
    return;
  }
  if (action === 'recent-clear') {
    invoke('clear_recent_files')
      .then(() => {
        refreshRecent();
        toast('已清除最近记录');
      })
      .catch((err) => toast('清除失败：' + err));
    return;
  }
  switch (action) {
    case 'new-window':
      // 宿主负责开窗、建独立会话；本窗口不换文档，也不抢新窗口的焦点
      invoke('new_window').catch((err) => toast('新建窗口失败：' + err));
      return;
    case 'new-file':
      // 内核在有未保存改动时会先问一句；取消返回 null，这里什么都不做。
      queue = queue.then(async () => {
        const snap = await invoke<Snapshot | null>('new_file');
        if (!snap) return;
        adoptSnapshot(snap);
        toast('已新建文档');
      }).catch((err) => toast('新建失败：' + err));
      break;
    case 'open-file':
      // 先等在途 dispatch 回包，否则旧 revision 的 reply 会盖掉刚打开的文档。
      // 对话框与「未保存改动」确认都由 Rust 侧弹；用户取消返回 null，什么都不动。
      queue = queue.then(async () => {
        const snap = await invoke<Snapshot | null>('open_file');
        if (!snap) return;
        adoptSnapshot(snap);
        toast('已打开文件');
        refreshRecent();
      }).catch((err) => toast('打开文件失败：' + err));
      break;
    case 'reload-file':
      // 同一条队列：在途 dispatch 的回包不能盖掉刚读回来的内容。
      // 用户取消确认时返回 null（静默）；未命名文档由 Rust 直接报错。
      queue = queue.then(async () => {
        const snap = await invoke<Snapshot | null>('reload_file');
        if (!snap) return;
        adoptSnapshot(snap);
        toast('已重新加载');
      }).catch((err) => toast('重新加载失败：' + err));
      break;
    case 'undo': send({ kind: 'undo' }); break;
    case 'redo': send({ kind: 'redo' }); break;
    // 查找栏自己管焦点，不能落到函数末尾的 focusActiveEditor()
    case 'find': openFind(false); return;
    case 'replace': openFind(true); return;
    case 'save': flushSource(); send({ kind: 'save' }); break;
    case 'save-as': requestSavePath(); break;
    case 'copy': send({ kind: 'copy' }); break;
    case 'cut': send({ kind: 'cut' }); break;
    case 'select-all': send({ kind: 'selectAll' }); break;
    case 'paste':
      // 菜单点击是用户手势，可以读系统剪贴板；失败就退回快捷键提示
      if (navigator.clipboard && navigator.clipboard.readText) {
        navigator.clipboard.readText().then((text) => {
          if (text) send({ kind: 'insertText', text });
        }).catch(() => toast('请按 Ctrl+V 粘贴'));
      } else {
        toast('请按 Ctrl+V 粘贴');
      }
      break;
    case 'theme-system':
    case 'theme-light':
    case 'theme-dark':
      mdviewTheme.set(action.slice(6));
      break;
    case 'skin-hyperos':
    case 'skin-macos': {
      mdviewSkin.set(action.slice(5) as mdviewSkin.SkinName);
      // 两套皮肤的标题栏高度、侧栏宽度、状态栏高度都不同：光标矩形与自绘
      // 滚动条的轨道都是按实测几何算的，下一帧统一重量一次，否则会错位。
      requestAnimationFrame(() => {
        applyCaret(caret, false, true);
        window.__mdviewRefreshScrollbar?.();
      });
      break;
    }
    case 'mode-wysiwyg':
      flushSource(); applyViewMode('wysiwyg'); send({ kind: 'setMode', mode: 'wysiwyg' }); break;
    case 'mode-reading':
      flushSource(); applyViewMode('reading'); send({ kind: 'setMode', mode: 'reading' }); break;
    case 'mode-source':
      // 切到源码视图前先把 sourceText 灌进 textarea（applyViewMode 内完成）
      applyViewMode('source'); send({ kind: 'setMode', mode: 'source' }); break;
    case 'line-home':
    case 'line-end':
      // 复用键盘路径：行首/行尾的视觉行判定已经在那里实现
      window.dispatchEvent(new KeyboardEvent('keydown', {
        key: action === 'line-home' ? 'Home' : 'End', bubbles: true, cancelable: true,
      }));
      break;
    case 'doc-start': send({ kind: 'setCaret', stop: 0, extend: false }); break;
    case 'doc-end': send({ kind: 'setCaret', stop: maxStop, extend: false }); break;
    case 'outline-prev':
    case 'outline-next': {
      if (!outline.length) { toast('文档没有标题'); break; }
      const step = action === 'outline-next' ? 1 : -1;
      const from = activeOutlineIdx < 0 ? (step > 0 ? -1 : outline.length) : activeOutlineIdx;
      jumpToOutline(Math.max(0, Math.min(outline.length - 1, from + step)));
      break;
    }
    // 「关于」是纯外壳动作，由 bridge 直接开弹窗，不再落到内核里弹 toast。
    default: break;
  }
  pushShellState();
  focusActiveEditor();
}
window.__mdviewRunAction = runAction;

// ---- IPC -------------------------------------------------------------

// 所有消息串行发出：内核虽然有锁，但 DOM 的 patch 必须按顺序应用
let queue: Promise<void> = Promise.resolve();

/** 只入队，不动状态。上下移动专用，否则期望列每步都被清掉。 */
function enqueue(message: Wire) {
  queue = queue.then(() => dispatchOne(message)).catch((err) => {
    console.error(err);
    toast('内核报错：' + err);
  });
  return queue;
}

/**
 * 普通消息。任何非上下移动的操作都要清掉「期望列」——
 * 否则按过上下键之后再点击、输入，列还会被上一次的方向键带跑。
 */
function send(message: Wire) {
  lines.reset();
  pendingStop = null;
  return enqueue(message);
}

/**
 * 上下方向键：按真实视觉行移动，落点是目标行里离当前 x 最近的停靠点。
 *
 * 按住不放时按键比 IPC 回包快，`caret` 会落后一步，于是每一下都从同一个位置
 * 起算、原地不动。这里用 `pendingStop` 记住「还没回包但已发出」的落点。
 */
function moveVertical(dir: number, extend: boolean) {
  if (!caret) return;
  const from = pendingStop === null ? caret.stop : pendingStop;
  const target = lines.step(from, dir);
  // 已在首行/末行：标准行为是不动，而不是跳到块首
  if (target === null) return;
  pendingStop = target;
  enqueue({ kind: 'setCaret', stop: target, extend });
}

async function dispatchOne(message: Wire) {
  const reply = await invoke<Reply>('dispatch', { message });
  applyReply(reply);
}

function applyReply(reply: Reply) {
  // 内容/结构变了：点击大纲时的意图过期，高亮交还给滚动位置判定
  if (reply.full || reply.patch || reply.outline) outlinePinned = null;
  if (reply.full) {
    renderAll(reply.full);
  } else if (reply.patch) {
    applyPatch(reply.patch);
  }
  if (reply.outline) rebuildOutline(reply.outline);
  else refreshOutlineAnchors();
  if (reply.caret) applyCaret(reply.caret);
  paintSelection(reply.selection ?? null);
  meta.revision = reply.revision;
  meta.dirty = reply.dirty;
  if (typeof reply.words === 'number') meta.words = reply.words;
  // 另存为成功后内核会把文档改绑到新路径，标题栏要跟着换名字
  if (reply.path) meta.path = reply.path;
  // 内核回传的源文本真值。源码视图里 textarea 是用户正在敲的那一侧，
  // 它的值可能与这条回包同revision，不要回写覆盖，否则光标会跳。
  if (typeof reply.source === 'string') {
    sourceText = reply.source;
    if (window.__mdviewMode !== 'source' && !sameSource(sourceEl.value, sourceText)) {
      sourceEl.value = sourceText;
    }
  }
  if (typeof reply.clipboard === 'string') writeClipboard(reply.clipboard, reply.clipboard_html);
  if (reply.conflict) toast('文件已被外部修改，保存被拒');
  else if (reply.error) toast(reply.error);
  if (reply.saved) {
    toast('已保存');
    // 另存为会把文档改绑到新路径，最近列表要跟着多一条。
    refreshRecent();
  }
  // 内核没有路径：弹框问用户要一个，拿到后补发 saveTo
  if (reply.needs_path) requestSavePath();
  // 文档变了，查找的命中位置也要跟着重算（不抢光标，见 refreshFindAfterEdit）
  refreshFindAfterEdit();
  renderStatus();
  if (window.__mdviewRefreshScrollbar) window.__mdviewRefreshScrollbar();
}

/**
 * 把内核算出的选区源码交给系统剪贴板。
 *
 * 写入是异步的（还要退回到 execCommand），所以**不 await**：剪贴板失败不该
 * 拖住编辑循环，最多弹一条提示。退回路径会抢走输入框焦点，写完要还回去。
 */
let lastClipboard: string | null = null;   // 最近一次交出去的文本，自检用它核对整条链路

function writeClipboard(text: string, html?: string | null) {
  lastClipboard = text;
  copyText(text, html ?? '')
    .then((ok) => {
      if (!ok) toast('复制失败：系统剪贴板不可用');
    })
    .catch(() => toast('复制失败：系统剪贴板不可用'))
    .finally(() => {
      focusActiveEditor();
    });
}

/** 用一份内核快照全量重绘。boot 与「打开文件」共用，避免两处渲染逻辑漂移。 */
function applySnapshot(snap: Snapshot) {
  meta = {
    revision: snap.revision,
    dirty: snap.dirty,
    path: snap.path,
    stops: snap.stops,
    words: typeof snap.words === 'number' ? snap.words : 0,
  };
  // 换文档/新建/启动：源文本真值整份替换，textarea 无条件跟上
  sourceText = snap.source ?? '';
  sourceEl.value = sourceText;
  renderAll(snap.blocks);
  rebuildOutline(snap.outline || []);
  applyCaret(snap.caret);
  paintSelection(snap.selection);
  // 换文档后旧的命中已经不成立，重算（查找栏开着就保持开着）
  refreshFindAfterEdit();
  renderStatus();
  if (window.__mdviewRefreshScrollbar) window.__mdviewRefreshScrollbar();
}

/**
 * 换文档后的统一收尾：全量重绘、回到顶部、刷新滚动条、把焦点还给输入框。
 *
 * 「打开文件」「重新加载」「新建」「二次启动转入文件」几条路都要做同样四件事，
 * 收在这里免得哪天加一条路又漏掉某一步（漏掉焦点，键盘就再也打不进字）。
 */
function adoptSnapshot(snap: Snapshot) {
  applySnapshot(snap);
  window.scrollTo(0, 0);
  sourceEl.scrollTop = 0;  // 源码视图自滚，换文档也要回到顶部
  if (window.__mdviewRefreshScrollbar) window.__mdviewRefreshScrollbar();
  focusActiveEditor();
}

/**
 * 事件负载里取动作名。Rust 侧既可能 emit 纯字符串，也可能 emit `{ action }`，
 * 两种都在用（原生菜单 / 单例插件），所以统一在这里收口。
 */
function actionOf(payload: unknown): string | null {
  if (typeof payload === 'string') return payload;
  if (payload && typeof payload === 'object' && 'action' in payload) {
    const action = (payload as { action?: unknown }).action;
    return typeof action === 'string' ? action : null;
  }
  return null;
}

async function boot() {
  // 主题必须在渲染前定下来，避免浅色闪一下再切深色
  mdviewTheme.init();
  applySnapshot(await invoke<Snapshot>('snapshot'));
  // 焦点必须落在透明输入框上：输入法只认真正的可编辑元素
  focusActiveEditor();
  // 给内核一个「页面已接管」的信号，日志里能对上游标是否真的画出来了
  invoke('ready').catch(() => {});
  // 最近打开列表：首帧就拉一次，文件菜单不用等到第一次保存才有内容。
  refreshRecent();
  // 外部修改：后端 Watcher 发现磁盘上的文件被别的进程改了会 emit 这个事件。
  // 只提示，不自动重载——自动重载会冲掉用户未保存的编辑。
  if (listen) {
    listen('external-change', () => {
      toast('文件已被外部修改，请重新打开以载入最新内容');
    }).catch(() => {});
    // 关窗被内核拦下了（有未保存改动）：问一句，确认后放行这一次关闭。
    // 原生对话框不能放在窗口事件回调里，所以由这里用 async 命令去弹。
    listen('close-requested', () => {
      invoke<boolean>('confirm_discard', { action: 'close' })
        .then((ok) => {
          if (ok) return invoke('force_close');
          return undefined;
        })
        .catch((err) => toast('关闭确认失败：' + err));
    }).catch(() => {});
    // macOS 原生菜单栏点选后，Rust 侧把动作名 emit 过来。
    // 走外壳动作桥而不是直接 runAction：「检查更新」这类纯外壳动作只有桥接得住，
    // 其余动作由桥原样转回 runAction。
    listen<unknown>('menu-action', (event) => {
      const action = actionOf(event.payload);
      if (!action) return;
      if (window.__mdviewBridge) window.__mdviewBridge.dispatch(action);
      else runAction(action);
    }).catch(() => {});
    // 别的窗口打开/另存/清空过文件：本窗口的「最近打开」也要跟着刷新
    listen('recent-changed', () => refreshRecent()).catch(() => {});
    // 拖放打开：宿主只负责判定「落进来的是不是 Markdown」，打开仍走 open_file，
    // 这样未保存改动的确认、最近列表的更新都与菜单打开完全一致。
    listen<unknown>('drop-open', (event) => {
      const path = typeof event.payload === 'string' ? event.payload : null;
      if (path) openPath(path);
    }).catch(() => {});
    // 悬停反馈：拖进来时给正文加一圈虚线，离开/松手就撤掉。
    listen<unknown>('drop-hover', (event) => {
      if (event.payload === true) document.documentElement.dataset.drop = '1';
      else delete document.documentElement.dataset.drop;
    }).catch(() => {});
  }
}

// ---- 输入 ------------------------------------------------------------

/** 把透明输入框挪到光标上：它决定输入法候选窗弹在哪里。 */
function placeIme() {
  if (!caret) return;
  const pos = rectOf(caret.stop);
  if (!pos) return;
  imeEl.style.left = pos.x + window.scrollX - 1 + 'px';
  imeEl.style.top = pos.top + window.scrollY + 'px';
  imeEl.style.height = Math.max(pos.height, 18) + 'px';
}

/** preedit 的字体必须跟光标所在的块一致，否则标题里组合会明显偏小。 */
function setPreedit(text: string) {
  if (!text) {
    preeditEl.style.display = 'none';
    preeditEl.textContent = '';
    return;
  }
  if (!caret) {
    // 极早期（还没拿到光标）也要把字显示出来，否则用户看不到任何反馈
    preeditEl.textContent = text;
    preeditEl.style.display = 'block';
    return;
  }
  const pos = rectOf(caret.stop);
  if (pos) {
    // 增量重绘会换掉节点，旧引用取不到计算样式，退回当前元素
    const anchor = imeAnchorEl && imeAnchorEl.isConnected ? imeAnchorEl : pos.el;
    const cs = getComputedStyle(anchor);
    preeditEl.style.fontFamily = cs.fontFamily;
    preeditEl.style.fontSize = cs.fontSize;
    preeditEl.style.fontWeight = cs.fontWeight;
    preeditEl.style.lineHeight = cs.lineHeight;
    preeditEl.style.left = pos.x + window.scrollX + 'px';
    preeditEl.style.top = pos.top + window.scrollY + 'px';
    preeditEl.style.minHeight = Math.max(pos.height, 18) + 'px';
  }
  preeditEl.textContent = text;
  preeditEl.style.display = 'block';
}

// ime.js 的 JSDoc 把消息标成 object（它不认识 IPC 契约），这里收窄回 Wire
const ime = createIme({ send: (message) => send(message as Wire), setPreedit });

function blockBounds(): { start: number; end: number } | null {
  if (!caret) return null;
  const el = byBefore.get(caret.stop) || byAfter.get(caret.stop);
  const blk = el && el.closest<HTMLElement>('.blk');
  if (!blk) return null;
  return { start: Number(blk.dataset.bb), end: Number(blk.dataset.be) };
}

window.addEventListener('keydown', (event) => {
  // 查找 / 替换：任何视图都要能唤起，必须排在下面各视图分支之前。
  const accel = event.ctrlKey || event.metaKey;
  const accelKey = event.key.toLowerCase();
  if (accel && (accelKey === 'f' || accelKey === 'h')) {
    event.preventDefault();
    openFind(accelKey === 'h');
    return;
  }
  if (event.key === 'F3') {
    event.preventDefault();
    stepFind(event.shiftKey ? -1 : 1);
    return;
  }
  // 焦点在查找栏里：键盘归它自己。不返回的话每个字符都会被当成正文输入，
  // Ctrl+A/C/V 也会被内核抢走，输入框直接没法用。
  // 合成事件的 target 可能是 window（不是 Node），先做类型收窄再 contains。
  const target = event.target instanceof Node ? event.target : null;
  if (target && findEl.contains(target)) {
    if (event.key === 'Escape') {
      event.preventDefault();
      closeFind();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) replaceAllMatches();
      else stepFind(event.shiftKey ? -1 : 1);
    } else if (event.altKey) {
      // 与 VSCode 一致的开关快捷键：Alt+C / Alt+W / Alt+R
      const key = event.key.toLowerCase();
      if (key === 'c') { event.preventDefault(); toggleFindCase(); }
      else if (key === 'w') { event.preventDefault(); toggleFindWord(); }
      else if (key === 'r') { event.preventDefault(); toggleFindRegex(); }
    }
    return;
  }
  // 链接编辑面板同理：输入框自己收字符，Esc 关面板。
  if (target && linkEditEl.contains(target)) {
    if (event.key === 'Escape') {
      event.preventDefault();
      closeLinkEditor();
    }
    return;
  }
  // 重新加载：三个视图都能用，但输入框（查找栏 / 链接面板）里不抢键。
  // 必须在这里拦下——WebView 把 Ctrl+R 当刷新页面，漏掉 preventDefault
  // 会整个页面重载、内存状态全丢。
  if (accel && accelKey === 'r') {
    event.preventDefault();
    runAction('reload-file');
    return;
  }
  // 源码视图：textarea 自己就是编辑器，有原生光标/选区/撤销栈。
  // 只拦文件类快捷键；其余（输入、方向、Ctrl+A/C/V/Z/Y、Tab）全部放行，
  // 由 input 事件把文本整篇同步回内核。
  if (window.__mdviewMode === 'source') {
    if (event.ctrlKey || event.metaKey) {
      const key = event.key.toLowerCase();
      if (key === 's') {
        event.preventDefault();
        if (event.shiftKey) requestSavePath();
        else { flushSource(); send({ kind: 'save' }); }
        return;
      }
      if (key === 'n') {
        event.preventDefault();
        runAction(event.shiftKey ? 'new-window' : 'new-file');
        return;
      }
      if (key === 'o') { event.preventDefault(); runAction('open-file'); return; }
    }
    return;
  }
  // 阅读视图：只读，拦掉一切会改文档的输入，仅放行导航与文件快捷键。
  if (window.__mdviewMode === 'reading') {
    if (event.ctrlKey || event.metaKey) {
      const key = event.key.toLowerCase();
      if (key === 's') {
        event.preventDefault();
        if (event.shiftKey) requestSavePath();
        else { flushSource(); send({ kind: 'save' }); }
        return;
      }
      if (key === 'n') {
        event.preventDefault();
        runAction(event.shiftKey ? 'new-window' : 'new-file');
        return;
      }
      if (key === 'o') { event.preventDefault(); runAction('open-file'); return; }
    }
    // Escape 留给菜单；方向/翻页/滚动放行，其余编辑键吞掉
    const NAV = ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'PageUp', 'PageDown',
      'Home', 'End', 'Escape', 'Tab'];
    if (NAV.indexOf(event.key) < 0 && !(event.ctrlKey || event.metaKey || event.altKey)
        && event.key.length === 1) {
      event.preventDefault();
    }
    return;
  }

  // 组合期与 229 一律放行，preedit 交给输入法自己渲染（内核侧同样会挂起重绘）
  if (ime.shouldIgnoreKey(event)) return;

  const cmd = event.ctrlKey || event.metaKey;
  if (cmd) {
    const key = event.key.toLowerCase();
    if (key === 'home' || key === 'end') {
      event.preventDefault();
      send({ kind: 'setCaret', stop: key === 'home' ? 0 : maxStop, extend: event.shiftKey });
      return;
    }
    if (key === 'n') {
      event.preventDefault();
      runAction(event.shiftKey ? 'new-window' : 'new-file');
      return;
    }
    if (key === 'o') { event.preventDefault(); runAction('open-file'); return; }
    // Ctrl+S 在未命名文档上会由内核回 needs_path，前端再弹保存对话框；
    // Ctrl+Shift+S 是明确的「另存为」，总是弹框。
    if (key === 's') {
      event.preventDefault();
      if (event.shiftKey) requestSavePath();
      else { flushSource(); send({ kind: 'save' }); }
      return;
    }
    if (key === 'z') { event.preventDefault(); send({ kind: event.shiftKey ? 'redo' : 'undo' }); return; }
    if (key === 'y') { event.preventDefault(); send({ kind: 'redo' }); return; }
    // 全选、复制、剪切都必须走内核：原生选区已被清掉（见 selection.js），
    // 文本真值和折叠标记的状态都在内核里，浏览器默认行为给不出正确的源码
    if (key === 'a') { event.preventDefault(); send({ kind: 'selectAll' }); return; }
    if (key === 'c') { event.preventDefault(); send({ kind: 'copy' }); return; }
    if (key === 'x') { event.preventDefault(); send({ kind: 'cut' }); return; }
    return;
  }

  switch (event.key) {
    case 'Backspace': event.preventDefault(); send({ kind: 'backspace' }); return;
    case 'Delete': event.preventDefault(); send({ kind: 'deleteForward' }); return;
    case 'Enter': event.preventDefault(); send({ kind: event.shiftKey ? 'hardBreak' : 'newline' }); return;
    case 'Tab':
      event.preventDefault();
      // 表格内 Tab 跨一格，行末自动接到下一行；其余情况交给内核：
      // 列表项按两个空格一级缩进，普通块插两个空格。
      if (inTable()) send({ kind: 'tableMoveCell', delta: event.shiftKey ? -1 : 1 });
      else send({ kind: 'indent', outdent: event.shiftKey });
      return;
    case 'ArrowLeft':
      event.preventDefault();
      send({ kind: 'moveCaret', delta: -1, extend: event.shiftKey });
      return;
    case 'ArrowRight':
      event.preventDefault();
      send({ kind: 'moveCaret', delta: 1, extend: event.shiftKey });
      return;
    case 'ArrowUp':
      event.preventDefault();
      moveVertical(-1, event.shiftKey);
      return;
    case 'ArrowDown':
      event.preventDefault();
      moveVertical(1, event.shiftKey);
      return;
    case 'Home': {
      if (!caret) return;
      event.preventDefault();
      // 行首而不是块首：软换行段落里这两个位置不一样
      const stop = lines.edge(caret.stop, -1);
      const target = stop !== null ? stop : blockBounds()?.start;
      if (target !== undefined) send({ kind: 'setCaret', stop: target, extend: event.shiftKey });
      return;
    }
    case 'End': {
      if (!caret) return;
      event.preventDefault();
      const stop = lines.edge(caret.stop, 1);
      const target = stop !== null ? stop : blockBounds()?.end;
      if (target !== undefined) send({ kind: 'setCaret', stop: target, extend: event.shiftKey });
      return;
    }
    default:
      break;
  }

  // 可打印字符。中文输入走输入法的 composition 事件（见上面 ime 段），
  // 或直接粘贴，两条路都不经过这里；这里只补 ASCII 单字符的 keydown
  if (event.key.length === 1 && !event.altKey) {
    event.preventDefault();
    send({ kind: 'insertText', text: event.key });
  }
});

/**
 * 老 WebView 的 `paste` 事件把数据挂在 window 上（标准版本挂在 event 上）。
 * 这条兜底路径保留着，类型只在这里补一个形状。
 */
type WindowWithClipboard = Window & { clipboardData?: DataTransfer };

window.addEventListener('paste', (event) => {
  // 源码视图：让 textarea 原生粘贴源文本，随后 input 事件整篇同步给内核。
  if (window.__mdviewMode === 'source') return;
  // 查找 / 替换 / 链接面板里的输入框自己处理粘贴，别把内容插进正文
  const target = event.target instanceof HTMLElement ? event.target : null;
  if (target && (target.closest('#findbar') || target.closest('#linkedit'))) return;
  // Windows 上 `clipboardData` 一定在；老 WebView 才有 window 兜底，故一并保留
  const data = event.clipboardData ?? (window as WindowWithClipboard).clipboardData;
  const text = data?.getData('text/plain') ?? '';
  const html = data?.getData('text/html') ?? '';
  // 富文本优先：从浏览器 / Word 复制过来的标题、列表、表格转成 Markdown。
  // 纯文本复制（VSCode、终端）也会带一份 <div> 包起来的 HTML，那种不转，
  // 否则每一行都会被拆成独立段落、缩进也会丢。
  let payload = text;
  if (html && looksLikeRichHtml(html, text)) {
    const markdown = htmlToMarkdown(html);
    if (markdown) payload = markdown;
  }
  if (!payload) return;
  event.preventDefault();
  if (window.__mdviewMode === 'reading') return;  // 阅读视图只读
  send({ kind: 'insertText', text: payload });
});

// 文件拖进窗口时，浏览器默认会尝试把应用导航到该文件（整个界面被顶掉）。
// 真正的 Drop 由宿主在窗口层接管并 emit `drop-open`；这里只掐掉 webview 的默认行为。
window.addEventListener('dragover', (event) => event.preventDefault());
window.addEventListener('drop', (event) => {
  event.preventDefault();
  delete document.documentElement.dataset.drop;
});

// 源码视图：textarea 是输入源，去抖后把整篇 Markdown 写回内核。// 不回写 textarea 的值（内核 reply 里带 source 也不覆盖正在编辑的一侧），
// 只在换文档 / 切换视图时用 sourceText 灌入。
sourceEl.addEventListener('input', () => {
  if (window.__mdviewMode !== 'source') return;
  if (window.clearTimeout) window.clearTimeout(sourceSyncTimer);
  sourceSyncTimer = window.setTimeout(() => {
    sourceSyncTimer = 0;
    if (window.__mdviewMode !== 'source') return;
    if (sameSource(sourceEl.value, sourceText)) return;
    sourceText = sourceEl.value;
    send({ kind: 'replaceSource', text: sourceText });
  }, 120);
});
// 失焦时立即同步，避免切窗口/点菜单前还有去抖未落盘的内容
sourceEl.addEventListener('blur', flushSource);

// 输入法：所有 composition 事件都走 ime.js 的状态机，这里只做 DOM 搬运。
// 有一条必须守住——组合期间不碰 docEl，否则会把浏览器正在画的 preedit 冲掉。
imeEl.addEventListener('compositionstart', () => ime.start());
imeEl.addEventListener('compositionupdate', (event) => ime.update(event.data));
imeEl.addEventListener('compositionend', (event) => {
  // 先读 value 再清空：Safari 有时不回 data，只能退回输入框里的内容
  ime.end(event.data, imeEl.value);
  imeEl.value = '';
  placeIme();
});

// 不走 keydown 的输入路径（emoji 面板、死键、输入法直出）
imeEl.addEventListener('input', () => {
  if (ime.input(imeEl.value)) imeEl.value = '';
});

// 失焦时浏览器可能不发 compositionend（切窗口、点标题栏）。自己收尾，
// 否则内核会停在挂起态，回来之后任何输入都无声无息。
imeEl.addEventListener('blur', () => {
  if (!ime.isComposing()) return;
  ime.end('', '');
  imeEl.value = '';
});

// ---- 鼠标 ------------------------------------------------------------

/** 点击点 → 停靠点：命中元素左半取左边界，右半取右边界。 */
function stopFromPoint(x: number, y: number): number | null {
  const hit = document.elementFromPoint(x, y);
  if (!hit) return null;
  const el = hit.closest<HTMLElement>('[data-b]');
  // 没命中任何字符说明点在空白处（行尾右侧、行间、块内留白）。
  // 这里不能直接放弃：编辑器的惯例是「点哪儿光标就落在那一行」。
  if (!el) return stopFromBlank(x, y);
  const before = Number(el.dataset.b);
  if (!el.hasAttribute('data-a')) return before;
  const rect = el.getBoundingClientRect();
  if (rect.width === 0) return before;
  return x > rect.left + rect.width / 2 ? Number(el.dataset.a) : before;
}

/**
 * 空白处的落点：先借浏览器给出的「最近文本位置」定位到某一视觉行，
 * 再在这一行里按 x 选最近的停靠点。
 *
 * 不能按 y 遍历所有停靠点自己找行——那在长文档里拖拽时会退化成全表扫描。
 * `caretRangeFromPoint` 是浏览器自己实现的命中测试，代价与排版一致。
 */
function stopFromBlank(x: number, y: number): number | null {
  const seed = nearestStopFromBrowser(x, y);
  if (seed === null) return null;
  const stop = lines.nearestInLine(seed, x);
  return stop === null ? seed : stop;
}

/** 浏览器认为离这一点最近的文本位置，换算成停靠点序号。 */
function nearestStopFromBrowser(x: number, y: number): number | null {
  let node: Node | null = null;
  if (document.caretRangeFromPoint) {
    const range = document.caretRangeFromPoint(x, y);
    node = range ? range.startContainer : null;
  } else if (document.caretPositionFromPoint) {
    const pos = document.caretPositionFromPoint(x, y);
    node = pos ? pos.offsetNode : null;
  }
  if (!node) return null;
  // 文本节点落在字符里，元素节点就是宿主自己；两者都要能往上找到 [data-b]
  const host = (node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement) as HTMLElement | null;
  const holder = host && host.closest<HTMLElement>('[data-b]');
  if (!holder) return null;
  // 只用来定位行，方向由 nearestInLine 按 x 重选；给右端是为了让
  // 「点在字与字之间的空隙」也落到靠右的位置，与系统 caret 的习惯一致。
  const after = holder.dataset.a;
  return Number(after !== undefined ? after : holder.dataset.b);
}

/**
 * 拖动落点的探测范围：**正文真正可见的那一块**，而不是整块视口。
 *
 * 视口边缘会被固定浮层盖住——本项目的状态栏 `#bar` 就是 `position: fixed`，
 * 视口最下面那几十像素归它。若把探针压到视口下沿，`elementFromPoint` 命中
 * 状态栏，落点一路返回 null：页面自己在滚、选区却一动不动（自检正是这么抓到的）。
 */
function probeBox(): Box {
  const doc = docEl.getBoundingClientRect();
  const bar = barEl.getBoundingClientRect();
  const top = Math.max(doc.top, 0);
  const bottom = Math.min(
    doc.bottom,
    window.innerHeight,
    bar.height > 0 ? bar.top : window.innerHeight,
  );
  return {
    left: Math.max(doc.left, 0),
    top: top,
    // 状态栏比正文还高（窗口被压到极扁）时退回视口，别造出一个空的探测区
    right: Math.min(doc.right, window.innerWidth),
    bottom: bottom > top ? bottom : window.innerHeight,
  };
}

/**
 * 边缘自动滚动：指针停在视口上下沿（或拖出窗口）时，选区要自己往下长。
 *
 * 为什么要自走的帧循环：拖选靠 `mousemove` 驱动，而指针停在边缘时浏览器
 * 不再发新事件——选区就冻结在那一屏。这里每帧「滚一点 + 重新求指针下的落点」，
 * 指针不动、页面在动，落点自然一路延伸。
 *
 * 落在指针外的位置一律先压进正文可见区：拖出窗口时 `elementFromPoint` 返回 null，
 * 求不出落点；用户的意思很明确——选到屏幕边缘那一行。
 */
const dragScroll = createDragAutoScroll({
  speed: (y) => edgeSpeed(y, window.innerHeight, { margin: DRAG_EDGE, maxSpeed: DRAG_MAX_SPEED }),
  scrollBy: (dy) => window.scrollBy(0, dy),
  commit: () => {
    if (!dragging || !dragPoint) return;
    const point = clampToBox(dragPoint, probeBox(), 1);
    const stop = stopFromPoint(point.x, point.y);
    if (stop === null || stop === dragStop) return;
    dragStop = stop;
    // 走 enqueue 而不是 send：一帧一次的连续落点不该反复重置期望列
    enqueue({ kind: 'setCaret', stop, extend: true });
  },
  schedule: (fn) => window.requestAnimationFrame(fn),
  cancel: (id) => window.cancelAnimationFrame(id),
});

function endDrag() {
  if (!dragging) return;
  dragging = false;
  dragPoint = null;
  dragStop = null;
  dragScroll.stop();
}

docEl.addEventListener('mousedown', (event) => {
  const stop = stopFromPoint(event.clientX, event.clientY);
  if (stop === null) return;
  event.preventDefault();
  dragging = true;
  dragPoint = { x: event.clientX, y: event.clientY };
  dragStop = stop;
  focusActiveEditor();
  send({ kind: 'setCaret', stop, extend: event.shiftKey });
});

/**
 * 外链的打开方式：编辑态 Ctrl/Cmd+Click，阅读态单击。
 *
 * 投影里不产出 `<a href>`（链接字符只带 `data-href`，见 `preview.rs`），
 * 所以不存在「webview 自己把应用导航走」的问题。编辑态必须把单击留给
 * 光标定位——正文整块都是可编辑区，抢走单击等于废掉「点哪儿光标落哪儿」。
 */
let linkPress: { x: number; y: number } | null = null;

docEl.addEventListener('mousedown', (event) => {
  linkPress = { x: event.clientX, y: event.clientY };
});

docEl.addEventListener('click', (event) => {
  const start = linkPress;
  linkPress = null;
  const target = event.target as Element | null;
  const el = target?.closest<HTMLElement>('[data-href]');
  const image = target?.closest<HTMLElement>('.atom.img[data-asset]');
  // 拖选之后浏览器也会补发一次 click。位移超过阈值说明这次是选字，不是点链接。
  if (start && Math.hypot(event.clientX - start.x, event.clientY - start.y) > 4) return;
  const modifier = event.ctrlKey || event.metaKey;
  if (el?.dataset.href) {
    if (window.__mdviewMode === 'wysiwyg' && !modifier) {
      // 编辑态单击：不跳转，弹地址/文字面板；光标已经由 mousedown 落在链接上
      openLinkEditor(el);
      return;
    }
    if (window.__mdviewMode !== 'reading' && !modifier) return;
    event.preventDefault();
    invoke('open_url', { url: el.dataset.href }).catch((err) => toast('打不开链接：' + err));
    return;
  }
  if (image && window.__mdviewMode === 'wysiwyg' && !modifier) {
    openLinkEditor(image);
  }
});

// 脚注跳转：引用 → 定义，回链 → 引用。编辑态沿用外链的 Ctrl/Cmd 约定，
// 阅读态单击即可。不走 <a href> 的默认行为——mousedown 的光标定位会 preventDefault 吃掉它。
let fnPress: { x: number; y: number } | null = null;
docEl.addEventListener('mousedown', (event) => {
  fnPress = { x: event.clientX, y: event.clientY };
});
docEl.addEventListener('click', (event) => {
  const start = fnPress;
  fnPress = null;
  const el = (event.target as Element | null)?.closest<HTMLElement>('[data-fnref], [data-fnback]');
  if (!el) return;
  if (window.__mdviewMode !== 'reading' && !(event.ctrlKey || event.metaKey)) return;
  // 拖选之后浏览器也会补一次 click
  if (start && Math.hypot(event.clientX - start.x, event.clientY - start.y) > 4) return;
  event.preventDefault();
  const num = el.dataset.fnfref ?? el.dataset.fnfback;
  if (!num) return;
  const target = el.dataset.fnfref
    ? docEl.querySelector<HTMLElement>(`[id="fn${num}"]`)
    : docEl.querySelector<HTMLElement>(
        `[data-fnref="${num}"][data-fnbyte="${el.dataset.fnfbyte}"]`,
      );
  target?.scrollIntoView({ block: 'center', behavior: 'smooth' });
});

window.addEventListener('mousemove', (event) => {
  if (!dragging) return;
  // 在窗口外松手时 `mouseup` 收不到，靠这个兜底，否则指针一回到正文里
  // 就又接着改选区（而且自动滚动循环也停不下来）
  if (event.buttons === 0) {
    endDrag();
    return;
  }
  dragPoint = { x: event.clientX, y: event.clientY };
  const point = clampToBox(dragPoint, probeBox(), 1);
  const stop = stopFromPoint(point.x, point.y);
  if (stop !== null && stop !== dragStop) {
    dragStop = stop;
    send({ kind: 'setCaret', stop, extend: true });
  }
  // 在正文中部时这一步是空操作；进入边缘带才会真的排上帧
  dragScroll.update(event.clientY);
});

window.addEventListener('mouseup', endDrag);
// 切走窗口时既收不到 mouseup，也不会有 mousemove，必须自己收尾
window.addEventListener('blur', endDrag);

// 用户自己滚动的信号。只认输入事件，不认 `scroll`：平滑滚动期间 scroll 一直在
// 发，按它撤钉等于点击后立刻打回原形（见 releaseOutlinePin）。
window.addEventListener('wheel', releaseOutlinePin, { passive: true });
window.addEventListener('touchmove', releaseOutlinePin, { passive: true });
// 拖自绘滚动条：pointerdown 落在轨道或滑块上都算用户接管
window.addEventListener(
  'pointerdown',
  (event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (target && target.closest('.tb-vscroll, .tb-hscroll')) releaseOutlinePin();
  },
  { passive: true },
);
// 键盘导航（PageDown / Home / End…）。空格在正文里是输入，不算滚动。
window.addEventListener(
  'keydown',
  (event) => {
    const nav = ['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End'];
    if (nav.indexOf(event.key) >= 0) releaseOutlinePin();
  },
  { passive: true },
);

// 滚动时**不能**把光标拉回视口：光标是文档坐标，自己会跟着走。
// 这里只刷新像素（元素可能被 patch 换掉），滚动意图完全交给用户；
// 同时按锚点更新大纲高亮（当前节）。
window.addEventListener('scroll', () => {
  if (caret) applyCaret(caret, false);
  updateActiveOutline();
}, { passive: true });
// 窗口宽度变了，行会重新折行，旧的期望 x 已经没意义；光标可能因此落到视口外，
// 这里的落点没变却必须重新露出来，所以带 force
window.addEventListener('resize', () => {
  lines.reset();
  if (caret) applyCaret(caret, true, true);
  // 拖动中途缩放：边缘带的像素位置变了，重新判一次
  if (dragging && dragPoint) dragScroll.update(dragPoint.y);
  // 折行会改变标题的纵坐标，锚点要重测
  refreshOutlineAnchors();
});

// 大纲点击（事件委托：侧栏会被整块重建）
outlineEl.addEventListener('click', (event) => {
  const link = (event.target as Element | null)?.closest<HTMLElement>('a[data-idx]');
  if (!link) return;
  event.preventDefault();
  jumpToOutline(Number(link.dataset.idx));
});

// 状态栏按钮（事件委托：renderStatus 每次都会重建 innerHTML）
barEl.addEventListener('click', (event) => {
  const tableBtn = (event.target as Element | null)?.closest<HTMLElement>('[data-table]');
  if (!tableBtn) return;
  const ops: Record<string, Wire> = {
    'row+': { kind: 'tableInsertRow', after: true },
    'row-': { kind: 'tableDeleteRow' },
    'col+': { kind: 'tableInsertColumn', after: true },
    'col-': { kind: 'tableDeleteColumn' },
  };
  const message = ops[tableBtn.dataset.table ?? ''];
  if (message) send(message);
});

// ---- 自检 ------------------------------------------------------------

/**
 * `--self-test` 启动时由 Rust 侧在页面接管后调用。
 *
 * 合成 composition 事件走一遍真实链路：DOM 事件 → 状态机 → IPC → 内核事务
 * → 增量补丁 → 光标回填 → 镜像清理。操作系统的输入法没法在无人值守下驱动，
 * 但除 IME 本身之外，这条链路上跑的就是生产代码。
 */
window.__mdviewSelfTest = async function () {
  const checks: Check[] = [];
  const check = (name: string, ok: unknown, detail: unknown = '') =>
    checks.push({ name, ok: !!ok, detail: detail ? String(detail) : '' });
  const fire = (type: string, data?: string) =>
    imeEl.dispatchEvent(new CompositionEvent(type, { data: data || '', bubbles: true }));
  // 外壳组件已 Vue 化，点菜单后 DOM 在下一个微任务才更新。
  const nextTick = window.__mdviewNextTick || (() => Promise.resolve());

  try {
    check('焦点落在透明输入框', document.activeElement === imeEl,
      document.activeElement ? document.activeElement.id : 'none');

    const before = docEl.textContent;
    const rev0 = meta.revision;

    fire('compositionstart');
    await queue;
    fire('compositionupdate', 'zijiAnCi');
    check('组合期 preedit 可见', preeditEl.style.display === 'block');
    check('组合期 preedit 文本正确', preeditEl.textContent === 'zijiAnCi', preeditEl.textContent);
    check('组合期文档不变', docEl.textContent === before);
    check('组合期不产生事务', meta.revision === rev0);

    fire('compositionend', '自检词');
    await queue;
    check('提交后文档出现该词', docEl.textContent.indexOf('自检词') >= 0);
    check('一次提交只走一次事务', meta.revision === rev0 + 1, meta.revision + ' vs ' + (rev0 + 1));
    check('提交后镜像隐藏', preeditEl.style.display === 'none');
    check('提交后光标仍在', caret !== null && caret.stop > 0,
      caret ? caret.stop : '');

    send({ kind: 'undo' });
    await queue;
    check('一次撤销回到提交前', docEl.textContent === before);

    // 取消候选之后内核必须已复位，否则后续输入全被静默吞掉
    fire('compositionstart');
    await queue;
    fire('compositionend', '');
    await queue;
    send({ kind: 'insertText', text: '丙' });
    await queue;
    check('取消组合后仍能输入', docEl.textContent.indexOf('丙') >= 0);

    // ---- 上下移动 ----
    // 软换行只有浏览器算得出来，所以这一段的判据只能在这里跑。
    // 下面几条断言里的 `caret!` 是刻意的：boot 之后内核必报光标。真拿到 null 时，
    // 读属性会直接抛到外层 catch（报成「自检未抛异常」），比让断言悄悄判否更醒目。
    const press = (key: string, shift?: boolean) => window.dispatchEvent(
      new KeyboardEvent('keydown', { key, shiftKey: !!shift, bubbles: true, cancelable: true })
    );
    const caretX = () => {
      const pos = caret ? rectOf(caret.stop) : null;
      return pos ? Math.round(pos.x) : -1;
    };

    send({ kind: 'setCaret', stop: 0, extend: false });
    await queue;
    const firstBlock = caret!.block ?? -1;
    const firstX = caretX();
    press('ArrowDown');
    await queue;
    check('↓ 从标题进到下一块', caret!.block === firstBlock + 1, caret!.block + ' vs ' + (firstBlock + 1));
    check('↓ 之后 x 仍对齐同一列', Math.abs(caretX() - firstX) <= 3, caretX() + ' vs ' + firstX);

    press('ArrowDown');
    await queue;
    check('↓ 继续进到列表块', caret!.block === firstBlock + 2, String(caret!.block));

    press('ArrowUp');
    await queue;
    press('ArrowUp');
    await queue;
    check('↑ 两次回到标题块', caret!.block === firstBlock, String(caret!.block));

    press('ArrowUp');
    await queue;
    check('首行再按 ↑ 不动', caret!.block === firstBlock, String(caret!.block));
    check('首行按 ↑ 后停在停靠点 0', caret!.stop === 0, String(caret!.stop));

    // 按键连发：两下都还没回包就发出了，落点必须依次推进而不是原地打转
    send({ kind: 'setCaret', stop: 0, extend: false });
    await queue;
    press('ArrowDown');
    press('ArrowDown');
    await queue;
    check('连发 ↓ 两次跨两块', caret!.block === firstBlock + 2, String(caret!.block));

    // Home / End：单行段落里就等于块首块尾（软换行的分支由 caretline.test.js 覆盖）
    const body = docEl.children[firstBlock + 1] as HTMLElement;
    const bodyStart = Number(body.dataset.bb);
    const bodyEnd = Number(body.dataset.be);
    send({ kind: 'setCaret', stop: bodyStart + 2, extend: false });
    await queue;
    press('Home');
    await queue;
    check('Home 落到段落首', caret!.stop === bodyStart, caret!.stop + ' vs ' + bodyStart);
    press('End');
    await queue;
    check('End 落到段落尾', caret!.stop === bodyEnd, caret!.stop + ' vs ' + bodyEnd);

    // 点行尾右侧空白：那个位置没有命中任何字符，光标必须落到这一行的末尾。
    // 只靠 elementFromPoint 会直接放弃，用户看到的是「点了没反应」。
    send({ kind: 'setCaret', stop: bodyStart, extend: false });
    await queue;
    const tailStop = lines.edge(bodyEnd, 1);
    const tailRect = tailStop !== null ? rectOf(tailStop) : null;
    if (tailRect) {
      const docRect = docEl.getBoundingClientRect();
      const blankX = Math.min(tailRect.x + 80, docRect.right - 8);
      const blankY = tailRect.top + tailRect.height / 2;
      const at = document.elementFromPoint(blankX, blankY);
      check('空白处确实没有命中字符', !(at && at.closest('[data-b]')),
        at ? at.className || at.tagName : 'null');
      docEl.dispatchEvent(new MouseEvent('mousedown', {
        clientX: blankX, clientY: blankY, bubbles: true, cancelable: true,
      }));
      await queue;
      window.dispatchEvent(new MouseEvent('mouseup'));
      check('点行尾空白落到行尾', caret!.stop === tailStop, caret!.stop + ' vs ' + tailStop);
    } else {
      check('点行尾空白落到行尾', false, '取不到行尾矩形');
    }

    // ---- 拖到窗口边缘自动滚动 ----
    // 指针停在边缘时浏览器不再发 `mousemove`，选区只能靠自走的帧循环延伸。
    // 这里合成「在正文里按下 → 拖到视口下沿之外」，断言页面自己滚下去了、
    // 选区也跟着长。纯 Node 用例只能测几何，滚不滚得动只有这里能验。
    {
      window.scrollTo(0, 0);
      await new Promise((r) => setTimeout(r, 80));
      const dragScroller = document.scrollingElement || document.documentElement;
      const room = dragScroller.scrollHeight - window.innerHeight;
      // 帧循环靠 rAF 驱动，而不可见/被遮挡的窗口会把 rAF 完全停掉；
      // 先确认帧真的会跑，否则这里只会得到一条与代码无关的假红
      const rafAlive = await Promise.race([
        new Promise((r) => window.requestAnimationFrame(() => r(true))),
        new Promise((r) => setTimeout(() => r(false), 300)),
      ]);
      if (room < 200 || !rafAlive) {
        check('拖到边缘会自动滚动', true,
          '跳过（可滚 ' + room + 'px，rAF ' + (rafAlive ? '正常' : '被节流') + '）');
      } else {
        const firstRect = docEl.children[0].getBoundingClientRect();
        send({ kind: 'setCaret', stop: 0, extend: false });
        await queue;
        const from = { x: firstRect.left + 2, y: firstRect.top + firstRect.height / 2 };
        docEl.dispatchEvent(new MouseEvent('mousedown', {
          clientX: from.x, clientY: from.y, buttons: 1, bubbles: true, cancelable: true,
        }));
        await queue;
        const fromStop = caret ? caret.stop : 0;
        const scrolledFrom = window.scrollY;
        // 指针直接拖出视口下沿：合成事件里的 buttons 必须为 1，
        // 否则会被当成「在窗口外松了手」直接结束拖动
        window.dispatchEvent(new MouseEvent('mousemove', {
          clientX: from.x, clientY: window.innerHeight + 60,
          buttons: 1, bubbles: true, cancelable: true,
        }));
        await new Promise((r) => setTimeout(r, 260));
        const scrolledBy = window.scrollY - scrolledFrom;
        const dragged = await invoke<Snapshot>('snapshot');
        // 拖动中的锚点就是按下时的那个停靠点；head 必须已经跑到它下面几十行
        const anchor = dragged.selection ? dragged.selection[0] : -1;
        check('拖到边缘会自动向下滚', scrolledBy > 0, '+' + Math.round(scrolledBy) + 'px');
        check('选区随滚动一路延伸',
          !!dragged.selection && dragged.selection[1] > fromStop + 5,
          'anchor=' + anchor + ' head=' + (dragged.selection ? dragged.selection[1] : 'null') +
            ' 起点=' + fromStop);
        // 松手必须停：否则指针离开窗口之后页面会一直滚到文件末尾
        window.dispatchEvent(new MouseEvent('mouseup'));
        await queue;
        const stoppedAt = window.scrollY;
        await new Promise((r) => setTimeout(r, 200));
        check('松手后不再继续滚', window.scrollY === stoppedAt,
          stoppedAt + ' → ' + window.scrollY);
        window.scrollTo(0, 0);
        await new Promise((r) => setTimeout(r, 80));
      }
    }
    // 光标被滚出视口之后，按键要能把它找回来。
    //
    // 这里只测「reveal 仍然生效」，不测「滚动不被拽回」：`self-test` 里
    // 只能用 `scrollTo` 模拟滚动，而 Chromium 会抑制滚动过程中脚本发起的
    // 滚动，所以无论有没有这层修复都测不出差别——那种断言是假绿。
    send({ kind: 'setCaret', stop: 0, extend: false });
    await queue;
    const scroller = document.scrollingElement || document.documentElement;
    const maxScroll = scroller.scrollHeight - window.innerHeight;
    if (maxScroll < 120) {
      check('按键后光标重新可见', true, '文档太短，跳过（可滚 ' + maxScroll + 'px）');
    } else {
      window.scrollTo(0, maxScroll);
      await new Promise((r) => setTimeout(r, 120));
      press('ArrowDown');
      await queue;
      const back = caret ? rectOf(caret.stop) : null;
      check('按键后光标重新可见',
        !!back && back.top >= 0 && back.top < window.innerHeight,
        back ? 'top=' + Math.round(back.top) : '没有光标');
      window.scrollTo(0, 0);
      await new Promise((r) => setTimeout(r, 80));
    }

    // ---- 全选 / 复制 / 剪切 ----
    // 原生选区已被清掉（见 selection.js），Ctrl+A 只能由内核算，
    // 复制出去的也必须是源码：视图里 `**`、`# ` 是折叠的，照 DOM 拼会丢格式。
    const pressCtrl = (key: string) => window.dispatchEvent(
      new KeyboardEvent('keydown', {
        key, ctrlKey: true, bubbles: true, cancelable: true,
      })
    );

    send({ kind: 'setCaret', stop: 0, extend: false });
    await queue;
    pressCtrl('a');
    await queue;
    const all = await invoke<Snapshot>('snapshot');
    check('Ctrl+A 选到整篇',
      !!all.selection && all.selection[0] === 0 && all.selection[1] === all.stops - 1,
      JSON.stringify(all.selection) + ' / stops=' + all.stops);
    check('全选后高亮铺满可见字符', docEl.querySelectorAll('.sel').length > 0,
      String(docEl.querySelectorAll('.sel').length));

    // 走真实按键：Ctrl+C → 内核 → 回复 → 写剪贴板，这条链路上的每一环都别漏
    const beforeCopy = docEl.textContent;
    // 第一块是标题时，源码里必然以 `#` 开头——可见文本里没有它，
    // 所以这一条正好区分「复制源码」与「照 DOM 拼可见文本」
    const firstIsHeading = docEl.children[0].classList.contains('kind-heading');
    pressCtrl('c');
    await queue;
    const copied = lastClipboard;
    check('Ctrl+C 把源码而不是可见文本交给剪贴板',
      typeof copied === 'string' && copied.length > 0 &&
        (!firstIsHeading || copied.startsWith('#')),
      JSON.stringify((copied || '').slice(0, 24)));
    check('Ctrl+C 之后文档没被改动',
      docEl.textContent === beforeCopy, JSON.stringify(docEl.textContent.slice(0, 16)));
    // 剪贴板写入可能永远不 settle：窗口没拿到焦点时 Chromium 会把写请求
    // 挂起等一个永远不会来的授权。测试里必须自带超时，否则整条自检卡死
    const wrote = await Promise.race([
      copyText(copied || ''),
      new Promise((resolve) => setTimeout(() => resolve('timeout'), 1000)),
    ]);
    check('系统剪贴板可写', wrote === true, String(wrote));

    // 剪切：删掉选区、把同样的文本交出去，且一次撤销就能整体回来。
    // 拿不到足够长的文档就跳过——短文档里没有两个可选的停靠点
    send({ kind: 'setCaret', stop: 0, extend: false });
    await queue;
    if (all.stops > 6) {
      send({ kind: 'setCaret', stop: 3, extend: false });
      await queue;
      send({ kind: 'setCaret', stop: 5, extend: true });
      await queue;
      const beforeCut = docEl.textContent;
      lastClipboard = null;
      pressCtrl('x');
      await queue;
      // 上面刚赋成 null，TS 会把类型收窄成 null，而真正写入发生在 writeClipboard 里；
      // 读一次显式拓宽，别让类型收窄把断言变成「never」。
      const cut = lastClipboard as string | null;
      check('Ctrl+X 带走了选中的文本',
        typeof cut === 'string' && cut.length > 0,
        JSON.stringify((cut || '').slice(0, 12)));
      check('剪切后文档确实变短', docEl.textContent.length < beforeCut.length,
        docEl.textContent.length + ' vs ' + beforeCut.length);
      send({ kind: 'undo' });
      await queue;
      check('一次撤销恢复剪切', docEl.textContent === beforeCut,
        JSON.stringify(docEl.textContent.slice(0, 20)));
    } else {
      check('Ctrl+X 带走了选中的文本', true, '文档太短，跳过（stops=' + all.stops + '）');
    }

    // 原生选区的清理**不在这里测**：自检窗口拿不到键盘焦点，`addRange` 之后
    // 浏览器立刻就把选区收走了，关不关清理逻辑读到的都是 rangeCount=0——
    // 那种断言只会给假绿。判据由 `selection.test.js` 覆盖。

    // ---- M2：大纲侧栏 / 同步滚动 / 主题 ----
    const fresh = await invoke<Snapshot>('snapshot');
    check('大纲渲染出标题项',
      (fresh.outline || []).length > 0 && outlineEl.querySelectorAll('a[data-idx]').length === fresh.outline.length,
      'outline=' + (fresh.outline || []).length + ' links=' + outlineEl.querySelectorAll('a[data-idx]').length);
    check('大纲锚点按文档坐标递增',
      outlineAnchors.length === outline.length &&
        outlineAnchors.every((a, i) => i === 0 || a.top >= outlineAnchors[i - 1].top),
      JSON.stringify(outlineAnchors.map((a) => Math.round(a.top))));
    // 点大纲最后一项应把对应标题滚进视口（anchor 是有限值，target ≥ 0）
    if (outlineAnchors.length > 1) {
      const last = outlineAnchors.length - 1;
      const target = outlineScrollToAnchor(outlineAnchors[last].top, OUTLINE_LINE);
      check('大纲跳转目标在文档内', target >= 0 && isFinite(target),
        'target=' + Math.round(target));
    } else {
      check('大纲跳转目标在文档内', true, '只有一个标题，跳过');
    }
    // 大纲滚动条：与正文共用 .tb-vscroll 视觉，原生条让位，且内容溢出时
    // 自绘滑块会出现（样张标题少不溢出，临时撑高大纲再刷新来覆盖这条路径）
    const oTrack = document.getElementById('outline-scroll');
    const oThumb = document.getElementById('outline-scroll-thumb');
    const docTrack = document.getElementById('vscroll');
    const docThumb = document.getElementById('vscroll-thumb');
    check('大纲与正文共用自绘滚动条样式',
      !!oTrack && !!oThumb && !!docTrack && !!docThumb &&
        oTrack.className === docTrack.className && oThumb.className === docThumb.className,
      oTrack ? oTrack.className : '无大纲滚动条');
    check('大纲原生滚动条已让位',
      getComputedStyle(outlineEl).scrollbarWidth === 'none',
      getComputedStyle(outlineEl).scrollbarWidth);
    if (oThumb) {
      const probe = document.createElement('div');
      probe.style.height = '5000px';
      outlineEl.appendChild(probe);
      window.__mdviewRefreshScrollbar?.();
      check('大纲溢出时自绘滑块出现',
        oThumb.style.display !== 'none' && parseFloat(oThumb.style.height) >= 28,
        'display=' + oThumb.style.display + ' height=' + oThumb.style.height);
      probe.remove();
      window.__mdviewRefreshScrollbar?.();
    }
    // 主题：data-theme 已被 init 定下，切换按钮能翻到另一档
    const themeBefore = document.documentElement.getAttribute('data-theme');
    check('主题属性已初始化', themeBefore === 'light' || themeBefore === 'dark',
      String(themeBefore));
    // 记下原始偏好，测完原样恢复，不把「跟随系统」改成固定值
    let savedPref: string | null = null;
    try { savedPref = localStorage.getItem('mdview-theme'); } catch (_) {}
    const themeAfter = mdviewTheme.toggle();
    check('主题切换生效',
      themeAfter !== themeBefore &&
        document.documentElement.getAttribute('data-theme') === themeAfter,
      themeBefore + ' → ' + themeAfter);
    try {
      if (savedPref === null) localStorage.removeItem('mdview-theme');
      else localStorage.setItem('mdview-theme', savedPref);
    } catch (_) {}
    mdviewTheme.init(); // 按恢复后的偏好重新应用

    // 视图 → 主题 是三档子菜单，当前档带圆点；三个动作都要真正落地。
    // 视图菜单里现在有两个子菜单（外观 / 主题），所以按标签取，不能取第一个。
    const viewBtn = document.querySelector<HTMLElement>('#tb-menus [data-menu="view"]');
    if (viewBtn) {
      viewBtn.click();
      await nextTick();
      const popup = document.getElementById('tb-menu-popup');
      const triggers = popup
        ? Array.from(popup.querySelectorAll<HTMLElement>('.submenu-trigger'))
        : [];
      const labelOf = (el: HTMLElement | undefined) =>
        el?.querySelector('.label')?.textContent ?? '';
      const trigger = triggers.find((t) => labelOf(t) === '主题') ?? null;
      check('视图菜单有主题子菜单', !!trigger, triggers.map(labelOf).join('/') || '无子菜单');
      check('视图菜单有外观子菜单',
        triggers.some((t) => labelOf(t) === '外观'), triggers.map(labelOf).join('/'));
      if (trigger && popup) {
        trigger.dispatchEvent(new Event('pointerenter'));
        await nextTick();
        const subBtns = Array.from(
          popup.querySelectorAll<HTMLElement>('.tb-submenu button[data-action]')
        );
        const actions = subBtns.map((b) => b.dataset.action).join(',');
        check('主题子菜单是三档',
          actions === 'theme-system,theme-light,theme-dark', actions);
        const active = 'theme-' + mdviewTheme.mode();
        // 菜单是皮肤差异最容易翻车的地方，两项都按皮肤各验一遍：
        // ① 选中标记的形状由皮肤决定（macOS 圆点、HyperOS 强调色勾），只要求
        //    「当前档位有一个真正可见的标记」，不绑定具体类名；
        // ② 分隔线不能复用 --hairline —— HyperOS 为了无边框标题栏把它设成了
        //    transparent，复用就会连分隔线一起消失。
        const alphaOf = (color: string): number => {
          const m = color.match(/rgba?\(([^)]+)\)/);
          if (!m) return 0;
          const parts = m[1].split(',').map((v) => parseFloat(v));
          return parts.length > 3 ? parts[3] : 1;
        };
        const skinNow = mdviewSkin.mode();
        for (const skin of ['hyperos', 'macos'] as const) {
          mdviewSkin.set(skin);
          await new Promise((r) => requestAnimationFrame(r));
          const cur = Array.from(
            popup.querySelectorAll<HTMLElement>('.tb-submenu button[data-action]')
          ).find((b) => b.dataset.action === active);
          check(`当前档位带标记(${skin})`,
            !!cur && Array.from(cur.querySelector('.gutter[data-mark]')?.children ?? [])
              .some((el) => (el as HTMLElement).getClientRects().length > 0), active);
          const sepEl = popup.querySelector<HTMLElement>('.tb-sep');
          const sepBg = sepEl ? getComputedStyle(sepEl).backgroundColor : '';
          check(`菜单分隔线可见(${skin})`, alphaOf(sepBg) > .02, sepBg || '无分隔线');
        }
        mdviewSkin.set(skinNow);
        // 回归：鼠标移到子菜单项上不能让面板消失。普通菜单项的 pointerenter
        // 会收起子菜单，但子菜单内部的项必须豁免，否则永远点不到这些项。
        if (subBtns.length) {
          subBtns[0].dispatchEvent(new Event('pointerenter'));
          check('悬停子菜单项面板不消失',
            !!popup.querySelector('.tb-submenu'), '面板被误关');
        }
      }
      viewBtn.click(); // 收起菜单，后面的按键测试不受影响
    }
    const themePref = mdviewTheme.mode();
    window.__mdviewRunAction?.('theme-light');
    check('主题菜单选亮色生效',
      mdviewTheme.mode() === 'light' &&
        document.documentElement.getAttribute('data-theme') === 'light', mdviewTheme.mode());
    window.__mdviewRunAction?.('theme-dark');
    check('主题菜单选暗色生效',
      mdviewTheme.mode() === 'dark' &&
        document.documentElement.getAttribute('data-theme') === 'dark', mdviewTheme.mode());
    window.__mdviewRunAction?.('theme-system');
    check('主题菜单跟随系统生效',
      mdviewTheme.mode() === 'system' &&
        document.documentElement.getAttribute('data-theme') ===
          mdviewTheme.resolve(null, mdviewTheme.systemTheme()), mdviewTheme.mode());
    mdviewTheme.set(themePref); // 恢复原始偏好

    // 外观（皮肤）与主题正交：切换要真的写到 <html data-skin>，不只是菜单圆点
    const skinPref = mdviewSkin.mode();
    window.__mdviewRunAction?.('skin-hyperos');
    check('外观切到 HyperOS 生效',
      mdviewSkin.mode() === 'hyperos' &&
        document.documentElement.getAttribute('data-skin') === 'hyperos', mdviewSkin.mode());
    window.__mdviewRunAction?.('skin-macos');
    check('外观切到 macOS 生效',
      mdviewSkin.mode() === 'macos' &&
        document.documentElement.getAttribute('data-skin') === 'macos', mdviewSkin.mode());
    mdviewSkin.set(skinPref); // 恢复原始皮肤，别把评审者的选择改掉

    // 标题栏标识走 asset import，路径写错时 <img> 只会静默空着，肉眼很难发现。
    // 这里按解码后的实际尺寸判断，而不是看 src 有没有值。
    {
      const logo = document.querySelector<HTMLImageElement>('#titlebar .tb-logo img');
      check('标题栏标识已加载',
        !!logo && logo.complete && logo.naturalWidth > 0,
        logo ? `${logo.naturalWidth}×${logo.naturalHeight}` : '无 img');
    }

    // 三种视图必须真正切换 DOM，而不是只换菜单圆点、正文永远是所见即所得
    runAction('mode-source');
    check('切到源码视图 data-view=source',
      document.documentElement.dataset.view === 'source',
      String(document.documentElement.dataset.view));
    check('源码视图显示 Markdown 源文本而非 HTML',
      sourceEl.value.length > 0 && !sourceEl.value.includes('<h1') &&
        sourceEl.value.includes('#'),
      sourceEl.value.slice(0, 40));
    check('源码视图焦点在 textarea',
      document.activeElement === sourceEl,
      document.activeElement?.id ?? '无');
    runAction('mode-reading');
    check('切到阅读视图 data-view=reading',
      document.documentElement.dataset.view === 'reading',
      String(document.documentElement.dataset.view));
    // 阅读视图隐藏了状态栏，HyperOS 的大纲是个浮起卡片、bottom 按状态栏高度
    // 算的固定值，不跟着变就会在底部空出一条 46px 的缝（看起来像大纲没长到底）。
    // 两套皮肤都要量：HyperOS 期望留 --canvas-gap，macOS 贴边为 0。
    for (const skin of ['hyperos', 'macos'] as const) {
      window.__mdviewRunAction?.(`skin-${skin}`);
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      const navBox = document.getElementById('outline')?.getBoundingClientRect();
      const gapPx = parseFloat(
        getComputedStyle(document.documentElement).getPropertyValue('--canvas-gap'),
      ) || 0;
      check(`阅读视图大纲贴到底边(${skin})`,
        !!navBox && Math.abs(window.innerHeight - navBox.bottom - gapPx) <= 1.5,
        navBox ? `底部缝 ${+(window.innerHeight - navBox.bottom).toFixed(1)}px / 期望 ${gapPx}px` : '无大纲');
    }
    mdviewSkin.set(skinPref); // 恢复原始皮肤
    runAction('mode-wysiwyg');
    check('切回所见即所得 data-view=wysiwyg',
      document.documentElement.dataset.view === 'wysiwyg',
      String(document.documentElement.dataset.view));

    // 表格：网格渲染 + Tab 跨格 + 行增删，整条 IPC 链路都要走到
    const table = docEl.querySelector<HTMLElement>('.md-table');
    check('表格渲染成真实网格', !!table && !!table.querySelector('[data-cell="0,0"]'));
    if (table) {
      // 边框/圆角是纯 CSS，只有真实 WebView2 的计算样式能证明样式表确实嵌进来了
      const cs = getComputedStyle(table);
      check('表格默认边框为 1px',
        cs.borderTopWidth === '1px' && cs.borderRightWidth === '1px' &&
          cs.borderBottomWidth === '1px' && cs.borderLeftWidth === '1px',
        [cs.borderTopWidth, cs.borderRightWidth, cs.borderBottomWidth, cs.borderLeftWidth].join('/'));
      check('表格圆角为 4px',
        cs.borderTopLeftRadius === '4px' && cs.borderTopRightRadius === '4px' &&
          cs.borderBottomLeftRadius === '4px' && cs.borderBottomRightRadius === '4px',
        cs.borderTopLeftRadius);
      const head = table.querySelector<HTMLElement>('[data-cell="0,0"] [data-b]');
      const firstStop = head ? Number(head.dataset.b) : -1;
      check('表格首格有停靠点', firstStop >= 0, String(firstStop));
      if (firstStop >= 0) {
        send({ kind: 'setCaret', stop: firstStop, extend: false });
        await queue;
        check('表格内显示行列工具条',
          inTable() && !!barEl.querySelector('[data-table="row+"]'));
        const beforeTab = caret ? caret.stop : -1;
        send({ kind: 'tableMoveCell', delta: 1 });
        await queue;
        check('Tab 跨到下一格', caret !== null && caret.stop !== beforeTab,
          beforeTab + ' → ' + (caret ? caret.stop : 'null'));
        // 行列增删的几何正确性由 Rust 单测覆盖；这里只验证浏览器 → IPC → 内核
        // 的事务链路确实通了（revision 前进）。
        const revBefore = meta.revision;
        send({ kind: 'tableInsertRow', after: true });
        await queue;
        check('插行事务经 IPC 到达内核', meta.revision > revBefore,
          revBefore + ' → ' + meta.revision);
        send({ kind: 'undo' });
        await queue;
        check('插行后可撤销', meta.revision > revBefore);
      }
    }

    // 文件 → 打开文件：菜单项接线 + 按路径注入走完整 IPC → 内核 → 全量重绘链路。
    // 用临时文件，验证完打开回原样张，避免影响可能的后续断言。
    const fileMenu = document.querySelector<HTMLElement>('#tb-menus [data-menu="file"]');
    if (fileMenu) {
      fileMenu.click();
      await nextTick();
      const openItem = document.querySelector('#tb-menu-popup button[data-action="open-file"]');
      check('文件菜单有打开文件项', !!openItem,
        openItem ? openItem.textContent : '无');
      const saveAsItem = document.querySelector('#tb-menu-popup button[data-action="save-as"]');
      check('文件菜单有另存为项', !!saveAsItem,
        saveAsItem ? saveAsItem.textContent : '无');
      const newItem = document.querySelector('#tb-menu-popup button[data-action="new-file"]');
      check('文件菜单有新建项', !!newItem,
        newItem ? newItem.textContent : '无');
      const recentTrigger = document.querySelector('#tb-menu-popup .submenu-trigger');
      check('文件菜单有最近打开项',
        !!recentTrigger && (recentTrigger.textContent || '').includes('最近打开'),
        recentTrigger ? recentTrigger.textContent : '无');
      fileMenu.click(); // 收起
    } else {
      check('文件菜单有打开文件项', false, '无文件菜单');
    }
    const originalPath = meta.path;
    try {
      // 用注入路径打开已知存在的样张自身（不弹对话框），验证 open_file 返回
      // 完整快照、前端能全量重绘、光标归零。Rust 单测另覆盖了「换成不同文件」。
      const snap = await invoke<Snapshot | null>('open_file', { path: originalPath });
      check('open_file 返回完整快照',
        !!snap && Array.isArray(snap.blocks) && snap.blocks.length > 0 && snap.path === originalPath,
        snap ? 'blocks=' + snap.blocks.length + ' path=' + snap.path : 'null');
      if (snap) {
        applySnapshot(snap);
        check('打开文件后正文全量重绘',
          docEl.children.length === snap.blocks.length,
          docEl.children.length + ' vs ' + snap.blocks.length);
        check('打开文件后回到文档开头', caret !== null && caret.stop === 0,
          caret ? String(caret.stop) : 'null');
      }
    } catch (err) {
      check('open_file 注入路径可用', false, String(err));
    }

    // 新建 + 未命名保存：验证「没有路径时必须问，而不是假装写过」这条链路。
    // 这里直接 invoke('dispatch') 而不走 send/applyReply——后者收到 needs_path
    // 会真的弹出保存对话框，把自检挂住。
    try {
      const fresh = await invoke<Snapshot | null>('new_file');
      check('新建得到未命名空文档',
        !!fresh && fresh.path === null && !fresh.dirty && fresh.stops >= 1,
        fresh ? 'path=' + fresh.path + ' stops=' + fresh.stops : 'null');
      if (fresh) adoptSnapshot(fresh);
      // 先敲一个字让它变脏，再看保存是否如实要求路径（而不是回「已保存」）
      send({ kind: 'insertText', text: '草' });
      await queue;
      const counted = await invoke<Snapshot>('snapshot');
      check('字数统计跟随输入', counted.words === 1, String(counted.words));
      check('状态栏显示字数', (barEl.textContent || '').includes('字数'),
        (barEl.textContent || '').slice(0, 24));
      const reply = await invoke<Reply>('dispatch', { message: { kind: 'save' } });
      check('未命名保存要求路径',
        reply.needs_path === true && reply.saved === false && reply.dirty === true,
        'needs_path=' + reply.needs_path + ' saved=' + reply.saved + ' dirty=' + reply.dirty);
    } catch (err) {
      check('新建链路可用', false, String(err));
    }

    // ---- 查找 / 替换 ----
    // 在刚新建的「草」文档上跑，命中数确定；替换走真实 IPC → 事务 → 重绘。
    try {
      runAction('find');
      check('查找栏打开并聚焦输入框',
        !findEl.hidden && document.activeElement === findInput,
        document.activeElement?.id ?? '无');
      findInput.value = '草';
      findInput.dispatchEvent(new Event('input', { bubbles: true }));
      await queue;
      check('查找命中一处', findCount.textContent === '1/1', String(findCount.textContent));
      check('命中高亮落在正文', docEl.querySelectorAll('.find-hit').length > 0,
        String(docEl.querySelectorAll('.find-hit').length));
      check('查找栏有 Aa/ab/.* 三个开关',
        ['case', 'word', 'regex'].every((name) => !!findEl.querySelector(`[data-find="${name}"]`)),
        '');
      // 正则写错时计数位要显示「无效正则」，而不是静默无结果
      toggleFindRegex();
      findInput.value = '([';
      findInput.dispatchEvent(new Event('input', { bubbles: true }));
      check('无效正则给出提示', findCount.textContent === '无效正则',
        String(findCount.textContent));
      toggleFindRegex();
      findInput.value = '草';
      findInput.dispatchEvent(new Event('input', { bubbles: true }));
      await queue;
      check('取消失败正则后命中恢复', findCount.textContent === '1/1',
        String(findCount.textContent));
      replaceInput.value = '青';
      replaceAllMatches();
      await queue;
      const replaced = await invoke<Snapshot>('snapshot');
      check('全部替换写回源码', replaced.source === '青', JSON.stringify(replaced.source));
      closeFind();
      check('查找栏可关闭', findEl.hidden, String(findEl.hidden));
    } catch (err) {
      check('查找替换链路可用', false, String(err));
    }

    // ---- Tab 缩进 / Shift+Tab 反缩进 ----
    try {
      send({ kind: 'setCaret', stop: 0, extend: false });
      await queue;
      press('Tab');
      await queue;
      const tabbed = await invoke<Snapshot>('snapshot');
      check('Tab 在段落插入两个空格', tabbed.source === '  青', JSON.stringify(tabbed.source));
      press('Tab', true);
      await queue;
      const outdented = await invoke<Snapshot>('snapshot');
      check('Shift+Tab 反缩进', outdented.source === '青', JSON.stringify(outdented.source));
    } catch (err) {
      check('Tab 缩进链路可用', false, String(err));
    }

    // ---- 链接 / 图片行内编辑 ----
    try {
      send({ kind: 'replaceSource', text: '[官网](a.md)' });
      await queue;
      const linkEl = docEl.querySelector<HTMLElement>('[data-href]');
      check('链接字符带 data-href', !!linkEl && linkEl.dataset.href === 'a.md',
        linkEl ? String(linkEl.dataset.href) : '无');
      if (linkEl) {
        openLinkEditor(linkEl);
        check('链接面板带出原地址与原文字',
          !linkEditEl.hidden && linkUrlInput.value === 'a.md' &&
            linkTextInput.value === '官网',
          linkUrlInput.value + ' / ' + linkTextInput.value);
        linkUrlInput.value = 'b.md';
        submitLinkEdit(false);
        await queue;
        const edited = await invoke<Snapshot>('snapshot');
        check('链接改写写回源码', edited.source === '[官网](b.md)',
          JSON.stringify(edited.source));
      }

      send({ kind: 'replaceSource', text: '![图](a.png)' });
      await queue;
      const imgEl = docEl.querySelector<HTMLElement>('.atom.img[data-asset]');
      check('图片渲染为原子节点', !!imgEl && imgEl.dataset.asset === 'a.png',
        imgEl ? String(imgEl.dataset.asset) : '无');
      if (imgEl) {
        openLinkEditor(imgEl);
        check('图片面板带出 alt 与原地址',
          !linkEditEl.hidden && linkTextInput.value === '图' && linkUrlInput.value === 'a.png',
          linkTextInput.value + ' / ' + linkUrlInput.value);
        submitLinkEdit(true);
        await queue;
        const removed = await invoke<Snapshot>('snapshot');
        check('删除图片写回源码', removed.source === '', JSON.stringify(removed.source));
      }
    } catch (err) {
      check('链接图片编辑链路可用', false, String(err));
    }
  } catch (err) {
    check('自检未抛异常', false, String(err));
  }

  const failed = checks.filter((c) => !c.ok);
  // 不能叫 lines：那会遮蔽全局的视觉行模块，函数内前面用到它时直接 TDZ 报错
  const summary = checks.map(
    (c) => (c.ok ? '· ' : '× ') + c.name + (c.detail ? '(' + c.detail + ')' : '')
  );
  const report =
    (failed.length ? 'FAIL ' : 'OK ') +
    (checks.length - failed.length) + '/' + checks.length + ' ' + summary.join(' | ');
  await invoke('selftest', { report });
};

boot();

// 前端只做两件事：把「停靠点」映射成像素，把用户操作翻译成消息。
// 文本真值与解析结果都在 Rust 侧，这里不留任何文档状态。
'use strict';

const { invoke } = window.__TAURI__.core;

const docEl = document.getElementById('doc');
const caretEl = document.getElementById('caret');
const preeditEl = document.getElementById('preedit');
const imeEl = document.getElementById('ime');
const barEl = document.getElementById('bar');
const toastEl = document.getElementById('toast');

let byBefore = new Map();   // 停靠点 → 从它开始的元素
let byAfter = new Map();    // 停靠点 → 在它结束的元素
let marks = [];             // 全部带 data-b 的元素
let maxStop = -1;           // 最大停靠点序号，供上下移动取上界
let caret = null;           // 最近一次内核回报的光标
let pendingStop = null;     // 已发出、尚未回包的垂直移动落点
let meta = { revision: 0, dirty: false, path: null, stops: 0 };
let dragging = false;
let dragPoint = null;       // 拖动期间指针的视口坐标（自动滚动要按它反复求落点）
let dragStop = null;        // 拖动中已发出的落点，避免同一落点每帧都过一趟 IPC
let imeAnchorEl = null;     // preedit 复制字号用的参照元素（光标所在的字）
let revealedStop = null;    // 上一次把光标滚进视口时的停靠点

/** 光标进出视口时上下各留的余量，避免贴着边缘打字。 */
const REVEAL_MARGIN = 32;
/** 拖动时指针进入视口上下多深就开始自动滚动，以及每帧最多滚多少。 */
const DRAG_EDGE = 48;
const DRAG_MAX_SPEED = 24;

// ---- 渲染 ------------------------------------------------------------

function fragments(html) {
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
  for (const el of docEl.querySelectorAll('[data-b]')) {
    const before = Number(el.dataset.b);
    if (!byBefore.has(before)) byBefore.set(before, el);
    const after = el.hasAttribute('data-a') ? Number(el.dataset.a) : before;
    if (el.hasAttribute('data-a')) byAfter.set(after, el);
    if (before > maxStop) maxStop = before;
    if (after > maxStop) maxStop = after;
    marks.push(el);
  }
}

function renderAll(blocks) {
  docEl.replaceChildren(...fragments(blocks.map((b) => b.html).join('')));
  reindex();
}

/**
 * 增量重绘。前后缀比对保证 [0,start) 与尾部原样不动，
 * 只有 [start,end) 是新的块，需要换掉的旧块数由 old_len - new_len 推出。
 */
function applyPatch(patch) {
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
}

// ---- 光标 ------------------------------------------------------------

/**
 * 停靠点 → 像素。停靠点的 x 就是「以它结尾的元素的右边缘」，
 * 没有这个元素时取「以它开始的元素的左边缘」。全部由浏览器测量，内核不参与。
 *
 * `mk` 用于上下移动时的并列判决：折叠标记零宽，它的右端与相邻文字共享同一列，
 * 落到标记里会让下一次退格先碰标记。`self-test` 里对这一点有断言。
 */
function rectOf(stop) {
  const probes = [
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
function revealCaret(pos) {
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
function applyCaret(next, reveal = true, force = false) {
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

function paintSelection(range) {
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
  if (!shouldClearSelection(sel, imeEl)) return;
  clearingSelection = true;
  sel.removeAllRanges();
  clearingSelection = false;
});

function renderStatus() {
  const blocks = docEl.children.length;
  const block = caret && caret.block !== null && caret.block !== undefined ? caret.block + 1 : '-';
  const name = meta.path ? meta.path.split(/[\\/]/).pop() : '未命名';
  barEl.innerHTML =
    '<span>' + name + (meta.dirty ? ' <b class="dirty">●</b>' : '') + '</span>' +
    '<span>块 <b>' + block + '/' + blocks + '</b></span>' +
    '<span>停靠点 <b>' + (caret ? caret.stop : '-') + '</b></span>' +
    '<span>列 <b>' + (caret && caret.visual !== null ? caret.visual : '-') + '</b></span>' +
    '<span>r<b>' + meta.revision + '</b></span>' +
    '<span class="hint">← → 移动 · Shift 扩选 · Enter 分段 · Ctrl+S 保存 · Ctrl+Z 撤销</span>';
}

let toastTimer = 0;
function toast(text) {
  toastEl.textContent = text;
  toastEl.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.remove('show'), 1400);
}

// ---- IPC -------------------------------------------------------------

// 所有消息串行发出：内核虽然有锁，但 DOM 的 patch 必须按顺序应用
let queue = Promise.resolve();

/** 只入队，不动状态。上下移动专用，否则期望列每步都被清掉。 */
function enqueue(message) {
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
function send(message) {
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
function moveVertical(dir, extend) {
  if (!caret) return;
  const from = pendingStop === null ? caret.stop : pendingStop;
  const target = lines.step(from, dir);
  // 已在首行/末行：标准行为是不动，而不是跳到块首
  if (target === null) return;
  pendingStop = target;
  enqueue({ kind: 'setCaret', stop: target, extend });
}

async function dispatchOne(message) {
  const reply = await invoke('dispatch', { message });
  applyReply(reply);
}

function applyReply(reply) {
  if (reply.full) {
    renderAll(reply.full);
  } else if (reply.patch) {
    applyPatch(reply.patch);
  }
  if (reply.caret) applyCaret(reply.caret);
  paintSelection(reply.selection);
  meta.revision = reply.revision;
  meta.dirty = reply.dirty;
  if (typeof reply.clipboard === 'string') writeClipboard(reply.clipboard);
  if (reply.conflict) toast('文件已被外部修改，保存被拒');
  if (reply.saved) toast('已保存');
  renderStatus();
}

/**
 * 把内核算出的选区源码交给系统剪贴板。
 *
 * 写入是异步的（还要退回到 execCommand），所以**不 await**：剪贴板失败不该
 * 拖住编辑循环，最多弹一条提示。退回路径会抢走输入框焦点，写完要还回去。
 */
let lastClipboard = null;   // 最近一次交出去的文本，自检用它核对整条链路

function writeClipboard(text) {
  lastClipboard = text;
  copyText(text)
    .then((ok) => {
      if (!ok) toast('复制失败：系统剪贴板不可用');
    })
    .catch(() => toast('复制失败：系统剪贴板不可用'))
    .finally(() => {
      if (document.activeElement !== imeEl) imeEl.focus({ preventScroll: true });
    });
}

async function boot() {
  const snap = await invoke('snapshot');
  meta = { revision: snap.revision, dirty: snap.dirty, path: snap.path, stops: snap.stops };
  renderAll(snap.blocks);
  applyCaret(snap.caret);
  paintSelection(snap.selection);
  // 焦点必须落在透明输入框上：输入法只认真正的可编辑元素
  imeEl.focus({ preventScroll: true });
  // 给内核一个「页面已接管」的信号，日志里能对上游标是否真的画出来了
  invoke('ready').catch(() => {});
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
function setPreedit(text) {
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

const ime = createIme({ send: (message) => send(message), setPreedit });

function blockBounds() {
  if (!caret) return null;
  const el = byBefore.get(caret.stop) || byAfter.get(caret.stop);
  const blk = el && el.closest('.blk');
  if (!blk) return null;
  return { start: Number(blk.dataset.bb), end: Number(blk.dataset.be) };
}

window.addEventListener('keydown', (event) => {
  // 组合期与 229 一律放行，preedit 交给输入法自己渲染（内核侧同样会挂起重绘）
  if (ime.shouldIgnoreKey(event)) return;

  const cmd = event.ctrlKey || event.metaKey;
  if (cmd) {
    const key = event.key.toLowerCase();
    if (key === 's') { event.preventDefault(); send({ kind: 'save' }); return; }
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
    case 'Tab': event.preventDefault(); send({ kind: 'insertText', text: '  ' }); return;
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
      const target = stop !== null ? stop : (blockBounds() || {}).start;
      if (target !== undefined) send({ kind: 'setCaret', stop: target, extend: event.shiftKey });
      return;
    }
    case 'End': {
      if (!caret) return;
      event.preventDefault();
      const stop = lines.edge(caret.stop, 1);
      const target = stop !== null ? stop : (blockBounds() || {}).end;
      if (target !== undefined) send({ kind: 'setCaret', stop: target, extend: event.shiftKey });
      return;
    }
    default:
      break;
  }

  // 可打印字符。中文输入走输入法或粘贴，见 README 里的说明
  if (event.key.length === 1 && !event.altKey) {
    event.preventDefault();
    send({ kind: 'insertText', text: event.key });
  }
});

window.addEventListener('paste', (event) => {
  const text = (event.clipboardData || window.clipboardData).getData('text/plain');
  if (!text) return;
  event.preventDefault();
  send({ kind: 'insertText', text });
});

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
function stopFromPoint(x, y) {
  const hit = document.elementFromPoint(x, y);
  if (!hit) return null;
  const el = hit.closest('[data-b]');
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
function stopFromBlank(x, y) {
  const seed = nearestStopFromBrowser(x, y);
  if (seed === null) return null;
  const stop = lines.nearestInLine(seed, x);
  return stop === null ? seed : stop;
}

/** 浏览器认为离这一点最近的文本位置，换算成停靠点序号。 */
function nearestStopFromBrowser(x, y) {
  let node = null;
  if (document.caretRangeFromPoint) {
    const range = document.caretRangeFromPoint(x, y);
    node = range ? range.startContainer : null;
  } else if (document.caretPositionFromPoint) {
    const pos = document.caretPositionFromPoint(x, y);
    node = pos ? pos.offsetNode : null;
  }
  if (!node) return null;
  const host = node.nodeType === 1 ? node : node.parentElement;
  const holder = host && host.closest('[data-b]');
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
function probeBox() {
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
  imeEl.focus({ preventScroll: true });
  send({ kind: 'setCaret', stop, extend: event.shiftKey });
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

// 滚动时**不能**把光标拉回视口：光标是文档坐标，自己会跟着走。
// 这里只刷新像素（元素可能被 patch 换掉），滚动意图完全交给用户。
window.addEventListener('scroll', () => { if (caret) applyCaret(caret, false); }, { passive: true });
// 窗口宽度变了，行会重新折行，旧的期望 x 已经没意义；光标可能因此落到视口外，
// 这里的落点没变却必须重新露出来，所以带 force
window.addEventListener('resize', () => {
  lines.reset();
  if (caret) applyCaret(caret, true, true);
  // 拖动中途缩放：边缘带的像素位置变了，重新判一次
  if (dragging && dragPoint) dragScroll.update(dragPoint.y);
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
  const checks = [];
  const check = (name, ok, detail) => checks.push({ name, ok: !!ok, detail: detail || '' });
  const fire = (type, data) =>
    imeEl.dispatchEvent(new CompositionEvent(type, { data: data || '', bubbles: true }));

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
    check('提交后光标仍在', !!caret && caret.stop > 0, caret && caret.stop);

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
    const press = (key, shift) => window.dispatchEvent(
      new KeyboardEvent('keydown', { key, shiftKey: !!shift, bubbles: true, cancelable: true })
    );
    const caretX = () => {
      const pos = caret && rectOf(caret.stop);
      return pos ? Math.round(pos.x) : -1;
    };

    send({ kind: 'setCaret', stop: 0, extend: false });
    await queue;
    const firstBlock = caret.block;
    const firstX = caretX();
    press('ArrowDown');
    await queue;
    check('↓ 从标题进到下一块', caret.block === firstBlock + 1, caret.block + ' vs ' + (firstBlock + 1));
    check('↓ 之后 x 仍对齐同一列', Math.abs(caretX() - firstX) <= 3, caretX() + ' vs ' + firstX);

    press('ArrowDown');
    await queue;
    check('↓ 继续进到列表块', caret.block === firstBlock + 2, String(caret.block));

    press('ArrowUp');
    await queue;
    press('ArrowUp');
    await queue;
    check('↑ 两次回到标题块', caret.block === firstBlock, String(caret.block));

    press('ArrowUp');
    await queue;
    check('首行再按 ↑ 不动', caret.block === firstBlock, String(caret.block));
    check('首行按 ↑ 后停在停靠点 0', caret.stop === 0, String(caret.stop));

    // 按键连发：两下都还没回包就发出了，落点必须依次推进而不是原地打转
    send({ kind: 'setCaret', stop: 0, extend: false });
    await queue;
    press('ArrowDown');
    press('ArrowDown');
    await queue;
    check('连发 ↓ 两次跨两块', caret.block === firstBlock + 2, String(caret.block));

    // Home / End：单行段落里就等于块首块尾（软换行的分支由 caretline.test.js 覆盖）
    const body = docEl.children[firstBlock + 1];
    const bodyStart = Number(body.dataset.bb);
    const bodyEnd = Number(body.dataset.be);
    send({ kind: 'setCaret', stop: bodyStart + 2, extend: false });
    await queue;
    press('Home');
    await queue;
    check('Home 落到段落首', caret.stop === bodyStart, caret.stop + ' vs ' + bodyStart);
    press('End');
    await queue;
    check('End 落到段落尾', caret.stop === bodyEnd, caret.stop + ' vs ' + bodyEnd);

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
      check('点行尾空白落到行尾', caret.stop === tailStop, caret.stop + ' vs ' + tailStop);
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
        const dragged = await invoke('snapshot');
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
    const pressCtrl = (key) => window.dispatchEvent(
      new KeyboardEvent('keydown', {
        key, ctrlKey: true, bubbles: true, cancelable: true,
      })
    );

    send({ kind: 'setCaret', stop: 0, extend: false });
    await queue;
    pressCtrl('a');
    await queue;
    const all = await invoke('snapshot');
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
      check('Ctrl+X 带走了选中的文本',
        typeof lastClipboard === 'string' && lastClipboard.length > 0,
        JSON.stringify((lastClipboard || '').slice(0, 12)));
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

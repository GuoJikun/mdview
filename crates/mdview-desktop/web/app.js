// 前端只做两件事：把「停靠点」映射成像素，把用户操作翻译成消息。
// 文本真值与解析结果都在 Rust 侧，这里不留任何文档状态。
'use strict';

const { invoke } = window.__TAURI__.core;

const docEl = document.getElementById('doc');
const caretEl = document.getElementById('caret');
const barEl = document.getElementById('bar');
const toastEl = document.getElementById('toast');

let byBefore = new Map();   // 停靠点 → 从它开始的元素
let byAfter = new Map();    // 停靠点 → 在它结束的元素
let marks = [];             // 全部带 data-b 的元素
let caret = null;           // 最近一次内核回报的光标
let meta = { revision: 0, dirty: false, path: null, stops: 0 };
let dragging = false;

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
  for (const el of docEl.querySelectorAll('[data-b]')) {
    const before = Number(el.dataset.b);
    if (!byBefore.has(before)) byBefore.set(before, el);
    if (el.hasAttribute('data-a')) byAfter.set(Number(el.dataset.a), el);
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
      return { x: edge === 'right' ? rect.right : rect.left, top: rect.top, height: rect.height, el };
    }
  }
  return null;
}

function applyCaret(next) {
  caret = next;
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

  const top = pos.top + window.scrollY;
  const bottom = top + pos.height;
  if (top < window.scrollY || bottom > window.scrollY + window.innerHeight - 32) {
    pos.el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }
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

function send(message) {
  queue = queue.then(() => dispatchOne(message)).catch((err) => {
    console.error(err);
    toast('内核报错：' + err);
  });
  return queue;
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
  if (reply.conflict) toast('文件已被外部修改，保存被拒');
  if (reply.saved) toast('已保存');
  renderStatus();
}

async function boot() {
  const snap = await invoke('snapshot');
  meta = { revision: snap.revision, dirty: snap.dirty, path: snap.path, stops: snap.stops };
  renderAll(snap.blocks);
  applyCaret(snap.caret);
  paintSelection(snap.selection);
  docEl.focus({ preventScroll: true });
  // 给内核一个「页面已接管」的信号，日志里能对上游标是否真的画出来了
  invoke('ready').catch(() => {});
}

// ---- 输入 ------------------------------------------------------------

function blockBounds() {
  if (!caret) return null;
  const el = byBefore.get(caret.stop) || byAfter.get(caret.stop);
  const blk = el && el.closest('.blk');
  if (!blk) return null;
  return { start: Number(blk.dataset.bb), end: Number(blk.dataset.be) };
}

window.addEventListener('keydown', (event) => {
  // 输入法组合期间一律不拦，preedit 交给浏览器（内核侧同样会挂起重绘）
  if (event.isComposing || event.keyCode === 229) return;

  const cmd = event.ctrlKey || event.metaKey;
  if (cmd) {
    const key = event.key.toLowerCase();
    if (key === 's') { event.preventDefault(); send({ kind: 'save' }); return; }
    if (key === 'z') { event.preventDefault(); send({ kind: event.shiftKey ? 'redo' : 'undo' }); return; }
    if (key === 'y') { event.preventDefault(); send({ kind: 'redo' }); return; }
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
    case 'Home': {
      const bounds = blockBounds();
      if (bounds) { event.preventDefault(); send({ kind: 'setCaret', stop: bounds.start, extend: event.shiftKey }); }
      return;
    }
    case 'End': {
      const bounds = blockBounds();
      if (bounds) { event.preventDefault(); send({ kind: 'setCaret', stop: bounds.end, extend: event.shiftKey }); }
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

// 输入法：composition 期间只通知内核挂起，提交时一次性送入
docEl.addEventListener('compositionstart', () => send({ kind: 'compositionStart' }));
docEl.addEventListener('compositionend', (event) => {
  if (event.data) send({ kind: 'compositionCommit', text: event.data });
});

// ---- 鼠标 ------------------------------------------------------------

/** 点击点 → 停靠点：命中元素左半取左边界，右半取右边界。 */
function stopFromPoint(x, y) {
  const hit = document.elementFromPoint(x, y);
  if (!hit) return null;
  const el = hit.closest('[data-b]');
  if (!el) return null;
  const before = Number(el.dataset.b);
  if (!el.hasAttribute('data-a')) return before;
  const rect = el.getBoundingClientRect();
  if (rect.width === 0) return before;
  return x > rect.left + rect.width / 2 ? Number(el.dataset.a) : before;
}

docEl.addEventListener('mousedown', (event) => {
  const stop = stopFromPoint(event.clientX, event.clientY);
  if (stop === null) return;
  event.preventDefault();
  dragging = true;
  docEl.focus({ preventScroll: true });
  send({ kind: 'setCaret', stop, extend: event.shiftKey });
});

window.addEventListener('mousemove', (event) => {
  if (!dragging) return;
  const stop = stopFromPoint(event.clientX, event.clientY);
  if (stop === null || (caret && stop === caret.stop)) return;
  send({ kind: 'setCaret', stop, extend: true });
});

window.addEventListener('mouseup', () => { dragging = false; });

window.addEventListener('scroll', () => { if (caret) applyCaret(caret); }, { passive: true });
window.addEventListener('resize', () => { if (caret) applyCaret(caret); });

boot();

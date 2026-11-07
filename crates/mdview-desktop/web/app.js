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
let caret = null;           // 最近一次内核回报的光标
let meta = { revision: 0, dirty: false, path: null, stops: 0 };
let dragging = false;
let imeAnchorEl = null;     // preedit 复制字号用的参照元素（光标所在的字）

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
  imeEl.focus({ preventScroll: true });
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
  } catch (err) {
    check('自检未抛异常', false, String(err));
  }

  const failed = checks.filter((c) => !c.ok);
  const lines = checks.map(
    (c) => (c.ok ? '· ' : '× ') + c.name + (c.detail ? '(' + c.detail + ')' : '')
  );
  const report =
    (failed.length ? 'FAIL ' : 'OK ') +
    (checks.length - failed.length) + '/' + checks.length + ' ' + lines.join(' | ');
  await invoke('selftest', { report });
};

boot();

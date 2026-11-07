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

function applyCaret(next) {
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
// 窗口宽度变了，行会重新折行，旧的期望 x 已经没意义
window.addEventListener('resize', () => { lines.reset(); if (caret) applyCaret(caret); });

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

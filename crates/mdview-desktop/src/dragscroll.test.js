// 边缘自动滚动的无头测试。运行：node dragscroll.test.js
// 由 crates/mdview-desktop/tests/web_ime.rs 调起，避免逻辑随代码漂移。
'use strict';

const assert = require('assert');
const { edgeSpeed, clampPoint, clampToBox, createDragAutoScroll } = require('./dragscroll.js');

const VIEW = 800;

/**
 * 用「手动帧队列」代替 `requestAnimationFrame`：
 * 帧什么时候跑由测试决定，才能断言「指针不动也会继续滚」。
 */
function harness(options) {
  const opts = Object.assign({ height: VIEW, margin: 48, maxSpeed: 24 }, options || {});
  const frames = [];
  let scrolled = 0;
  let commits = 0;
  let cancelled = 0;
  const ctl = createDragAutoScroll({
    speed: (y) => edgeSpeed(y, opts.height, opts),
    scrollBy: (dy) => { scrolled += dy; },
    commit: () => { commits += 1; },
    schedule: (fn) => { frames.push(fn); return frames.length; },
    cancel: () => { cancelled += 1; },
  });
  return {
    ctl,
    scrolled: () => scrolled,
    commits: () => commits,
    cancelled: () => cancelled,
    pending: () => frames.length,
    frame() {
      const fn = frames.shift();
      if (fn) fn();
    },
    frames(n) {
      for (let i = 0; i < n; i++) this.frame();
    },
  };
}

const tests = [];
function test(name, fn) {
  tests.push([name, fn]);
}

// ---- 速度 ----------------------------------------------------------

test('指针在正文中部时不滚', () => {
  assert.strictEqual(edgeSpeed(400, VIEW), 0);
  assert.strictEqual(edgeSpeed(48, VIEW), 0, '恰好贴着边缘带外沿不算进入');
  assert.strictEqual(edgeSpeed(VIEW - 48, VIEW), 0);
});

test('靠近上沿向上滚，越深越快', () => {
  const shallow = edgeSpeed(40, VIEW);
  const deep = edgeSpeed(10, VIEW);
  assert.ok(shallow < 0 && deep < shallow, '越靠上滚动越快');
  assert.strictEqual(edgeSpeed(0, VIEW), -24, '到上沿就是满速');
});

test('靠近下沿向下滚，越深越快', () => {
  const shallow = edgeSpeed(VIEW - 40, VIEW);
  const deep = edgeSpeed(VIEW - 5, VIEW);
  assert.ok(shallow > 0 && deep > shallow);
  assert.strictEqual(edgeSpeed(VIEW, VIEW), 24, '到下沿就是满速');
});

test('拖出窗口之后封顶，不会越拖越疯', () => {
  assert.strictEqual(edgeSpeed(-200, VIEW), -24);
  assert.strictEqual(edgeSpeed(VIEW + 2000, VIEW), 24);
});

test('刚进边缘带就给 1px，不会因为取整变成 0 而卡死', () => {
  // 47.6 → 深度 0.4，按比例算不到 1px
  assert.strictEqual(edgeSpeed(47.6, VIEW), -1);
  assert.strictEqual(edgeSpeed(VIEW - 47.6, VIEW), 1);
});

test('视口很矮时两条边缘带不互相吃掉', () => {
  // 视口 40px：margin 被压到 20，中线仍有一条不滚的安全带
  assert.strictEqual(edgeSpeed(20, 40, { margin: 48 }), 0);
  assert.ok(edgeSpeed(19, 40, { margin: 48 }) < 0);
  assert.ok(edgeSpeed(21, 40, { margin: 48 }) > 0);
});

test('参数退化时安静地不滚', () => {
  assert.strictEqual(edgeSpeed(0, 0), 0, '视口高度为 0');
  assert.strictEqual(edgeSpeed(0, VIEW, { maxSpeed: 0 }), 0, '最大速度为 0');
  assert.strictEqual(edgeSpeed(0, VIEW, { margin: 0 }), 0, '关掉边缘带');
  assert.strictEqual(edgeSpeed(NaN, VIEW), 0);
});

// ---- 坐标压缩 ------------------------------------------------------

test('拖出窗口的指针压回视口内侧', () => {
  const p = clampPoint({ x: 400, y: 1200 }, 900, VIEW);
  assert.deepStrictEqual(p, { x: 400, y: VIEW - 1 });
  const q = clampPoint({ x: -30, y: -90 }, 900, VIEW);
  assert.deepStrictEqual(q, { x: 1, y: 1 });
  // 视口内的点原样返回
  assert.deepStrictEqual(clampPoint({ x: 100, y: 100 }, 900, VIEW), { x: 100, y: 100 });
});

test('压进正文可见区而不是整块视口：状态栏盖住的那条边要被避开', () => {
  // 视口 800，状态栏占最下面 40px，正文列 100..880
  const box = { left: 100, top: 0, right: 880, bottom: 760 };
  assert.deepStrictEqual(clampToBox({ x: 400, y: 900 }, box, 1),
    { x: 400, y: 759 }, '不能落到状态栏里');
  assert.deepStrictEqual(clampToBox({ x: 400, y: -50 }, box, 1), { x: 400, y: 1 });
  assert.deepStrictEqual(clampToBox({ x: -20, y: 300 }, box, 1), { x: 101, y: 300 });
  assert.deepStrictEqual(clampToBox({ x: 5000, y: 300 }, box, 1), { x: 879, y: 300 });
});

test('探测区被压扁时不产生反向区间', () => {
  // 窗口极扁：bottom 落在 top 上方，夹出来的仍是一个点而不是倒挂的区间
  const box = { left: 0, top: 100, right: 400, bottom: 90 };
  const p = clampToBox({ x: 10, y: 500 }, box, 1);
  assert.strictEqual(p.y, 101);
  assert.ok(p.y <= 101);
});

// ---- 循环 ----------------------------------------------------------

test('指针不动也会自走：一次 update 之后每帧都滚并回写落点', () => {
  const h = harness();
  h.ctl.update(VIEW + 4);        // 已经拖出视口下沿 → 满速 24px/帧
  assert.strictEqual(h.pending(), 1, '进入边缘带应当排上第一帧');
  h.frames(3);
  assert.strictEqual(h.scrolled(), 24 * 3);
  assert.strictEqual(h.commits(), 3, '每帧都要重新求一次落点');
  assert.strictEqual(h.pending(), 1, '循环自己续帧，不依赖 mousemove');
});

test('向上拖到顶也一样，只是方向相反', () => {
  const h = harness();
  h.ctl.update(-4);
  h.frames(2);
  assert.strictEqual(h.scrolled(), -24 * 2);
});

test('指针回到正文中部就停，不再续帧', () => {
  const h = harness();
  h.ctl.update(VIEW + 4);
  h.frames(1);
  const after = h.scrolled();
  h.ctl.update(400);
  h.frames(5);
  assert.strictEqual(h.scrolled(), after, '离开边缘带后不能再滚');
  assert.strictEqual(h.ctl.active(), false);
});

test('stop 取消已排的帧，之后彻底安静', () => {
  const h = harness();
  h.ctl.update(VIEW + 4);
  h.ctl.stop();
  assert.strictEqual(h.cancelled(), 1);
  assert.strictEqual(h.ctl.active(), false);
  h.frames(5);
  assert.strictEqual(h.scrolled(), 0);
  assert.strictEqual(h.commits(), 0);
});

test('反复 update 不会叠出多个循环', () => {
  const h = harness();
  h.ctl.update(VIEW + 2);
  h.ctl.update(VIEW + 3);
  h.ctl.update(VIEW + 4);
  assert.strictEqual(h.pending(), 1, '同一时刻只能有一个待跑的帧');
  h.frames(2);
  assert.strictEqual(h.scrolled(), 24 * 2);
  assert.strictEqual(h.commits(), 2);
});

test('没有 commit 也能滚（hook 可选）', () => {
  const frames = [];
  let scrolled = 0;
  const ctl = createDragAutoScroll({
    speed: (y) => edgeSpeed(y, VIEW),
    scrollBy: (dy) => { scrolled += dy; },
    schedule: (fn) => { frames.push(fn); return frames.length; },
    cancel: () => {},
  });
  ctl.update(VIEW - 1);
  const fn = frames.shift();
  fn();
  assert.ok(scrolled > 0);
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log('ok   ' + name);
  } catch (err) {
    failed += 1;
    console.error('FAIL ' + name);
    console.error('     ' + (err && err.message));
  }
}

if (failed > 0) {
  console.error('\n' + failed + '/' + tests.length + ' 项失败');
  process.exit(1);
}
console.log('\n' + tests.length + ' 项全过');

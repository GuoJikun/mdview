// 上下移动几何的无头测试。运行：node caretline.test.js
// 由 crates/mdview-desktop/tests/web_ime.rs 调起，避免逻辑随代码漂移。
'use strict';

const assert = require('assert');
const { createLineMotion, sameLine, scrollDelta } = require('./caretline.js');

/**
 * 用矩形数组描述版面：一行 = 一批同 top 的停靠点。
 * 真实渲染里一行有 n+1 个停靠点，这里简化成 n 个——几何判据只关心 x/top/height。
 */
function scene() {
  const rects = [];
  return {
    rects,
    line(opts) {
      const { top, count, x0 = 10, step = 9, height = 20, markers = [] } = opts;
      const start = rects.length;
      for (let i = 0; i < count; i++) {
        rects.push({ x: x0 + i * step, top, height, mk: markers.indexOf(i) >= 0 });
      }
      return start;
    },
    motion() {
      return createLineMotion({
        count: () => rects.length,
        rect: (stop) => (stop >= 0 && stop < rects.length ? rects[stop] : null),
      });
    },
  };
}

const tests = [];
function test(name, fn) {
  tests.push([name, fn]);
}

test('同一行按纵向重叠判定，不同字号也算一行', () => {
  const body = { x: 0, top: 10, height: 24 };
  const code = { x: 40, top: 8, height: 28 };
  const next = { x: 0, top: 40, height: 24 };
  assert.strictEqual(sameLine(body, code), true);
  assert.strictEqual(sameLine(body, next), false);
});

test('下移落到下一块并保持像素列', () => {
  const s = scene();
  s.line({ top: 0, count: 10 });    // x = 10,19,...,91
  s.line({ top: 40, count: 10 });
  const m = s.motion();
  // 停靠点 4 的 x = 46，下一行同位置正好是 14 号停靠点
  assert.strictEqual(m.step(4, 1), 14);
  assert.strictEqual(s.rects[14].x, s.rects[4].x);
});

test('上移回到上一块', () => {
  const s = scene();
  s.line({ top: 0, count: 10 });
  s.line({ top: 40, count: 10 });
  const m = s.motion();
  assert.strictEqual(m.step(13, -1), 3);
});

test('经过短行时期望列不缩水', () => {
  const s = scene();
  s.line({ top: 0, count: 10 });              // x 到 91
  const short = s.line({ top: 40, count: 3 }); // 只有 3 个停靠点
  const long2 = s.line({ top: 80, count: 10 });
  const m = s.motion();
  // 从 x=91（停靠点 9）下移：短行只能落到行尾，但期望列仍是 91
  const mid = m.step(9, 1);
  assert.strictEqual(mid, short + 2, '短行应落到行尾');
  assert.strictEqual(m.desiredX(), 91);
  // 再下一行必须回到 x=91，而不是跟着缩到短行行尾
  const down = m.step(mid, 1);
  assert.strictEqual(down, long2 + 9);
  assert.strictEqual(s.rects[down].x, 91);
});

test('reset 之后以当前列为准', () => {
  const s = scene();
  s.line({ top: 0, count: 10 });
  const short = s.line({ top: 40, count: 3 });
  s.line({ top: 80, count: 10 });
  const m = s.motion();
  const mid = m.step(9, 1);
  assert.strictEqual(mid, short + 2);
  m.reset();
  assert.strictEqual(m.desiredX(), null);
  const down = m.step(mid, 1);
  assert.strictEqual(s.rects[down].x, s.rects[mid].x);
});

test('段内软换行也按像素走', () => {
  const s = scene();
  s.line({ top: 0, count: 6 });
  s.line({ top: 20, count: 6 }); // 同一块里被自动折行的下一行
  const m = s.motion();
  assert.strictEqual(m.step(1, 1), 7);
});

test('首行按上、末行按下都不动', () => {
  const s = scene();
  s.line({ top: 0, count: 5 });
  s.line({ top: 20, count: 5 });
  const m = s.motion();
  assert.strictEqual(m.step(2, -1), null);
  assert.strictEqual(m.step(7, 1), null);
});

test('落点取最近字符而不是行首或行尾', () => {
  const s = scene();
  s.line({ top: 0, count: 4, x0: 10, step: 10 }); // x = 10,20,30,40
  s.line({ top: 20, count: 5, x0: 10, step: 7 }); // x = 10,17,24,31,38
  const m = s.motion();
  // 当前 x=30，下一行最近的是 31（距离 1），不是 24（距离 6）
  assert.strictEqual(m.step(2, 1), 4 + 3);
});

test('并列时优先文字停靠点，不停进折叠标记', () => {
  const s = scene();
  s.line({ top: 0, count: 3, x0: 10, step: 10 }); // x = 10,20,30
  // 目标行：行内第 1 个停靠点是折叠标记的右端，第 2 个是文字前
  const start = s.line({ top: 20, count: 3, x0: 10, step: 10, markers: [1] });
  s.rects[start + 1].x = 30; // 标记零宽，右端与文字前共享同一列
  const m = s.motion();
  assert.strictEqual(m.step(2, 1), start + 2, '应选文字停靠点而不是折叠标记');
});

test('矩形缺失的停靠点被跳过，不会卡住', () => {
  const s = scene();
  s.line({ top: 0, count: 4 });
  s.line({ top: 20, count: 4 });
  const m = s.motion();
  s.rects[5] = null; // 模拟元素被 patch 换掉、取不到矩形
  const target = m.step(1, 1);
  assert.ok(target !== null && target >= 4 && target <= 7, '应当落到下一行: ' + target);
});

test('Home/End 落到视觉行两端而不是块首块尾', () => {
  const s = scene();
  s.line({ top: 0, count: 4 });        // 块内第一视觉行：0..3
  const second = s.line({ top: 20, count: 4 }); // 折行后的第二行：4..7
  const m = s.motion();
  assert.strictEqual(m.edge(5, -1), second);
  assert.strictEqual(m.edge(5, 1), second + 3);
  // 第二行内的 Home 不能跑到块首
  assert.notStrictEqual(m.edge(5, -1), 0);
});

test('单行段落的 Home/End 与块首块尾一致', () => {
  const s = scene();
  s.line({ top: 0, count: 5 });
  const m = s.motion();
  assert.strictEqual(m.edge(2, -1), 0);
  assert.strictEqual(m.edge(2, 1), 4);
});

test('点击行尾右侧空白落到行尾', () => {
  const s = scene();
  s.line({ top: 0, count: 4, x0: 10, step: 10 }); // x = 10,20,30,40
  const m = s.motion();
  // 点得再远也必须停在行尾，而不是「离得最近的字」——它俩本来就是同一个
  assert.strictEqual(m.nearestInLine(0, 400), 3);
  assert.strictEqual(m.nearestInLine(0, 41), 3);
});

test('点击行首左侧空白落到行首', () => {
  const s = scene();
  s.line({ top: 0, count: 4, x0: 10, step: 10 });
  const m = s.motion();
  assert.strictEqual(m.nearestInLine(2, 0), 0);
});

test('词尾并列时点击空白取 End 的落点', () => {
  const s = scene();
  // 4 与 5 是闭合标记前后两端，共享同一 x；End 落在序号大的 5
  s.line({ top: 0, count: 6, x0: 10, step: 10, markers: [5] });
  s.rects[5].x = s.rects[4].x;
  const m = s.motion();
  assert.strictEqual(m.nearestInLine(4, 200), 5, '行外取序号更大的停靠点');
  // 行内落在标记左侧的字上时，仍按最近选，不会强行跳到行尾
  assert.strictEqual(m.nearestInLine(4, 12), 0);
});

test('行内空白按最近停靠点落，不跳到行尾', () => {
  const s = scene();
  s.line({ top: 0, count: 4, x0: 10, step: 20 }); // x = 10,30,50,70
  const m = s.motion();
  // 点在 40：离 30 与 50 各 10，取靠左的 1（与系统 caret 的向下取整一致）
  assert.strictEqual(m.nearestInLine(0, 40), 1);
});

// ---- 光标进出视口 ---------------------------------------------------

test('光标完整在视口内时纹丝不动', () => {
  assert.strictEqual(scrollDelta({ top: 200, height: 22 }, 800, 32), 0);
  // 上下恰好卡在留白边界也算在里面，不然会一直微调
  assert.strictEqual(scrollDelta({ top: 32, height: 22 }, 800, 32), 0);
  assert.strictEqual(scrollDelta({ top: 800 - 32 - 22, height: 22 }, 800, 32), 0);
});

test('光标被推到视口上沿之上要向上滚回留白处', () => {
  // 负的 top 是滚动之后光标跑到视口外的样子
  assert.strictEqual(scrollDelta({ top: -12, height: 20 }, 800, 32), -44);
  // 还在视口里但贴得太近，也要让出留白
  assert.strictEqual(scrollDelta({ top: 10, height: 20 }, 800, 32), -22);
});

test('光标掉到视口下方要向下滚', () => {
  assert.strictEqual(scrollDelta({ top: 830, height: 20 }, 800, 32), 82);
  assert.strictEqual(scrollDelta({ top: 780, height: 20 }, 800, 32), 32);
});

test('margin 为 0 时只保证可见', () => {
  assert.strictEqual(scrollDelta({ top: 0, height: 20 }, 800, 0), 0);
  assert.strictEqual(scrollDelta({ top: -1, height: 20 }, 800, 0), -1);
  assert.strictEqual(scrollDelta({ top: 800, height: 20 }, 800, 0), 20);
});

test('比视口还高的目标不会来回跳', () => {
  // 上沿在上面：只把上沿拉回，不会因为「下沿看不见」又往下滚
  assert.strictEqual(scrollDelta({ top: -200, height: 2000 }, 800, 32), -232);
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

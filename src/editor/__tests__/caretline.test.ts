// 上下移动几何：判据全是矩形坐标，不碰真实 DOM，跑 node 环境。
import { describe, expect, it } from 'vitest';
import { createLineMotion, sameLine, scrollDelta } from '../caretline.js';

type Rect = { x: number; top: number; height: number; mk: boolean };

/**
 * 用矩形数组描述版面：一行 = 一批同 top 的停靠点。
 * 真实渲染里一行有 n+1 个停靠点，这里简化成 n 个——几何判据只关心 x/top/height。
 *
 * rects 允许为 null：`矩形缺失的停靠点被跳过` 那条要模拟元素被 patch 换掉。
 */
function scene() {
  const rects: (Rect | null)[] = [];
  return {
    rects,
    line(opts: {
      top: number;
      count: number;
      x0?: number;
      step?: number;
      height?: number;
      markers?: number[];
    }) {
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
        rect: (stop: number) => (stop >= 0 && stop < rects.length ? rects[stop] : null),
      });
    },
  };
}

/** 几何断言里假定该停靠点有矩形；取不到就是测试自身写错了，用 `!` 尽早炸出来。 */
const at = (s: ReturnType<typeof scene>, i: number): Rect => s.rects[i]!;

describe('sameLine', () => {
  it('同一行按纵向重叠判定，不同字号也算一行', () => {
    const body = { x: 0, top: 10, height: 24 };
    const code = { x: 40, top: 8, height: 28 };
    const next = { x: 0, top: 40, height: 24 };
    expect(sameLine(body, code)).toBe(true);
    expect(sameLine(body, next)).toBe(false);
  });
});

describe('createLineMotion：上下移动', () => {
  it('下移落到下一块并保持像素列', () => {
    const s = scene();
    s.line({ top: 0, count: 10 });    // x = 10,19,...,91
    s.line({ top: 40, count: 10 });
    const m = s.motion();
    // 停靠点 4 的 x = 46，下一行同位置正好是 14 号停靠点
    expect(m.step(4, 1)).toBe(14);
    expect(at(s, 14).x).toBe(at(s, 4).x);
  });

  it('上移回到上一块', () => {
    const s = scene();
    s.line({ top: 0, count: 10 });
    s.line({ top: 40, count: 10 });
    const m = s.motion();
    expect(m.step(13, -1)).toBe(3);
  });

  it('经过短行时期望列不缩水', () => {
    const s = scene();
    s.line({ top: 0, count: 10 });              // x 到 91
    const short = s.line({ top: 40, count: 3 }); // 只有 3 个停靠点
    const long2 = s.line({ top: 80, count: 10 });
    const m = s.motion();
    // 从 x=91（停靠点 9）下移：短行只能落到行尾，但期望列仍是 91
    const mid = m.step(9, 1);
    expect(mid, '短行应落到行尾').toBe(short + 2);
    expect(m.desiredX()).toBe(91);
    // 再下一行必须回到 x=91，而不是跟着缩到短行行尾
    const down = m.step(mid!, 1);
    expect(down).toBe(long2 + 9);
    expect(at(s, down!).x).toBe(91);
  });

  it('reset 之后以当前列为准', () => {
    const s = scene();
    s.line({ top: 0, count: 10 });
    const short = s.line({ top: 40, count: 3 });
    s.line({ top: 80, count: 10 });
    const m = s.motion();
    const mid = m.step(9, 1);
    expect(mid).toBe(short + 2);
    m.reset();
    expect(m.desiredX()).toBeNull();
    const down = m.step(mid!, 1);
    expect(at(s, down!).x).toBe(at(s, mid!).x);
  });

  it('段内软换行也按像素走', () => {
    const s = scene();
    s.line({ top: 0, count: 6 });
    s.line({ top: 20, count: 6 }); // 同一块里被自动折行的下一行
    const m = s.motion();
    expect(m.step(1, 1)).toBe(7);
  });

  it('首行按上、末行按下都不动', () => {
    const s = scene();
    s.line({ top: 0, count: 5 });
    s.line({ top: 20, count: 5 });
    const m = s.motion();
    expect(m.step(2, -1)).toBeNull();
    expect(m.step(7, 1)).toBeNull();
  });

  it('落点取最近字符而不是行首或行尾', () => {
    const s = scene();
    s.line({ top: 0, count: 4, x0: 10, step: 10 }); // x = 10,20,30,40
    s.line({ top: 20, count: 5, x0: 10, step: 7 }); // x = 10,17,24,31,38
    const m = s.motion();
    // 当前 x=30，下一行最近的是 31（距离 1），不是 24（距离 6）
    expect(m.step(2, 1)).toBe(4 + 3);
  });

  it('并列时优先文字停靠点，不停进折叠标记', () => {
    const s = scene();
    s.line({ top: 0, count: 3, x0: 10, step: 10 }); // x = 10,20,30
    // 目标行：行内第 1 个停靠点是折叠标记的右端，第 2 个是文字前
    const start = s.line({ top: 20, count: 3, x0: 10, step: 10, markers: [1] });
    at(s, start + 1).x = 30; // 标记零宽，右端与文字前共享同一列
    const m = s.motion();
    expect(m.step(2, 1), '应选文字停靠点而不是折叠标记').toBe(start + 2);
  });

  it('矩形缺失的停靠点被跳过，不会卡住', () => {
    const s = scene();
    s.line({ top: 0, count: 4 });
    s.line({ top: 20, count: 4 });
    const m = s.motion();
    s.rects[5] = null; // 模拟元素被 patch 换掉、取不到矩形
    const target = m.step(1, 1);
    expect(target, '应当落到下一行').not.toBeNull();
    expect(target!).toBeGreaterThanOrEqual(4);
    expect(target!).toBeLessThanOrEqual(7);
  });
});

describe('createLineMotion：Home / End', () => {
  it('落到视觉行两端而不是块首块尾', () => {
    const s = scene();
    s.line({ top: 0, count: 4 });        // 块内第一视觉行：0..3
    const second = s.line({ top: 20, count: 4 }); // 折行后的第二行：4..7
    const m = s.motion();
    expect(m.edge(5, -1)).toBe(second);
    expect(m.edge(5, 1)).toBe(second + 3);
    // 第二行内的 Home 不能跑到块首
    expect(m.edge(5, -1)).not.toBe(0);
  });

  it('单行段落的 Home/End 与块首块尾一致', () => {
    const s = scene();
    s.line({ top: 0, count: 5 });
    const m = s.motion();
    expect(m.edge(2, -1)).toBe(0);
    expect(m.edge(2, 1)).toBe(4);
  });
});

describe('createLineMotion：点击落点', () => {
  it('点击行尾右侧空白落到行尾', () => {
    const s = scene();
    s.line({ top: 0, count: 4, x0: 10, step: 10 }); // x = 10,20,30,40
    const m = s.motion();
    // 点得再远也必须停在行尾，而不是「离得最近的字」——它俩本来就是同一个
    expect(m.nearestInLine(0, 400)).toBe(3);
    expect(m.nearestInLine(0, 41)).toBe(3);
  });

  it('点击行首左侧空白落到行首', () => {
    const s = scene();
    s.line({ top: 0, count: 4, x0: 10, step: 10 });
    const m = s.motion();
    expect(m.nearestInLine(2, 0)).toBe(0);
  });

  it('词尾并列时点击空白取 End 的落点', () => {
    const s = scene();
    // 4 与 5 是闭合标记前后两端，共享同一 x；End 落在序号大的 5
    s.line({ top: 0, count: 6, x0: 10, step: 10, markers: [5] });
    at(s, 5).x = at(s, 4).x;
    const m = s.motion();
    expect(m.nearestInLine(4, 200), '行外取序号更大的停靠点').toBe(5);
    // 行内落在标记左侧的字上时，仍按最近选，不会强行跳到行尾
    expect(m.nearestInLine(4, 12)).toBe(0);
  });

  it('行内空白按最近停靠点落，不跳到行尾', () => {
    const s = scene();
    s.line({ top: 0, count: 4, x0: 10, step: 20 }); // x = 10,30,50,70
    const m = s.motion();
    // 点在 40：离 30 与 50 各 10，取靠左的 1（与系统 caret 的向下取整一致）
    expect(m.nearestInLine(0, 40)).toBe(1);
  });
});

describe('scrollDelta：光标进出视口', () => {
  it('光标完整在视口内时纹丝不动', () => {
    expect(scrollDelta({ top: 200, height: 22 }, 800, 32)).toBe(0);
    // 上下恰好卡在留白边界也算在里面，不然会一直微调
    expect(scrollDelta({ top: 32, height: 22 }, 800, 32)).toBe(0);
    expect(scrollDelta({ top: 800 - 32 - 22, height: 22 }, 800, 32)).toBe(0);
  });

  it('光标被推到视口上沿之上要向上滚回留白处', () => {
    // 负的 top 是滚动之后光标跑到视口外的样子
    expect(scrollDelta({ top: -12, height: 20 }, 800, 32)).toBe(-44);
    // 还在视口里但贴得太近，也要让出留白
    expect(scrollDelta({ top: 10, height: 20 }, 800, 32)).toBe(-22);
  });

  it('光标掉到视口下方要向下滚', () => {
    expect(scrollDelta({ top: 830, height: 20 }, 800, 32)).toBe(82);
    expect(scrollDelta({ top: 780, height: 20 }, 800, 32)).toBe(32);
  });

  it('margin 为 0 时只保证可见', () => {
    expect(scrollDelta({ top: 0, height: 20 }, 800, 0)).toBe(0);
    expect(scrollDelta({ top: -1, height: 20 }, 800, 0)).toBe(-1);
    expect(scrollDelta({ top: 800, height: 20 }, 800, 0)).toBe(20);
  });

  it('比视口还高的目标不会来回跳', () => {
    // 上沿在上面：只把上沿拉回，不会因为「下沿看不见」又往下滚
    expect(scrollDelta({ top: -200, height: 2000 }, 800, 32)).toBe(-232);
  });
});

// 边缘自动滚动：速度曲线与帧循环，只依赖注入的钩子，不碰真实 DOM，跑 node 环境。
import { describe, expect, it } from 'vitest';
import { clampPoint, clampToBox, createDragAutoScroll, edgeSpeed } from '../dragscroll.js';

const VIEW = 800;

/**
 * 用「手动帧队列」代替 `requestAnimationFrame`：
 * 帧什么时候跑由测试决定，才能断言「指针不动也会继续滚」。
 */
function harness(options?: { height?: number; margin?: number; maxSpeed?: number }) {
  const opts = { height: VIEW, margin: 48, maxSpeed: 24, ...options };
  const frames: Array<() => void> = [];
  let scrolled = 0;
  let commits = 0;
  let cancelled = 0;
  const ctl = createDragAutoScroll({
    speed: (y) => edgeSpeed(y, opts.height, opts),
    scrollBy: (dy) => {
      scrolled += dy;
    },
    commit: () => {
      commits += 1;
    },
    schedule: (fn) => {
      frames.push(fn);
      return frames.length;
    },
    cancel: () => {
      cancelled += 1;
    },
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
    frames(n: number) {
      for (let i = 0; i < n; i++) this.frame();
    },
  };
}

describe('edgeSpeed：速度曲线', () => {
  it('指针在正文中部时不滚', () => {
    expect(edgeSpeed(400, VIEW)).toBe(0);
    expect(edgeSpeed(48, VIEW), '恰好贴着边缘带外沿不算进入').toBe(0);
    expect(edgeSpeed(VIEW - 48, VIEW)).toBe(0);
  });

  it('靠近上沿向上滚，越深越快', () => {
    const shallow = edgeSpeed(40, VIEW);
    const deep = edgeSpeed(10, VIEW);
    expect(shallow, '越靠上滚动越快').toBeLessThan(0);
    expect(deep).toBeLessThan(shallow);
    expect(edgeSpeed(0, VIEW), '到上沿就是满速').toBe(-24);
  });

  it('靠近下沿向下滚，越深越快', () => {
    const shallow = edgeSpeed(VIEW - 40, VIEW);
    const deep = edgeSpeed(VIEW - 5, VIEW);
    expect(shallow).toBeGreaterThan(0);
    expect(deep).toBeGreaterThan(shallow);
    expect(edgeSpeed(VIEW, VIEW), '到下沿就是满速').toBe(24);
  });

  it('拖出窗口之后封顶，不会越拖越疯', () => {
    expect(edgeSpeed(-200, VIEW)).toBe(-24);
    expect(edgeSpeed(VIEW + 2000, VIEW)).toBe(24);
  });

  it('刚进边缘带就给 1px，不会因为取整变成 0 而卡死', () => {
    // 47.6 → 深度 0.4，按比例算不到 1px
    expect(edgeSpeed(47.6, VIEW)).toBe(-1);
    expect(edgeSpeed(VIEW - 47.6, VIEW)).toBe(1);
  });

  it('视口很矮时两条边缘带不互相吃掉', () => {
    // 视口 40px：margin 被压到 20，中线仍有一条不滚的安全带
    expect(edgeSpeed(20, 40, { margin: 48 })).toBe(0);
    expect(edgeSpeed(19, 40, { margin: 48 })).toBeLessThan(0);
    expect(edgeSpeed(21, 40, { margin: 48 })).toBeGreaterThan(0);
  });

  it('参数退化时安静地不滚', () => {
    expect(edgeSpeed(0, 0), '视口高度为 0').toBe(0);
    expect(edgeSpeed(0, VIEW, { maxSpeed: 0 }), '最大速度为 0').toBe(0);
    expect(edgeSpeed(0, VIEW, { margin: 0 }), '关掉边缘带').toBe(0);
    expect(edgeSpeed(NaN, VIEW)).toBe(0);
  });
});

describe('坐标压缩', () => {
  it('拖出窗口的指针压回视口内侧', () => {
    const p = clampPoint({ x: 400, y: 1200 }, 900, VIEW);
    expect(p).toStrictEqual({ x: 400, y: VIEW - 1 });
    const q = clampPoint({ x: -30, y: -90 }, 900, VIEW);
    expect(q).toStrictEqual({ x: 1, y: 1 });
    // 视口内的点原样返回
    expect(clampPoint({ x: 100, y: 100 }, 900, VIEW)).toStrictEqual({ x: 100, y: 100 });
  });

  it('压进正文可见区而不是整块视口：状态栏盖住的那条边要被避开', () => {
    // 视口 800，状态栏占最下面 40px，正文列 100..880
    const box = { left: 100, top: 0, right: 880, bottom: 760 };
    expect(clampToBox({ x: 400, y: 900 }, box, 1), '不能落到状态栏里')
      .toStrictEqual({ x: 400, y: 759 });
    expect(clampToBox({ x: 400, y: -50 }, box, 1)).toStrictEqual({ x: 400, y: 1 });
    expect(clampToBox({ x: -20, y: 300 }, box, 1)).toStrictEqual({ x: 101, y: 300 });
    expect(clampToBox({ x: 5000, y: 300 }, box, 1)).toStrictEqual({ x: 879, y: 300 });
  });

  it('探测区被压扁时不产生反向区间', () => {
    // 窗口极扁：bottom 落在 top 上方，夹出来的仍是一个点而不是倒挂的区间
    const box = { left: 0, top: 100, right: 400, bottom: 90 };
    const p = clampToBox({ x: 10, y: 500 }, box, 1);
    expect(p.y).toBe(101);
    expect(p.y).toBeLessThanOrEqual(101);
  });
});

describe('createDragAutoScroll：帧循环', () => {
  it('指针不动也会自走：一次 update 之后每帧都滚并回写落点', () => {
    const h = harness();
    h.ctl.update(VIEW + 4);        // 已经拖出视口下沿 → 满速 24px/帧
    expect(h.pending(), '进入边缘带应当排上第一帧').toBe(1);
    h.frames(3);
    expect(h.scrolled()).toBe(24 * 3);
    expect(h.commits(), '每帧都要重新求一次落点').toBe(3);
    expect(h.pending(), '循环自己续帧，不依赖 mousemove').toBe(1);
  });

  it('向上拖到顶也一样，只是方向相反', () => {
    const h = harness();
    h.ctl.update(-4);
    h.frames(2);
    expect(h.scrolled()).toBe(-24 * 2);
  });

  it('指针回到正文中部就停，不再续帧', () => {
    const h = harness();
    h.ctl.update(VIEW + 4);
    h.frames(1);
    const after = h.scrolled();
    h.ctl.update(400);
    h.frames(5);
    expect(h.scrolled(), '离开边缘带后不能再滚').toBe(after);
    expect(h.ctl.active()).toBe(false);
  });

  it('stop 取消已排的帧，之后彻底安静', () => {
    const h = harness();
    h.ctl.update(VIEW + 4);
    h.ctl.stop();
    expect(h.cancelled()).toBe(1);
    expect(h.ctl.active()).toBe(false);
    h.frames(5);
    expect(h.scrolled()).toBe(0);
    expect(h.commits()).toBe(0);
  });

  it('反复 update 不会叠出多个循环', () => {
    const h = harness();
    h.ctl.update(VIEW + 2);
    h.ctl.update(VIEW + 3);
    h.ctl.update(VIEW + 4);
    expect(h.pending(), '同一时刻只能有一个待跑的帧').toBe(1);
    h.frames(2);
    expect(h.scrolled()).toBe(24 * 2);
    expect(h.commits()).toBe(2);
  });

  it('没有 commit 也能滚（hook 可选）', () => {
    const frames: Array<() => void> = [];
    let scrolled = 0;
    const ctl = createDragAutoScroll({
      speed: (y) => edgeSpeed(y, VIEW),
      scrollBy: (dy) => {
        scrolled += dy;
      },
      schedule: (fn) => {
        frames.push(fn);
        return frames.length;
      },
      cancel: () => {},
    });
    ctl.update(VIEW - 1);
    const fn = frames.shift();
    fn!();
    expect(scrolled).toBeGreaterThan(0);
  });
});

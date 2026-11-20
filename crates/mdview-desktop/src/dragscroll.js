// 拖动选区时的边缘自动滚动。
//
// 为什么需要它：拖选靠 `mousemove` 驱动，而指针一旦停在视口边缘（或直接拖出
// 窗口），浏览器就不再发新的 `mousemove`——选区停在原地，用户只能松手、滚动、
// 再按住 Shift 补选。体验上等于「拖不出这一屏」。
//
// 做法：`mousemove` 只负责更新指针位置，滚动由一个自走的帧循环完成，
// 每帧「滚一点 + 重新求指针下的落点」。指针不动、页面在动，落点自然一路延伸。
//
// 本模块刻意不碰 DOM：帧调度、滚动、落点回写全部由 hooks 提供，
// 因此 Node 能直接测（`node dragscroll.test.js`）。
'use strict';

(function (root) {
  /** 距离视口上下沿多少像素内开始自动滚动。 */
  const DEFAULT_MARGIN = 48;
  /** 每帧最多滚多少像素。60fps 下约 1440px/s，比手动拖快一档，但不会失控。 */
  const DEFAULT_MAX_SPEED = 24;

  /**
   * 指针在视口边缘时每帧应该滚多少像素（正数向下，负数向上），不在边缘带内返回 0。
   *
   * 速度与「陷进边缘多深」成正比，到视口外封顶——指针拖出窗口之后仍然继续滚，
   * 但不会因为继续往下拖而越滚越疯。
   *
   * `margin` 会被压到视口高度的一半：视口很矮时（小窗、横向分栏），
   * 上下两条边缘带不能互相吃掉，否则中间那一小段正常区域会消失、滚动永远停不下来。
   *
   * @param {number} y          指针的视口纵坐标（可以是负数或超出视口，表示拖到窗口外）
   * @param {number} viewHeight 视口高度，即 `window.innerHeight`
   * @param {{margin?: number, maxSpeed?: number}} [options]
   */
  function edgeSpeed(y, viewHeight, options) {
    const opts = options || {};
    const margin = Math.min(
      Math.max(opts.margin === undefined ? DEFAULT_MARGIN : opts.margin, 0),
      Math.max(viewHeight, 0) / 2,
    );
    const maxSpeed = Math.max(opts.maxSpeed === undefined ? DEFAULT_MAX_SPEED : opts.maxSpeed, 0);
    if (!(margin > 0) || !(maxSpeed > 0) || !(viewHeight > 0) || !Number.isFinite(y)) return 0;

    if (y < margin) {
      const depth = Math.min(margin - y, margin);
      if (depth <= 0) return 0;
      // 取整可能把「刚进边缘带」压成 0，那样指针停在边缘时就再也不动了，
      // 所以只要进了带子至少给 1px
      return -Math.max(1, Math.round((depth / margin) * maxSpeed));
    }

    const bottom = viewHeight - margin;
    if (y > bottom) {
      const depth = Math.min(y - bottom, margin);
      if (depth <= 0) return 0;
      return Math.max(1, Math.round((depth / margin) * maxSpeed));
    }
    return 0;
  }

  /**
   * 把指针坐标压进一个矩形区域（box 的 `right`/`bottom` 与
   * `getBoundingClientRect` 一致，是外边界）。
   *
   * 为什么不能只压回视口：视口边缘常常被固定浮层盖着（本项目的状态栏
   * `#bar` 就是 `position: fixed`），`elementFromPoint` 命中的是浮层，
   * 落点求不出来——页面在滚、选区不动。调用方应当传入「正文真正可见的区域」。
   *
   * @param {{x: number, y: number}} point
   * @param {{left: number, top: number, right: number, bottom: number}} box
   * @param {number} [inset] 边缘内缩，至少留多少像素
   */
  function clampToBox(point, box, inset) {
    const pad = Math.max(inset === undefined ? 1 : inset, 0);
    const left = box.left + pad;
    const top = box.top + pad;
    const right = Math.max(box.right - pad, left);
    const bottom = Math.max(box.bottom - pad, top);
    return {
      x: Math.min(Math.max(point.x, left), right),
      y: Math.min(Math.max(point.y, top), bottom),
    };
  }

  /** 压回视口：就是「整块视口」这个 box 的特例。 */
  function clampPoint(point, width, height, inset) {
    return clampToBox(point, { left: 0, top: 0, right: width, bottom: height }, inset);
  }

  /**
   * 自走的滚动循环。
   *
   * `update` 只在指针移动时被调用；拖动期间指针常常是**不动**的（贴在边缘），
   * 所以循环必须自己续帧，不能指望下一次 `mousemove`。
   *
   * @param {{
   *   speed: (y: number) => number,
   *   scrollBy: (dy: number) => void,
   *   commit?: () => void,
   *   schedule: (fn: () => void) => any,
   *   cancel: (handle: any) => void,
   * }} hooks
   */
  function createDragAutoScroll(hooks) {
    let y = null;      // 最近一次指针的纵向位置
    let frame = null;  // 已排队但还没跑的下一帧

    function cancelFrame() {
      if (frame !== null) {
        hooks.cancel(frame);
        frame = null;
      }
    }

    function tick() {
      frame = null;
      if (y === null) return;
      const dy = hooks.speed(y);
      // 指针回到正文中部：停下来，但保留 y，下一次 update 会重新判断
      if (dy === 0) return;
      hooks.scrollBy(dy);
      // 页面动过了，指针下的落点变了：交给调用方重新求一次
      if (hooks.commit) hooks.commit();
      frame = hooks.schedule(tick);
    }

    return {
      /** 指针移动时调用；进入边缘带就启动循环，离开就停。 */
      update: function (nextY) {
        y = nextY;
        if (hooks.speed(y) === 0) {
          cancelFrame();
          return;
        }
        if (frame === null) frame = hooks.schedule(tick);
      },
      /** 松手、失焦、拖动结束时调用。 */
      stop: function () {
        y = null;
        cancelFrame();
      },
      active: function () {
        return frame !== null;
      },
    };
  }

  const api = {
    edgeSpeed: edgeSpeed,
    clampPoint: clampPoint,
    clampToBox: clampToBox,
    createDragAutoScroll: createDragAutoScroll,
    DEFAULT_MARGIN: DEFAULT_MARGIN,
    DEFAULT_MAX_SPEED: DEFAULT_MAX_SPEED,
  };
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    root.edgeSpeed = edgeSpeed;
    root.clampPoint = clampPoint;
    root.clampToBox = clampToBox;
    root.createDragAutoScroll = createDragAutoScroll;
  }
})(typeof window !== 'undefined' ? window : this);

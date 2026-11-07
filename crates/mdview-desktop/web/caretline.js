// 上下方向键的视觉行移动。
//
// 为什么放在前端而不是内核：软换行只有浏览器知道。内核只认「停靠点 + 视觉列」，
// 一行的宽度取决于字体、窗口宽度和缩放，内核拿不到这些，于是它眼里的「下一行」
// 只能是「下一块」——长段落里按一下方向键就整段跳走。
//
// 这里换成纯像素判据：**同一视觉行的判据是矩形纵向重叠**，
// 目标是「下一行里离当前 x 最近的那个停靠点」。软换行、标题与正文混排、
// 行内代码撑高行盒，都不需要特判。
//
// 本模块刻意不碰 DOM：矩形由 hooks 提供，因此 Node 能直接测（`node caretline.test.js`）。
'use strict';

(function (root) {
  /** 纵向重叠超过较矮者的一半，才算同一视觉行。 */
  const SAME_LINE_RATIO = 0.5;
  /** 距离在这个像素范围内视为并列，此时优先选文字停靠点而非标记停靠点。 */
  const TIE_EPSILON = 0.5;

  function sameLine(a, b) {
    const overlap =
      Math.min(a.top + a.height, b.top + b.height) - Math.max(a.top, b.top);
    const shorter = Math.min(a.height, b.height);
    return shorter > 0 && overlap > shorter * SAME_LINE_RATIO;
  }

  /**
   * @param {{
   *   count: () => number,
   *   rect: (stop: number) => ({x: number, top: number, height: number, mk?: boolean}|null),
   * }} hooks
   */
  function createLineMotion(hooks) {
    // 期望的 x。连续按上下键时不能每步都重新取当前 x，
    // 否则经过一行短行（只能落到行尾）之后，列就永久缩水了。
    let desiredX = null;

    function rectAt(stop) {
      if (stop < 0 || stop >= hooks.count()) return null;
      return hooks.rect(stop);
    }

    /** 停靠点所在的整条视觉行（连续区间，按停靠点序号）。 */
    function lineAround(stop) {
      const anchor = rectAt(stop);
      if (!anchor) return null;
      let lo = stop;
      let hi = stop;
      while (lo > 0) {
        const prev = rectAt(lo - 1);
        if (!prev || !sameLine(prev, anchor)) break;
        lo--;
      }
      while (hi < hooks.count() - 1) {
        const next = rectAt(hi + 1);
        if (!next || !sameLine(next, anchor)) break;
        hi++;
      }
      return { lo, hi };
    }

    /** 相邻视觉行的第一个停靠点；到底了返回 null。 */
    function adjacentLine(stop, dir) {
      const anchor = rectAt(stop);
      if (!anchor) return null;
      if (dir > 0) {
        for (let i = stop + 1; i < hooks.count(); i++) {
          const r = rectAt(i);
          if (!r) continue;
          if (r.top > anchor.top && !sameLine(r, anchor)) return i;
        }
        return null;
      }
      for (let i = stop - 1; i >= 0; i--) {
        const r = rectAt(i);
        if (!r) continue;
        if (r.top < anchor.top && !sameLine(r, anchor)) return i;
      }
      return null;
    }

    /** 行内离 desiredX 最近的停靠点。并列时优先文字停靠点，避免停在折叠标记里。 */
    function nearest(lo, hi, x) {
      let best = -1;
      let bestDistance = 0;
      let bestIsMarker = true;
      for (let i = lo; i <= hi; i++) {
        const r = rectAt(i);
        if (!r) continue;
        const distance = Math.abs(r.x - x);
        const isMarker = r.mk === true;
        const better =
          best < 0 ||
          distance < bestDistance - TIE_EPSILON ||
          (distance <= bestDistance + TIE_EPSILON && bestIsMarker && !isMarker);
        if (better) {
          best = i;
          bestDistance = distance;
          bestIsMarker = isMarker;
        }
      }
      return best < 0 ? null : best;
    }

    return {
      /** 任何非上下移动的操作都要清掉期望列，否则列会被上一次操作带跑。 */
      reset: function () {
        desiredX = null;
      },
      desiredX: function () {
        return desiredX;
      },

      /**
       * 当前视觉行的首/尾停靠点。
       * @param {number} stop 当前全局停靠点序号
       * @param {-1|1} side -1 行首，1 行尾
       * @returns {number|null} 矩形取不到时返回 null
       */
      edge: function (stop, side) {
        const line = lineAround(stop);
        if (!line) return null;
        return side < 0 ? line.lo : line.hi;
      },

      /**
       * 移动到相邻视觉行。
       * @param {number} stop 当前全局停靠点序号
       * @param {number} dir  1 向下，-1 向上
       * @returns {number|null} 目标停靠点；已在首/末行时返回 null
       */
      step: function (stop, dir) {
        const current = rectAt(stop);
        if (!current) return null;
        if (desiredX === null) desiredX = current.x;
        const start = adjacentLine(stop, dir);
        if (start === null) return null;
        const line = lineAround(start);
        if (!line) return null;
        return nearest(line.lo, line.hi, desiredX);
      },
    };
  }

  const api = { createLineMotion: createLineMotion, sameLine: sameLine };
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    root.createLineMotion = createLineMotion;
    root.sameLine = sameLine;
  }
})(typeof window !== 'undefined' ? window : this);

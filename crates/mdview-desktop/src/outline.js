// 大纲联动的纯几何：不碰 DOM，Node 可直接测。
//
// 「同步滚动按锚点块插值，禁止用滚动百分比换算」（PLAN 硬约束 10）。
// 这里的锚点是每个标题块在文档中的实际纵坐标 scrollTop（而不是它在列表里的
// 序号）。判定当前所在节时，取「最后一个已经越过阅读基准线的标题」。用实际
// 位置而非百分比，才能在标题疏密不均、末尾留白很大的文档里仍然对得上。
(function (root) {
  'use strict';

  /**
   * 当前应该高亮哪一个大纲项。
   *
   * @param {Array<{top:number}>} anchors 每个大纲项的锚点，top 为该标题在
   *        文档坐标系里的纵坐标（element.offsetTop 累加，或 rect.top + scrollY）。
   *        必须与大纲项同序。
   * @param {number} line 阅读基准线在文档坐标系里的纵坐标。通常取视口顶部往下
   *        一小段（如 scrollY + 24px），避免标题刚冒头就切换。
   * @returns {number} 命中的大纲项下标；一个都还没越过基准线时返回 -1。
   */
  function activeIndex(anchors, line) {
    let hit = -1;
    for (let i = 0; i < anchors.length; i++) {
      if (anchors[i].top <= line) hit = i;
      else break; // 锚点按文档顺序递增，后面的只会更靠下
    }
    return hit;
  }

  /**
   * 点击大纲项后，正文要滚到哪一行。
   *
   * 返回期望的 scrollY：让该标题落在视口顶部往下 `line` 处（line 是相对视口
   * 的偏移）。同样按锚点的实际文档坐标算，不做百分比换算。
   *
   * @param {number} anchorTop 目标标题在文档坐标系里的纵坐标
   * @param {number} lineOffset 希望标题停在视口顶部下方多少像素
   * @returns {number} 目标 scrollY（可能为负，调用方 clamp 到 0）
   */
  function scrollToAnchor(anchorTop, lineOffset) {
    return Math.max(0, anchorTop - lineOffset);
  }

  const api = { activeIndex: activeIndex, scrollToAnchor: scrollToAnchor };
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    root.outlineActiveIndex = activeIndex;
    root.outlineScrollToAnchor = scrollToAnchor;
  }
})(typeof window !== 'undefined' ? window : this);

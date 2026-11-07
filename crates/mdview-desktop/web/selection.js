// 原生选区与自绘选区的关系。
//
// 选区的真值在内核（anchor/head），页面上那片高亮由 `.sel` 自绘。浏览器自己
// 那套选区却仍然会出现：Ctrl+A、焦点回到 body 之后的拖动、程序化 `addRange`。
// 两套高亮叠在一起时颜色和边界都不对，所以原生的一律清掉。
//
// 例外只有一处：透明输入框内部的选区归浏览器管（输入法的组合、光标停车点），
// 清掉会打断输入。
//
// 本模块刻意不碰 DOM：选区从参数进来，Node 能直接测（`node selection.test.js`）。
'use strict';

(function (root) {
  /**
   * @param {{rangeCount: number, isCollapsed: boolean, anchorNode: Node|null}|null} sel
   *   `document.getSelection()` 的返回值
   * @param {{contains: (node: Node) => boolean}|null} host 透明输入框
   * @returns {boolean} 需要清空吗
   */
  function shouldClearSelection(sel, host) {
    // 没有选区、或只是光标停车点：不用管
    if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return false;
    const node = sel.anchorNode;
    // 输入框内部的选区交给浏览器，否则输入法组合会被打断
    if (node && host && host.contains(node)) return false;
    return true;
  }

  const api = { shouldClearSelection: shouldClearSelection };
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    root.shouldClearSelection = shouldClearSelection;
  }
})(typeof window !== 'undefined' ? window : this);

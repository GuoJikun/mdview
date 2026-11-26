// 皮肤预置。必须在首帧之前跑完，否则记住 HyperOS 的用户每次启动都会先
// 闪一下 macOS 配色（主题的 data-theme 由 JS 引导，同样有这个窗口期）。
//
// 只能放在 <head> 里、且早于样式表：classic script 会阻塞渲染，
// module 脚本是 defer 的，赶不上首帧。CSP 是 default-src 'self'，
// 所以这里走外部文件而不是内联脚本。
(function () {
  try {
    var skin = localStorage.getItem('mdview-skin');
    if (skin === 'hyperos' || skin === 'macos') {
      document.documentElement.setAttribute('data-skin', skin);
    }
  } catch (e) {
    /* 隐私模式读不到 storage：留空即默认皮肤，交给 CSS 兜底 */
  }
})();

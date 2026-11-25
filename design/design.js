// 风格 / 主题切换：只改 <html> 上的 data-style 与 data-theme，
// 其余视觉全部由 CSS 变量驱动，DOM 不重建。
(function () {
  const root = document.documentElement;

  document.querySelectorAll('.seg').forEach(function (seg) {
    const key = seg.dataset.group; // "style" | "theme"
    seg.addEventListener('click', function (e) {
      const btn = e.target.closest('button[data-value]');
      if (!btn) return;
      seg.querySelectorAll('button').forEach(function (b) {
        b.classList.toggle('on', b === btn);
      });
      root.setAttribute('data-' + key, btn.dataset.value);
    });
  });

  // 键盘：1 / 2 切风格，L / D 切主题，方便设计评审时快速对照
  window.addEventListener('keydown', function (e) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const tag = (e.target.tagName || '').toLowerCase();
    if (tag === 'input' || tag === 'textarea') return;

    const setSeg = function (group, value) {
      const seg = document.querySelector('.seg[data-group="' + group + '"]');
      if (!seg) return;
      seg.querySelectorAll('button').forEach(function (b) {
        b.classList.toggle('on', b.dataset.value === value);
      });
      root.setAttribute('data-' + group, value);
    };

    if (e.key === '1') setSeg('style', 'hyperos');
    if (e.key === '2') setSeg('style', 'macos');
    if (e.key.toLowerCase() === 'l') setSeg('theme', 'light');
    if (e.key.toLowerCase() === 'd') setSeg('theme', 'dark');
  });
})();

// 自绘标题栏（Windows / Linux）与菜单动作入口。
//
// macOS 下这份 UI 被 CSS 隐藏：标题栏和系统菜单栏由 Rust 侧负责，那里发出的
// `menu-action` 事件与这里的菜单项共用同一套 action 字符串。
// 普通 <script> 共享全局作用域，包一层 IIFE 避免和 app.js 的顶层 const 撞名。
(function () {
'use strict';

const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const listen = window.__TAURI__.event?.listen;
const appWindow = getCurrentWindow();

const rootEl = document.documentElement;
const barEl = document.getElementById('titlebar');
const menusEl = document.getElementById('tb-menus');
const popupEl = document.getElementById('tb-menu-popup');
const titleEl = document.getElementById('tb-title');
const maxBtn = document.getElementById('tb-max');
const statusEl = document.getElementById('bar');
const vscrollEl = document.getElementById('vscroll');
const thumbEl = document.getElementById('vscroll-thumb');

/**
 * 菜单项定义。`action` 是唯一动作名：前端菜单点击、macOS 原生菜单事件都用它。
 * 只列内核/前端真正实现了的动作，不摆好看但点了没反应的空项。
 */
const MENUS = {
  file: [
    { action: 'save', label: '保存', acc: 'Ctrl+S' },
    { sep: true },
    { action: 'close-window', label: '关闭窗口', acc: 'Alt+F4' },
  ],
  edit: [
    { action: 'undo', label: '撤销', acc: 'Ctrl+Z' },
    { action: 'redo', label: '重做', acc: 'Ctrl+Y' },
    { sep: true },
    { action: 'cut', label: '剪切', acc: 'Ctrl+X' },
    { action: 'copy', label: '复制', acc: 'Ctrl+C' },
    { action: 'paste', label: '粘贴', acc: 'Ctrl+V' },
    { sep: true },
    { action: 'select-all', label: '全选', acc: 'Ctrl+A' },
  ],
  selection: [
    { action: 'select-all', label: '全选', acc: 'Ctrl+A' },
    { sep: true },
    { action: 'line-home', label: '行首', acc: 'Home' },
    { action: 'line-end', label: '行尾', acc: 'End' },
  ],
  view: [
    { action: 'theme', label: '切换浅色 / 深色' },
    { sep: true },
    { action: 'mode-wysiwyg', label: '所见即所得' },
    { action: 'mode-reading', label: '阅读视图' },
    { action: 'mode-source', label: '源码视图' },
  ],
  go: [
    { action: 'outline-prev', label: '上一个标题' },
    { action: 'outline-next', label: '下一个标题' },
    { sep: true },
    { action: 'doc-start', label: '文档开头', acc: 'Ctrl+Home' },
    { action: 'doc-end', label: '文档结尾', acc: 'Ctrl+End' },
  ],
  help: [
    { action: 'about', label: '关于 mdview' },
  ],
};

let openMenu = null;          // 当前展开的 data-menu 值
let activeItemIndex = -1;     // 键盘高亮的下标（只在可点击项里计数）

function itemsOf(menu) {
  return MENUS[menu] || [];
}

/** 当前弹出菜单里所有可点击的 <button>。 */
function popupButtons() {
  return Array.from(popupEl.querySelectorAll('button[data-action]'));
}

function closePopup() {
  openMenu = null;
  activeItemIndex = -1;
  popupEl.hidden = true;
  popupEl.replaceChildren();
  for (const btn of menusEl.querySelectorAll('.tb-menu')) btn.classList.remove('open');
}

/** 标题栏标题跟随文件名，和 VSCode「文件名 — 应用名」的写法一致。 */
function setTitle(path, dirty) {
  const name = path ? String(path).split(/[\\/]/).pop() : '未命名';
  const text = name + (dirty ? ' ●' : '') + ' — mdview';
  titleEl.textContent = text;
  document.title = text;
  appWindow.setTitle(text).catch(() => {});
}
window.__mdviewSetTitle = setTitle;

function highlight(index) {
  const buttons = popupButtons();
  activeItemIndex = (index + buttons.length) % buttons.length;
  buttons.forEach((b, i) => b.classList.toggle('focused', i === activeItemIndex));
}

/** 在按钮下方弹出菜单，右侧放不下时整体左移。 */
function showPopup(menu, anchor) {
  const items = itemsOf(menu);
  if (!items.length) return;
  popupEl.replaceChildren(...items.map((item) => {
    if (item.sep) {
      const sep = document.createElement('div');
      sep.className = 'sep';
      return sep;
    }
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.setAttribute('role', 'menuitem');
    btn.dataset.action = item.action;
    const label = document.createElement('span');
    label.textContent = item.label;
    btn.appendChild(label);
    if (item.acc) {
      const acc = document.createElement('span');
      acc.className = 'acc';
      acc.textContent = item.acc;
      btn.appendChild(acc);
    }
    return btn;
  }));
  popupEl.hidden = false;
  activeItemIndex = -1;

  const rect = anchor.getBoundingClientRect();
  const width = popupEl.offsetWidth;
  const left = Math.max(4, Math.min(rect.left, window.innerWidth - width - 4));
  popupEl.style.left = left + 'px';
  popupEl.style.top = barEl.getBoundingClientRect().bottom + 'px';

  openMenu = menu;
  for (const btn of menusEl.querySelectorAll('.tb-menu')) {
    btn.classList.toggle('open', btn === anchor);
  }
}

function toggleMenu(menu, anchor) {
  if (openMenu === menu) {
    closePopup();
  } else {
    showPopup(menu, anchor);
  }
}

function switchMenu(dir) {
  const buttons = Array.from(menusEl.querySelectorAll('.tb-menu'));
  const current = buttons.findIndex((b) => b.dataset.menu === openMenu);
  const next = buttons[(current + dir + buttons.length) % buttons.length];
  showPopup(next.dataset.menu, next);
}

function run(action) {
  closePopup();
  if (action === 'close-window') { appWindow.close(); return; }
  if (typeof window.__mdviewRunAction === 'function') {
    window.__mdviewRunAction(action);
  }
}

// ---- 窗口按钮 --------------------------------------------------------

document.getElementById('tb-min').addEventListener('click', () => appWindow.minimize());
maxBtn.addEventListener('click', () => appWindow.toggleMaximize());
document.getElementById('tb-close').addEventListener('click', () => appWindow.close());

async function syncMaximizeIcon() {
  const maximized = await appWindow.isMaximized().catch(() => false);
  maxBtn.querySelector('.tb-max-rect').style.display = maximized ? 'none' : '';
  maxBtn.querySelector('.tb-restore-icon').style.display = maximized ? '' : 'none';
  maxBtn.title = maximized ? '还原' : '最大化';
}
syncMaximizeIcon();
if (listen) listen('tauri://resize', syncMaximizeIcon).catch(() => {});

// 双击标题栏空白处切换最大化；按钮与菜单项不算（它们自己有点击行为）。
barEl.addEventListener('dblclick', (event) => {
  if (event.target === barEl || event.target.hasAttribute('data-tauri-drag-region')) {
    appWindow.toggleMaximize();
  }
});

// ---- 菜单交互 --------------------------------------------------------

menusEl.addEventListener('click', (event) => {
  const btn = event.target.closest('.tb-menu');
  if (btn) toggleMenu(btn.dataset.menu, btn);
});

// 菜单展开时，鼠标移到另一个顶级菜单上直接切换（VSCode / Windows 惯例）
menusEl.addEventListener('mouseover', (event) => {
  if (!openMenu) return;
  const btn = event.target.closest('.tb-menu');
  if (btn && btn.dataset.menu !== openMenu) showPopup(btn.dataset.menu, btn);
});

popupEl.addEventListener('click', (event) => {
  const btn = event.target.closest('button[data-action]');
  if (btn) run(btn.dataset.action);
});

document.addEventListener('mousedown', (event) => {
  if (!openMenu) return;
  if (!popupEl.contains(event.target) && !event.target.closest('.tb-menu')) closePopup();
});

document.addEventListener('keydown', (event) => {
  if (!openMenu) return;
  const NAV = ['Escape', 'ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight', 'Enter'];
  if (NAV.indexOf(event.key) < 0) return;
  // 菜单开着时这些键归菜单：不能让 app.js 的 window 监听器同时挪动光标
  event.preventDefault();
  event.stopPropagation();
  if (event.key === 'Escape') closePopup();
  else if (event.key === 'ArrowDown') highlight(activeItemIndex + 1);
  else if (event.key === 'ArrowUp') highlight(activeItemIndex - 1);
  else if (event.key === 'ArrowLeft') switchMenu(-1);
  else if (event.key === 'ArrowRight') switchMenu(1);
  else if (activeItemIndex >= 0) {
    const btn = popupButtons()[activeItemIndex];
    if (btn) run(btn.dataset.action);
  }
});

// ---- 自绘纵向滚动条 --------------------------------------------------
// 原生滚动条属于窗口视口，从 y=0 一直画到窗口底，必然压住自绘标题栏和状态栏。
// 这里隐藏它，用一个只覆盖中间内容区的滚动条代替；滚动本身仍是 window 的，
// 所以 caret / 大纲 / 拖拽自动滚动那套视口坐标逻辑完全不用改。

function scrollMax() {
  const scroller = document.scrollingElement || document.documentElement;
  return Math.max(0, scroller.scrollHeight - window.innerHeight);
}

function refreshScrollbar() {
  if (!rootEl.classList.contains('custom-titlebar')) return;
  const scroller = document.scrollingElement || document.documentElement;
  const max = scrollMax();
  const top = barEl.getBoundingClientRect().height;
  const bottom = statusEl ? statusEl.getBoundingClientRect().height : 0;
  vscrollEl.style.top = top + 'px';
  vscrollEl.style.bottom = bottom + 'px';
  if (max <= 0) {
    thumbEl.style.display = 'none';
    return;
  }
  thumbEl.style.display = '';
  const track = Math.max(1, window.innerHeight - top - bottom);
  const height = Math.max(28, Math.min(track, track * window.innerHeight / scroller.scrollHeight));
  thumbEl.style.height = height + 'px';
  thumbEl.style.top = (window.scrollY / max) * (track - height) + 'px';
}
window.__mdviewRefreshScrollbar = refreshScrollbar;

let scrollDrag = null;
thumbEl.addEventListener('mousedown', (event) => {
  event.preventDefault();
  scrollDrag = { y: event.clientY, top: window.scrollY, max: scrollMax() };
  vscrollEl.classList.add('dragging');
});
vscrollEl.addEventListener('mousedown', (event) => {
  if (event.target === thumbEl) return;
  const max = scrollMax();
  if (max <= 0) return;
  const rect = vscrollEl.getBoundingClientRect();
  const ratio = (event.clientY - rect.top - thumbEl.offsetHeight / 2) /
    Math.max(1, rect.height - thumbEl.offsetHeight);
  window.scrollTo(0, Math.max(0, Math.min(max, ratio * max)));
});
document.addEventListener('mousemove', (event) => {
  if (!scrollDrag) return;
  const rect = vscrollEl.getBoundingClientRect();
  const room = Math.max(1, rect.height - thumbEl.offsetHeight);
  const delta = ((event.clientY - scrollDrag.y) / room) * scrollDrag.max;
  window.scrollTo(0, Math.max(0, Math.min(scrollDrag.max, scrollDrag.top + delta)));
});
document.addEventListener('mouseup', () => {
  if (!scrollDrag) return;
  scrollDrag = null;
  vscrollEl.classList.remove('dragging');
});
window.addEventListener('scroll', refreshScrollbar, { passive: true });
window.addEventListener('resize', refreshScrollbar);

// ---- 平台复核 --------------------------------------------------------
// <head> 里的 UA 判定只是为了首帧不闪；真正的判据是 Rust 侧的 decorations 配置。
invoke('use_custom_titlebar').then((custom) => {
  rootEl.classList.toggle('custom-titlebar', !!custom);
  rootEl.classList.toggle('native-titlebar', !custom);
}).catch(() => {});
})();

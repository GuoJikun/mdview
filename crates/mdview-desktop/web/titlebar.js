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
 * 菜单项定义。结构对齐 image-view 的 MenuEntry：
 *   sep      分隔线
 *   action   动作名（与 macOS 原生菜单、app.js 的 runAction 共用）
 *   acc      快捷键文案，右对齐
 *   checked  勾选状态，布尔或函数（打开菜单时求值，跟随主题 / 视图模式）
 *   mark     'check'（✓）或 'dot'（•），默认 check
 *   danger   危险项，红色文字
 *   sub      子菜单：{ label, items:[...] }
 * 只列内核/前端真正实现了的动作，不摆点了没反应的空项。
 */
const MENUS = {
  file: [
    { action: 'save', label: '保存', acc: 'Ctrl+S' },
    { sep: true },
    { action: 'close-window', label: '关闭窗口', acc: 'Alt+F4', danger: true },
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
    {
      sub: {
        label: '主题',
        items: [
          {
            action: 'theme-system', label: '跟随系统', mark: 'dot',
            checked: () => window.mdviewTheme.mode() === 'system',
          },
          {
            action: 'theme-light', label: '亮色', mark: 'dot',
            checked: () => window.mdviewTheme.mode() === 'light',
          },
          {
            action: 'theme-dark', label: '暗色', mark: 'dot',
            checked: () => window.mdviewTheme.mode() === 'dark',
          },
        ],
      },
    },
    { sep: true },
    {
      action: 'mode-wysiwyg', label: '所见即所得', mark: 'dot',
      checked: () => window.__mdviewMode === 'wysiwyg',
    },
    {
      action: 'mode-reading', label: '阅读视图', mark: 'dot',
      checked: () => window.__mdviewMode === 'reading',
    },
    {
      action: 'mode-source', label: '源码视图', mark: 'dot',
      checked: () => window.__mdviewMode === 'source',
    },
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
let openSubHost = null;       // 当前展开的子菜单宿主元素

function itemsOf(menu) {
  return MENUS[menu] || [];
}

/** 当前弹出菜单里所有可点击的 <button>（含已展开的子菜单）。 */
function popupButtons() {
  return Array.from(popupEl.querySelectorAll('button[data-action]'));
}

function checkedOf(item) {
  return typeof item.checked === 'function' ? !!item.checked() : !!item.checked;
}

/** 销毁已展开的子菜单面板，回到只有顶级项的干净状态。 */
function closeSubmenu() {
  if (!openSubHost) return;
  const panels = openSubHost.getElementsByClassName('tb-submenu');
  while (panels.length) panels[0].remove();
  const trigger = openSubHost.querySelector('.submenu-trigger');
  if (trigger) trigger.classList.remove('open');
  openSubHost = null;
}

function closePopup() {
  openMenu = null;
  activeItemIndex = -1;
  closeSubmenu();
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

/**
 * 把一个菜单项渲染成 DOM：分隔线 / 动作按钮 / 子菜单宿主。
 * checked 求值放在打开菜单的瞬间，所以勾选状态永远是最新的。
 */
function renderEntry(entry) {
  if (entry.sep) {
    const sep = document.createElement('div');
    sep.className = 'tb-sep';
    sep.setAttribute('role', 'separator');
    return sep;
  }

  // 子菜单：一行带右箭头的触发器，悬停 / 点击时在右侧展开子面板。
  if (entry.sub) {
    const host = document.createElement('div');
    host.className = 'submenu-host';

    const trigger = document.createElement('button');
    trigger.type = 'button';
    trigger.className = 'submenu-trigger';
    trigger.setAttribute('role', 'menuitem');
    trigger.setAttribute('aria-haspopup', 'true');
    const gutter = document.createElement('span');
    gutter.className = 'gutter';
    const label = document.createElement('span');
    label.className = 'label';
    // 标签写在 sub.label 里（见 MENUS 注释），entry.label 作兼容兜底
    label.textContent = entry.sub.label || entry.label || '';
    const chev = document.createElement('span');
    chev.className = 'chev';
    chev.textContent = '›';
    trigger.append(gutter, label, chev);
    host.appendChild(trigger);

    const openSub = () => {
      if (openSubHost === host) return;
      closeSubmenu();
      const panel = document.createElement('div');
      panel.className = 'tb-submenu';
      panel.setAttribute('role', 'menu');
      const children = (entry.sub.items || []).map(renderEntry);
      // 给子菜单里的动作按钮补上 data-action（renderEntry 对 plain 项已经做了，
      // 这里再兜一次，保证点击链路一致）。
      panel.replaceChildren(...children);
      host.appendChild(panel);
      openSubHost = host;
      trigger.classList.add('open');
    };
    trigger.addEventListener('pointerenter', openSub);
    trigger.addEventListener('click', (event) => {
      event.stopPropagation();
      if (openSubHost === host) closeSubmenu();
      else openSub();
    });
    return host;
  }

  const btn = document.createElement('button');
  btn.type = 'button';
  btn.setAttribute('role', entry.mark === 'dot' ? 'menuitemradio' : 'menuitem');
  btn.dataset.action = entry.action;
  const isChecked = checkedOf(entry);
  if (entry.mark === 'dot') btn.setAttribute('aria-checked', String(isChecked));

  const gutter = document.createElement('span');
  gutter.className = 'gutter';
  gutter.textContent = isChecked ? (entry.mark === 'dot' ? '•' : '✓') : '';
  btn.appendChild(gutter);

  const label = document.createElement('span');
  label.className = 'label';
  label.textContent = entry.label;
  btn.appendChild(label);

  if (entry.acc) {
    const acc = document.createElement('span');
    acc.className = 'acc';
    acc.textContent = entry.acc;
    btn.appendChild(acc);
  }

  if (entry.danger) btn.classList.add('danger');
  // 主菜单项悬停时收起子菜单（VS Code / macOS 行为）。
  // 但不能对子菜单内部的项生效：否则鼠标刚移到子菜单项上，面板就被自己
  // 的 pointerenter 销毁了，永远点不到。用 closest 判断而非传参，多级嵌套
  // 也自动成立。
  btn.addEventListener('pointerenter', (event) => {
    if (!event.currentTarget.closest('.tb-submenu')) closeSubmenu();
  });
  return btn;
}

/** 在按钮下方弹出菜单，右侧放不下时整体左移。 */
function showPopup(menu, anchor) {
  const items = itemsOf(menu);
  if (!items.length) return;
  closeSubmenu();
  popupEl.replaceChildren(...items.map(renderEntry));
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
// 这里隐藏它，用一个只覆盖中间内容区的滚动条代替；滚动本身仍是各容器自己的，
// 所以 caret / 大纲锚点 / 拖拽自动滚动那套坐标逻辑完全不用改。
//
// 正文（window 滚动）和大纲（#outline 自身滚动）共用这一套视觉与拖动，
// 差异只在「怎么读写滚动量」——由 spec 传进来。返回的 refresh 用来重算滑块。
function attachScrollbar(spec) {
  const track = spec.track;
  const thumb = spec.thumb;
  let drag = null;

  function refresh() {
    if (!rootEl.classList.contains('custom-titlebar')) return;
    if (spec.edges) {
      const e = spec.edges();
      track.style.top = e.top + 'px';
      track.style.bottom = e.bottom + 'px';
    }
    const max = spec.max();
    if (max <= 0) { thumb.style.display = 'none'; return; }
    thumb.style.display = '';
    const trackPx = Math.max(1, track.getBoundingClientRect().height);
    const height = Math.max(28, Math.min(trackPx,
      trackPx * spec.viewport() / spec.content()));
    thumb.style.height = height + 'px';
    thumb.style.top = (spec.pos() / max) * (trackPx - height) + 'px';
  }

  thumb.addEventListener('mousedown', (event) => {
    event.preventDefault();
    drag = { y: event.clientY, pos: spec.pos(), max: spec.max() };
    track.classList.add('dragging');
  });
  track.addEventListener('mousedown', (event) => {
    if (event.target === thumb) return;
    const max = spec.max();
    if (max <= 0) return;
    const rect = track.getBoundingClientRect();
    const ratio = (event.clientY - rect.top - thumb.offsetHeight / 2) /
      Math.max(1, rect.height - thumb.offsetHeight);
    spec.setPos(Math.max(0, Math.min(max, ratio * max)));
  });
  document.addEventListener('mousemove', (event) => {
    if (!drag) return;
    const rect = track.getBoundingClientRect();
    const room = Math.max(1, rect.height - thumb.offsetHeight);
    const delta = ((event.clientY - drag.y) / room) * drag.max;
    spec.setPos(Math.max(0, Math.min(drag.max, drag.pos + delta)));
  });
  document.addEventListener('mouseup', () => {
    if (!drag) return;
    drag = null;
    track.classList.remove('dragging');
  });
  return refresh;
}

function docScrollSpec() {
  const scroller = document.scrollingElement || document.documentElement;
  return {
    track: vscrollEl, thumb: thumbEl,
    viewport: () => window.innerHeight,
    content: () => scroller.scrollHeight,
    max: () => Math.max(0, scroller.scrollHeight - window.innerHeight),
    pos: () => window.scrollY,
    setPos: (v) => window.scrollTo(0, v),
    // 正文轨道要避开标题栏和状态栏，高度随两者实测值变化
    edges: () => ({
      top: barEl.getBoundingClientRect().height,
      bottom: statusEl ? statusEl.getBoundingClientRect().height : 0,
    }),
  };
}

function outlineScrollSpec() {
  const nav = document.getElementById('outline');
  return {
    track: document.getElementById('outline-scroll'),
    thumb: document.getElementById('outline-scroll-thumb'),
    viewport: () => nav.clientHeight,
    content: () => nav.scrollHeight,
    max: () => Math.max(0, nav.scrollHeight - nav.clientHeight),
    pos: () => nav.scrollTop,
    setPos: (v) => { nav.scrollTop = v; },
    // 大纲轨道上下边缘由 CSS 固定（top:30px / bottom:36px），无需动态设
  };
}

const refreshDocScroll = attachScrollbar(docScrollSpec());
const refreshOutlineScroll = attachScrollbar(outlineScrollSpec());

function refreshScrollbar() {
  refreshDocScroll();
  refreshOutlineScroll();
}
window.__mdviewRefreshScrollbar = refreshScrollbar;

window.addEventListener('scroll', refreshScrollbar, { passive: true });
document.getElementById('outline').addEventListener('scroll', refreshScrollbar, { passive: true });
window.addEventListener('resize', refreshScrollbar);

// ---- 平台复核 --------------------------------------------------------
// <head> 里的 UA 判定只是为了首帧不闪；真正的判据是 Rust 侧的 decorations 配置。
invoke('use_custom_titlebar').then((custom) => {
  rootEl.classList.toggle('custom-titlebar', !!custom);
  rootEl.classList.toggle('native-titlebar', !custom);
}).catch(() => {});
})();

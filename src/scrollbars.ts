// 自绘滚动条。原生滚动条属于窗口视口，会纵跨标题栏和状态栏；这里隐藏它，
// 用只覆盖中间内容区的滚动条代替。滚动本身仍归各容器，所以 caret / 大纲
// 锚点 / 拖拽自动滚动那套坐标逻辑完全不用改。
//
// 三套共用这一套视觉与拖动：正文纵向（window 或 textarea）、大纲纵向
// （#outline）、源码视图横向（textarea 长行）。差异只在「沿哪个轴」与
// 「怎么读写滚动量」——都由 spec 传进来。
const rootEl = document.documentElement;

/** 轨道要在某条边处让位：返回窗口底边到那条边的距离，元素没渲染时按 0 处理。
 *
 *  为什么不用 height：胶囊状态栏离窗口底还有 10px 外缝，只减 height 会让轨道
 *  落进那道缝里、滑块贴着胶囊右侧划过去。要让位的是「上面那条边」，所以状态栏
 *  取 rect.top；而浮动侧栏卡片要让位的是它自己的下沿，取 rect.bottom。 */
function insetTo(el: Element | null, edge: 'top' | 'bottom'): number {
  if (!el) return 0;
  const rect = el.getBoundingClientRect();
  if (!rect.height) return 0;
  return window.innerHeight - (edge === 'top' ? rect.top : rect.bottom);
}

interface ScrollEdges {
  top?: number;
  bottom?: number;
  left?: number;
  right?: number;
}

interface ScrollSpec {
  axis: 'y' | 'x';
  track: HTMLElement | null;
  thumb: HTMLElement | null;
  viewport: () => number;
  content: () => number;
  max: () => number;
  pos: () => number;
  setPos: (v: number) => void;
  edges?: () => ScrollEdges;
}

function attachScrollbar(spec: ScrollSpec): () => void {
  // 骨架 DOM 缺了某一对（比如某个容器在某个视图下不该有滚动条）就不装，
  // 滚动本身不受影响——自绘条只是装饰，不该让整页崩掉。
  if (!spec.track || !spec.thumb) return () => {};
  // 判空之后另起别名：TS 不把「已判空」的结论带进下面这些闭包。
  const trackEl = spec.track as HTMLElement;
  const thumbEl = spec.thumb as HTMLElement;
  const horizontal = spec.axis === 'x';
  let drag: { coord: number; pos: number; max: number } | null = null;

  // 沿轴取坐标/尺寸，横向纵向共用同一套公式
  const mouseCoord = (event: MouseEvent) => horizontal ? event.clientX : event.clientY;
  const trackStart = () => {
    const rect = trackEl.getBoundingClientRect();
    return horizontal ? rect.left : rect.top;
  };
  const trackSize = () => {
    const rect = trackEl.getBoundingClientRect();
    return horizontal ? rect.width : rect.height;
  };
  const thumbSize = () => horizontal ? thumbEl.offsetWidth : thumbEl.offsetHeight;

  function refresh(): void {
    if (!rootEl.classList.contains('custom-titlebar')) return;
    // 边缘与轴无关：横向条也要按状态栏高度更新 bottom（隐藏后归零贴底），
    // 不能只在纵向分支处理 top/bottom——否则隐藏状态栏后横向条悬在半空。
    if (spec.edges) {
      const e = spec.edges();
      if (e.top !== undefined) trackEl.style.top = e.top + 'px';
      if (e.bottom !== undefined) trackEl.style.bottom = e.bottom + 'px';
      if (e.left !== undefined) trackEl.style.left = e.left + 'px';
      if (e.right !== undefined) trackEl.style.right = e.right + 'px';
    }
    const max = spec.max();
    if (max <= 0) { thumbEl.style.display = 'none'; return; }
    thumbEl.style.display = '';
    const trackPx = Math.max(1, trackSize());
    const size = Math.max(28, Math.min(trackPx,
      trackPx * spec.viewport() / spec.content()));
    const offset = (spec.pos() / max) * (trackPx - size);
    if (horizontal) {
      thumbEl.style.width = size + 'px';
      thumbEl.style.left = offset + 'px';
    } else {
      thumbEl.style.height = size + 'px';
      thumbEl.style.top = offset + 'px';
    }
  }

  thumbEl.addEventListener('mousedown', (event) => {
    event.preventDefault();
    drag = { coord: mouseCoord(event), pos: spec.pos(), max: spec.max() };
    trackEl.classList.add('dragging');
  });
  trackEl.addEventListener('mousedown', (event) => {
    if (event.target === thumbEl) return;
    const max = spec.max();
    if (max <= 0) return;
    const ratio = (mouseCoord(event) - trackStart() - thumbSize() / 2) /
      Math.max(1, trackSize() - thumbSize());
    spec.setPos(Math.max(0, Math.min(max, ratio * max)));
  });
  document.addEventListener('mousemove', (event) => {
    if (!drag) return;
    const room = Math.max(1, trackSize() - thumbSize());
    const delta = ((mouseCoord(event) - drag.coord) / room) * drag.max;
    spec.setPos(Math.max(0, Math.min(drag.max, drag.pos + delta)));
  });
  document.addEventListener('mouseup', () => {
    if (!drag) return;
    drag = null;
    trackEl.classList.remove('dragging');
  });
  return refresh;
}

function docScrollSpec(): ScrollSpec {
  const scroller = document.scrollingElement || document.documentElement;
  const sourceEl = document.getElementById('source') as HTMLTextAreaElement | null;
  // 源码视图里正文区是 textarea 自身滚动（window 不再滚动），同一个
  // #vscroll 改读写它的 scrollTop；其余视图仍驱动 window。
  const inSource = () => !!sourceEl && window.__mdviewMode === 'source';
  return {
    axis: 'y',
    track: document.getElementById('vscroll') as HTMLElement,
    thumb: document.getElementById('vscroll-thumb') as HTMLElement,
    viewport: () => inSource() ? sourceEl!.clientHeight : window.innerHeight,
    content: () => inSource() ? sourceEl!.scrollHeight : scroller.scrollHeight,
    max: () => inSource()
      ? Math.max(0, sourceEl!.scrollHeight - sourceEl!.clientHeight)
      : Math.max(0, scroller.scrollHeight - window.innerHeight),
    pos: () => inSource() ? sourceEl!.scrollTop : window.scrollY,
    setPos: (v) => {
      if (inSource()) sourceEl!.scrollTop = v;
      else window.scrollTo(0, v);
    },
    // 正文轨道要避开标题栏和状态栏，高度随两者实测值变化。
    // 标题栏现在由 Vue 渲染，必须惰性取，模块求值时它还不存在。
    // 用 rect.bottom 而不是 height：胶囊状态栏离窗口底边有 10px 缝，
    // 取 height 会让轨道伸进那道缝、滑块压到胶囊上。
    edges: () => {
      const titlebar = document.getElementById('titlebar');
      return {
        top: titlebar ? titlebar.getBoundingClientRect().bottom : 0,
        bottom: insetTo(document.getElementById('bar'), 'top'),
      };
    },
  };
}

function outlineScrollSpec(): ScrollSpec {
  const nav = document.getElementById('outline') as HTMLElement;
  return {
    axis: 'y',
    track: document.getElementById('outline-scroll') as HTMLElement,
    thumb: document.getElementById('outline-scroll-thumb') as HTMLElement,
    viewport: () => nav.clientHeight,
    content: () => nav.scrollHeight,
    max: () => Math.max(0, nav.scrollHeight - nav.clientHeight),
    pos: () => nav.scrollTop,
    setPos: (v) => { nav.scrollTop = v; },
    // 大纲轨道也跟着标题栏/状态栏实测高度走：阅读与源码视图隐藏状态栏后，
    // 轨道要延伸到底，滑块才能滚到最后一项（原来写死 CSS 的 29px 会悬空）。
    // 另外皮肤可以把侧栏做成浮动卡片：此时卡片底边比状态栏高，轨道要停在
    // 卡片底边，取两者中更靠上的那个。
    edges: () => {
      const titlebar = document.getElementById('titlebar');
      const navTop = nav.getBoundingClientRect().top;
      return {
        top: Math.max(titlebar ? titlebar.getBoundingClientRect().bottom : 0, navTop),
        bottom: Math.max(insetTo(document.getElementById('bar'), 'top'), insetTo(nav, 'bottom')),
      };
    },
  };
}

function filesScrollSpec(): ScrollSpec {
  const panel = document.getElementById('files') as HTMLElement;
  return {
    axis: 'y',
    track: document.getElementById('files-scroll') as HTMLElement,
    thumb: document.getElementById('files-scroll-thumb') as HTMLElement,
    viewport: () => panel.clientHeight,
    content: () => panel.scrollHeight,
    max: () => Math.max(0, panel.scrollHeight - panel.clientHeight),
    pos: () => panel.scrollTop,
    setPos: (v) => { panel.scrollTop = v; },
    // 轨道与面板等高：上沿避开标题栏，下沿停在面板自己的底边（面板只占侧栏
    // 上半段，轨道跟着它收，不然滑块会压到大纲上）。
    edges: () => {
      const titlebar = document.getElementById('titlebar');
      const box = panel.getBoundingClientRect();
      return {
        top: Math.max(titlebar ? titlebar.getBoundingClientRect().bottom : 0, box.top),
        bottom: window.innerHeight - box.bottom,
      };
    },
  };
}

function sourceHScrollSpec(): ScrollSpec {
  const sourceEl = document.getElementById('source') as HTMLTextAreaElement;
  return {
    axis: 'x',
    track: document.getElementById('hscroll') as HTMLElement,
    thumb: document.getElementById('hscroll-thumb') as HTMLElement,
    viewport: () => sourceEl.clientWidth,
    content: () => sourceEl.scrollWidth,
    max: () => Math.max(0, sourceEl.scrollWidth - sourceEl.clientWidth),
    pos: () => sourceEl.scrollLeft,
    setPos: (v) => { sourceEl.scrollLeft = v; },
    // 横向轨道贴状态栏上沿；左右就是窗口两侧（CSS 已 fixed 到 0）
    edges: () => ({
      bottom: insetTo(document.getElementById('bar'), 'top'),
    }),
  };
}

const refreshDocScroll = attachScrollbar(docScrollSpec());
const refreshOutlineScroll = attachScrollbar(outlineScrollSpec());
const refreshFilesScroll = attachScrollbar(filesScrollSpec());
const refreshSourceHScroll = attachScrollbar(sourceHScrollSpec());

export function refreshScrollbar(): void {
  refreshDocScroll();
  refreshOutlineScroll();
  refreshFilesScroll();
  refreshSourceHScroll();
}

window.__mdviewRefreshScrollbar = refreshScrollbar;

window.addEventListener('scroll', refreshScrollbar, { passive: true });
document.getElementById('outline')?.addEventListener('scroll', refreshScrollbar, { passive: true });
document.getElementById('files')?.addEventListener('scroll', refreshScrollbar, { passive: true });
document.getElementById('source')?.addEventListener('scroll', refreshScrollbar, { passive: true });
window.addEventListener('resize', refreshScrollbar);

declare global {
  interface Window {
    __mdviewRefreshScrollbar?: () => void;
  }
}

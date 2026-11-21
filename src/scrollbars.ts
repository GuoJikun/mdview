// 自绘滚动条。原生滚动条属于窗口视口，会纵跨标题栏和状态栏；这里隐藏它，
// 用只覆盖中间内容区的滚动条代替。滚动本身仍归各容器，所以 caret / 大纲
// 锚点 / 拖拽自动滚动那套坐标逻辑完全不用改。
//
// 三套共用这一套视觉与拖动：正文纵向（window 或 textarea）、大纲纵向
// （#outline）、源码视图横向（textarea 长行）。差异只在「沿哪个轴」与
// 「怎么读写滚动量」——都由 spec 传进来。
const rootEl = document.documentElement;

interface ScrollEdges {
  top?: number;
  bottom?: number;
  left?: number;
  right?: number;
}

interface ScrollSpec {
  axis: 'y' | 'x';
  track: HTMLElement;
  thumb: HTMLElement;
  viewport: () => number;
  content: () => number;
  max: () => number;
  pos: () => number;
  setPos: (v: number) => void;
  edges?: () => ScrollEdges;
}

function attachScrollbar(spec: ScrollSpec): () => void {
  const track = spec.track;
  const thumb = spec.thumb;
  const horizontal = spec.axis === 'x';
  let drag: { coord: number; pos: number; max: number } | null = null;

  // 沿轴取坐标/尺寸，横向纵向共用同一套公式
  const mouseCoord = (event: MouseEvent) => horizontal ? event.clientX : event.clientY;
  const trackStart = () => {
    const rect = track.getBoundingClientRect();
    return horizontal ? rect.left : rect.top;
  };
  const trackSize = () => {
    const rect = track.getBoundingClientRect();
    return horizontal ? rect.width : rect.height;
  };
  const thumbSize = () => horizontal ? thumb.offsetWidth : thumb.offsetHeight;

  function refresh(): void {
    if (!rootEl.classList.contains('custom-titlebar')) return;
    // 边缘与轴无关：横向条也要按状态栏高度更新 bottom（隐藏后归零贴底），
    // 不能只在纵向分支处理 top/bottom——否则隐藏状态栏后横向条悬在半空。
    if (spec.edges) {
      const e = spec.edges();
      if (e.top !== undefined) track.style.top = e.top + 'px';
      if (e.bottom !== undefined) track.style.bottom = e.bottom + 'px';
      if (e.left !== undefined) track.style.left = e.left + 'px';
      if (e.right !== undefined) track.style.right = e.right + 'px';
    }
    const max = spec.max();
    if (max <= 0) { thumb.style.display = 'none'; return; }
    thumb.style.display = '';
    const trackPx = Math.max(1, trackSize());
    const size = Math.max(28, Math.min(trackPx,
      trackPx * spec.viewport() / spec.content()));
    const offset = (spec.pos() / max) * (trackPx - size);
    if (horizontal) {
      thumb.style.width = size + 'px';
      thumb.style.left = offset + 'px';
    } else {
      thumb.style.height = size + 'px';
      thumb.style.top = offset + 'px';
    }
  }

  thumb.addEventListener('mousedown', (event) => {
    event.preventDefault();
    drag = { coord: mouseCoord(event), pos: spec.pos(), max: spec.max() };
    track.classList.add('dragging');
  });
  track.addEventListener('mousedown', (event) => {
    if (event.target === thumb) return;
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
    track.classList.remove('dragging');
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
    edges: () => {
      const titlebar = document.getElementById('titlebar');
      const statusbar = document.getElementById('bar');
      return {
        top: titlebar ? titlebar.getBoundingClientRect().height : 0,
        bottom: statusbar ? statusbar.getBoundingClientRect().height : 0,
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
    edges: () => {
      const titlebar = document.getElementById('titlebar');
      const statusbar = document.getElementById('bar');
      return {
        top: titlebar ? titlebar.getBoundingClientRect().height : 0,
        bottom: statusbar ? statusbar.getBoundingClientRect().height : 0,
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
    edges: () => {
      const statusbar = document.getElementById('bar');
      return {
        bottom: statusbar ? statusbar.getBoundingClientRect().height : 0,
      };
    },
  };
}

const refreshDocScroll = attachScrollbar(docScrollSpec());
const refreshOutlineScroll = attachScrollbar(outlineScrollSpec());
const refreshSourceHScroll = attachScrollbar(sourceHScrollSpec());

export function refreshScrollbar(): void {
  refreshDocScroll();
  refreshOutlineScroll();
  refreshSourceHScroll();
}

window.__mdviewRefreshScrollbar = refreshScrollbar;

window.addEventListener('scroll', refreshScrollbar, { passive: true });
document.getElementById('outline')?.addEventListener('scroll', refreshScrollbar, { passive: true });
document.getElementById('source')?.addEventListener('scroll', refreshScrollbar, { passive: true });
window.addEventListener('resize', refreshScrollbar);

declare global {
  interface Window {
    __mdviewRefreshScrollbar?: () => void;
  }
}

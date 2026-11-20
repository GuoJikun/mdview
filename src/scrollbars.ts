// 自绘纵向滚动条。原生滚动条属于窗口视口，会纵跨标题栏和状态栏；
// 这里隐藏它，用一个只覆盖中间内容区的滚动条代替。滚动本身仍归各容器，
// 所以 caret / 大纲锚点 / 拖拽自动滚动那套坐标逻辑完全不用改。
//
// 正文（window 滚动）和大纲（#outline 自身滚动）共用这一套视觉与拖动，
// 差异只在「怎么读写滚动量」——由 spec 传进来。
const rootEl = document.documentElement;

interface ScrollSpec {
  track: HTMLElement;
  thumb: HTMLElement;
  viewport: () => number;
  content: () => number;
  max: () => number;
  pos: () => number;
  setPos: (v: number) => void;
  edges?: () => { top: number; bottom: number };
}

function attachScrollbar(spec: ScrollSpec): () => void {
  const track = spec.track;
  const thumb = spec.thumb;
  let drag: { y: number; pos: number; max: number } | null = null;

  function refresh(): void {
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

function docScrollSpec(): ScrollSpec {
  const scroller = document.scrollingElement || document.documentElement;
  return {
    track: document.getElementById('vscroll') as HTMLElement,
    thumb: document.getElementById('vscroll-thumb') as HTMLElement,
    viewport: () => window.innerHeight,
    content: () => scroller.scrollHeight,
    max: () => Math.max(0, scroller.scrollHeight - window.innerHeight),
    pos: () => window.scrollY,
    setPos: (v) => window.scrollTo(0, v),
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
    track: document.getElementById('outline-scroll') as HTMLElement,
    thumb: document.getElementById('outline-scroll-thumb') as HTMLElement,
    viewport: () => nav.clientHeight,
    content: () => nav.scrollHeight,
    max: () => Math.max(0, nav.scrollHeight - nav.clientHeight),
    pos: () => nav.scrollTop,
    setPos: (v) => { nav.scrollTop = v; },
    // 大纲轨道上下边缘由 CSS 固定（top:30px / bottom:29px），无需动态设
  };
}

const refreshDocScroll = attachScrollbar(docScrollSpec());
const refreshOutlineScroll = attachScrollbar(outlineScrollSpec());

export function refreshScrollbar(): void {
  refreshDocScroll();
  refreshOutlineScroll();
}

window.__mdviewRefreshScrollbar = refreshScrollbar;

window.addEventListener('scroll', refreshScrollbar, { passive: true });
document.getElementById('outline')?.addEventListener('scroll', refreshScrollbar, { passive: true });
window.addEventListener('resize', refreshScrollbar);

declare global {
  interface Window {
    __mdviewRefreshScrollbar?: () => void;
  }
}

// 源码视图的滚动回归：textarea 必须是定高、可滚动的容器，且自绘的
// #vscroll 在 source 模式下改由它的 scrollTop 驱动（不再随 window 滚）。
// 需要真实布局，故走 dom（chromium）project。
import '../app.css';
import { beforeAll, describe, expect, it } from 'vitest';

describe('源码视图滚动', () => {
  beforeAll(async () => {
    document.documentElement.classList.add('custom-titlebar');
    document.documentElement.dataset.view = 'source';
    (window as Window & { __mdviewMode?: string }).__mdviewMode = 'source';
    document.body.innerHTML = `
      <div id="vscroll" class="tb-vscroll"><div id="vscroll-thumb" class="tb-vscroll-thumb"></div></div>
      <div id="hscroll" class="tb-hscroll"><div id="hscroll-thumb" class="tb-hscroll-thumb"></div></div>
      <nav id="outline"></nav>
      <div id="outline-scroll" class="tb-vscroll"><div id="outline-scroll-thumb" class="tb-vscroll-thumb"></div></div>
      <textarea id="source"></textarea>
      <div id="bar"></div>
    `;
    const source = document.getElementById('source') as HTMLTextAreaElement;
    source.value = '这是一行很长很长的内容'.repeat(80) + '\n' +
      Array.from({ length: 120 }, (_, i) => `第 ${i} 行内容`).join('\n');
    await import('../scrollbars');
  });

  it('textarea 是可滚动的定高容器，源码视图不再隐藏自绘滚动条', () => {
    const source = document.getElementById('source') as HTMLTextAreaElement;
    const track = document.getElementById('vscroll') as HTMLElement;
    expect(getComputedStyle(source).overflowY).toBe('auto');
    expect(getComputedStyle(track).display).not.toBe('none');
    expect(source.scrollHeight).toBeGreaterThan(source.clientHeight);
  });

  it('源码视图隐藏状态栏，textarea 与滚动条延伸到底部', () => {
    const bar = document.getElementById('bar') as HTMLElement;
    const source = document.getElementById('source') as HTMLTextAreaElement;
    const htrack = document.getElementById('hscroll') as HTMLElement;
    const otrack = document.getElementById('outline-scroll') as HTMLElement;
    expect(getComputedStyle(bar).display).toBe('none');
    expect(getComputedStyle(source).bottom).toBe('0px');
    // 状态栏高度为 0 时，横向条与大纲纵向条都要刷新到贴底，不能留在 CSS 的 29px
    window.__mdviewRefreshScrollbar?.();
    expect(htrack.style.bottom).toBe('0px');
    expect(otrack.style.bottom).toBe('0px');
  });

  it('滑块随 textarea 滚动量移动，方向键可抵达文末', () => {
    const source = document.getElementById('source') as HTMLTextAreaElement;
    const thumb = document.getElementById('vscroll-thumb') as HTMLElement;
    source.scrollTop = 0;
    window.__mdviewRefreshScrollbar?.();
    expect(thumb.style.display).not.toBe('none');
    const topAtStart = parseFloat(thumb.style.top) || 0;

    source.scrollTop = source.scrollHeight;  // 方向键/滚轮走到最下方
    source.dispatchEvent(new Event('scroll'));
    expect(source.scrollTop).toBeGreaterThan(0);
    const topAtEnd = parseFloat(thumb.style.top);
    expect(topAtEnd).toBeGreaterThan(topAtStart);
  });

  it('拖动滑块反过来驱动 textarea 滚动', () => {
    const source = document.getElementById('source') as HTMLTextAreaElement;
    const thumb = document.getElementById('vscroll-thumb') as HTMLElement;
    source.scrollTop = 0;
    thumb.dispatchEvent(new MouseEvent('mousedown', { clientY: 40, bubbles: true }));
    document.dispatchEvent(new MouseEvent('mousemove', { clientY: 240, bubbles: true }));
    document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
    expect(source.scrollTop).toBeGreaterThan(0);
  });

  it('横向条用自绘而非原生：贴窗口底、6px 粗、非文字光标', () => {
    const track = document.getElementById('hscroll') as HTMLElement;
    const thumb = document.getElementById('hscroll-thumb') as HTMLElement;
    const source = document.getElementById('source') as HTMLTextAreaElement;
    expect(getComputedStyle(track).display).not.toBe('none');
    expect(getComputedStyle(track).left).toBe('0px');
    expect(getComputedStyle(thumb).height).toBe('6px');
    expect(getComputedStyle(thumb).cursor).not.toBe('text');
    expect(source.scrollWidth).toBeGreaterThan(source.clientWidth);
  });

  it('横向滑块随 textarea 横向滚动移动，拖动它反向滚动', () => {
    const source = document.getElementById('source') as HTMLTextAreaElement;
    const thumb = document.getElementById('hscroll-thumb') as HTMLElement;
    source.scrollLeft = 0;
    window.__mdviewRefreshScrollbar?.();
    expect(thumb.style.display).not.toBe('none');
    const leftAtStart = parseFloat(thumb.style.left) || 0;

    source.scrollLeft = source.scrollWidth;  // 滚到最右
    source.dispatchEvent(new Event('scroll'));
    expect(source.scrollLeft).toBeGreaterThan(0);
    expect(parseFloat(thumb.style.left)).toBeGreaterThan(leftAtStart);

    source.scrollLeft = 0;
    thumb.dispatchEvent(new MouseEvent('mousedown', { clientX: 40, bubbles: true }));
    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 240, bubbles: true }));
    document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
    expect(source.scrollLeft).toBeGreaterThan(0);
  });
});

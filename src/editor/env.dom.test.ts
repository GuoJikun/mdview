// 环境自检：确认浏览器模式真的提供了 happy-dom 给不了的东西。
//
// 这些断言在 happy-dom 里会全部失败（几何恒为 0、命中恒为 null），只有真浏览器
// 才成立——所以它们同时也是「这条用例确实跑在浏览器里」的证据。留着的价值在于：
// 浏览器或 vitest 升级后能力若退化，这几条会最先红。
import { describe, expect, it } from 'vitest';

function box(css: string): HTMLElement {
  const el = document.createElement('div');
  el.style.cssText = css;
  el.textContent = '甲乙丙丁';
  document.body.append(el);
  return el;
}

describe('真实布局', () => {
  it('getBoundingClientRect 给出真实像素', () => {
    const el = box('position:absolute;left:40px;top:20px;width:120px;height:30px');
    const r = el.getBoundingClientRect();
    expect(r.left).toBe(40);
    expect(r.top).toBe(20);
    expect(r.width).toBe(120);
    expect(r.height).toBe(30);
  });

  it('文字排版后 lineHeight / 字宽可测', () => {
    const el = box(
      'position:absolute;left:0;top:0;font:16px/24px monospace;white-space:nowrap',
    );
    const range = document.createRange();
    range.selectNodeContents(el);
    const r = range.getBoundingClientRect();
    expect(r.height).toBeGreaterThan(0);
    expect(r.width).toBeGreaterThan(0);
  });
});

describe('命中测试（点击落点换算依赖它）', () => {
  it('elementFromPoint 命中真实元素', () => {
    const el = box('position:absolute;left:100px;top:100px;width:200px;height:40px');
    const hit = document.elementFromPoint(110, 110);
    expect(hit).toBe(el);
  });

  it('caretRangeFromPoint 可用（Chromium 专有 API）', () => {
    expect(typeof document.caretRangeFromPoint).toBe('function');
    const el = box('position:absolute;left:0;top:200px;width:300px;height:24px;font:16px/24px monospace');
    const range = document.caretRangeFromPoint(5, 212);
    expect(range).not.toBeNull();
    expect(el.contains(range!.startContainer)).toBe(true);
  });
});

describe('视口与滚动', () => {
  it('window.scrollTo / scrollY 真实生效', () => {
    const tall = document.createElement('div');
    tall.style.cssText = 'height:3000px;width:10px';
    document.body.append(tall);
    window.scrollTo(0, 500);
    expect(window.scrollY).toBe(500);
    window.scrollTo(0, 0);
  });

  it('viewport 尺寸来自真实窗口', () => {
    expect(window.innerWidth).toBeGreaterThan(0);
    expect(window.innerHeight).toBeGreaterThan(0);
  });
});

describe('事件', () => {
  it('CompositionEvent 可以派发（输入法状态机需要）', () => {
    const target = box('position:absolute;left:0;top:0');
    let seen = '';
    target.addEventListener('compositionstart', (e) => {
      seen = (e as CompositionEvent).data;
    });
    target.dispatchEvent(new CompositionEvent('compositionstart', { data: 'ni', bubbles: true }));
    expect(seen).toBe('ni');
  });

  it('键盘事件的 key/shiftKey 保真', () => {
    let captured: KeyboardEvent | null = null;
    const h = (e: KeyboardEvent) => { captured = e; };
    window.addEventListener('keydown', h);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true }));
    expect(captured!.key).toBe('ArrowDown');
    expect(captured!.shiftKey).toBe(true);
    window.removeEventListener('keydown', h);
  });
});

describe('样式计算', () => {
  it('getComputedStyle 解析 CSS 变量', () => {
    document.documentElement.style.setProperty('--probe-color', 'rgb(1, 2, 3)');
    const el = box('color:var(--probe-color)');
    expect(getComputedStyle(el).color).toBe('rgb(1, 2, 3)');
  });
});

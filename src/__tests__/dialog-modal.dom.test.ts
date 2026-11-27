// 弹窗焦点陷阱：Tab 必须首尾相接关在弹窗里，不能漏到背后的正文。
// 量的是「焦点实际落在哪个元素」，happy-dom 没有真实的 Tab 行为，
// 所以这一组走 dom（chromium）project。
import { beforeEach, describe, expect, it } from 'vitest';
import { focusablesIn, trapTab } from '../dialog-modal';

let dialog: HTMLElement;
let first: HTMLButtonElement;
let middle: HTMLInputElement;
let last: HTMLButtonElement;
let outside: HTMLButtonElement;

beforeEach(() => {
  document.body.innerHTML = `
    <button id="outside">正文里更靠前的按钮</button>
    <div class="dialog-backdrop">
      <section id="panel" role="dialog" tabindex="-1">
        <button id="first">一</button>
        <input id="middle">
        <button id="hidden" hidden>看不见</button>
        <button id="last">末</button>
      </section>
    </div>
  `;
  dialog = document.getElementById('panel') as HTMLElement;
  first = document.getElementById('first') as HTMLButtonElement;
  middle = document.getElementById('middle') as HTMLInputElement;
  last = document.getElementById('last') as HTMLButtonElement;
  outside = document.getElementById('outside') as HTMLButtonElement;
});

describe('focusablesIn', () => {
  it('只收可见的可聚焦元素，跳过 hidden 与 tabindex=-1 的容器', () => {
    expect(focusablesIn(dialog).map((el) => el.id)).toEqual(['first', 'middle', 'last']);
    expect(focusablesIn(dialog)).not.toContain(dialog);
  });
});

describe('trapTab', () => {
  it('末项上按 Tab 回到首项', () => {
    last.focus();
    expect(trapTab(dialog, { key: 'Tab', shiftKey: false }, last)).toBe(true);
    expect(document.activeElement).toBe(first);
  });

  it('首项上按 Shift+Tab 跳到末项', () => {
    first.focus();
    expect(trapTab(dialog, { key: 'Tab', shiftKey: true }, first)).toBe(true);
    expect(document.activeElement).toBe(last);
  });

  it('焦点在中间时不接管，交给浏览器按默认顺序走', () => {
    expect(trapTab(dialog, { key: 'Tab', shiftKey: false }, middle)).toBe(false);
  });

  it('焦点跑到弹窗外时先把它收回弹窗内', () => {
    outside.focus();
    expect(trapTab(dialog, { key: 'Tab', shiftKey: false }, outside)).toBe(true);
    expect(document.activeElement).toBe(first);
    expect(trapTab(dialog, { key: 'Tab', shiftKey: true }, outside)).toBe(true);
    expect(document.activeElement).toBe(last);
  });

  it('非 Tab 键一律不接管', () => {
    expect(trapTab(dialog, { key: 'Enter', shiftKey: false }, first)).toBe(false);
  });
});

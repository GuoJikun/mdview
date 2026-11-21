// 原生选区清理判据：只做形状判断，不碰真实 DOM，跑 node 环境。
import { describe, expect, it } from 'vitest';
import { shouldClearSelection } from './selection.js';

type Sel = Parameters<typeof shouldClearSelection>[0];
type Host = Parameters<typeof shouldClearSelection>[1];

/** 假的透明输入框：只有 contains 参与判断。 */
function host(children: unknown[]) {
  return { contains: (node: unknown) => children.indexOf(node) >= 0 };
}

function selection(fields: Record<string, unknown>) {
  return Object.assign({ rangeCount: 1, isCollapsed: false, anchorNode: {} }, fields);
}

/**
 * 这里传的全是假对象（只求形状够判据用），因此统一经 unknown 转换一次。
 * 用真实 Selection 反而测不到「锚点取不到」这类边界。
 */
function clear(sel: unknown, h: unknown): boolean {
  return shouldClearSelection(sel as Sel, h as Host);
}

describe('shouldClearSelection', () => {
  it('没有选区时不动手', () => {
    expect(clear(null, host([]))).toBe(false);
    expect(clear(selection({ rangeCount: 0 }), host([]))).toBe(false);
    // 只是个光标停车点（点击后锚点塌陷），清它没有意义
    expect(clear(selection({ isCollapsed: true }), host([]))).toBe(false);
  });

  it('页面上的原生选区要清掉', () => {
    // Ctrl+A、焦点在 body 上拖动、程序化 addRange 都走这条
    expect(clear(selection({}), host([]))).toBe(true);
  });

  it('输入框内部的选区放行', () => {
    const inside = {};
    expect(clear(selection({ anchorNode: inside }), host([inside]))).toBe(false);
  });

  it('锚点取不到也要清（宁可多清一次）', () => {
    expect(clear(selection({ anchorNode: null }), host([]))).toBe(true);
  });
});

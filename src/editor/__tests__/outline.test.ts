// 大纲联动纯函数：不碰 DOM（锚点坐标由调用方量好传进来），跑 node 环境。
import { describe, expect, it } from 'vitest';
import { activeIndex, scrollToAnchor } from '../outline.js';

const anchors = [{ top: 0 }, { top: 100 }, { top: 350 }, { top: 900 }];

describe('activeIndex：基准线命中的大纲项', () => {
  it('基准线在首标题之前返回 -1', () => {
    expect(activeIndex(anchors, -10)).toBe(-1);
  });

  it('基准线等于锚点时命中该项', () => {
    // 正好压在标题上算越过
    expect(activeIndex(anchors, 0)).toBe(0);
  });

  it('两锚点之间取前一个', () => {
    expect(activeIndex(anchors, 200)).toBe(1);
  });

  it('越过第三个锚点', () => {
    expect(activeIndex(anchors, 500)).toBe(2);
  });

  it('越过全部取最后一项', () => {
    expect(activeIndex(anchors, 5000)).toBe(3);
  });

  it('空锚点返回 -1', () => {
    expect(activeIndex([], 100)).toBe(-1);
  });

  it('稀疏锚点按坐标而非序号', () => {
    // 标题间隔很大时依然按实际坐标判定，而不是按序号均分
    const sparse = [{ top: 0 }, { top: 2000 }];
    expect(activeIndex(sparse, 1500)).toBe(0);
  });

  it('稀疏锚点越过后才切', () => {
    const sparse = [{ top: 0 }, { top: 2000 }];
    expect(activeIndex(sparse, 2100)).toBe(1);
  });
});

describe('scrollToAnchor：点击跳转的目标 scrollY', () => {
  it('跳转目标 = 锚点坐标 - 行偏移', () => {
    // 让标题落在视口顶部下方 lineOffset 处
    expect(scrollToAnchor(500, 24)).toBe(476);
  });

  it('顶部标题不会滚成负数', () => {
    expect(scrollToAnchor(10, 24)).toBe(0);
  });
});

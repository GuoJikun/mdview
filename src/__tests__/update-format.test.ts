// 更新弹窗的格式化函数：不碰 DOM 与 Tauri，跑 node 环境。
import { describe, expect, it } from 'vitest';
import { formatBytes, progressPercent } from '../update-format';

describe('formatBytes：下载量展示', () => {
  it('小于 1 KB 按字节显示', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1023 B');
  });

  it('KB / MB 保留一位小数', () => {
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(3 * 1024 * 1024)).toBe('3.0 MB');
  });

  it('非法数值归零，不显示 NaN', () => {
    expect(formatBytes(Number.NaN)).toBe('0 B');
    expect(formatBytes(-1)).toBe('0 B');
  });
});

describe('progressPercent：进度条宽度', () => {
  it('没有 Content-Length 时返回 0（不画假进度）', () => {
    expect(progressPercent(100, null)).toBe(0);
    expect(progressPercent(100, 0)).toBe(0);
  });

  it('按已下载字节四舍五入', () => {
    expect(progressPercent(50, 200)).toBe(25);
    expect(progressPercent(1, 3)).toBe(33);
  });

  it('超过总量时封顶 100', () => {
    expect(progressPercent(300, 200)).toBe(100);
  });
});

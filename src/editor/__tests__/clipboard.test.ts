import { afterEach, describe, expect, it, vi } from 'vitest';
import { copyText } from '../clipboard.js';

class FakeClipboardItem {
  constructor(public readonly data: Record<string, Blob>) {}
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('copyText', () => {
  it('带 HTML 时一次写入 text/html 与 text/plain', async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { write, writeText } });
    vi.stubGlobal('ClipboardItem', FakeClipboardItem);

    expect(await copyText('**粗**', '<p><strong>粗</strong></p>')).toBe(true);
    expect(write).toHaveBeenCalledTimes(1);
    const [items] = write.mock.calls[0] as [FakeClipboardItem[]];
    expect(Object.keys(items[0].data).sort()).toEqual(['text/html', 'text/plain']);
    expect(writeText).not.toHaveBeenCalled();
  });

  it('富文本写入被拒时退回纯文本', async () => {
    const write = vi.fn().mockRejectedValue(new Error('denied'));
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { write, writeText } });
    vi.stubGlobal('ClipboardItem', FakeClipboardItem);

    expect(await copyText('甲', '<p>甲</p>')).toBe(true);
    expect(writeText).toHaveBeenCalledWith('甲');
  });

  it('没有 HTML 时只写纯文本', async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { write, writeText } });
    vi.stubGlobal('ClipboardItem', FakeClipboardItem);

    expect(await copyText('甲')).toBe(true);
    expect(write).not.toHaveBeenCalled();
    expect(writeText).toHaveBeenCalledWith('甲');
  });

  it('空文本直接失败，不碰剪贴板', async () => {
    const writeText = vi.fn();
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    expect(await copyText('')).toBe(false);
    expect(writeText).not.toHaveBeenCalled();
  });
});

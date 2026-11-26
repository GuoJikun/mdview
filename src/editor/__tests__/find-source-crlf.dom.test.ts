// 回归：CRLF 文档在源码视图里查找，第一行之后也必须能搜到。
//
// 根因：`textarea.value` 被 HTML 规范把 `\r\n` 归一成 `\n`，而内核下发的
// `data-byte` 是 CRLF 偏移，每个换行差 1 字节。旧实现拿 LF 文本算 UTF-8 字节
// 再去换停靠点，`stopRangeFor` 要求精确命中，于是首行之后全部被丢掉，
// 界面上就是「切到源码视图就搜不到」。
//
// 源码视图本来就不需要停靠点（高亮靠 textarea 原生选区），跳过换算即可。
import { expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  // 内核真值：CRLF 原样保留
  source: '# 标题\r\n\r\n正文第一行\r\n正文第二行\r\n',
  snapshot: {
    revision: 1,
    path: null,
    dirty: false,
    stops: 2,
    blocks: [
      {
        id: 0,
        base: 0,
        end: 2,
        html:
          '<section class="blk" data-bb="0" data-be="2">'
          + '<span data-b="0" data-a="1">标题</span>'
          + '<span data-b="1" data-a="2">正文</span>'
          + '</section>',
      },
    ],
    caret: { stop: 0, char_offset: 0, block: 0, visual: 0 },
    selection: null,
    outline: [{ level: 1, title: '标题', block: 0 }],
    source: '',
  },
}));

h.snapshot.source = h.source;

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args?: { message?: unknown }) => {
    if (cmd === 'dispatch' && args?.message) {
      const msg = args.message as Record<string, unknown>;
      if (msg.kind === 'setMode') {
        return { revision: 1, dirty: false, source: h.source } as never;
      }
    }
    return cmd === 'snapshot' ? h.snapshot : {};
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

it('源码视图里 CRLF 文档的首行之后也能搜到', async () => {
  document.body.innerHTML = `
    <div id="vscroll"><div id="vscroll-thumb"></div></div>
    <div id="hscroll"><div id="hscroll-thumb"></div></div>
    <nav id="outline"></nav>
    <div id="outline-scroll"><div id="outline-scroll-thumb"></div></div>
    <div id="doc" tabindex="-1" spellcheck="false"></div>
    <textarea id="source" spellcheck="false"></textarea>
    <div id="caret"></div>
    <div id="preedit"></div>
    <div id="findbar" hidden>
      <div class="find-row"><input id="find-input"><span id="find-count">0/0</span></div>
      <div id="find-replace-row" hidden><input id="replace-input"></div>
    </div>
    <div id="find-menu" hidden></div>
    <div id="linkedit" hidden>
      <input id="link-url"><input id="link-text">
    </div>
    <textarea id="ime" rows="1"></textarea>
    <div id="bar"></div>
    <div id="vue-app"></div>
  `;

  await import('../editor.ts');
  await vi.waitFor(() => {
    expect(document.getElementById('doc')!.children.length).toBe(1);
  });

  const run = window.__mdviewRunAction!;
  const src = document.getElementById('source') as HTMLTextAreaElement;
  const input = document.getElementById('find-input') as HTMLInputElement;
  const count = document.getElementById('find-count') as HTMLElement;

  run('mode-source');
  expect(window.__mdviewMode).toBe('source');
  // 浏览器已经把 CRLF 归一成 LF——这正是偏移对不上的来源
  await vi.waitFor(() => {
    expect(src.value).toContain('正文第二行');
  });
  expect(src.value).not.toContain('\r');

  run('find');
  input.value = '正文第二行';
  input.dispatchEvent(new Event('input', { bubbles: true }));

  // 旧实现按 LF 算出的字节在 CRLF 的停靠点表里找不到，计数会是「无结果」
  await vi.waitFor(() => {
    expect(count.textContent).toBe('1/1');
  });
});

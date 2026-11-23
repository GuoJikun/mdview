// 回归：CRLF 文档在源码视图与所见即所得之间来回切换，不该被误判成编辑。
//
// 根因：textarea.value 会把 \r\n 规范化成 \n（HTML 规范），而内核 read_to_string
// 原样保留文件的 \r\n。sourceText（内核真值，CRLF）与 textarea.value（规范化后 LF）
// 直接 === 比较恒不相等，切视图时的 flushSource 就发出 replaceSource，静默把全文
// 换行改写并置脏。比较必须等价归一。
//
// 必须走 dom（chromium）：换行规范化是真实浏览器的行为，happy-dom 不复现。
import { expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
  const CRLF = '# CRLF 标题\r\n\r\n正文第一行\r\n正文第二行\r\n';
  const snapshot = {
    revision: 1,
    path: null,
    dirty: false,
    stops: 2,
    blocks: [
      {
        id: 0,
        base: 0,
        end: 2,
        html: '<section class="blk" data-bb="0" data-be="2">'
          + '<span data-b="0" data-a="1">你好</span>'
          + '<span data-b="1" data-a="2">世界</span>'
          + '</section>',
      },
    ],
    caret: { stop: 0, char_offset: 0, block: 0, visual: 0 },
    selection: null,
    outline: [{ level: 1, title: '你好世界', block: 0 }],
    source: CRLF,
  };
  return {
    snapshot,
    CRLF,
    dispatchCalls: [] as Record<string, unknown>[],
  };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args?: { message?: unknown }) => {
    if (cmd === 'dispatch' && args?.message) {
      // 回包带内核源文本真值（CRLF），模拟真实内核
      const msg = args.message as Record<string, unknown>;
      h.dispatchCalls.push(msg);
      if (msg.kind === 'setMode') {
        return {
          revision: 1,
          dirty: false,
          source: h.snapshot.source,
        } as never;
      }
    }
    return cmd === 'snapshot' ? h.snapshot : {};
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

it('CRLF 文档只在视图间切换，不发出 replaceSource、不误置脏', async () => {
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
    <textarea id="ime" rows="1"></textarea>
    <div id="bar"></div>
    <div id="vue-app"></div>
  `;

  await import('../editor.ts');
  await vi.waitFor(() => {
    expect(document.getElementById('doc')!.children.length).toBe(1);
  });

  const src = document.getElementById('source') as HTMLTextAreaElement;
  // boot 时 sourceText 是 CRLF 真值，灌进 textarea 后被浏览器规范化成 LF
  expect(h.snapshot.source).toContain('\r\n');
  expect(src.value).not.toContain('\r');
  expect(src.value).toContain('# CRLF 标题');

  const run = window.__mdviewRunAction!;

  // 源码 ↔ wysiwyg ↔ 阅读 来回切，全程没有用户编辑
  h.dispatchCalls.length = 0;
  run('mode-source');
  run('mode-wysiwyg');
  run('mode-source');
  run('mode-reading');
  run('mode-wysiwyg');
  await vi.waitFor(() => {
    // 等所有 setMode 回包落地（它们都会带 CRLF source）
    expect(h.dispatchCalls.filter((m) => m.kind === 'setMode').length).toBe(5);
  });

  // 关键断言：一次 replaceSource 都不该有——否则文档被改写、标题栏误标脏
  expect(h.dispatchCalls.filter((m) => m.kind === 'replaceSource')).toEqual([]);
});

it('CRLF 文档真被编辑时，仍会写回内核', async () => {
  const src = document.getElementById('source') as HTMLTextAreaElement;
  const run = window.__mdviewRunAction!;
  run('mode-source');
  h.dispatchCalls.length = 0;

  src.value = '#  Edited\r\n正文';
  src.dispatchEvent(new Event('input', { bubbles: true }));
  await vi.waitFor(() => {
    expect(h.dispatchCalls.some((m) => m.kind === 'replaceSource')).toBe(true);
  });
  const replaced = h.dispatchCalls.find((m) => m.kind === 'replaceSource')!;
  // textarea 规范化后写回的是 LF，这是正确行为
  expect((replaced as { text: string }).text).toBe('#  Edited\n正文');
});

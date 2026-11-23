// 内核启动测试：把真正的 editor.ts 跑起来。
//
// 铺好 index.html 的骨架、mock 掉 Tauri IPC、import 模块、等 boot() 跑完，
// 然后检查投影出来的 DOM。这一层之所以必须用浏览器：光标定位的判据是
// `getBoundingClientRect`，而 happy-dom 里它恒为 0，断言将毫无意义。
import { expect, it, vi } from 'vitest';

// vi.mock 的工厂会被提升到 import 之前，不能引用模块作用域变量，必须走 hoisted。
const h = vi.hoisted(() => {
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
    source: '# 你好世界\n\n**粗体** 正文',
  };
  return {
    snapshot,
    calls: [] as string[],
    dispatchCalls: [] as Record<string, unknown>[],
    openUrls: [] as string[],
  };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args?: { message?: unknown; url?: string }) => {
    h.calls.push(cmd);
    if (cmd === 'dispatch' && args?.message) {
      h.dispatchCalls.push(args.message as Record<string, unknown>);
    }
    if (cmd === 'open_url' && args?.url) h.openUrls.push(args.url);
    return cmd === 'snapshot' ? h.snapshot : {};
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

it('editor.ts 能在浏览器模式里启动并投影出 DOM', async () => {
  // index.html 的骨架：内核启动时直接 getElementById，缺一个就崩在顶层。
  document.body.innerHTML = `
    <div id="vscroll"><div id="vscroll-thumb"></div></div>
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

  // boot() 不 await，import 返回时它还在跑；等投影落地。
  await import('../editor.ts');
  await vi.waitFor(() => {
    expect(document.getElementById('doc')!.children.length).toBe(1);
  });

  const doc = document.getElementById('doc')!;
  const blk = doc.firstElementChild as HTMLElement;

  // 块结构与停靠点属性都在
  expect(blk.className).toBe('blk');
  expect(blk.dataset.bb).toBe('0');
  expect(blk.dataset.be).toBe('2');
  expect(blk.querySelectorAll('[data-b]').length).toBe(2);

  // 大纲侧栏渲染出了条目
  await vi.waitFor(() => {
    expect(document.querySelectorAll('#outline a[data-idx]').length).toBe(1);
  });
  expect(document.querySelector('#outline a')!.textContent).toContain('你好世界');

  // 光标被定位到了真实像素上（这需要真实布局，happy-dom 给不了）
  const caret = document.getElementById('caret')!;
  await vi.waitFor(() => {
    expect(caret.style.left).toMatch(/px$/);
  });
  expect(parseFloat(caret.style.top)).toBeGreaterThanOrEqual(0);

  // 内核启动只调 snapshot、ready（前端就绪信号）与 recent_files（最近打开菜单），
  // 不该有别的 IPC 意外发生。
  expect(h.calls.filter((c) => !['snapshot', 'ready', 'recent_files'].includes(c))).toEqual([]);
  expect(h.calls).toContain('ready');

  // 三种视图真正切换 DOM：不再是"点了只换圆点、正文永远所见即所得"
  const run = window.__mdviewRunAction!;
  const html = document.documentElement;
  expect(html.dataset.view ?? 'wysiwyg').toBe('wysiwyg');

  // 源码视图：data-view 变化 + textarea 拿到原始 Markdown 真值，不是渲染后的 HTML
  run('mode-source');
  expect(html.dataset.view).toBe('source');
  expect(window.__mdviewMode).toBe('source');
  const src = document.getElementById('source') as HTMLTextAreaElement;
  expect(src.value).toContain('# 你好世界');
  expect(src.value).not.toContain('<h1');
  expect(src.value).toContain('**粗体**');

  // 在源码视图编辑 textarea：去抖后整篇 Markdown 写回内核（不是富文本 HTML）
  h.calls.length = 0;
  src.value = '# 改过的标题\n\n- 列表项';
  src.dispatchEvent(new Event('input', { bubbles: true }));
  // input 是 120ms 去抖，等 replaceSource 真正写回内核
  await vi.waitFor(() => {
    expect(h.dispatchCalls.some((m) => m.kind === 'replaceSource')).toBe(true);
  });
  const replaced = h.dispatchCalls.find((m) => m.kind === 'replaceSource');
  expect(replaced).toBeDefined();
  expect((replaced as { text: string }).text).toBe('# 改过的标题\n\n- 列表项');

  // 阅读视图：只读呈现，data-view 跟上
  run('mode-reading');
  expect(html.dataset.view).toBe('reading');
  expect(window.__mdviewMode).toBe('reading');

  // 切回所见即所得
  run('mode-wysiwyg');
  expect(html.dataset.view).toBe('wysiwyg');
  expect(window.__mdviewMode).toBe('wysiwyg');

  // 外链：投影里的链接字符只带 data-href（没有 <a href>，否则 webview 会就地导航）
  const link = document.createElement('span');
  link.className = 'ch a';
  link.dataset.href = 'https://example.com/a?b=1';
  link.textContent = '链';
  doc.appendChild(link);

  // 编辑态单击属于光标定位，不能顺手把浏览器拉起来
  h.calls.length = 0;
  link.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
  expect(h.openUrls).toEqual([]);

  // Ctrl+Click 才是「跟随链接」
  link.dispatchEvent(
    new MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: true }),
  );
  expect(h.openUrls).toEqual(['https://example.com/a?b=1']);

  // 阅读态没有「编辑光标」这回事，单击直接打开
  run('mode-reading');
  h.openUrls.length = 0;
  link.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
  expect(h.openUrls).toEqual(['https://example.com/a?b=1']);
  run('mode-wysiwyg');
});

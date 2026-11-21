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
  };
  return { snapshot, calls: [] as string[] };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    h.calls.push(cmd);
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
    <div id="caret"></div>
    <div id="preedit"></div>
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

  // 内核启动只调 snapshot 和 ready（后者是「前端就绪」信号），
  // 不该有别的 IPC 意外发生。
  expect(h.calls.filter((c) => c !== 'snapshot' && c !== 'ready')).toEqual([]);
  expect(h.calls).toContain('ready');
});

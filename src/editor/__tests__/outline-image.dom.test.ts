// 回归：源码里有图片时，点大纲跳到图片之后的标题不能偏。
//
// 图片是异步加载的：大纲锚点在 boot 时量过一次，图片撑开后所有标题都会下移。
// 这条用例故意让图片先以 0 高度存在、再撑到 400px，然后点第二个标题——
// 落点必须仍在阅读基准线（80px）附近，而不是差出一张图的高度。
import { expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
  const image = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';
  const heading = (id: number, level: number, text: string) => ({
    id,
    base: 0,
    end: 2,
    html: `<section class="blk kind-heading" data-bb="0" data-be="2"><h${level}>`
      + `<span data-b="0" data-a="1">${text}</span>`
      + `</h${level}></section>`,
  });
  const paragraph = (id: number) => ({
    id,
    base: 0,
    end: 2,
    html: `<section class="blk" data-bb="0" data-be="2">`
      + `<span data-b="0" data-a="1">正文${id}</span></section>`,
  });
  const blocks = [
    heading(0, 1, '一'),
    {
      id: 1,
      base: 1,
      end: 2,
      html: '<section class="blk" data-bb="1" data-be="2">'
        + `<span class="atom img" data-b="1" data-a="2" data-byte="0" data-block="1" data-asset="${image}" title="图">`
        + '<img alt="" style="height:0;display:block" src="' + image + '"></span></section>',
    },
    heading(2, 2, '二'),
    ...Array.from({ length: 60 }, (_, i) => paragraph(i + 3)),
  ];
  return {
    snapshot: {
      revision: 1,
      path: null,
      dirty: false,
      stops: 60,
      blocks,
      caret: { stop: 0, char_offset: 0, block: 0, visual: 0 },
      selection: null,
      outline: [
        { level: 1, title: '一', block: 0 },
        { level: 2, title: '二', block: 2 },
      ],
      source: '# 一\n\n![图](x.gif)\n\n## 二\n\n正文',
    },
  };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => (cmd === 'snapshot' ? h.snapshot : [])),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

it('图片撑开高度后，点大纲仍落在阅读基准线', async () => {
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
    <div id="linkedit" hidden><input id="link-url"><input id="link-text"></div>
    <textarea id="ime" rows="1"></textarea>
    <div id="bar"></div>
    <div id="vue-app"></div>
  `;

  await import('../editor.ts');
  const doc = document.getElementById('doc')!;
  await vi.waitFor(() => {
    expect(document.querySelectorAll('#outline a[data-idx]').length).toBe(2);
  });

  const img = doc.querySelector<HTMLImageElement>('.atom.img img')!;
  expect(img).toBeTruthy();
  // 模拟「图片加载完成、撑开 400px」：锚点必须跟着重测
  img.style.height = '400px';
  img.dispatchEvent(new Event('load'));
  await new Promise((resolve) => setTimeout(resolve, 50));

  window.scrollTo(0, 0);
  const second = doc.children[2] as HTMLElement;
  (document.querySelectorAll<HTMLElement>('#outline a[data-idx]')[1]).click();

  // 平滑滚动是异步的：等标题落到基准线附近（editor.ts 的 OUTLINE_LINE = 80）
  await vi.waitFor(
    () => {
      expect(Math.abs(second.getBoundingClientRect().top - 80)).toBeLessThan(3);
    },
    { timeout: 2000 },
  );
});

import { describe, expect, it } from 'vitest';
import { htmlToMarkdown, looksLikeRichHtml } from '../html-to-md.js';

const md = (html: string) => htmlToMarkdown(html);

describe('块级结构', () => {
  it('标题按级别输出', () => {
    expect(md('<h1>大标题</h1><h3>小标题</h3>')).toBe('# 大标题\n\n### 小标题');
  });

  it('段落之间空一行，行内空白折叠', () => {
    expect(md('<p>第一段   有空格</p>\n<p>第二段</p>')).toBe(
      '第一段 有空格\n\n第二段',
    );
  });

  it('br 输出反斜杠硬换行', () => {
    expect(md('<p>上<br>下</p>')).toBe('上\\\n下');
  });

  it('引用逐行加 > 前缀', () => {
    expect(md('<blockquote><p>甲</p><p>乙</p></blockquote>')).toBe('> 甲\n>\n> 乙');
  });

  it('分隔线', () => {
    expect(md('<hr>')).toBe('---');
  });

  it('脚本与样式整段丢弃，普通标签保留文字', () => {
    expect(md('<p>前<script>alert(1)</script><style>p{}</style>后</p>')).toBe('前后');
    expect(md('<p><span>甲</span><font>乙</font></p>')).toBe('甲乙');
  });
});

describe('行内格式', () => {
  it('粗体、斜体、删除线、行内代码', () => {
    expect(md('<p><strong>粗</strong><em>斜</em><del>删</del><code>a`b</code></p>')).toBe(
      '**粗***斜*~~删~~``a`b``',
    );
  });

  it('链接只保留安全协议', () => {
    expect(md('<p><a href="https://example.com/a_(b)">文</a><a href="javascript:alert(1)">坏</a></p>')).toBe(
      '[文](<https://example.com/a_(b)>)坏',
    );
  });

  it('图片输出 alt 与地址，data 只放行图片', () => {
    expect(md('<p><img src="img/a.png" alt="图"></p>')).toBe('![图](img/a.png)');
    expect(md('<p><img src="data:image/png;base64,AAA" alt="内联"></p>')).toBe(
      '![内联](data:image/png;base64,AAA)',
    );
    expect(md('<p><img src="javascript:alert(1)" alt="坏"></p>')).toBe('');
  });

  it('字面量里的 Markdown 元字符会被转义', () => {
    expect(md('<p>2*3 和 [x]</p>')).toBe('2\\*3 和 \\[x\\]');
  });
});

describe('列表', () => {
  it('无序与有序列表', () => {
    expect(md('<ul><li>甲</li><li>乙</li></ul>')).toBe('- 甲\n- 乙');
    expect(md('<ol start="3"><li>丙</li><li>丁</li></ol>')).toBe('3. 丙\n4. 丁');
  });

  it('嵌套列表缩进两格', () => {
    expect(md('<ul><li>甲<ul><li>乙</li></ul></li><li>丙</li></ul>')).toBe(
      '- 甲\n  - 乙\n- 丙',
    );
  });

  it('列表项里的段落与嵌套列表分层', () => {
    const html = '<ul><li><p>甲段一</p><p>甲段二</p><ul><li>乙</li></ul></li></ul>';
    expect(md(html)).toBe('- 甲段一\n\n  甲段二\n  - 乙');
  });
});

describe('代码块', () => {
  it('保留缩进与语言标记', () => {
    const html = '<pre><code class="language-js">if (a) {\n  b();\n}</code></pre>';
    expect(md(html)).toBe('```js\nif (a) {\n  b();\n}\n```');
  });

  it('内容含反引号时围栏加长', () => {
    expect(md('<pre><code>a```b</code></pre>')).toBe('````\na```b\n````');
  });
});

describe('表格', () => {
  it('th 表头与对齐', () => {
    const html =
      '<table><tr><th>左</th><th align="center">中</th><th style="text-align:right">右</th></tr>' +
      '<tr><td>1</td><td>2</td><td>3</td></tr></table>';
    expect(md(html)).toBe(
      '| 左 | 中 | 右 |\n| --- | :---: | ---: |\n| 1 | 2 | 3 |',
    );
  });

  it('没有 th 时补一行空表头，不丢数据', () => {
    const html = '<table><tr><td>甲</td><td>乙</td></tr></table>';
    expect(md(html)).toBe('|  |  |\n| --- | --- |\n| 甲 | 乙 |');
  });

  it('单元格里的竖线会被转义', () => {
    const html = '<table><tr><th>a|b</th></tr><tr><td>c|d</td></tr></table>';
    expect(md(html)).toBe('| a\\|b |\n| --- |\n| c\\|d |');
  });
});

describe('富文本判定', () => {
  it('结构化标签才走转换', () => {
    expect(looksLikeRichHtml('<p>正文</p>', '正文')).toBe(true);
    expect(looksLikeRichHtml('<div>第一行</div><div>第二行</div>', '第一行\n第二行')).toBe(
      false,
    );
    expect(looksLikeRichHtml('<div>纯文本</div>', '纯文本')).toBe(false);
  });

  it('没有纯文本兜底时无条件转换', () => {
    expect(looksLikeRichHtml('<div>只有 HTML</div>', '')).toBe(true);
  });
});

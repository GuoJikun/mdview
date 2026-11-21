// 输入法状态机：纯状态逻辑，不碰 DOM，跑 node 环境。
import { describe, expect, it } from 'vitest';
import { createIme } from './ime.js';

/**
 * 最小宿主：记录发出的消息与 preedit 文本。
 * `clock` 可注入，回车抑制窗口那两条要靠它推进时间。
 */
function harness(clock?: () => number) {
  const sent: object[] = [];
  const state = { preedit: '' };
  const ime = createIme({
    send: (message) => {
      sent.push(message);
    },
    setPreedit: (text) => {
      state.preedit = text || '';
    },
  }, clock);
  return { ime, sent, state };
}

describe('组合生命周期', () => {
  it('组合期间只发一条 start，不往文档里塞任何东西', () => {
    const { ime, sent } = harness();
    ime.start();
    ime.update('ni');
    ime.update('nihao');
    expect(sent).toStrictEqual([{ kind: 'compositionStart' }]);
  });

  it('preedit 跟随 update，结束后清空', () => {
    const { ime, state } = harness();
    ime.start();
    ime.update('ni');
    expect(state.preedit).toBe('ni');
    ime.update('你好');
    expect(state.preedit).toBe('你好');
    ime.end('你好', '你好');
    expect(state.preedit).toBe('');
  });

  it('组合结束一次性提交整段文本', () => {
    const { ime, sent } = harness();
    ime.start();
    ime.update('nihao');
    ime.end('你好', '你好');
    expect(sent).toStrictEqual([
      { kind: 'compositionStart' },
      { kind: 'compositionCommit', text: '你好' },
    ]);
    expect(ime.isComposing()).toBe(false);
  });

  it('data 缺失时退回输入框 value', () => {
    const { ime, sent } = harness();
    ime.start();
    ime.end('', '你好');
    expect(sent[1]).toStrictEqual({ kind: 'compositionCommit', text: '你好' });
  });

  it('取消候选不产生事务，但必须通知内核复位挂起态', () => {
    const { ime, sent } = harness();
    ime.start();
    ime.end('', '');
    expect(sent).toStrictEqual([
      { kind: 'compositionStart' },
      // 少这一条，内核会永远停在 composing，之后所有输入被吞
      { kind: 'compositionCancel' },
    ]);
    expect(ime.isComposing()).toBe(false);
  });

  it('取消之后输入恢复正常', () => {
    const { ime, sent } = harness();
    ime.start();
    ime.end('', '');
    expect(ime.input('甲')).toBe(true);
    expect(sent[2]).toStrictEqual({ kind: 'insertText', text: '甲' });
  });
});

describe('input 事件', () => {
  it('组合期的 input 事件被吞掉，避免和提交重复插入', () => {
    const { ime, sent } = harness();
    ime.start();
    expect(ime.input('你好')).toBe(false);
    expect(sent).toStrictEqual([{ kind: 'compositionStart' }]);
  });

  it('非组合期的 input 直出为 insertText', () => {
    const { ime, sent } = harness();
    expect(ime.input('😀')).toBe(true);
    expect(sent).toStrictEqual([{ kind: 'insertText', text: '😀' }]);
  });

  it('空 input 不产生消息', () => {
    const { ime, sent } = harness();
    expect(ime.input('')).toBe(false);
    expect(sent).toStrictEqual([]);
  });
});

describe('按键放行判据', () => {
  it('组合期与非组合期的按键都要放行', () => {
    const { ime } = harness();
    expect(ime.shouldIgnoreKey({ isComposing: true, keyCode: 65 })).toBe(true);
    expect(ime.shouldIgnoreKey({ isComposing: false, keyCode: 229 })).toBe(true);
    expect(ime.shouldIgnoreKey({ isComposing: false, keyCode: 65 })).toBe(false);
    ime.start();
    // 组合中即便浏览器漏报 isComposing，也不能让内核抢键
    expect(ime.shouldIgnoreKey({ isComposing: false, keyCode: 65 })).toBe(true);
  });

  it('结束后的按键重新交给内核', () => {
    const { ime } = harness();
    ime.start();
    ime.end('甲', '甲');
    expect(ime.shouldIgnoreKey({ isComposing: false, keyCode: 65 })).toBe(false);
  });

  it('确认候选补发的回车被吞掉，稍后的回车照常分段', () => {
    let t = 1000;
    const { ime } = harness(() => t);
    ime.start();
    ime.end('你好', '你好');
    t += 20;
    expect(
      ime.shouldIgnoreKey({ key: 'Enter', isComposing: false, keyCode: 13 }),
      'compositionend 之后紧跟的回车会多分一段，必须拦',
    ).toBe(true);
    t += 300;
    expect(
      ime.shouldIgnoreKey({ key: 'Enter', isComposing: false, keyCode: 13 }),
      '真正想分段时不能一并吞掉',
    ).toBe(false);
  });

  it('回车抑制窗口不影响其它按键', () => {
    let t = 1000;
    const { ime } = harness(() => t);
    ime.start();
    ime.end('甲', '甲');
    t += 20;
    expect(ime.shouldIgnoreKey({ key: 'a', isComposing: false, keyCode: 65 })).toBe(false);
    expect(ime.shouldIgnoreKey({ key: 'Backspace', isComposing: false, keyCode: 8 })).toBe(false);
  });
});

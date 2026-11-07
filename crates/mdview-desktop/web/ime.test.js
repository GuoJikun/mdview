// 输入法状态机的无头测试。运行：node ime.test.js
// 由 crates/mdview-desktop/tests/web_ime.rs 调起，保证不会随代码漂移。
'use strict';

const assert = require('assert');
const { createIme } = require('./ime.js');

function harness(clock) {
  const sent = [];
  const state = { preedit: '' };
  const ime = createIme({
    send: (message) => sent.push(message),
    setPreedit: (text) => {
      state.preedit = text || '';
    },
  }, clock);
  return { ime, sent, state };
}

const tests = [];
function test(name, fn) {
  tests.push([name, fn]);
}

test('组合期间只发一条 start，不往文档里塞任何东西', () => {
  const { ime, sent } = harness();
  ime.start();
  ime.update('ni');
  ime.update('nihao');
  assert.deepStrictEqual(sent, [{ kind: 'compositionStart' }]);
});

test('preedit 跟随 update，结束后清空', () => {
  const { ime, state } = harness();
  ime.start();
  ime.update('ni');
  assert.strictEqual(state.preedit, 'ni');
  ime.update('你好');
  assert.strictEqual(state.preedit, '你好');
  ime.end('你好', '你好');
  assert.strictEqual(state.preedit, '');
});

test('组合结束一次性提交整段文本', () => {
  const { ime, sent } = harness();
  ime.start();
  ime.update('nihao');
  ime.end('你好', '你好');
  assert.deepStrictEqual(sent, [
    { kind: 'compositionStart' },
    { kind: 'compositionCommit', text: '你好' },
  ]);
  assert.strictEqual(ime.isComposing(), false);
});

test('组合期的 input 事件被吞掉，避免和提交重复插入', () => {
  const { ime, sent } = harness();
  ime.start();
  const consumed = ime.input('你好');
  assert.strictEqual(consumed, false);
  assert.deepStrictEqual(sent, [{ kind: 'compositionStart' }]);
});

test('非组合期的 input 直出为 insertText', () => {
  const { ime, sent } = harness();
  assert.strictEqual(ime.input('😀'), true);
  assert.deepStrictEqual(sent, [{ kind: 'insertText', text: '😀' }]);
});

test('空 input 不产生消息', () => {
  const { ime, sent } = harness();
  assert.strictEqual(ime.input(''), false);
  assert.deepStrictEqual(sent, []);
});

test('data 缺失时退回输入框 value', () => {
  const { ime, sent } = harness();
  ime.start();
  ime.end('', '你好');
  assert.deepStrictEqual(sent[1], { kind: 'compositionCommit', text: '你好' });
});

test('取消候选（空提交）不产生事务', () => {
  const { ime, sent } = harness();
  ime.start();
  ime.end('', '');
  assert.deepStrictEqual(sent, [{ kind: 'compositionStart' }]);
  assert.strictEqual(ime.isComposing(), false);
});

test('组合期与非组合期的按键都要放行', () => {
  const { ime } = harness();
  assert.strictEqual(ime.shouldIgnoreKey({ isComposing: true, keyCode: 65 }), true);
  assert.strictEqual(ime.shouldIgnoreKey({ isComposing: false, keyCode: 229 }), true);
  assert.strictEqual(ime.shouldIgnoreKey({ isComposing: false, keyCode: 65 }), false);
  ime.start();
  // 组合中即便浏览器漏报 isComposing，也不能让内核抢键
  assert.strictEqual(ime.shouldIgnoreKey({ isComposing: false, keyCode: 65 }), true);
});

test('结束后的按键重新交给内核', () => {
  const { ime } = harness();
  ime.start();
  ime.end('甲', '甲');
  assert.strictEqual(ime.shouldIgnoreKey({ isComposing: false, keyCode: 65 }), false);
});

test('确认候选补发的回车被吞掉，稍后的回车照常分段', () => {
  let t = 1000;
  const { ime } = harness(() => t);
  ime.start();
  ime.end('你好', '你好');
  t += 20;
  assert.strictEqual(
    ime.shouldIgnoreKey({ key: 'Enter', isComposing: false, keyCode: 13 }),
    true,
    'compositionend 之后紧跟的回车会多分一段，必须拦',
  );
  t += 300;
  assert.strictEqual(
    ime.shouldIgnoreKey({ key: 'Enter', isComposing: false, keyCode: 13 }),
    false,
    '真正想分段时不能一并吞掉',
  );
});

test('回车抑制窗口不影响其它按键', () => {
  let t = 1000;
  const { ime } = harness(() => t);
  ime.start();
  ime.end('甲', '甲');
  t += 20;
  assert.strictEqual(ime.shouldIgnoreKey({ key: 'a', isComposing: false, keyCode: 65 }), false);
  assert.strictEqual(ime.shouldIgnoreKey({ key: 'Backspace', isComposing: false, keyCode: 8 }), false);
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log('ok   ' + name);
  } catch (err) {
    failed += 1;
    console.error('FAIL ' + name);
    console.error('     ' + (err && err.message));
  }
}

if (failed > 0) {
  console.error('\n' + failed + '/' + tests.length + ' 项失败');
  process.exit(1);
}
console.log('\n' + tests.length + ' 项全过');

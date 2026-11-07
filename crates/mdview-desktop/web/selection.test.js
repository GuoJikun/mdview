// 原生选区清理判据的无头测试。运行：node selection.test.js
// 由 crates/mdview-desktop/tests/web_ime.rs 调起，避免逻辑随代码漂移。
'use strict';

const assert = require('assert');
const { shouldClearSelection } = require('./selection.js');

const tests = [];
function test(name, fn) {
  tests.push([name, fn]);
}

/** 假的透明输入框：只有 contains 参与判断。 */
function host(children) {
  return { contains: (node) => children.indexOf(node) >= 0 };
}

function selection(fields) {
  return Object.assign({ rangeCount: 1, isCollapsed: false, anchorNode: {} }, fields);
}

test('没有选区时不动手', () => {
  assert.strictEqual(shouldClearSelection(null, host([])), false);
  assert.strictEqual(shouldClearSelection(selection({ rangeCount: 0 }), host([])), false);
  // 只是个光标停车点（点击后锚点塌陷），清它没有意义
  assert.strictEqual(shouldClearSelection(selection({ isCollapsed: true }), host([])), false);
});

test('页面上的原生选区要清掉', () => {
  // Ctrl+A、焦点在 body 上拖动、程序化 addRange 都走这条
  assert.strictEqual(shouldClearSelection(selection({}), host([])), true);
});

test('输入框内部的选区放行', () => {
  const inside = {};
  assert.strictEqual(shouldClearSelection(selection({ anchorNode: inside }), host([inside])), false);
});

test('锚点取不到也要清（宁可多清一次）', () => {
  assert.strictEqual(shouldClearSelection(selection({ anchorNode: null }), host([])), true);
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

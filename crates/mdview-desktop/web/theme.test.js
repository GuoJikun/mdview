// 主题解析逻辑的 Node 单测。
'use strict';

const { resolve, toggleNext } = require('./theme.js');

let passed = 0;
let failed = 0;
function check(name, cond, detail) {
  if (cond) passed++;
  else { failed++; console.error('FAIL ' + name + (detail ? ' — ' + detail : '')); }
}

// 无偏好时跟随系统
check('无偏好+系统深色 → 深色', resolve(null, 'dark') === 'dark',
  'got ' + resolve(null, 'dark'));
check('无偏好+系统浅色 → 浅色', resolve(null, 'light') === 'light',
  'got ' + resolve(null, 'light'));
// 手动偏好覆盖系统
check('手动浅色覆盖系统深色', resolve('light', 'dark') === 'light',
  'got ' + resolve('light', 'dark'));
check('手动深色覆盖系统浅色', resolve('dark', 'light') === 'dark',
  'got ' + resolve('dark', 'light'));
// 非法值回落到系统
check('非法存储值回落系统深色', resolve('purple', 'dark') === 'dark',
  'got ' + resolve('purple', 'dark'));
// 切换取反
check('深色切到浅色', toggleNext('dark') === 'light', 'got ' + toggleNext('dark'));
check('浅色切到深色', toggleNext('light') === 'dark', 'got ' + toggleNext('light'));

console.log('theme: ' + passed + ' 项全过 / ' + (passed + failed));
if (failed > 0) process.exit(1);

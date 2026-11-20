// 大纲联动纯函数的 Node 单测。cargo test 的 web_ime.rs 守门会跑它。
'use strict';

const { activeIndex, scrollToAnchor } = require('./outline.js');

let passed = 0;
let failed = 0;
function check(name, cond, detail) {
  if (cond) {
    passed++;
  } else {
    failed++;
    console.error('FAIL ' + name + (detail ? ' — ' + detail : ''));
  }
}

const anchors = [{ top: 0 }, { top: 100 }, { top: 350 }, { top: 900 }];

// 基准线还在第一个标题之前：没有高亮项
check('基准线在首标题之前返回 -1', activeIndex(anchors, -10) === -1,
  'got ' + activeIndex(anchors, -10));
// 正好压在标题上算越过
check('基准线等于锚点时命中该项', activeIndex(anchors, 0) === 0,
  'got ' + activeIndex(anchors, 0));
check('两锚点之间取前一个', activeIndex(anchors, 200) === 1,
  'got ' + activeIndex(anchors, 200));
check('越过第三个锚点', activeIndex(anchors, 500) === 2,
  'got ' + activeIndex(anchors, 500));
check('越过全部取最后一项', activeIndex(anchors, 5000) === 3,
  'got ' + activeIndex(anchors, 5000));
// 空大纲
check('空锚点返回 -1', activeIndex([], 100) === -1,
  'got ' + activeIndex([], 100));
// 疏密不均：标题间隔很大时依然按实际坐标判定，而不是按序号均分
const sparse = [{ top: 0 }, { top: 2000 }];
check('稀疏锚点按坐标而非序号', activeIndex(sparse, 1500) === 0,
  'got ' + activeIndex(sparse, 1500));
check('稀疏锚点越过后才切', activeIndex(sparse, 2100) === 1,
  'got ' + activeIndex(sparse, 2100));

// 点击跳转：让标题落在视口顶部下方 lineOffset 处
check('跳转目标 = 锚点坐标 - 行偏移', scrollToAnchor(500, 24) === 476,
  'got ' + scrollToAnchor(500, 24));
check('顶部标题不会滚成负数', scrollToAnchor(10, 24) === 0,
  'got ' + scrollToAnchor(10, 24));

console.log('outline: ' + passed + ' 项全过 / ' + (passed + failed));
if (failed > 0) process.exit(1);

// 输入法组合状态机。
//
// 刻意不碰 DOM：所有副作用都通过 hooks 注入，因此可以用 Node 直接跑测试
// （`node ime.test.js`）。app.js 只负责把 DOM 事件喂进来。
//
// 三条规则：
// 1. 组合期间只发一条 compositionStart，其余全部挂起——内核此时不得重绘，
//    否则会把浏览器正在渲染的 preedit 冲掉。
// 2. preedit 只画在镜像层，不进文档。
// 3. 组合结束一次性提交，整段文本在文档里只占一步撤销。
'use strict';

(function (root) {
  /**
   * @param {{
   *   send: (message: object) => void,
   *   setPreedit: (text: string) => void,
   * }} hooks
   * @param {() => number} [now] 时钟，测试时注入；默认用 Date.now
   */
  function createIme(hooks, now) {
    const clock = now || function () { return Date.now(); };
    let composing = false;
    let preedit = '';
    let endedAt = 0;

    function send(message) {
      hooks.send(message);
    }

    return {
      isComposing: function () {
        return composing;
      },
      preedit: function () {
        return preedit;
      },

      /** 组合开始：通知内核挂起，镜像层清空 */
      start: function () {
        composing = true;
        preedit = '';
        hooks.setPreedit('');
        send({ kind: 'compositionStart' });
      },

      /** 组合中：只更新镜像文本，文档与内核都不动 */
      update: function (text) {
        if (!composing) return;
        preedit = text || '';
        hooks.setPreedit(preedit);
      },

      /**
       * 组合结束。
       *
       * `data` 为空时退回输入框的 value：Safari 在部分输入法下单不回报 data，
       * 只看 data 会吞掉整个词。空串（取消候选 / 只按了 Esc）不提交，
       * 否则文档里会留下一个不该存在的空事务和一步撤销。
       */
      end: function (data, fallback) {
        const text = data ? data : fallback || '';
        composing = false;
        preedit = '';
        endedAt = clock();
        hooks.setPreedit('');
        if (text) send({ kind: 'compositionCommit', text: text });
        return text;
      },

      /**
       * 非组合期的 input 事件（emoji 面板、死键、部分输入法直出）。
       * 返回 true 表示这段文本已被消费，调用方应清空输入框。
       */
      input: function (value) {
        if (composing || !value) return false;
        send({ kind: 'insertText', text: value });
        return true;
      },

      /** 组合期与 keyCode 229 的按键一律不拦，让输入法自己走 */
      shouldIgnoreKey: function (event) {
        if (composing || event.isComposing === true || event.keyCode === 229) return true;
        // 确认候选用的那一下回车：浏览器会在 compositionend **之后**再补一个
        // keydown，此时 isComposing 已是 false。不拦就会多分出一段。
        if (event.key === 'Enter' && clock() - endedAt < 120) return true;
        return false;
      },
    };
  }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = { createIme: createIme };
  } else {
    root.createIme = createIme;
  }
})(typeof window !== 'undefined' ? window : this);

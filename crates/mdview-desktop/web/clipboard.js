/* 系统剪贴板的写入端。
 *
 * 文本真值在内核：Ctrl+C / Ctrl+X 先发给内核，内核把选区源码随回复带回来
 * （`reply.clipboard`），这个文件只负责「把这段文字交给系统」。
 * 不在前端按 DOM 拼文本，是因为视图里 `**`、`# ` 这些标记是折叠的——
 * 照 DOM 拼出来的是「看到的」，而复制出去的必须是「能重新解析回同样结构的」。
 */

/** 剪贴板写入成功与否（失败只影响这一次复制，不该打断编辑）。 */
async function copyText(text) {
  if (typeof text !== 'string' || text.length === 0) return false;
  // 首选异步 API：WebView2 里 tauri.localhost 属于安全上下文，正常可用
  try {
    if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch (err) {
    // 权限被拒（或者焦点不在窗口上）时退到 execCommand
  }
  return legacyCopy(text);
}

/**
 * 老路：临时 textarea + execCommand('copy')。
 *
 * 它会短暂抢走焦点，调用方负责把焦点还给输入框——否则输入法会以为自己失焦，
 * 正在组合的词就被丢掉了。
 */
function legacyCopy(text) {
  const area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  // 不能设 display:none 或 visibility:hidden，那样选不中
  area.style.position = 'fixed';
  area.style.top = '-1000px';
  area.style.left = '0';
  area.style.opacity = '0';
  document.body.appendChild(area);
  let ok = false;
  try {
    area.select();
    ok = document.execCommand('copy');
  } catch (err) {
    ok = false;
  }
  area.remove();
  return ok;
}

const clipboardApi = { copyText, legacyCopy };
if (typeof module !== 'undefined' && module.exports) {
  module.exports = clipboardApi;
} else {
  window.copyText = copyText;
}

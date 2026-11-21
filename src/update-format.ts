// 更新弹窗的纯格式化函数。
//
// 单独成文件是为了能在 node 环境下直接单测：`update.ts` 会 import Tauri 的
// 运行时包，而这些函数连 DOM 都不需要。

/** 字节数 → 「1.2 MB」这类可读文本；未知总量（0 / NaN）显示 0 B。 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * 下载百分比。服务端没给 `Content-Length` 时返回 0——此时界面只显示已下载字节，
 * 不画进度条，避免出现永远停在 0% 的假进度。
 */
export function progressPercent(received: number, total: number | null): number {
  if (!total || total <= 0) return 0;
  return Math.min(100, Math.round((received / total) * 100));
}

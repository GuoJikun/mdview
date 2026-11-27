// 模态弹窗的共用行为：锁住页面滚动、把 Tab 关在弹窗里、Esc 归自己、
// 关掉时把焦点还给打开它的地方。
//
// 「关于」与「检查更新」只认底部的关闭按钮——遮罩与 Esc 都不关窗，免得误触把
// 正在下载的更新丢掉；「偏好设置」「键盘快捷键」反过来，Esc 就关。两种策略都在
// 这里用参数表达，行为不会各写一套走偏。
import { onBeforeUnmount, onMounted, type Ref } from 'vue';

/** 能 Tab 到的元素。不含 `tabindex="-1"`（那些只能被脚本聚焦）。 */
const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

/**
 * 容器里可 Tab 到的元素，按 DOM 顺序。
 *
 * 用 `getClientRects().length` 而不是 `offsetParent`：`position: fixed` 的元素
 * `offsetParent` 恒为 null，会被误判成不可见；`v-if` 折叠掉的那些 rect 为 0，
 * 正好该排除。
 */
export function focusablesIn(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => el.getClientRects().length > 0,
  );
}

/**
 * 把 Tab / Shift+Tab 关在容器里，首尾相接。
 *
 * 返回 `true` 表示这次按键由本函数接管（调用方要 `preventDefault`）。
 * 焦点已经在容器内的中间位置时返回 `false`，交给浏览器按默认顺序走——那一步
 * 本来就还在容器内，没必要自己算。
 */
export function trapTab(
  container: HTMLElement,
  event: { key: string; shiftKey: boolean },
  active: HTMLElement | null = document.activeElement as HTMLElement | null,
): boolean {
  if (event.key !== 'Tab') return false;
  const items = focusablesIn(container);
  // 一个可聚焦的都没有（纯文本弹窗）：也把 Tab 吞掉，别让焦点漏到背后的正文
  if (items.length === 0) return true;
  const first = items[0]!;
  const last = items[items.length - 1]!;
  if (!active || !container.contains(active)) {
    (event.shiftKey ? last : first).focus();
    return true;
  }
  if (event.shiftKey && active === first) {
    last.focus();
    return true;
  }
  if (!event.shiftKey && active === last) {
    first.focus();
    return true;
  }
  return false;
}

export interface ModalOptions {
  /** Esc 是否关闭弹窗。默认 `false`：只认关闭按钮。 */
  closeOnEscape?: boolean;
  /** `closeOnEscape` 为真时，Esc 按下后调它。 */
  onEscape?: () => void;
}

/**
 * @param focusTarget 打开后焦点落到哪儿（关闭按钮或面板本身）。
 *   焦点陷阱的边界按它所在的 `[role="dialog"]` 算，所以不必另传面板。
 */
export function useModalDialog(
  focusTarget: Ref<HTMLElement | null>,
  options: ModalOptions = {},
): void {
  let previousOverflow = '';
  let restoreTo: HTMLElement | null = null;

  function container(): HTMLElement | null {
    return focusTarget.value?.closest<HTMLElement>('[role="dialog"]') ?? focusTarget.value;
  }

  function onKeydown(event: KeyboardEvent): void {
    const panel = container();
    if (!panel) return;
    if (event.key === 'Escape') {
      // 菜单与编辑器也在听 keydown；弹窗打开时 Esc 只归弹窗。
      event.preventDefault();
      event.stopPropagation();
      if (options.closeOnEscape) options.onEscape?.();
      return;
    }
    if (trapTab(panel, event)) {
      event.preventDefault();
      event.stopPropagation();
    }
  }

  onMounted(() => {
    // 记下打开弹窗时焦点在哪：关掉之后要还回去，否则焦点掉到 body，
    // 键盘用户得从标题栏重新 Tab 一遍才能回到正文。
    restoreTo = (document.activeElement as HTMLElement | null) ?? null;
    // 弹窗是模态的：锁住页面滚动，避免滚轮/键盘把底下的正文带走。
    previousOverflow = document.documentElement.style.overflow;
    document.documentElement.style.overflow = 'hidden';
    // 捕获阶段：抢在任何局部处理器之前，Tab 不会先被编辑器接走
    document.addEventListener('keydown', onKeydown, true);
    focusTarget.value?.focus();
  });

  onBeforeUnmount(() => {
    document.documentElement.style.overflow = previousOverflow;
    document.removeEventListener('keydown', onKeydown, true);
    // 打开时焦点在 body 上（比如菜单刚关掉）就没有可还的地方，别硬塞
    if (restoreTo && restoreTo.isConnected && restoreTo !== document.body) {
      restoreTo.focus({ preventScroll: true });
    }
  });
}

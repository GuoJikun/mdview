// 模态弹窗的共用行为：锁住页面滚动、把 Esc 拦下来、焦点落到关闭按钮。
// 「关于」与「检查更新」都只认底部的关闭按钮——遮罩与 Esc 都不关窗，
// 免得误触把正在下载的更新丢掉。两份弹窗共用这一处，行为不会走偏。
import { onBeforeUnmount, onMounted, type Ref } from 'vue';

export function useModalDialog(focusTarget: Ref<HTMLElement | null>): void {
  let previousOverflow = '';

  function onKeydown(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    // 菜单与编辑器也在听 keydown；弹窗打开时 Esc 只归弹窗。
    event.preventDefault();
    event.stopPropagation();
  }

  onMounted(() => {
    // 弹窗是模态的：锁住页面滚动，避免滚轮/键盘把底下的正文带走。
    previousOverflow = document.documentElement.style.overflow;
    document.documentElement.style.overflow = 'hidden';
    document.addEventListener('keydown', onKeydown, true);
    focusTarget.value?.focus();
  });

  onBeforeUnmount(() => {
    document.documentElement.style.overflow = previousOverflow;
    document.removeEventListener('keydown', onKeydown, true);
  });
}

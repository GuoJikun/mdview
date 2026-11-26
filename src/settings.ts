// 偏好设置弹窗的外壳状态。
//
// 与 about.ts 同构：只碰响应式状态，不引用 bridge（bridge 要调 openSettings，
// 反向 import 会成环）。真正的偏读写与校验在 src/prefs.ts。
import { reactive } from 'vue';

export const settings = reactive({
  open: false,
});

export function openSettings(): void {
  settings.open = true;
}

export function closeSettings(): void {
  settings.open = false;
}

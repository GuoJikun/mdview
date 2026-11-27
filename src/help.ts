// 帮助体系的外壳状态：快捷键一览。与 about.ts / settings.ts 同构，
// 只碰响应式状态，不引用 bridge（避免成环）。
import { reactive } from 'vue';

export const help = reactive({
  shortcuts: false,
});

export function openShortcuts(): void {
  help.shortcuts = true;
}

export function closeShortcuts(): void {
  help.shortcuts = false;
}

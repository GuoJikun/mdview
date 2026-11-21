// 前端测试统一入口。
//
// 用例与实现同目录，统一收在各模块的 `__tests__/` 下（当前只有
// `src/editor/__tests__/`），src/ 目录树里实现与用例靠目录分开。
//
// 按「是否需要真实布局」分两个 project：
//   unit —— 纯逻辑（输入法状态机、上下移动几何、滚动速度曲线…），跑 node，
//           毫秒级，不依赖浏览器；`cargo test` 的 web_ime.rs 守门也跑这一支。
//   dom  —— 碰真实 DOM 的东西（内核启动、点击落点、光标像素定位），跑真浏览器。
//
// 为什么 dom 必须用浏览器而不是 happy-dom：这些断言的判据是**布局**。
// happy-dom 里 `getBoundingClientRect()` 恒为 0，光标定位、点击落点、
// 视口滚动全都没有意义，测了也只是在测 mock。
import { defineConfig } from 'vitest/config';
import { playwright } from '@vitest/browser-playwright';

export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: 'unit',
          environment: 'node',
          include: ['src/**/*.test.ts'],
          exclude: ['src/**/*.dom.test.ts', '**/node_modules/**'],
        },
      },
      {
        // 预优化 Tauri 的两个包，否则首次 import 时 Vite 会中途 reload 测试
        // （报 "Vite unexpectedly reloaded a test"，表现为偶发失败）。
        optimizeDeps: {
          include: ['@tauri-apps/api/core', '@tauri-apps/api/event'],
        },
        test: {
          name: 'dom',
          include: ['src/**/*.dom.test.ts'],
          browser: {
            // launchOptions 是 provider 级选项（透传给 playwright.launch），
            // 不是 instance 级——写到 instances 里会被静默忽略，然后转去下载
            // Playwright 自带那份 Chromium。
            // channel: 'chrome' 复用系统已装的 Chrome 或 Edge。
            provider: playwright({ launchOptions: { channel: 'chrome' } }),
            enabled: true,
            headless: true,
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
});

// 平台外观要在首帧前定下来：macOS 用系统标题栏，Windows/Linux 用自绘标题栏。
// 真正的判据在 Rust（与窗口 decorations 一致），这里先用 UA 抢一个初值避免闪一下。
// 原来这段是 index.html 的内联 <script>，会被 CSP 的 default-src 'self' 拦掉；
// 挪成模块后既满足 CSP，也保证在 app.js 启动前执行。
const mac = /Macintosh|Mac OS X/i.test(navigator.userAgent) || /^Mac/i.test(navigator.platform);
document.documentElement.classList.add(mac ? 'native-titlebar' : 'custom-titlebar');

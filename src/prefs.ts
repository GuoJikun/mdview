// 偏好设置：排版与启动行为。
//
// 存 localStorage 一份，**读写都带校验**：localStorage 被手改坏、或被别的
// 版本写进未知取值时回落默认值，而不是让界面带着一条脏数据渲染。
// 生效方式是往 <html> 写 CSS 变量，正文样式全部读变量——改字号不需要重建 DOM，
// 也就不会碰到投影层与停靠点。
import { reactive } from 'vue';

/** 正文字体族。三档够用：读文档、读散文、读代码。 */
export type FontChoice = 'system' | 'serif' | 'mono';
/** 启动时进入哪个视图。 */
export type StartupView = 'wysiwyg' | 'reading' | 'source';

export interface Prefs {
  font: FontChoice;
  /** 正文字号，px。 */
  fontSize: number;
  /** 正文最大宽度，px。 */
  width: number;
  /** Tab / Shift+Tab 的缩进宽度，空格数。 */
  indent: number;
  startupView: StartupView;
}

/** 各档字体的完整栈。写全比只写一个族名可靠：系统缺字体时还有退路。 */
export const FONT_STACKS: Record<FontChoice, string> = {
  system: '-apple-system, "Segoe UI", "Microsoft YaHei", "PingFang SC", sans-serif',
  serif: 'Georgia, "Songti SC", SimSun, "Noto Serif CJK SC", serif',
  mono: '"Cascadia Code", Consolas, "Courier New", monospace',
};

export const FONT_LABELS: Record<FontChoice, string> = {
  system: '系统无衬线',
  serif: '衬线（宋体类）',
  mono: '等宽',
};

export const VIEW_LABELS: Record<StartupView, string> = {
  wysiwyg: '所见即所得',
  reading: '阅读视图',
  source: '源码视图',
};

export const DEFAULTS: Prefs = {
  font: 'system',
  fontSize: 14,
  width: 780,
  indent: 2,
  startupView: 'wysiwyg',
};

/** 数值的可调范围。面板上的滑杆与校验共用同一份边界，不会各写一套。 */
export const LIMITS = {
  fontSize: { min: 12, max: 20, step: 1 },
  width: { min: 560, max: 1200, step: 20 },
  indent: { min: 2, max: 8, step: 2 },
} as const;

const STORAGE_KEY = 'mdview-prefs';

function clampInt(value: unknown, min: number, max: number, fallback: number): number {
  const n = typeof value === 'number' ? Math.round(value) : Number.parseInt(String(value), 10);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, n));
}

/**
 * 把任意输入收敛成一份合法偏好。未知字段丢弃、越界数值夹紧、
 * 枚举值不在白名单里回落默认——localStorage 里的东西一律不可信。
 */
export function sanitize(raw: unknown): Prefs {
  const obj = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
  return {
    font: obj.font === 'serif' || obj.font === 'mono' ? obj.font : DEFAULTS.font,
    fontSize: clampInt(obj.fontSize, LIMITS.fontSize.min, LIMITS.fontSize.max, DEFAULTS.fontSize),
    width: clampInt(obj.width, LIMITS.width.min, LIMITS.width.max, DEFAULTS.width),
    indent: clampInt(obj.indent, LIMITS.indent.min, LIMITS.indent.max, DEFAULTS.indent),
    startupView:
      obj.startupView === 'reading' || obj.startupView === 'source'
        ? obj.startupView
        : DEFAULTS.startupView,
  };
}

/** 读 localStorage。解析失败、值为 null 都当作「还没设过」。 */
export function load(): Prefs {
  try {
    const text = localStorage.getItem(STORAGE_KEY);
    return text ? sanitize(JSON.parse(text)) : { ...DEFAULTS };
  } catch {
    return { ...DEFAULTS };
  }
}

function write(prefs: Prefs): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    /* 隐私模式下写不进去，本次会话仍然生效，忽略 */
  }
}

/**
 * 把偏好写成 CSS 变量。正文样式全部读这几个变量，所以改完立刻可见。
 *
 * 字号与宽度变化会改变折行与字符坐标，**调用方还要让编辑器重新测量光标**
 * （`applyCaret(caret, true, true)` + 大纲锚点重测），否则自绘光标会停在旧位置。
 */
export function apply(prefs: Prefs): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.style.setProperty('--doc-font-size', `${prefs.fontSize}px`);
  root.style.setProperty('--doc-width', `${prefs.width}px`);
  root.style.setProperty('--doc-font', FONT_STACKS[prefs.font]);
}

/** 当前偏好。面板双向绑定它，改完调 `set` 落地。 */
export const prefs = reactive<Prefs>(load());

/** 合并一处改动：收敛 → 记住 → 应用，返回收敛后的完整偏好。 */
export function set(patch: Partial<Prefs>): Prefs {
  const next = sanitize({ ...prefs, ...patch });
  Object.assign(prefs, next);
  write(next);
  apply(next);
  return next;
}

/** 恢复默认值。 */
export function reset(): Prefs {
  return set({ ...DEFAULTS });
}

/** 启动时调用：读出偏好并应用，返回当前值供启动视图使用。 */
export function init(): Prefs {
  const stored = load();
  Object.assign(prefs, stored);
  apply(stored);
  return stored;
}

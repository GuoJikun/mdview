// 前端与内核之间的 IPC 契约，类型逐一对应 `src-tauri/src/session.rs`。
//
// 这份文件是「前端能编译」与「前端真能跟内核对话」之间的唯一凭据：字段名大小写、
// 可选与否、`usize` 对应 `number`，都在这里显式写死。`session.rs` 的
// `wire_json_matches_frontend_contract` 用同一组字面量做反方向的断言，改一边就要改两边。
//
// 只放类型，不放逻辑：运行时零成本，打包后被完全擦除。

/** 发往内核的消息。`kind` 是 serde 的 tag，其余字段随 kind 变化。 */
export type Wire =
  | { kind: 'insertText'; text: string }
  | { kind: 'backspace' }
  | { kind: 'deleteForward' }
  | { kind: 'newline' }
  | { kind: 'hardBreak' }
  | { kind: 'moveCaret'; delta: number; extend: boolean }
  | { kind: 'compositionStart' }
  | { kind: 'compositionCommit'; text: string }
  /** 取消组合（Esc / 失焦 / 空提交）。漏发会让内核永久挂在 composing。 */
  | { kind: 'compositionCancel' }
  /** 落点由前端按「停靠点 → 像素」算出，直接报全局序号。 */
  | { kind: 'setCaret'; stop: number; extend: boolean }
  | { kind: 'select'; anchor: number; head: number }
  | { kind: 'undo' }
  | { kind: 'redo' }
  | { kind: 'save' }
  /**
   * 另存为：路径由宿主弹框问到后回传。
   * 与 `save` 分开——「写哪个文件」这个决定只有窗口层能做。
   */
  | { kind: 'saveTo'; path: string }
  | { kind: 'selectAll' }
  | { kind: 'copy' }
  | { kind: 'cut' }
  /** 源码视图整篇替换：text 是原始 Markdown，不是渲染后的 HTML。 */
  | { kind: 'replaceSource'; text: string }
  | { kind: 'setMode'; mode: string }
  | { kind: 'tableMoveCell'; delta: number }
  /** Tab / Shift+Tab：列表按层级缩进，其余块插入 / 删除两个空格。 */
  | { kind: 'indent'; outdent: boolean }
  /** 改链接 / 图片：字段缺省表示保持原值。 */
  | { kind: 'editLink'; stop: number; url?: string; text?: string; remove?: boolean }
  | { kind: 'tableInsertRow'; after: boolean }
  | { kind: 'tableDeleteRow' }
  | { kind: 'tableInsertColumn'; after: boolean }
  | { kind: 'tableDeleteColumn' };

/** 一个块的投影：`html` 已经是最终 DOM，前端只负责换节点。 */
export interface BlockDto {
  id: number;
  base: number;
  end: number;
  html: string;
}

/**
 * 光标位置。字段名是内核原始命名（`CaretDto` 没有 serde rename），
 * 所以这里保留 snake_case，别顺手改成 camelCase。
 */
export interface CaretDto {
  stop: number;
  char_offset: number;
  block: number | null;
  /** 视觉列（软换行后的列号）；取不到时为 null。 */
  visual: number | null;
}

/** 增量帧：替换块节点 [start, end)，旧块数由 old_len/new_len 推出。 */
export interface PatchDto {
  start: number;
  end: number;
  old_len: number;
  new_len: number;
  html: string;
}

/**
 * 大纲项。`block` 是块索引，前端据此找到对应 DOM 元素定位滚动。
 * 与 `bridge.ts` 里的 `OutlineItem` 是同一份数据的两种视角（那边是给组件用的形状）。
 */
export interface OutlineDto {
  level: number;
  title: string;
  block: number;
}

/** `dispatch` 的回包。除 `revision`/`dirty` 外都可缺省。 */
export interface Reply {
  revision: number;
  dirty: boolean;
  /** 正文字数（可见文本，不含 Markdown 标记与空白）。 */
  words: number;
  patch?: PatchDto | null;
  full?: BlockDto[] | null;
  caret?: CaretDto | null;
  /** 已归一的选区停靠点区间；null 表示没有选区。 */
  selection?: [number, number] | null;
  /** 要写入系统剪贴板的文本（Ctrl+C / Ctrl+X）。 */
  clipboard?: string | null;
  conflict: boolean;
  saved: boolean;
  /** 内核没有落盘路径，需要弹保存对话框。置位时 dirty/saved 都不动。 */
  needs_path: boolean;
  /** 写盘失败的原因（磁盘满、只读、父目录被删）。与 conflict 是两回事。 */
  error?: string | null;
  /** 回包时的文档路径；另存为成功后前端据此更新标题栏。 */
  path?: string | null;
  outline?: OutlineDto[] | null;
  /** 内核里的 Markdown 源文本真值，供源码视图同步（不覆盖正在编辑的 textarea）。 */
  source?: string | null;
}

/** `snapshot` / `open_file` 的返回：整篇文档的一次全量投影。 */
export interface Snapshot {
  revision: number;
  path: string | null;
  dirty: boolean;
  stops: number;
  /** 正文字数（可见文本，不含 Markdown 标记与空白）。 */
  words: number;
  blocks: BlockDto[];
  caret: CaretDto | null;
  selection: [number, number] | null;
  outline: OutlineDto[];
  /** Markdown 源文本真值。源码视图显示这一份，不是渲染后的 HTML。 */
  source: string;
}

/** 「停靠点 → 像素」的测量结果。`el` 供 preedit 复制字体、判定表格用。 */
export interface CaretPos {
  x: number;
  top: number;
  height: number;
  el: HTMLElement;
  /** 是否是折叠标记（零宽），并列判决时要让位给文字停靠点。 */
  mk: boolean;
}

/** 视口坐标中的一个点。鼠标事件、拖动落点、探测点都用它。 */
export interface Point {
  x: number;
  y: number;
}

/** 正文真正可见的那一块（给探测点做边界用）。 */
export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** 一次自检断言的记录。 */
export interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

declare global {
  interface Window {
    /** `--self-test` 启动时由 Rust 侧在页面接管后调用。 */
    __mdviewSelfTest?: () => Promise<void>;
  }
}

export {};

/**
 * 简体中文界面文案。
 *
 * 这里是**用户能看到的每一句话**的唯一来源：菜单、弹窗、状态栏、toast。
 * 日志与 `console.warn` 里的中文不在此列——那些是写给开发者看的，不进文案表。
 *
 * 为什么先抽不翻译：现在只有中文一种语言，引入 `t('a.b.c')` 运行时查找只会
 * 换来「key 拼错要到运行时才发现」这一个新错误来源。导出成类型化对象后，
 * 访问路径由 TS 检查，写错立刻编译失败；将来加 `en-US` 时按 `type Dict = typeof zh`
 * 声明，少一条编译器就会报出来，不需要任何运行时机制。
 *
 * 分组按界面位置划分，不按文件划分——同一条文案被两个组件用到时，它应该只有
 * 一个位置，而不是各留一份。
 */

export const zh = {
  /** 应用名。窗口标题、「关于」弹窗、系统菜单都用它，改名只改这一处。 */
  appName: 'mdview',

  /** 通用按钮与占位词。 */
  common: {
    ok: '完成',
    close: '关闭',
    cancel: '取消',
    confirm: '确定',
    restoreDefaults: '恢复默认',
    gotIt: '知道了',
    /** 状态栏数字缺省时的占位（保持一格宽，不跳动）。 */
    dash: '-',
    /** 长文本缺省时的占位（「关于」里的版本号还没拿到）。 */
    emDash: '—',
    /** 空列表里的占位。 */
    none: '（暂无）',
  },

  /** 状态栏。 */
  statusBar: {
    blocks: '块',
    words: '字数',
    stops: '停靠点',
    column: '列',
    encodingTitle: (label: string) => `文件编码：${label}（保存时按它写回）`,
    hint: '← → 移动 · Shift 扩选 · Enter 分段 · Tab 跨格 · Ctrl+S 保存',
  },

  /** 表格工具条（光标在表格里时出现在状态栏右侧）。 */
  table: {
    rowAdd: '在下方插入行',
    rowDelete: '删除当前行',
    colAdd: '在右侧插入列',
    colDelete: '删除当前列',
    sortAsc: '按本列升序排列',
    sortDesc: '按本列降序排列',
  },

  /** 自绘标题栏（Windows / Linux；macOS 用系统标题栏，这里用不到）。 */
  titleBar: {
    menus: '应用菜单',
    minimize: '最小化',
    /** 按钮的悬停提示与无障碍名分开：提示说「最大化 / 还原」，读屏只说当前动作。 */
    maximizeTitle: '最大化 / 还原',
    maximize: '最大化',
    close: '关闭',
  },

  /** 「关于」弹窗。 */
  about: {
    title: (name: string) => `关于 ${name}`,
    version: (version: string) => `版本 ${version}`,
    desc: '一个用 Rust 写内核的 Markdown 编辑器：文本、解析与排版都在 Rust 侧完成，窗口与界面交给 Tauri + Vue。',
    kernel: '内核',
    shell: '外壳',
    license: '许可',
    project: '项目',
    licenseText: 'MIT OR Apache-2.0',
    projectUrl: 'github.com/GuoJikun/mdview',
  },

  /** 键盘快捷键面板。 */
  shortcuts: {
    title: '键盘快捷键',
    hint: 'macOS 上 Ctrl 换成 ⌘ · Esc 关闭',
    /** 菜单表达不了的键位，只能手写（见 `src/shortcuts.ts` 的 `EXTRA`）。 */
    findBar: '查找栏',
    editorArea: '编辑区',
    findNextPrev: '下一个 ／ 上一个',
    caseSensitive: '区分大小写',
    wholeWord: '全词匹配',
    regex: '正则表达式',
    closeFindBar: '关闭查找栏',
    indent: '缩进（表格里跳到下一格）',
    outdent: '反缩进（表格里跳到上一格）',
    clearSelection: '取消选区 ／ 关闭弹层',
    openLink: '在浏览器里打开链接',
  },

  /** 偏好设置弹窗。 */
  settings: {
    title: '偏好设置',
    font: '正文字体',
    fontSize: '字号',
    width: '正文宽度',
    indent: 'Tab 缩进',
    indentUnit: (count: number) => `${count} 空格`,
    startupView: '启动视图',
    autoSave: '自动保存',
    autoSaveHint: '有改动时静默写回原文件',
    /** 三段夹着两个 `<code>`，换语言时要按语序重排。 */
    indentHintA: '偏好存在本机，重启后保留。列表的缩进仍按标记宽度（',
    indentHintB: ' 两格、',
    indentHintC: ' 三格）——改这里不会把子项推进标记中间。',
    backupHint: '无论是否开启自动保存，未保存的改动都会每 5 秒备份到配置目录：崩溃或强制结束后重启，会问你要不要恢复。',
    fonts: {
      system: '系统无衬线',
      serif: '衬线（宋体类）',
      mono: '等宽',
    },
    views: {
      wysiwyg: '所见即所得',
      reading: '阅读视图',
      source: '源码视图',
    },
  },

  /** 检查更新弹窗。 */
  update: {
    title: '检查更新',
    currentVersion: '当前版本',
    checking: '正在检查更新…',
    upToDate: '当前已是最新版本。',
    available: (version: string) => `发现新版本 — ${version}`,
    notes: '更新内容',
    downloading: '正在下载…',
    ready: '更新已就绪',
    readyHint: '点击「安装并重启」完成更新。',
    failed: '检查更新失败',
    retry: '重试',
    install: '安装并重启',
    download: '下载更新',
    cancel: '取消',
    close: '关闭',
  },

  /** 未命名文档的显示名（标题栏、最近打开、消息里都用它）。 */
  untitled: '未命名',

  /**
   * 运行时提示：toast 与播报区。
   *
   * 带参数的条目一律是函数，错误信息原样接在冒号后面——厂商报错、系统错误码
   * 翻译不得，改了反而查不到。
   */
  toast: {
    // 文件
    fileOpened: '已打开文件',
    fileOpenFailed: (err: unknown) => `打开文件失败：${err}`,
    docCreated: '已新建文档',
    createFailed: (err: unknown) => `新建失败：${err}`,
    reloaded: '已重新加载',
    reloadFailed: (err: unknown) => `重新加载失败：${err}`,
    saved: '已保存',
    saveDialogFailed: (err: unknown) => `保存对话框打不开：${err}`,
    exported: (name: string) => `已导出：${name}`,
    exportFailed: (err: unknown) => `导出失败：${err}`,
    externalModifiedSaveRejected: '文件已被外部修改，保存被拒',
    externalModified: '文件已被外部修改，请重新打开以载入最新内容',
    recovered: '已恢复上次未保存的内容',
    closeConfirmFailed: (err: unknown) => `关闭确认失败：${err}`,
    recentCleared: '已清除最近记录',
    recentClearFailed: (err: unknown) => `清除失败：${err}`,
    windowOpenFailed: (err: unknown) => `新建窗口失败：${err}`,
    dirOpened: (path: string) => `已打开：${path}`,
    dirOpenFailed: (err: unknown) => `打不开目录：${err}`,

    // 编辑
    linkOpenFailed: (err: unknown) => `打不开链接：${err}`,
    imageInsertFailed: (err: unknown) => `插入图片失败：${err}`,
    imageAbsolutePath: '文档尚未保存，图片以绝对路径引用',
    copyFailedNoClipboard: '复制失败：系统剪贴板不可用',
    pasteHint: '请按 Ctrl+V 粘贴',
    readOnlyReading: '阅读视图不可编辑',
    readOnlySource: '源码视图里请直接输入 Markdown 标记',
    selectionRequiredForInSelectionFind: '先在正文里选中一段，才能只在选区内查找',
    noHeadings: '文档没有标题',
    kernelError: (err: unknown) => `内核报错：${err}`,
    welcomeShortcuts: '首次使用：Ctrl+/ 查看全部快捷键',

    // 播报区（看不见 toast 的人靠它知道发生了什么）
    viewSource: '已切到源码视图，可逐字朗读与编辑',
    viewReading: '已切到阅读视图，此视图不可编辑',
    viewWysiwyg: '已切到所见即所得视图',

    // 检查更新开关
    autoUpdateOn: '已开启启动时检查更新',
    autoUpdateOff: '已关闭启动时检查更新',
  },

  /** 文件侧栏（工作区）。 */
  files: {
    collapse: '收起文件侧栏',
    emptyDir: '（这个文件夹里没有 Markdown 文件）',
    truncated: '（部分目录项太多，只显示了前面一部分）',
    readFailed: (err: unknown) => `读不到这个目录：${err}`,
    openFailed: (err: unknown) => `打开文件夹失败：${err}`,
  },

  /** 编辑区里的零散文案（面板占位、浮动提示）。 */
  editor: {
    linkTextPlaceholder: '留空则用选中的文字',
    /** 未命名文档另存为时的建议文件名。 */
    untitledFileName: '未命名.md',
  },

  /** 大纲侧栏。 */
  outline: {
    title: '大纲',
    empty: '（无标题）',
  },

  /** 查找栏里的状态文字。 */
  find: {
    invalidRegex: '无效正则',
    noResults: '无结果',
  },

  /** 标题栏菜单。key 与 `menu.ts` 里的 `action` 对齐，便于对照。 */
  menu: {
    file: {
      title: '文件',
      newFile: '新建',
      newWindow: '新建窗口',
      openFile: '打开文件…',
      openFolder: '打开文件夹…',
      recent: '最近打开',
      recentClear: '清除记录',
      reloadFile: '重新加载',
      save: '保存',
      saveAs: '另存为…',
      exportHtml: '导出 HTML…',
      settings: '偏好设置…',
      closeWindow: '关闭窗口',
    },
    edit: {
      title: '编辑',
      undo: '撤销',
      redo: '重做',
      bold: '加粗',
      italic: '斜体',
      strike: '删除线',
      inlineCode: '行内代码',
      cut: '剪切',
      copy: '复制',
      paste: '粘贴',
      selectAll: '全选',
      find: '查找…',
      replace: '替换…',
    },
    insert: {
      title: '插入',
      image: '图片…',
      link: '链接…',
      table: '表格',
      hr: '分隔线',
      code: '代码块',
      quote: '引用',
      task: '任务列表',
    },
    selection: {
      title: '选择',
      selectAll: '全选',
      lineHome: '行首',
      lineEnd: '行尾',
    },
    view: {
      title: '视图',
      appearance: '外观',
      theme: '主题',
      themeSystem: '跟随系统',
      themeLight: '亮色',
      themeDark: '暗色',
      wysiwyg: '所见即所得',
      reading: '阅读视图',
      source: '源码视图',
      /** 皮肤名是产品名，不进翻译——换语言也不改。 */
      skinHyperos: 'HyperOS 4.0',
      skinMacos: 'macOS · Mica',
    },
    go: {
      title: '转到',
      prevHeading: '上一个标题',
      nextHeading: '下一个标题',
      docStart: '文档开头',
      docEnd: '文档结尾',
    },
    help: {
      title: '帮助',
      shortcuts: '键盘快捷键',
      openLogs: '打开日志目录',
      openBackups: '打开备份目录',
      update: '检查更新…',
      autoUpdate: '启动时检查更新',
      about: '关于',
    },
  },
} as const;

/** 文案表的形状。加语言时用它约束镜像表，少一条 key 就编译失败。 */
export type Dict = typeof zh;

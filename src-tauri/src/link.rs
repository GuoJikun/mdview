//! 外链交给系统默认浏览器打开。
//!
//! 文档里的链接**绝不能**让 webview 自己导航：那样整个应用会被一个链接带走，
//! 而且没有后退键能回来（重开窗口等于丢掉当前文档）。所以投影只输出
//! `data-href`，点击由前端转成一次 IPC，最终在这里落到系统 shell 上。
//!
//! 为什么不用 `<a href>`：即使拦住 `click`，WebView2 / WKWebView 仍可能通过
//! 中键、拖拽、右键菜单绕过去；投影层干脆不产出可导航的锚点。

/// 允许交给系统的协议。
///
/// Markdown 里 `[看这里](javascript:alert(1))` 是完全合法的语法，`dest_url`
/// 会原样落到属性上。若不加白名单，这里就成了「文档内容 → 任意协议执行」的
/// 通道；`file://` 还能拿来探本机文件。只放行三种对外协议。
pub fn is_allowed(url: &str) -> bool {
    let url = url.trim();
    if url.is_empty() || url.len() > 4096 {
        return false;
    }
    // 控制字符（含 NUL）会让部分 shell 提前截断参数，直接拒掉。
    if url.chars().any(char::is_control) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

/// 打开一个外链。
///
/// 前端只传 `data-href` 里那个原始 URL；校验在这里做，前端拦不拦都不影响安全。
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !is_allowed(&url) {
        log::warn!("拒绝打开非外链 URL：{url}");
        return Err("只允许打开 http(s) 与 mailto 链接".to_owned());
    }
    open_external(url.trim())
}

/// Windows：直接调 `ShellExecuteW`。
///
/// 不引 `tauri-plugin-opener`：本项目已经用裸 Win32 做过原子替换（见
/// `mdview-app::ports`），一个 API 调用不值得多一个依赖与一份权限配置。
/// 也不走 `cmd /c start`——那会闪一下控制台窗口。
#[cfg(windows)]
fn open_external(url: &str) -> Result<(), String> {
    // SW_SHOWNORMAL
    const SW_SHOWNORMAL: i32 = 1;

    #[link(name = "shell32")]
    unsafe extern "system" {
        #[link_name = "ShellExecuteW"]
        fn shell_execute_w(
            hwnd: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    let operation = wide("open");
    let file = wide(url);
    // SAFETY: 两个指针都指向以 NUL 结尾、调用期间有效的 UTF-16 缓冲区；
    // 其余参数传 null 是 ShellExecuteW 允许的缺省，`show` 为合法常量。
    let code = unsafe {
        shell_execute_w(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // 返回值 ≤ 32 才是错误码（SE_ERR_*），大于它只是实例句柄。
    if code <= 32 {
        Err(format!("系统未能打开链接（ShellExecute 返回 {code}）"))
    } else {
        Ok(())
    }
}

/// macOS：`open` 会交给 LaunchServices，按用户设置的默认浏览器打开。
#[cfg(target_os = "macos")]
fn open_external(url: &str) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("启动 open 失败：{err}"))
}

/// 其余 Unix：`xdg-open` 是桌面环境间的公约数。
#[cfg(all(unix, not(target_os = "macos")))]
fn open_external(url: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("启动 xdg-open 失败：{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_schemes_are_allowed() {
        for url in [
            "https://example.com/a?b=1#c",
            "http://127.0.0.1:1450",
            "HTTPS://EXAMPLE.COM",
            "mailto:someone@example.com",
            "  https://example.com  ",
        ] {
            assert!(is_allowed(url), "{url} 应当放行");
        }
    }

    #[test]
    fn dangerous_and_local_schemes_are_rejected() {
        // javascript: 是 Markdown 语法允许、但绝不能让系统执行的一条路
        for url in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "ms-settings:",
            "",
            "   ",
            "example.com",
        ] {
            assert!(!is_allowed(url), "{url} 应当被拒");
        }
    }

    #[test]
    fn control_characters_are_rejected() {
        assert!(!is_allowed("https://example.com\ncalc.exe"));
        assert!(!is_allowed("https://example.com\0x"));
    }
}

//! 检查更新 / 下载 / 安装。
//!
//! 走 `tauri-plugin-updater` 的 Rust API 而不是前端插件包：下载进度要能推给
//! 自绘弹窗，下载还要能取消，这两件事都只有持有 `Update` 的后端做得到。
//!
//! 流程：`check_update` 调插件 `check()` 拿到 `Update` 并暂存 → `download_update`
//! 调 `Update::download()`（进度经 `update:progress` 推送，「下载」与取消信号用
//! `tokio::select!` 竞速）→ `install_update` 调 `Update::install()` 完成签名校验
//! 并交给系统安装器。
//!
//! 签名公钥在 `tauri.conf.json` 的 `plugins.updater.pubkey`，私钥只在发布机上
//! （`TAURI_SIGNING_PRIVATE_KEY`），拿不到私钥伪造的包会在 install 阶段被拒。
//!
//! ## 代理
//!
//! 清单与安装包都由插件内部的 `reqwest` 客户端拉取。项目没有配
//! `plugins.updater.proxy`，也没有调 `no_proxy()`，所以走 **`reqwest` 的默认
//! 系统代理**（`system-proxy` 特性默认启用）：Windows 读注册表
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` 的
//! `ProxyEnable` / `ProxyServer` / `ProxyOverride`——就是「Internet 选项 →
//! 连接 → 局域网设置」那个代理，Clash、v2rayN 打开「系统代理」时写的正是它；
//! macOS 读系统网络配置。`HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY` 环境变量
//! 优先级更高，已有值时不会被系统设置覆盖。**不支持 PAC**（`AutoConfigURL`）：
//! 只开了 PAC 的机器上更新请求是直连的。插件也没启用 `reqwest` 的 `socks`
//! 特性，系统代理写成 `socks5://` 同样不生效——Clash 这类默认写的是 HTTP 代理，
//! 不受影响。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::watch;

// ---------------------------------------------------------------------------
// DTO
// ---------------------------------------------------------------------------

/// 前端可用的更新信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// Release 正文的原始 Markdown：留着排查渲染问题，弹窗排版只用 `body_html`。
    pub body: String,
    /// `body` 渲染后的安全 HTML 片段，弹窗直接 `v-html`（见 [`crate::release_notes`]）。
    pub body_html: String,
    pub date: Option<String>,
}

/// 下载进度事件（`update:progress`）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub received: u64,
    pub total: Option<u64>,
}

/// 命令错误。前端靠 `code` 区分「用户取消」这类可预期分支，`message` 只用于展示。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateError {
    pub code: String,
    pub message: String,
}

impl UpdateError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 全局更新状态
// ---------------------------------------------------------------------------

/// 已检查到的更新、已下载的字节，以及下载取消信号。
///
/// `Update` 本身不包含下载结果，所以 `download` 的返回值得自己存一份，
/// `install` 时再原样交回去。
pub struct UpdateState {
    cancel_tx: watch::Sender<bool>,
    update: Mutex<Option<Update>>,
    downloaded: Mutex<Option<Vec<u8>>>,
}

impl UpdateState {
    pub fn new() -> Self {
        let (cancel_tx, _) = watch::channel(false);
        Self {
            cancel_tx,
            update: Mutex::new(None),
            downloaded: Mutex::new(None),
        }
    }

    /// 用 `send_replace` 而不是 `send`：`send` 在没有任何接收者时直接失败、
    /// **不更新值**，而这两处恰恰经常发生在订阅之前。取消过一次后接收端随
    /// 下载结束被丢弃，再拿 `send(false)` 复位是空操作，下一轮下载一订阅
    /// 就立刻看到 `true`，被秒判为「已取消」。
    fn reset_cancel(&self) {
        self.cancel_tx.send_replace(false);
    }

    fn request_cancel(&self) {
        self.cancel_tx.send_replace(true);
    }

    fn cancel_rx(&self) -> watch::Receiver<bool> {
        self.cancel_tx.subscribe()
    }
}

impl Default for UpdateState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tauri Commands
// ---------------------------------------------------------------------------

/// 检查更新：有比当前版本更新的版本时返回其信息，已是最新返回 `None`。
///
/// 重新检查会丢掉上一次的下载结果——版本可能已经变了，旧字节不能拿去安装。
#[tauri::command]
pub async fn check_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, UpdateError> {
    let update = app
        .updater()
        .map_err(|err| UpdateError::new("updater_init", err.to_string()))?
        .check()
        .await
        .map_err(|err| UpdateError::new("update_check", err.to_string()))?;

    let state = app.state::<UpdateState>();
    state.reset_cancel();
    *state.downloaded.lock().unwrap() = None;

    let Some(update) = update else {
        *state.update.lock().unwrap() = None;
        return Ok(None);
    };

    let body = update.body.as_deref().unwrap_or_default();
    let info = UpdateInfo {
        version: update.version.clone(),
        body: body.to_owned(),
        body_html: crate::release_notes::render(body),
        date: update.date.map(|date| date.to_string()),
    };
    *state.update.lock().unwrap() = Some(update);
    Ok(Some(info))
}

/// 下载已检查到的更新；进度经 `update:progress` 事件推送，可被取消。
#[tauri::command]
pub async fn download_update(app: tauri::AppHandle) -> Result<(), UpdateError> {
    let state = app.state::<UpdateState>();
    state.reset_cancel();

    let update = state
        .update
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| UpdateError::new("no_update", "没有可用的更新"))?;

    let cancel_rx = state.cancel_rx();
    let received = Arc::new(AtomicU64::new(0));
    let received_chunk = received.clone();
    let app_handle = app.clone();

    let on_chunk = move |chunk_len: usize, content_length: Option<u64>| {
        let total =
            received_chunk.fetch_add(chunk_len as u64, Ordering::Relaxed) + chunk_len as u64;
        let _ = app_handle.emit(
            "update:progress",
            UpdateProgress {
                received: total,
                total: content_length,
            },
        );
    };
    let on_finish = || {};

    let bytes = tokio::select! {
        result = update.download(on_chunk, on_finish) => {
            result.map_err(|err| UpdateError::new("update_download", err.to_string()))?
        }
        _ = wait_cancel(cancel_rx) => {
            return Err(UpdateError::new("update_cancelled", "下载已取消"));
        }
    };

    *state.downloaded.lock().unwrap() = Some(bytes);
    Ok(())
}

/// 取消正在进行的下载。下载命令会以 `update_cancelled` 收尾。
#[tauri::command]
pub fn cancel_update_download(app: tauri::AppHandle) {
    app.state::<UpdateState>().request_cancel();
}

/// 安装已下载的更新。
///
/// - **Windows**：`install` 内部启动 NSIS/MSI 安装器后直接 `process::exit(0)`。
/// - **macOS / Linux**：`install` 返回后要自己退出，安装才会生效。
#[tauri::command]
pub fn install_update(app: tauri::AppHandle) -> Result<(), UpdateError> {
    let state = app.state::<UpdateState>();

    let update = state
        .update
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| UpdateError::new("no_update", "没有可用的更新"))?;

    let bytes = state
        .downloaded
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| UpdateError::new("no_download", "尚未下载更新包"))?;

    update
        .install(&bytes)
        .map_err(|err| UpdateError::new("update_install", err.to_string()))?;

    // 走到这里说明当前平台需要主动退出（Windows 已在 install 内退出）。
    std::thread::sleep(Duration::from_millis(300));
    app.exit(0);
    Ok(())
}

// ---------------------------------------------------------------------------
// 内部
// ---------------------------------------------------------------------------

/// 等取消信号变成 `true`。起始值为 `false`，`reset_cancel` 保证每轮重新等。
async fn wait_cancel(mut rx: watch::Receiver<bool>) {
    let _ = rx.wait_for(|cancelled| *cancelled).await;
}

#[cfg(test)]
mod tests {
    use super::UpdateState;

    /// 下载结束会丢掉接收端；此后再复位取消标志必须真的写进去，否则
    /// 「取消一次 → 重新下载」永远立刻失败，只能重启应用。
    #[test]
    fn cancel_flag_is_cleared_after_the_previous_download_ends() {
        let state = UpdateState::new();

        let rx = state.cancel_rx(); // 下载开始，订阅取消信号
        state.request_cancel(); // 用户点「取消」
        assert!(*rx.borrow(), "取消应当写进信号");
        drop(rx); // 下载结束，接收端被丢弃

        state.reset_cancel(); // 下一轮下载开始前复位
        let next = state.cancel_rx();
        assert!(!*next.borrow(), "新一轮下载不应继承上一轮的取消状态");
    }
}

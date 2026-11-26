//! Windows / macOS 系统底材。
//!
//! 系统材质只能由 Rust 侧设置；前端通过 [`backdrop_kind`] 得知当前材质，
//! 再决定要不要把 WebView 的表层背景改成透明。其他平台全部为空操作。
//!
//! Windows 用 DWM 的 Mica / Acrylic；macOS 用 NSVisualEffectView 的
//! Sidebar / HudWindow。`set_backdrop_mode` 是给开发时对照用的临时开关，
//! 不做持久化：关掉应用就回到默认材质。

use std::{collections::HashMap, sync::Mutex};

use tauri::{AppHandle, Manager, WebviewWindow};

#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    Mica,
    Acrylic,
    None,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "mica" => Some(Self::Mica),
            "acrylic" => Some(Self::Acrylic),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::Mica => "mica",
            Self::Acrylic => "acrylic",
            Self::None => "none",
        }
    }
}

/// 应用级材质状态。
///
/// `mode` / `dark` 所有窗口共用一套：同一台机器、同一份用户主题偏好。
/// 但「材质是否真的生效」必须按窗口分开记——底材是每个窗口各自 apply 的，
/// 一个窗口失败（比如正在销毁、或还没建好 WebView）不该让已经拿到材质的
/// 窗口跟着报 `none`，否则前端会把它已经透明化的表层留在外面露白板。
#[derive(Default)]
pub struct Backdrop {
    /// 每个窗口最近一次应用的结果，键为窗口 label。
    applied: Mutex<HashMap<String, bool>>,
    /// 当前材质；默认 Mica，临时切换时也写这里。
    mode: Mutex<Mode>,
    /// 前端同步过来的有效明暗；新窗口创建时沿用，避免闪一下亮色。
    dark: Mutex<Option<bool>>,
}

impl Backdrop {
    /// 这个窗口看到的材质。没应用过的窗口一律 `none`。
    pub fn kind(&self, label: &str) -> &'static str {
        if self.applied_for(label) {
            self.mode().kind()
        } else {
            "none"
        }
    }

    fn applied_for(&self, label: &str) -> bool {
        self.applied
            .lock()
            .ok()
            .and_then(|guard| guard.get(label).copied())
            .unwrap_or(false)
    }

    fn record(&self, label: &str, enabled: bool) {
        if let Ok(mut guard) = self.applied.lock() {
            guard.insert(label.to_string(), enabled);
        }
    }

    fn forget(&self, label: &str) {
        if let Ok(mut guard) = self.applied.lock() {
            guard.remove(label);
        }
    }

    fn mode(&self) -> Mode {
        self.mode
            .lock()
            .ok()
            .map(|guard| *guard)
            .unwrap_or_default()
    }

    fn set_mode(&self, mode: Mode) {
        if let Ok(mut guard) = self.mode.lock() {
            *guard = mode;
        }
    }

    /// 解析并切到新材质。认不出字符串时不改状态，避免前端手滑把材质关掉。
    fn switch_mode(&self, value: &str) -> bool {
        match Mode::parse(value) {
            Some(mode) => {
                self.set_mode(mode);
                true
            }
            None => false,
        }
    }

    pub fn set_dark(&self, dark: bool) {
        if let Ok(mut guard) = self.dark.lock() {
            *guard = Some(dark);
        }
    }

    pub fn desired_dark(&self) -> Option<bool> {
        self.dark.lock().ok().and_then(|guard| *guard)
    }

    /// 给单个窗口应用当前材质。`dark = None` 表示交给系统默认；
    /// macOS 的材质自己跟随系统明暗与窗口激活态，忽略这个参数。
    pub fn apply(&self, window: &WebviewWindow, dark: Option<bool>) -> bool {
        let mode = self.mode();
        #[cfg(target_os = "windows")]
        let applied = match mode {
            Mode::Mica => match window_vibrancy::apply_mica(window, dark) {
                Ok(()) => true,
                Err(err) => {
                    log::warn!("Mica 不可用，回退纯色：{err}");
                    false
                }
            },
            Mode::Acrylic => match window_vibrancy::apply_acrylic(window, None) {
                Ok(()) => true,
                Err(err) => {
                    log::warn!("Acrylic 不可用，回退纯色：{err}");
                    false
                }
            },
            Mode::None => {
                let _ = window_vibrancy::clear_mica(window);
                let _ = window_vibrancy::clear_acrylic(window);
                true
            }
        };
        #[cfg(target_os = "macos")]
        let applied = {
            let _ = dark;
            // crate 每次 apply 都会插入一个新的 effect view；重刷前先清掉旧的，
            // 否则主题切换几次就会叠出好几层模糊。clear_vibrancy 在主线程调用。
            let _ = window_vibrancy::clear_vibrancy(window);
            let apply = |material| {
                match window_vibrancy::apply_vibrancy(
                    window,
                    material,
                    Some(window_vibrancy::NSVisualEffectState::FollowsWindowActiveState),
                    Some(12.0), // 与 macOS 窗口的标准圆角一致
                ) {
                    Ok(()) => true,
                    Err(err) => {
                        log::warn!("macOS 系统材质不可用，回退纯色：{err}");
                        false
                    }
                }
            };
            match mode {
                Mode::Mica => apply(window_vibrancy::NSVisualEffectMaterial::Sidebar),
                Mode::Acrylic => apply(window_vibrancy::NSVisualEffectMaterial::HudWindow),
                Mode::None => true,
            }
        };
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let applied = {
            let _ = (window, dark);
            false
        };

        // 只记这个窗口的结果：别的窗口怎么着不影响它。
        self.record(window.label(), applied && mode != Mode::None);
        applied
    }

    /// 主题或材质切换后给所有现存窗口重刷。
    pub fn apply_all(&self, app: &AppHandle, dark: Option<bool>) {
        let windows = app.webview_windows();
        // 已关闭的窗口要把记录一起丢掉：label 会被下一个新窗口复用，
        // 留着上一轮的结果会让它 boot 时读到已经不存在的材质。
        for label in self.known_labels() {
            if !windows.contains_key(&label) {
                self.forget(&label);
            }
        }
        for window in windows.values() {
            self.apply(window, dark);
        }
    }

    fn known_labels(&self) -> Vec<String> {
        self.applied
            .lock()
            .map(|guard| guard.keys().cloned().collect())
            .unwrap_or_default()
    }
}

/// 前端询问这个窗口当前生效的材质，避免在没有系统材质时把背景透出去。
#[tauri::command]
pub fn backdrop_kind(window: WebviewWindow) -> &'static str {
    window.state::<Backdrop>().kind(window.label())
}

/// 前端把生效主题同步回来：Windows 让 DWM 的深色模式跟着走，
/// macOS 的系统材质自行跟随明暗，这里只更新新窗口沿用的偏好。
#[tauri::command]
pub fn sync_backdrop(app: AppHandle, theme: String) {
    let backdrop = app.state::<Backdrop>();
    backdrop.set_dark(theme == "dark");
    let dark = backdrop.desired_dark();
    backdrop.apply_all(&app, dark);
}

/// 临时切换材质：mica / acrylic / none。不写盘，重启后回到默认 Mica。
///
/// 返回**发起调用的这个窗口**切换后的材质；认不出的字符串保持原状。
#[tauri::command]
pub fn set_backdrop_mode(app: AppHandle, window: WebviewWindow, mode: String) -> &'static str {
    let backdrop = app.state::<Backdrop>();
    if backdrop.switch_mode(&mode) {
        let dark = backdrop.desired_dark();
        backdrop.apply_all(&app, dark);
    }
    backdrop.kind(window.label())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模拟一次 apply：真 apply 要 `WebviewWindow`，这里只喂它的结果。
    fn apply_result(backdrop: &Backdrop, label: &str, ok: bool) {
        backdrop.record(label, ok && backdrop.mode() != Mode::None);
    }

    #[test]
    fn kind_is_tracked_per_window() {
        let backdrop = Backdrop::default();
        // 还没 apply 过的窗口不能声称有材质，否则前端会清掉底色露出白板。
        assert_eq!(backdrop.kind("main"), "none");

        apply_result(&backdrop, "main", true);
        assert_eq!(backdrop.kind("main"), "mica");

        // 另一个窗口失败不能把已经拿到材质的窗口带下水（旧实现是一个全局 bool）。
        apply_result(&backdrop, "second", false);
        assert_eq!(backdrop.kind("second"), "none");
        assert_eq!(backdrop.kind("main"), "mica");
    }

    #[test]
    fn set_backdrop_mode_switches_every_window_and_ignores_unknown_values() {
        let backdrop = Backdrop::default();
        apply_result(&backdrop, "main", true);
        apply_result(&backdrop, "second", true);

        // mica → acrylic：两个窗口都跟着换。
        assert!(backdrop.switch_mode("acrylic"));
        apply_result(&backdrop, "main", true);
        apply_result(&backdrop, "second", true);
        assert_eq!(backdrop.kind("main"), "acrylic");
        assert_eq!(backdrop.kind("second"), "acrylic");

        // 认不出的字符串不改状态：前端传错时保持原材质。
        assert!(!backdrop.switch_mode("blur"));
        assert_eq!(backdrop.kind("main"), "acrylic");

        // 切到 none 后 apply 记成「无材质」，前端回退纯色。
        assert!(backdrop.switch_mode("none"));
        apply_result(&backdrop, "main", true);
        apply_result(&backdrop, "second", true);
        assert_eq!(backdrop.kind("main"), "none");
        assert_eq!(backdrop.kind("second"), "none");

        // 再切回来能恢复，不必重建窗口。
        assert!(backdrop.switch_mode("mica"));
        apply_result(&backdrop, "main", true);
        apply_result(&backdrop, "second", true);
        assert_eq!(backdrop.kind("main"), "mica");
        assert_eq!(backdrop.kind("second"), "mica");
    }

    #[test]
    fn closed_window_record_is_dropped() {
        let backdrop = Backdrop::default();
        apply_result(&backdrop, "doc-1", true);
        assert_eq!(backdrop.kind("doc-1"), "mica");
        // label 会被下一个窗口复用；忘掉之后新窗口在 apply 前只能读到 none。
        backdrop.forget("doc-1");
        assert_eq!(backdrop.kind("doc-1"), "none");
    }

    #[test]
    fn dark_preference_survives_for_new_windows() {
        let backdrop = Backdrop::default();
        assert_eq!(backdrop.desired_dark(), None);
        backdrop.set_dark(true);
        assert_eq!(backdrop.desired_dark(), Some(true));
        backdrop.set_dark(false);
        assert_eq!(backdrop.desired_dark(), Some(false));
    }
}

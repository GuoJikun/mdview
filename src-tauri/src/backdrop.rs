//! Windows / macOS 系统底材。
//!
//! 系统材质只能由 Rust 侧设置；前端通过 [`backdrop_kind`] 得知当前材质，
//! 再决定要不要把 WebView 的表层背景改成透明。其他平台全部为空操作。
//!
//! Windows 用 DWM 的 Mica / Acrylic；macOS 用 NSVisualEffectView 的
//! Sidebar / HudWindow。`set_backdrop_mode` 是给开发时对照用的临时开关，
//! 不做持久化：关掉应用就回到默认材质。

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

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

/// 应用级材质状态。所有窗口共用一套：同一台机器、同一份用户主题偏好。
#[derive(Default)]
pub struct Backdrop {
    /// 最近一次应用是否成功。前端据此决定是否放开 WebView 的透明背景。
    enabled: AtomicBool,
    /// 当前材质；默认 Mica，临时切换时也写这里。
    mode: Mutex<Mode>,
    /// 前端同步过来的有效明暗；新窗口创建时沿用，避免闪一下亮色。
    dark: Mutex<Option<bool>>,
}

impl Backdrop {
    pub fn kind(&self) -> &'static str {
        if self.enabled() {
            self.mode().kind()
        } else {
            "none"
        }
    }

    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    fn mode(&self) -> Mode {
        self.mode.lock().ok().map(|guard| *guard).unwrap_or_default()
    }

    fn set_mode(&self, mode: Mode) {
        if let Ok(mut guard) = self.mode.lock() {
            *guard = mode;
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

        self.enabled
            .store(applied && mode != Mode::None, Ordering::Relaxed);
        applied
    }

    /// 主题或材质切换后给所有现存窗口重刷。
    pub fn apply_all(&self, app: &AppHandle, dark: Option<bool>) {
        for window in app.webview_windows().values() {
            self.apply(window, dark);
        }
    }
}

/// 前端询问当前生效的材质，避免在没有系统材质时把背景透出去。
#[tauri::command]
pub fn backdrop_kind(app: AppHandle) -> &'static str {
    app.state::<Backdrop>().kind()
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
#[tauri::command]
pub fn set_backdrop_mode(app: AppHandle, mode: String) -> &'static str {
    let backdrop = app.state::<Backdrop>();
    if let Some(parsed) = Mode::parse(&mode) {
        backdrop.set_mode(parsed);
        let dark = backdrop.desired_dark();
        backdrop.apply_all(&app, dark);
    }
    backdrop.kind()
}

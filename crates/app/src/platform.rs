//! Small system services: tracing, clock, window drag, native view pointers, launch arguments.

use std::time::Instant;

use url::Url;
use winit::window::Window;
use wry::WebView;

use crate::i18n::t;

/// Debug builds only, with `BH_TRACE` set: one line on stderr for what a page asks the shell
/// (navigations, new windows, page events), to follow a click that misbehaves. Never in release.
/// When the process started: start-up steps are traced against it (`BH_TRACE`).
pub(crate) static LAUNCHED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Traces a start-up step with the time since launch (debug builds with `BH_TRACE` only).
pub(crate) fn trace_startup(step: &str) {
    trace(|| {
        let ms = LAUNCHED.get().map_or(0, |t| t.elapsed().as_millis());
        format!("startup {ms} ms: {step}")
    });
}

pub(crate) fn trace(line: impl FnOnce() -> String) {
    static ON: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| cfg!(debug_assertions) && std::env::var_os("BH_TRACE").is_some());
    if *ON {
        let line = line();
        if !line.is_empty() {
            eprintln!("[trace] {line}");
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn web_view_ptr(view: &WebView) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    use wry::WebViewExtMacOS;
    let web_view = view.webview();
    std::ptr::NonNull::new(std::ptr::from_ref(&*web_view).cast_mut().cast())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn web_view_ptr(_view: &WebView) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    None
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Moves a window with the mouse, from a press in the top bar its page draws. On Windows the move
/// is started natively (`blackhole_macos_webext::drag_window`): winit's `drag_window` stops working
/// for good after one quick click on the bar. Elsewhere winit's own.
pub(crate) fn drag_window(window: &Window) {
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle() {
            if let RawWindowHandle::Win32(h) = handle.as_raw() {
                blackhole_macos_webext::drag_window(h.hwnd.get());
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window.drag_window();
    }
}

/// Why the system cannot be asked to make BlackHole the default browser.
pub(crate) fn default_browser_unavailable() -> String {
    if cfg!(windows) {
        t("toast.default_browser_install")
    } else if cfg!(target_os = "macos") {
        t("toast.default_browser_bundle")
    } else {
        t("toast.default_browser_unsupported")
    }
}

/// The page this window was asked to open at launch.
pub(crate) fn launch_url() -> Option<String> {
    let mut args = std::env::args();
    args.find(|a| a == "--open")?;
    args.next()
        .filter(|u| matches!(Url::parse(u), Ok(u) if matches!(u.scheme(), "http" | "https")))
}

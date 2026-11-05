//! WebExtension host. On macOS 15.4+ WebKit can run real extensions (background pages,
//! `declarativeNetRequest`, content scripts…) inside any `WKWebView` whose configuration carries
//! a `WKWebExtensionController`. Other platforms get an inert stub.

/// Where a page view keeps its cookies and site data (a login is stored here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataStore {
    /// The browser's shared store.
    Default,
    /// A persistent store of its own, named by a UUID (macOS 14+).
    Identified([u8; 16]),
    /// Nothing is kept once the view is gone.
    Ephemeral,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    Loading,
    Loaded,
    Failed(String),
}

/// Toolbar button of a loaded extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInfo {
    pub id: String,
    pub label: String,
    pub badge: String,
    pub enabled: bool,
    /// `data:image/png;base64,…` of the extension's toolbar icon, when it has one.
    pub icon: Option<String>,
}

/// An extension found inside an installed app (Safari web extension bundle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppExtension {
    pub id: String,
    pub name: String,
    pub path: std::path::PathBuf,
}

/// Safari web extensions shipped by apps in `/Applications` and `~/Applications`
/// (`<App>.app/Contents/PlugIns/*.appex/Contents/Resources/manifest.json`).
pub fn discover_app_extensions() -> Vec<AppExtension> {
    let mut roots = vec![std::path::PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(std::path::Path::new(&home).join("Applications"));
    }
    discover_in(&roots)
}

pub fn discover_in(roots: &[std::path::PathBuf]) -> Vec<AppExtension> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(apps) = std::fs::read_dir(root) else {
            continue;
        };
        let mut apps: Vec<_> = apps
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .collect();
        apps.sort();
        for app in apps {
            let Ok(plugins) = std::fs::read_dir(app.join("Contents/PlugIns")) else {
                continue;
            };
            for appex in plugins
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "appex"))
            {
                let res = appex.join("Contents/Resources");
                if !res.join("manifest.json").is_file() {
                    continue;
                }
                let app_name = app
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let slug: String = app_name
                    .to_lowercase()
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                    .collect();
                out.push(AppExtension {
                    id: format!("app-{slug}"),
                    name: app_name,
                    path: res,
                });
            }
        }
    }
    out.dedup_by(|a, b| a.id == b.id);
    out
}

#[cfg(target_os = "macos")]
mod host;
#[cfg(target_os = "macos")]
mod tabs;
#[cfg(target_os = "macos")]
pub use host::ExtensionHost;

/// Rounds the corners of a page view (a live `WKWebView`) with its Core Animation layer.
#[cfg(target_os = "macos")]
pub fn round_view(view: std::ptr::NonNull<std::ffi::c_void>, radius: f64) {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::NSView;
    // Safety: `view` is a live NSView used on the main thread; the layer is only configured.
    unsafe {
        let view: &NSView = view.cast::<NSView>().as_ref();
        if !view.wantsLayer() {
            view.setWantsLayer(true);
        }
        let layer: Option<Retained<AnyObject>> = msg_send![view, layer];
        if let Some(layer) = layer {
            let radius_now: f64 = msg_send![&*layer, cornerRadius];
            if (radius_now - radius).abs() > f64::EPSILON {
                let _: () = msg_send![&*layer, setCornerRadius: radius];
            }
            let _: () = msg_send![&*layer, setMasksToBounds: true];
        }
    }
}

/// Moves a view above its siblings without recreating it (native stacking order).
#[cfg(target_os = "macos")]
pub fn raise_view(view: std::ptr::NonNull<std::ffi::c_void>) {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::NSView;
    // Safety: `view` is a live NSView used on the main thread; `NSWindowAbove` is 1 and a nil
    // sibling places the view in front of all the others.
    unsafe {
        let view: &NSView = view.cast::<NSView>().as_ref();
        let parent: Option<Retained<AnyObject>> = msg_send![view, superview];
        if let Some(parent) = parent {
            let nothing: *const AnyObject = std::ptr::null();
            let _: () = msg_send![&*parent, addSubview: view, positioned: 1isize, relativeTo: nothing];
        }
    }
}

/// The process macOS bills `pid` to: WebKit's content and network processes are launched by
/// launchd, so this is the only way to tie them back to the browser. Falls back to `pid`.
#[cfg(target_os = "macos")]
pub fn responsible_pid(pid: u32) -> u32 {
    extern "C" {
        fn responsibility_get_pid_responsible_for_pid(pid: i32) -> i32;
    }
    let Ok(raw) = i32::try_from(pid) else { return pid };
    // Safety: plain libsystem query that only reads the process table.
    let owner = unsafe { responsibility_get_pid_responsible_for_pid(raw) };
    u32::try_from(owner).ok().filter(|p| *p > 0).unwrap_or(pid)
}

/// Whether the browser is still the frontmost application. Window focus alone is not enough:
/// it also leaves the main window when a page view or an overlay takes the keyboard.
#[cfg(target_os = "macos")]
pub fn app_is_active() -> bool {
    use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
    // Safety: `isActive` is a plain getter on the shared application object.
    unsafe {
        let app: Option<Retained<AnyObject>> = msg_send![class!(NSApplication), sharedApplication];
        app.is_none_or(|app| msg_send![&*app, isActive])
    }
}

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::{app_is_active, raise_view, round_view, set_view_background, ExtensionHost};

#[cfg(not(any(target_os = "macos", windows)))]
mod stub {
    use super::LoadState;
    use std::path::Path;

    /// No extension host on this platform yet (WebView2 adapter pending).
    pub struct ExtensionHost;

    impl ExtensionHost {
        pub fn new() -> Option<Self> {
            None
        }
        pub fn load(&self, _id: &str, _path: &Path) {}
        pub fn unload(&self, _id: &str) {}
        pub fn state(&self, _id: &str) -> Option<LoadState> {
            None
        }
        pub fn actions(&self) -> Vec<super::ActionInfo> {
            Vec::new()
        }
        pub fn set_active_tab(
            &self,
            _view: Option<std::ptr::NonNull<std::ffi::c_void>>,
            _url: &str,
            _title: &str,
        ) {
        }
        pub fn show_popup(
            &self,
            _id: &str,
            _view: std::ptr::NonNull<std::ffi::c_void>,
            _rect: (f64, f64, f64, f64),
        ) -> bool {
            false
        }
    }
}
#[cfg(not(any(target_os = "macos", windows)))]
pub use stub::ExtensionHost;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_safari_web_extensions_inside_apps() {
        let t = std::env::temp_dir().join(format!("bh-appex-{}", std::process::id()));
        let res = t.join("Bitwarden.app/Contents/PlugIns/safari.appex/Contents/Resources");
        std::fs::create_dir_all(&res).unwrap();
        std::fs::write(res.join("manifest.json"), "{}").unwrap();
        std::fs::create_dir_all(t.join("Other.app/Contents/PlugIns/ql.appex/Contents/Resources"))
            .unwrap();
        let found = discover_in(std::slice::from_ref(&t));
        let _ = std::fs::remove_dir_all(&t);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].id.as_str(), found[0].name.as_str()),
            ("app-bitwarden", "Bitwarden")
        );
    }
}

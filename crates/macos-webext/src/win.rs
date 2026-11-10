//! WebExtensions on Windows: WebView2 runs Chromium extensions (service workers, content scripts,
//! `declarativeNetRequest`…) loaded from unpacked folders through the profile's browser-extension
//! API. Extensions are added and removed at runtime, no restart needed.

use crate::{ActionInfo, LoadState};
use base64::Engine;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};
use webview2_com::{
    take_pwstr, BrowserExtensionRemoveCompletedHandler, Microsoft::Web::WebView2::Win32::*,
    ProcessFailedEventHandler, ProfileAddBrowserExtensionCompletedHandler,
    ProfileGetBrowserExtensionsCompletedHandler, WebResourceRequestedEventHandler,
};
use windows_core::{Interface, HSTRING, PWSTR};
use wry::WebViewExtWindows;

/// Lowers (or restores) the memory target of a page view; a hidden tab can then be suspended.
pub fn set_view_background(view: &wry::WebView, background: bool) {
    let Ok(core) = view.webview().cast::<ICoreWebView2_19>() else {
        return;
    };
    let level = if background {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    // SAFETY: plain COM call on the thread that owns the view.
    let _ = unsafe { core.SetMemoryUsageTargetLevel(level) };
}

/// Native request blocking: WebView2 asks `blocked(request_url, page_url)` for the requests of
/// this view to `hosts` and their subdomains, and answers the refused ones itself (403, nothing
/// reaches the network). Only those hosts are watched, so the other requests of the page cost
/// nothing, and no function of the page is replaced.
pub fn block_hosts(view: &wry::WebView, hosts: &[&str], blocked: impl Fn(&str, &str) -> bool + 'static) {
    let (core, env) = (view.webview(), view.environment());
    // SAFETY: plain COM calls on the thread that owns the view. The handler reads the page's
    // address from its sender rather than holding the view (no reference cycle).
    unsafe {
        for host in hosts {
            for pattern in [format!("*://{host}/*"), format!("*://*.{host}/*")] {
                let _ = core.AddWebResourceRequestedFilter(
                    &HSTRING::from(pattern),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                );
            }
        }
        let mut token = Default::default();
        let _ = core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else {
                    return Ok(());
                };
                let request = {
                    let mut uri = PWSTR::null();
                    args.Request()?.Uri(&mut uri)?;
                    take_pwstr(uri)
                };
                let page = {
                    let mut uri = PWSTR::null();
                    sender.Source(&mut uri)?;
                    take_pwstr(uri)
                };
                if blocked(&request, &page) {
                    let response = env.CreateWebResourceResponse(
                        None,
                        403,
                        &HSTRING::from("Blocked"),
                        &HSTRING::new(),
                    )?;
                    args.SetResponse(&response)?;
                }
                Ok(())
            })),
            &mut token,
        );
    }
}

/// Calls `gone` when the renderer process of a page view exits (crash, or killed for its
/// memory use): the view then shows the engine's error page until it is reloaded.
pub fn on_renderer_gone(view: &wry::WebView, gone: impl Fn() + 'static) {
    // SAFETY: plain COM calls on the thread that owns the view.
    unsafe {
        let mut token = Default::default();
        let _ = view.webview().add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                args.ProcessFailedKind(&mut kind)?;
                if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                    gone();
                }
                Ok(())
            })),
            &mut token,
        );
    }
}

/// Clips a page view to rounded corners with a window region. The region keeps the size it was
/// made for, so call this again after every resize.
pub fn round_view(view: &wry::WebView, radius: i32) {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn},
        UI::WindowsAndMessaging::GetClientRect,
    };
    let mut host = HWND::default();
    let mut area = RECT::default();
    // SAFETY: plain Win32 calls on the UI thread; the system owns the region once it is set.
    unsafe {
        if view.controller().ParentWindow(&mut host).is_err()
            || GetClientRect(host, &mut area).is_err()
        {
            return;
        }
        let region = CreateRoundRectRgn(0, 0, area.right + 1, area.bottom + 1, radius * 2, radius * 2);
        SetWindowRgn(host, Some(region), true);
    }
}

/// Whether the foreground window belongs to this process (the main window, a page view, a
/// dialog of ours); the main window's own focus flickers when a child view takes the keyboard.
pub fn app_is_active() -> bool {
    use windows::Win32::{
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };
    // SAFETY: plain Win32 queries.
    unsafe {
        let mut owner = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut owner));
        owner == GetCurrentProcessId()
    }
}

/// Screen rectangle `(x, y, width, height)` in pixels of the foreground window when it belongs to
/// another application. `None` when it is the desktop or a minimised window.
pub fn frontmost_foreign_window() -> Option<(f64, f64, f64, f64)> {
    use windows::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{
            GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect, IsIconic,
        },
    };
    // SAFETY: plain Win32 queries.
    unsafe {
        let front = GetForegroundWindow();
        if front.0.is_null() || front == GetShellWindow() || front == GetDesktopWindow() || IsIconic(front).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(front, &mut r).ok()?;
        Some((f64::from(r.left), f64::from(r.top), f64::from(r.right - r.left), f64::from(r.bottom - r.top)))
    }
}

/// Moves a view's host window to the top of its siblings without recreating the view.
pub fn raise_view(view: &wry::WebView) {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE},
    };
    let mut host = HWND::default();
    // SAFETY: plain Win32 calls on the UI thread.
    unsafe {
        if view.controller().ParentWindow(&mut host).is_err() {
            return;
        }
        let _ = SetWindowPos(host, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// The parts of `manifest.json` the toolbar needs.
#[derive(Debug, Default, Clone)]
struct Manifest {
    name: String,
    popup: Option<String>,
    icon: Option<PathBuf>,
}

struct Ext {
    dir: PathBuf,
    manifest: Manifest,
    state: LoadState,
    browser_ext: Option<ICoreWebView2BrowserExtension>,
    /// Chromium's id of the extension, the host of its `chrome-extension://` pages.
    chromium_id: String,
}

#[derive(Default)]
struct Inner {
    profile: Option<ICoreWebView2Profile7>,
    /// False while stale extensions are being removed after a failed add; loads wait for it.
    ready: bool,
    /// Extensions already retried once after clearing the profile.
    retried: std::collections::HashSet<String>,
    exts: HashMap<String, Ext>,
}

/// Lives on the UI thread, like every WebView2 object.
pub struct ExtensionHost(Rc<RefCell<Inner>>);

impl ExtensionHost {
    pub fn new() -> Option<Self> {
        Some(Self(Rc::default()))
    }

    /// Gives the host the browser profile (shared by every view). Extensions requested before
    /// are loaded now.
    pub fn attach(&self, view: &wry::WebView) {
        let profile = view
            .webview()
            .cast::<ICoreWebView2_13>()
            .and_then(|w| unsafe { w.Profile() })
            .and_then(|p| p.cast::<ICoreWebView2Profile7>());
        let Ok(profile) = profile else {
            self.fail_all("this WebView2 runtime has no extension support");
            return;
        };
        {
            let mut inner = self.0.borrow_mut();
            inner.profile = Some(profile);
            inner.ready = false;
        }
        self.finish_clearing();
    }

    /// WebView2 keeps installed extensions in the profile between runs. Reusing them avoids a
    /// fresh install (and the welcome page extensions open for it) at each launch; only when adding
    /// fails (e.g. a stale copy after an update) the profile is cleared and the add retried.
    fn clear_previous_run(&self, profile: &ICoreWebView2Profile7) {
        let host = Self(Rc::clone(&self.0));
        let handler =
            ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_, list| {
                let mut stale = Vec::new();
                if let Some(list) = list {
                    let mut count = 0;
                    let _ = unsafe { list.Count(&mut count) };
                    for i in 0..count {
                        if let Ok(ext) = unsafe { list.GetValueAtIndex(i) } {
                            stale.push(ext);
                        }
                    }
                }
                if stale.is_empty() {
                    host.finish_clearing();
                    return Ok(());
                }
                let pending = Rc::new(std::cell::Cell::new(stale.len()));
                for ext in stale {
                    let (counter, host) = (Rc::clone(&pending), Self(Rc::clone(&host.0)));
                    let done =
                        BrowserExtensionRemoveCompletedHandler::create(Box::new(move |_| {
                            counter.set(counter.get() - 1);
                            if counter.get() == 0 {
                                host.finish_clearing();
                            }
                            Ok(())
                        }));
                    if unsafe { ext.Remove(&done) }.is_err() {
                        pending.set(pending.get() - 1);
                    }
                }
                if pending.get() == 0 {
                    host.finish_clearing();
                }
                Ok(())
            }));
        if unsafe { profile.GetBrowserExtensions(&handler) }.is_err() {
            self.finish_clearing();
        }
    }

    fn finish_clearing(&self) {
        let waiting: Vec<String> = {
            let mut inner = self.0.borrow_mut();
            if inner.ready {
                return;
            }
            inner.ready = true;
            inner
                .exts
                .iter()
                .filter(|(_, e)| e.state == LoadState::Loading)
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in waiting {
            self.add(&id);
        }
    }

    fn fail_all(&self, why: &str) {
        for e in self.0.borrow_mut().exts.values_mut() {
            e.state = LoadState::Failed(why.to_owned());
        }
    }

    /// `path` is either an unpacked extension folder or the `.zip` of a Chromium build.
    pub fn load(&self, id: &str, path: &Path) {
        self.unload(id);
        let dir = match unpack(id, path) {
            Ok(dir) => dir,
            Err(why) => {
                self.set_failed(id, path, why);
                return;
            }
        };
        let manifest = read_manifest(&dir);
        self.0.borrow_mut().exts.insert(
            id.to_owned(),
            Ext {
                dir,
                manifest,
                state: LoadState::Loading,
                browser_ext: None,
                chromium_id: String::new(),
            },
        );
        let ready = self.0.borrow().ready;
        if ready {
            self.add(id);
        }
    }

    fn set_failed(&self, id: &str, path: &Path, why: String) {
        self.0.borrow_mut().exts.insert(
            id.to_owned(),
            Ext {
                dir: path.to_owned(),
                manifest: Manifest::default(),
                state: LoadState::Failed(why),
                browser_ext: None,
                chromium_id: String::new(),
            },
        );
    }

    fn add(&self, id: &str) {
        let (profile, dir) = {
            let inner = self.0.borrow();
            let (Some(profile), Some(ext)) = (inner.profile.clone(), inner.exts.get(id)) else {
                return;
            };
            (profile, ext.dir.clone())
        };
        let (state, key) = (Rc::clone(&self.0), id.to_owned());
        let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
            move |result, extension| {
                if result.is_err() && state.borrow_mut().retried.insert(key.clone()) {
                    let profile = {
                        let mut inner = state.borrow_mut();
                        inner.ready = false;
                        if let Some(ext) = inner.exts.get_mut(&key) {
                            ext.state = LoadState::Loading;
                        }
                        inner.profile.clone()
                    };
                    if let Some(profile) = profile {
                        Self(Rc::clone(&state)).clear_previous_run(&profile);
                        return Ok(());
                    }
                }
                if let Some(ext) = state.borrow_mut().exts.get_mut(&key) {
                    match (result, extension) {
                        (Ok(()), Some(extension)) => {
                            ext.chromium_id = extension_id(&extension);
                            ext.browser_ext = Some(extension);
                            ext.state = LoadState::Loaded;
                        }
                        (Err(err), _) => ext.state = LoadState::Failed(err.message()),
                        _ => ext.state = LoadState::Failed("WebView2 returned no extension".into()),
                    }
                }
                Ok(())
            },
        ));
        if let Err(err) =
            unsafe { profile.AddBrowserExtension(&HSTRING::from(dir.as_path()), &handler) }
        {
            if let Some(ext) = self.0.borrow_mut().exts.get_mut(id) {
                ext.state = LoadState::Failed(err.message());
            }
        }
    }

    pub fn unload(&self, id: &str) {
        let removed = self.0.borrow_mut().exts.remove(id);
        if let Some(Ext {
            browser_ext: Some(extension),
            ..
        }) = removed
        {
            let done = BrowserExtensionRemoveCompletedHandler::create(Box::new(|_| Ok(())));
            let _ = unsafe { extension.Remove(&done) };
        }
    }

    pub fn state(&self, id: &str) -> Option<LoadState> {
        self.0.borrow().exts.get(id).map(|e| e.state.clone())
    }

    /// Toolbar buttons of the loaded extensions that have a popup.
    pub fn actions(&self) -> Vec<ActionInfo> {
        let inner = self.0.borrow();
        let mut out: Vec<ActionInfo> = inner
            .exts
            .iter()
            .filter(|(_, e)| e.state == LoadState::Loaded && e.manifest.popup.is_some())
            .map(|(id, e)| ActionInfo {
                id: id.clone(),
                label: e.manifest.name.clone(),
                badge: String::new(),
                enabled: true,
                icon: e.manifest.icon.as_deref().and_then(data_url),
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Address of the extension's popup page, to show in a web view of the same profile.
    pub fn popup_url(&self, id: &str) -> Option<String> {
        let inner = self.0.borrow();
        let ext = inner.exts.get(id)?;
        let page = ext.manifest.popup.as_ref()?;
        (ext.state == LoadState::Loaded && !ext.chromium_id.is_empty())
            .then(|| format!("chrome-extension://{}/{}", ext.chromium_id, page))
    }

    /// Extensions see the real tab on their own; nothing to forward on Windows.
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

fn extension_id(extension: &ICoreWebView2BrowserExtension) -> String {
    let mut raw = PWSTR::null();
    match unsafe { extension.Id(&mut raw) } {
        Ok(()) => take_pwstr(raw),
        Err(_) => String::new(),
    }
}

/// Extracts a Chromium `.zip` next to it (`unpacked/<id>`); a folder is used as is.
fn unpack(id: &str, path: &Path) -> Result<PathBuf, String> {
    if path.is_dir() {
        return Ok(path.to_owned());
    }
    let parent = path.parent().ok_or("invalid package path")?;
    let dest = parent.join("unpacked").join(id);
    let stamp = dest.join(".package");
    let size = std::fs::metadata(path)
        .map_err(|e| e.to_string())?
        .len()
        .to_string();
    if dest.join("manifest.json").is_file()
        && std::fs::read_to_string(&stamp).ok().as_deref() == Some(&size)
    {
        return Ok(dest);
    }
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    archive.extract(&dest).map_err(|e| e.to_string())?;
    // Some builds nest everything in one folder.
    let root = if dest.join("manifest.json").is_file() {
        dest.clone()
    } else {
        std::fs::read_dir(&dest)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.join("manifest.json").is_file())
            .ok_or("no manifest.json in the package")?
    };
    if root != dest {
        return Ok(root);
    }
    let _ = std::fs::write(&stamp, size);
    Ok(dest)
}

fn read_manifest(dir: &Path) -> Manifest {
    let Ok(text) = std::fs::read_to_string(dir.join("manifest.json")) else {
        return Manifest::default();
    };
    let json: serde_json::Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or_default();
    let action = json.get("action").or_else(|| json.get("browser_action"));
    let name = action
        .and_then(|a| a.get("default_title"))
        .or_else(|| json.get("name"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.starts_with("__MSG_"))
        .map(str::to_owned)
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let popup = action
        .and_then(|a| a.get("default_popup"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('/').to_owned());
    let icon = action
        .and_then(|a| a.get("default_icon"))
        .or_else(|| json.get("icons"))
        .and_then(|icons| match icons {
            serde_json::Value::String(path) => Some(path.clone()),
            serde_json::Value::Object(map) => map
                .iter()
                .filter_map(|(size, p)| Some((size.parse::<u32>().ok()?, p.as_str()?)))
                .max_by_key(|(size, _)| *size)
                .map(|(_, p)| p.to_owned()),
            _ => None,
        })
        .map(|p| dir.join(p.trim_start_matches('/')));
    Manifest { name, popup, icon }
}

fn data_url(path: &Path) -> Option<String> {
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return None,
    };
    let bytes = std::fs::read(path).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_toolbar_info_of_a_manifest() {
        let dir = std::env::temp_dir().join(format!("bh-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"name":"__MSG_name__","action":{"default_title":"Dark Reader","default_popup":"/ui/popup.html","default_icon":{"16":"i16.png","128":"i128.png"}}}"#,
        )
        .unwrap();
        let m = read_manifest(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(m.name, "Dark Reader");
        assert_eq!(m.popup.as_deref(), Some("ui/popup.html"));
        assert!(m.icon.unwrap().ends_with("i128.png"));
    }
}

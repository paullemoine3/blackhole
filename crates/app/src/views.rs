//! Construction of the native web views (WKWebView on macOS, WebView2 on Windows).

use crate::ipc::{Mailbox, Msg, PageEvent, UiCommand};
use blackhole_domain::{PrivacyDecision, PrivacyPolicy, TabId};
use std::sync::atomic::{AtomicBool, Ordering};
use url::Url;
use winit::window::Window;
use wry::{
    dpi::{LogicalPosition, LogicalSize},
    NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
};

const SHORTCUTS_JS: &str = include_str!("../ui/shortcuts.js");
const AUTOFILL_JS: &str = include_str!("../ui/autofill.js");
const PAGE_GUARD_JS: &str = include_str!("../ui/page_guard.js");
const FAVICON_JS: &str = include_str!("../ui/favicon.js");
const LIVE_CROP_JS: &str = include_str!("../ui/live_crop.js");
pub const LIVE_PICKER_JS: &str = include_str!("../ui/live_picker.js");

static HARDWARE_ACCELERATION: AtomicBool = AtomicBool::new(true);

/// Must be called before the first view is built: WebView2 fixes its GPU mode per process.
pub fn set_hardware_acceleration(on: bool) {
    HARDWARE_ACCELERATION.store(on, Ordering::Relaxed);
}

/// Every view comes from here so they all share the same engine flags (WebView2 requires it).
fn builder(ctx: &mut WebContext) -> WebViewBuilder<'_> {
    let builder = WebViewBuilder::new_with_web_context(ctx);
    // Browser extensions are a property of the engine: every view must agree on it.
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_browser_extensions_enabled(true)
    };
    #[cfg(windows)]
    let builder = if HARDWARE_ACCELERATION.load(Ordering::Relaxed) {
        builder
    } else {
        use wry::WebViewBuilderExtWindows;
        // Replaces wry's defaults, so they are repeated here.
        builder.with_additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-gpu",
        )
    };
    builder
}

/// Shell colour, visible before a page paints.
const SHELL_BG: (u8, u8, u8, u8) = (16, 15, 23, 255);
const CLEAR: (u8, u8, u8, u8) = (0, 0, 0, 0);
/// Pages that leave their canvas transparent must render on white, like in any browser.
const PAGE_BG: (u8, u8, u8, u8) = (255, 255, 255, 255);

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(x, y).into(),
        size: LogicalSize::new(w.max(1.0), h.max(1.0)).into(),
    }
}

/// One of BlackHole's own pages. It may talk to the shell and never navigates away.
/// `overlay` pages are transparent so they can float above the content.
pub fn ui_view(
    window: &Window,
    ctx: &mut WebContext,
    html: &'static str,
    bounds: Rect,
    visible: bool,
    overlay: bool,
    mailbox: &Mailbox,
) -> wry::Result<WebView> {
    let mailbox = mailbox.clone();
    builder(ctx)
        .with_html(html)
        .with_bounds(bounds)
        .with_visible(visible)
        .with_transparent(overlay)
        .with_background_color(if overlay { CLEAR } else { SHELL_BG })
        .with_devtools(cfg!(debug_assertions))
        .with_initialization_script_for_main_only(SHORTCUTS_JS, true)
        .with_ipc_handler(move |request| {
            if let Ok(command) = serde_json::from_str::<UiCommand>(request.body()) {
                mailbox.push(Msg::Ui(command));
            }
        })
        .with_navigation_handler(|url| matches!(url.split(':').next(), Some("about" | "data")))
        .build_as_child(window)
}

/// Content of the settings window: a regular (non-child) web view of its own window, so it has
/// no shortcut forwarding; the page handles its own keys.
pub fn settings_view(
    window: &Window,
    ctx: &mut WebContext,
    html: &'static str,
    mailbox: &Mailbox,
) -> wry::Result<WebView> {
    let mailbox = mailbox.clone();
    builder(ctx)
        .with_html(html)
        .with_background_color(SHELL_BG)
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |request| {
            if let Ok(command) = serde_json::from_str::<UiCommand>(request.body()) {
                mailbox.push(Msg::Ui(command));
            }
        })
        .with_navigation_handler(|url| matches!(url.split(':').next(), Some("about" | "data")))
        .build(window)
}

/// Own page filling a window as a child view (so live views can be stacked above it); like the
/// settings page it has no shortcut forwarding.
pub fn panel_view(
    window: &Window,
    ctx: &mut WebContext,
    html: &'static str,
    mailbox: &Mailbox,
) -> wry::Result<WebView> {
    let mailbox = mailbox.clone();
    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
    builder(ctx)
        .with_html(html)
        .with_bounds(rect(0.0, 0.0, size.width, size.height))
        .with_background_color(SHELL_BG)
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |request| {
            if let Ok(command) = serde_json::from_str::<UiCommand>(request.body()) {
                mailbox.push(Msg::Ui(command));
            }
        })
        .with_navigation_handler(|url| matches!(url.split(':').next(), Some("about" | "data")))
        .build_as_child(window)
}

/// The live view of an Easel card: the page of a live capture, cropped to its region. It shares
/// the profile (logins work), blocks trackers like a tab and has no channel to the shell.
pub fn live_view(
    window: &Window,
    ctx: &mut WebContext,
    url: &Url,
    selector: &str,
    policy: &PrivacyPolicy,
    mailbox: &Mailbox,
) -> wry::Result<WebView> {
    let hosts: Vec<&str> = policy.blocked_domains().collect();
    let guard = PAGE_GUARD_JS.replace(
        "__BLOCKED_HOSTS__",
        &serde_json::to_string(&hosts).unwrap_or_else(|_| "[]".to_owned()),
    );
    let crop = LIVE_CROP_JS.replace(
        "__SELECTOR__",
        &serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_owned()),
    );
    let (nav, popups) = (mailbox.clone(), mailbox.clone());
    let policy = policy.clone();
    builder(ctx)
        .with_url(url.as_str())
        .with_bounds(rect(0.0, 0.0, 1.0, 1.0))
        .with_visible(false)
        .with_background_color(PAGE_BG)
        .with_initialization_script(guard)
        .with_initialization_script_for_main_only(crop, true)
        .with_navigation_handler(move |raw| {
            let Ok(target) = Url::parse(&raw) else {
                return false;
            };
            match policy.evaluate(&target) {
                PrivacyDecision::Allow => true,
                PrivacyDecision::Rewrite(_) => false,
                PrivacyDecision::Block(why) => {
                    let host = target.host_str().unwrap_or_default().to_owned();
                    nav.push(Msg::Blocked { why, host });
                    false
                }
            }
        })
        .with_new_window_req_handler(move |raw, _| {
            if let Ok(url) = Url::parse(&raw) {
                popups.push(Msg::OpenTab(url));
            }
            NewWindowResponse::Deny
        })
        .build_as_child(window)
}

/// Shows the autofill suggestions (usernames only) under the focused login field.
pub fn autofill_show(view: &WebView, usernames: &[String]) {
    let list = serde_json::to_string(usernames).unwrap_or_else(|_| "[]".to_owned());
    let _ = view.evaluate_script(&format!(
        "window.__bhAutofill&&window.__bhAutofill.show({list})"
    ));
}

/// Fills the focused login form with the chosen credentials.
pub fn autofill_fill(view: &WebView, username: &str, password: &str) {
    let (u, p) = (
        serde_json::to_string(username).unwrap_or_default(),
        serde_json::to_string(password).unwrap_or_default(),
    );
    let _ = view.evaluate_script(&format!(
        "window.__bhAutofill&&window.__bhAutofill.fill({u},{p})"
    ));
}

/// Applies the enabled extensions' CSS (replacing the previous one) and runs their scripts.
pub fn apply_injection(view: &WebView, css: &str, js: &str) {
    let css = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".to_owned());
    let _ = view.evaluate_script(&format!(
        "(()=>{{const css={css};let s=document.getElementById('bh-ext-css');\
         if(!css){{s&&s.remove();}}else{{\
         if(!s){{s=document.createElement('style');s.id='bh-ext-css';(document.head||document.documentElement).appendChild(s);}}\
         s.textContent=css;}}}})();{js}"
    ));
}

/// A web page tab. Navigation is filtered by the privacy policy, and the tracker blocklist is
/// enforced inside the page because the native engines expose no network hook.
#[allow(clippy::too_many_arguments)]
pub fn tab_view(
    window: &Window,
    ctx: &mut WebContext,
    tab: TabId,
    url: &Url,
    bounds: Rect,
    policy: &PrivacyPolicy,
    mailbox: &Mailbox,
    host: Option<&blackhole_macos_webext::ExtensionHost>,
) -> wry::Result<WebView> {
    let hosts: Vec<&str> = policy.blocked_domains().collect();
    let guard = PAGE_GUARD_JS.replace(
        "__BLOCKED_HOSTS__",
        &serde_json::to_string(&hosts).unwrap_or_else(|_| "[]".to_owned()),
    );
    let (events, nav, popups, title, load) = (
        mailbox.clone(),
        mailbox.clone(),
        mailbox.clone(),
        mailbox.clone(),
        mailbox.clone(),
    );
    let (dl_start, dl_done) = (mailbox.clone(), mailbox.clone());
    let policy = policy.clone();
    let builder = builder(ctx);
    // Each page view gets its own configuration carrying the extension controller.
    #[cfg(target_os = "macos")]
    let builder = match host {
        Some(host) => {
            use wry::WebViewBuilderExtMacos;
            builder.with_webview_configuration(host.view_configuration())
        }
        None => builder,
    };
    #[cfg(not(target_os = "macos"))]
    let _ = host;
    builder
        .with_url(url.as_str())
        .with_bounds(bounds)
        .with_visible(false)
        .with_background_color(PAGE_BG)
        .with_devtools(cfg!(debug_assertions))
        .with_back_forward_navigation_gestures(true)
        .with_initialization_script(guard)
        .with_initialization_script_for_main_only(SHORTCUTS_JS, true)
        .with_initialization_script_for_main_only(AUTOFILL_JS, true)
        .with_initialization_script_for_main_only(FAVICON_JS, true)
        .with_ipc_handler(move |request| {
            if let Ok(event) = serde_json::from_str::<PageEvent>(request.body()) {
                events.push(Msg::Page(tab, event));
            }
        })
        .with_navigation_handler(move |raw| {
            let Ok(target) = Url::parse(&raw) else {
                return false;
            };
            // Extension UI frames (autofill menu, notification bar) are embedded in pages.
            if matches!(
                target.scheme(),
                "webkit-extension" | "safari-web-extension" | "chrome-extension"
            ) {
                return true;
            }
            match policy.evaluate(&target) {
                PrivacyDecision::Allow => true,
                PrivacyDecision::Rewrite(url) => {
                    nav.push(Msg::Redirect(tab, url));
                    false
                }
                PrivacyDecision::Block(why) => {
                    let host = target.host_str().unwrap_or_default().to_owned();
                    nav.push(Msg::Blocked { why, host });
                    false
                }
            }
        })
        .with_new_window_req_handler(move |raw, features| {
            // Script popups (OAuth/SSO logins) need `window.opener` and `window.close()`; a plain
            // tab would lose the link with the page and the login would never finish.
            #[cfg(windows)]
            if raw == "about:blank" || features.size.is_some() || features.position.is_some() {
                return NewWindowResponse::Allow;
            }
            #[cfg(not(windows))]
            let _ = features;
            if let Ok(url) = Url::parse(&raw) {
                popups.push(Msg::OpenTab(url));
            }
            NewWindowResponse::Deny
        })
        .with_download_started_handler(move |url, dest| {
            let name = dest.file_name().map_or_else(
                || "download".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
            let dir = blackhole_infrastructure::downloads_dir();
            let _ = std::fs::create_dir_all(&dir);
            *dest = blackhole_infrastructure::unique_path(&dir, &name);
            dl_start.push(Msg::DownloadStarted {
                url,
                path: dest.clone(),
            });
            true
        })
        .with_download_completed_handler(move |url, path, ok| {
            dl_done.push(Msg::DownloadDone { url, path, ok });
        })
        .with_document_title_changed_handler(move |text| title.push(Msg::Title(tab, text)))
        .with_on_page_load_handler(move |event: PageLoadEvent, raw| {
            if let Ok(url) = Url::parse(&raw) {
                if matches!(event, PageLoadEvent::Finished) {
                    load.push(Msg::Loaded(tab, url.clone()));
                }
                load.push(Msg::Url(tab, url));
            }
        })
        .build_as_child(window)
}

/// Toolbar popup of an extension: a page of the extension itself, in the shared profile.
#[cfg(windows)]
pub fn extension_popup(
    window: &Window,
    ctx: &mut WebContext,
    url: &str,
    bounds: Rect,
    mailbox: &Mailbox,
) -> wry::Result<WebView> {
    let links = mailbox.clone();
    builder(ctx)
        .with_url(url)
        .with_bounds(bounds)
        .with_background_color(PAGE_BG)
        .with_devtools(cfg!(debug_assertions))
        .with_navigation_handler(|raw| raw.starts_with("chrome-extension:"))
        .with_new_window_req_handler(move |raw, _| {
            if let Ok(url) = Url::parse(&raw) {
                links.push(Msg::OpenTab(url));
            }
            NewWindowResponse::Deny
        })
        .build_as_child(window)
}

/// Applies (or removes, when `css` is empty) the user's Boost in the page.
pub fn apply_boost(view: &WebView, css: &str) {
    let css = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".to_owned());
    let _ = view.evaluate_script(&format!(
        "(()=>{{const css={css};let s=document.getElementById('bh-boost');\
         if(!css){{s&&s.remove();return;}}\
         if(!s){{s=document.createElement('style');s.id='bh-boost';(document.head||document.documentElement).appendChild(s);}}\
         s.textContent=css;}})()"
    ));
}

pub fn go_back(view: &WebView) {
    let _ = view.evaluate_script("history.back()");
}

pub fn go_forward(view: &WebView) {
    let _ = view.evaluate_script("history.forward()");
}

pub fn reload(view: &WebView) {
    let _ = view.evaluate_script("location.reload()");
}

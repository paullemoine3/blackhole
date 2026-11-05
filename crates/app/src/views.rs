//! Construction of the native web views (WKWebView on macOS, WebView2 on Windows).

use crate::ipc::{Mailbox, Msg, PageEvent, UiCommand};
use blackhole_domain::{PrivacyDecision, PrivacyPolicy, TabId};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
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
const MEDIA_DEVICES_JS: &str = include_str!("../ui/media_devices.js");
const FIND_JS: &str = include_str!("../ui/find.js");
const DEBUG_LOG_JS: &str = include_str!("../ui/debug_log.js");
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
    let builder = {
        use wry::WebViewBuilderExtWindows;
        // Replaces wry's defaults, so they are repeated here. `--process-per-site` makes the
        // shell's own pages (all `about:blank`) and same-site tabs share one renderer instead
        // of one process per view.
        let mut args = String::from(
            "--disable-features=msWebOOUI,msPdfOOUI,            msSmartScreenProtection --process-per-site",
        );
        if !HARDWARE_ACCELERATION.load(Ordering::Relaxed) {
            args.push_str(" --disable-gpu");
        }
        builder.with_additional_browser_args(&args)
    };
    builder
}

#[cfg(target_os = "macos")]
const MACOS_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";

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
    store: Store,
) -> wry::Result<WebView> {
    let slot = TabSlot::new(AtomicU64::new(tab.0));
    build_tab(
        window, ctx, &slot, url, bounds, policy, mailbox, host, store,
    )
}

/// Which cookie jar (login session) a page view uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Store {
    /// The browser's main store: the first space, and every non-page view.
    Main,
    /// A space of its own: an account used there is not seen by the other spaces.
    Space(u64),
    /// A private window: nothing is kept.
    Private,
}

impl Store {
    /// macOS data store identifier of a space: a fixed prefix followed by the space id.
    #[cfg(target_os = "macos")]
    fn identifier(space: u64) -> [u8; 16] {
        let mut id = *b"BlackHoleSpace\0\0";
        id[8..].copy_from_slice(&space.to_be_bytes());
        id
    }
}

/// The tab a view reports for; a spare view is bound to its tab only when it is put to use.
pub type TabSlot = Arc<AtomicU64>;

/// A blank, hidden page view built ahead of time: creating a view is the slow part of opening
/// a tab, so tabs adopt a spare (`TabSlot::store`, then `load_url`) instead of building one.
pub fn spare_tab_view(
    window: &Window,
    ctx: &mut WebContext,
    policy: &PrivacyPolicy,
    mailbox: &Mailbox,
    host: Option<&blackhole_macos_webext::ExtensionHost>,
    store: Store,
) -> wry::Result<(TabSlot, WebView)> {
    let slot = TabSlot::new(AtomicU64::new(0));
    let blank = Url::parse("about:blank").expect("blank url");
    let bounds = rect(0.0, 0.0, 1.0, 1.0);
    let view = build_tab(
        window, ctx, &slot, &blank, bounds, policy, mailbox, host, store,
    )?;
    Ok((slot, view))
}

#[allow(clippy::too_many_arguments)]
fn build_tab(
    window: &Window,
    ctx: &mut WebContext,
    slot: &TabSlot,
    url: &Url,
    bounds: Rect,
    policy: &PrivacyPolicy,
    mailbox: &Mailbox,
    host: Option<&blackhole_macos_webext::ExtensionHost>,
    store: Store,
) -> wry::Result<WebView> {
    let id = |slot: &TabSlot| TabId(slot.load(Ordering::Relaxed));
    let (s_ipc, s_nav, s_title, s_load) = (slot.clone(), slot.clone(), slot.clone(), slot.clone());
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
    let builder = {
        use blackhole_macos_webext::DataStore;
        use wry::{WebViewBuilderExtDarwin, WebViewBuilderExtMacos};
        let data = match store {
            Store::Main => DataStore::Default,
            Store::Space(space) => DataStore::Identified(Store::identifier(space)),
            Store::Private => DataStore::Ephemeral,
        };
        match (host, data) {
            (Some(host), data) => builder.with_webview_configuration(host.view_configuration(data)),
            (None, DataStore::Identified(id)) => builder.with_data_store_identifier(id),
            (None, DataStore::Ephemeral) => builder.with_incognito(true),
            (None, DataStore::Default) => builder,
        }
    };
    // The bare WKWebView user agent lacks the Safari token, so Google Meet and others reject it.
    #[cfg(target_os = "macos")]
    let builder = builder.with_user_agent(MACOS_USER_AGENT);
    #[cfg(not(target_os = "macos"))]
    let _ = host;
    // WebView2: one profile per space inside the shared environment (a private window already
    // has a throwaway user data folder).
    #[cfg(windows)]
    let builder = match store {
        Store::Space(space) => {
            use wry::WebViewBuilderExtWindows;
            builder.with_profile_name(format!("BlackHole-space-{space}"))
        }
        _ => builder,
    };
    #[cfg(not(windows))]
    let _ = store;
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
        .with_initialization_script_for_main_only(MEDIA_DEVICES_JS, true)
        .with_initialization_script_for_main_only(FIND_JS, true)
        .with_initialization_script_for_main_only(
            if cfg!(debug_assertions) { DEBUG_LOG_JS } else { "" },
            false,
        )
        .with_ipc_handler(move |request| {
            if let Ok(event) = serde_json::from_str::<PageEvent>(request.body()) {
                events.push(Msg::Page(id(&s_ipc), event));
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
                    nav.push(Msg::Redirect(id(&s_nav), url));
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
        .with_document_title_changed_handler(move |text| {
            let tab = id(&s_title);
            if tab.0 != 0 {
                title.push(Msg::Title(tab, text));
            }
        })
        .with_on_page_load_handler(move |event: PageLoadEvent, raw| {
            let tab = id(&s_load);
            // A spare's own blank page is not navigation.
            if raw == "about:blank" || tab.0 == 0 {
                return;
            }
            if let Ok(url) = Url::parse(&raw) {
                match event {
                    PageLoadEvent::Started => load.push(Msg::LoadStart(tab)),
                    PageLoadEvent::Finished => load.push(Msg::Loaded(tab, url.clone())),
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

/// Puts views in front of every other view, in the order given, without recreating them.
/// Other platforms rebuild the views in the caller, when this returns false.
pub fn raise_views(views: &[&WebView]) -> bool {
    #[cfg(target_os = "macos")]
    {
        use wry::WebViewExtMacOS;
        for view in views {
            let native = view.webview();
            let Some(ptr) = std::ptr::NonNull::new(std::ptr::from_ref(&*native).cast_mut().cast())
            else {
                return false;
            };
            blackhole_macos_webext::raise_view(ptr);
        }
        true
    }
    #[cfg(windows)]
    {
        for view in views {
            blackhole_macos_webext::raise_view(view);
        }
        true
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = views;
        false
    }
}

/// Corner radius of the page card, in logical pixels.
pub const CARD_RADIUS: f64 = 14.0;

/// Rounds the corners of a view that fills the card. macOS keeps it for the life of the view;
/// on Windows the region has the size of the view, so this follows every resize.
pub fn round_corners(view: &WebView, scale: f64) {
    #[cfg(target_os = "macos")]
    {
        use wry::WebViewExtMacOS;
        let _ = scale;
        let native = view.webview();
        if let Some(ptr) = std::ptr::NonNull::new(std::ptr::from_ref(&*native).cast_mut().cast()) {
            blackhole_macos_webext::round_view(ptr, CARD_RADIUS);
        }
    }
    #[cfg(windows)]
    blackhole_macos_webext::round_view(view, (CARD_RADIUS * scale).round() as i32);
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (view, scale);
}

/// Asks the engine to trim a hidden tab's memory. Windows: WebView2 "low" memory target (also
/// lets it suspend the page). macOS: WebKit already throttles and suspends views that are
/// not visible, and exposes no finer knob, so nothing to do.
pub fn set_background(view: &WebView, background: bool) {
    #[cfg(windows)]
    blackhole_macos_webext::set_view_background(view, background);
    #[cfg(not(windows))]
    let _ = (view, background);
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
    let _ = view.go_back();
}

pub fn go_forward(view: &WebView) {
    let _ = view.go_forward();
}

pub fn reload(view: &WebView) {
    let _ = view.evaluate_script("location.reload()");
}

/// Hard refresh: drops service workers and Cache Storage, refetches the page bypassing the
/// HTTP cache, then reloads. Cookies and storage are kept so the user stays signed in.
pub fn hard_reload(view: &WebView) {
    let _ = view.evaluate_script(
        "(async()=>{try{for(const r of await navigator.serviceWorker.getRegistrations())await r.unregister()}catch(e){}\
         try{for(const k of await caches.keys())await caches.delete(k)}catch(e){}\
         try{await fetch(location.href,{cache:'reload',credentials:'same-origin'})}catch(e){}\
         location.reload()})()",
    );
}

/// Page zoom steps, like the other browsers (100% is the fifth).
pub const ZOOM_STEPS: [f64; 12] = [0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

/// The zoom step after (`up`) or before `current`.
pub fn next_zoom(current: f64, up: bool) -> f64 {
    let at = ZOOM_STEPS
        .iter()
        .position(|z| (z - current).abs() < 0.01)
        .unwrap_or(4);
    ZOOM_STEPS[if up { (at + 1).min(ZOOM_STEPS.len() - 1) } else { at.saturating_sub(1) }]
}

/// Floats the playing video above other windows (WebKit presentation mode, standard API elsewhere).
pub const PIP_ENTER_JS: &str = "(()=>{const v=[...document.querySelectorAll('video')].find(v=>!v.paused&&!v.ended&&v.videoWidth>0);if(!v||document.pictureInPictureElement||v.webkitPresentationMode==='picture-in-picture')return;try{if(v.webkitSetPresentationMode)v.webkitSetPresentationMode('picture-in-picture');else if(v.requestPictureInPicture)v.requestPictureInPicture().catch(()=>{})}catch(e){}})()";

/// Brings the video back into its page.
pub const PIP_EXIT_JS: &str = "(()=>{for(const v of document.querySelectorAll('video')){if(v.webkitPresentationMode==='picture-in-picture')v.webkitSetPresentationMode('inline')}if(document.pictureInPictureElement)document.exitPictureInPicture().catch(()=>{})})()";

/// Toggles Picture in Picture on the page's main video, playing or not.
pub const PIP_TOGGLE_JS: &str = "(()=>{const vs=[...document.querySelectorAll('video')];const on=vs.find(v=>v.webkitPresentationMode==='picture-in-picture');if(on||document.pictureInPictureElement){on?on.webkitSetPresentationMode('inline'):document.exitPictureInPicture();return}const v=vs.filter(v=>v.videoWidth>0).sort((a,b)=>(!b.paused-!a.paused)||(b.videoWidth*b.videoHeight-a.videoWidth*a.videoHeight))[0];if(!v)return;try{if(v.webkitSetPresentationMode)v.webkitSetPresentationMode('picture-in-picture');else v.requestPictureInPicture().catch(()=>{})}catch(e){}})()";

/// Audible media only: muted ambient videos do not count, but one the user muted from the card does.
/// Also reports the camera and microphone the page opened (see `ui/media_devices.js`).
pub const MEDIA_PROBE_JS: &str = "(()=>{const m=[...document.querySelectorAll('video,audio')].filter(v=>!v.muted||v.dataset.bhm);const on=m.filter(v=>!v.paused&&!v.ended&&v.readyState>2);return {playing:on.length>0,has:on.length>0||m.some(v=>v.currentTime>0&&!v.ended&&v.duration>1),muted:m.length>0&&m.some(v=>v.dataset.bhm),watching:[...document.querySelectorAll('video,audio')].some(v=>!v.paused&&!v.ended&&!v.loop&&v.readyState>2&&(!isFinite(v.duration)||v.duration>30)),pip:!!document.pictureInPictureElement||[...document.querySelectorAll('video')].some(v=>v.webkitPresentationMode==='picture-in-picture'),...(window.__bhRtc?window.__bhRtc.state():{})}})()";

/// Pauses what plays, or resumes the media that was being watched.
pub const MEDIA_TOGGLE_JS: &str = "(()=>{const m=[...document.querySelectorAll('video,audio')];const on=m.filter(v=>!v.paused&&!v.ended);if(on.length){on.forEach(v=>v.pause());return}const v=m.find(v=>v.currentTime>0&&!v.ended)||m[0];if(v)v.play().catch(()=>{})})()";

/// Mutes (or restores) the sound without touching videos the page itself keeps silent.
pub const MEDIA_MUTE_JS: &str = "(()=>{const m=[...document.querySelectorAll('video,audio')];const mute=!m.some(v=>v.dataset.bhm);m.forEach(v=>{if(mute){if(!v.muted){v.dataset.bhm='1';v.muted=true}}else if(v.dataset.bhm){delete v.dataset.bhm;v.muted=false}})})()";

/// Switches the camera or the microphone (`video` / `audio`) of a call on or off.
pub fn rtc_toggle_js(kind: &str) -> String {
    format!("window.__bhRtc&&window.__bhRtc.toggle('{kind}')")
}

pub const MEDIA_PAUSE_JS: &str =
    "[...document.querySelectorAll('video,audio')].forEach(v=>v.pause())";

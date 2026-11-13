//! Construction of the native web views (WKWebView on macOS, WebView2 on Windows).

use crate::ipc::{Mailbox, Msg, PageEvent, UiCommand};
use blackhole_domain::popup::{self, NewWindow};
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
/// Paints the highlights the shell hands a page, and reads its selection on demand.
const HIGHLIGHTS_JS: &str = include_str!("../ui/highlights.js");
/// Shortcut notation shared by BlackHole's own pages (`bhKeys`).
const KEYS_JS: &str = include_str!("../ui/keys.js");
/// Look shared by BlackHole's own pages: design tokens (`--bh-*`), icon scale, scroll bars.
const THEME_JS: &str = include_str!("../ui/theme.js");
/// Icons of BlackHole's own pages (`bhIcon`), drawn at the scale set by `THEME_JS`.
const ICONS_JS: &str = include_str!("../ui/icons.js");
/// `bhT`/`bhTn` and the catalog of the current language (see `crate::i18n`).
const I18N_JS: &str = include_str!("../ui/i18n.js");

fn i18n_script() -> String {
    I18N_JS.replace("__BH_I18N__", crate::i18n::page_json())
}

/// Sends the catalog of the current language to an open page of BlackHole (live switch).
pub fn send_language(view: &WebView) {
    let _ = view.evaluate_script(&format!(
        "window.bhI18n&&bhI18n.set({})",
        crate::i18n::page_json()
    ));
}
const SHIELDS_JS: &str = include_str!("../ui/shields.js");
/// What a page says about the product it shows, for the tab comparator.
pub const COMPARE_JS: &str = include_str!("../ui/compare.js");

/// The page clean-up script with the shields switched on now (fixed for the page's lifetime:
/// a change applies to pages opened or reloaded afterwards).
fn shields_script(policy: &PrivacyPolicy) -> String {
    let s = policy.shields();
    let config = serde_json::json!({
        "hideAds": s.hide_ads, "videoAds": s.video_ads, "cookieBanners": s.cookie_banners,
    });
    SHIELDS_JS.replace("__SHIELDS__", &config.to_string())
}
/// The request guard of page views. macOS and Windows block in the engine (`block_natively`),
/// so the script gets no host there and leaves the page's own functions alone.
fn guard_script(policy: &PrivacyPolicy) -> String {
    #[cfg(any(target_os = "macos", windows))]
    let hosts: Vec<&str> = {
        let _ = policy;
        Vec::new()
    };
    #[cfg(not(any(target_os = "macos", windows)))]
    let hosts: Vec<&str> = policy.blocked_domains().collect();
    PAGE_GUARD_JS.replace(
        "__BLOCKED_HOSTS__",
        &serde_json::to_string(&hosts).unwrap_or_else(|_| "[]".to_owned()),
    )
}

/// The name WebKit keeps a compiled blocklist under.
#[cfg(target_os = "macos")]
fn rule_list_id(name: &str) -> String {
    format!("blackhole-{name}")
}

/// Compiles the blocklists ahead of the first page view (see `block_natively`).
pub fn prepare_blocking(policy: &PrivacyPolicy) {
    #[cfg(target_os = "macos")]
    for (name, hosts) in policy.active_lists() {
        blackhole_macos_webext::compile_hosts(&rule_list_id(name), &hosts);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = policy;
}

/// macOS: the blocklists that are on now are enforced by WebKit itself in this view (content
/// rule lists: every frame and kind of request, no script in the page). Like the script
/// elsewhere, a view keeps the lists of its creation time.
#[cfg(target_os = "macos")]
fn block_natively(view: &WebView, policy: &PrivacyPolicy) {
    use wry::WebViewExtMacOS;
    let native = view.webview();
    let Some(ptr) = std::ptr::NonNull::new(std::ptr::from_ref(&*native).cast_mut().cast()) else {
        return;
    };
    for (name, hosts) in policy.active_lists() {
        blackhole_macos_webext::block_hosts(ptr, &rule_list_id(name), &hosts);
    }
}

/// Windows: WebView2 asks the policy for each request of this view to a listed host (third
/// parties only, live switches and the user's exceptions included) and refuses the blocked
/// ones itself; `report` gets their host. No script in the page, nothing for other requests.
#[cfg(windows)]
fn block_natively(view: &WebView, policy: &PrivacyPolicy, report: impl Fn(String) + 'static) {
    let hosts: Vec<&str> = policy.listed_domains().collect();
    let policy = policy.clone();
    blackhole_macos_webext::block_hosts(view, &hosts, move |raw, page| {
        let Ok(target) = Url::parse(raw) else {
            return false;
        };
        let host = target.host_str().unwrap_or_default();
        let page_host = Url::parse(page).ok().and_then(|u| u.host_str().map(str::to_owned));
        if !matches!(target.scheme(), "http" | "https")
            || !blackhole_domain::is_third_party(host, page_host.as_deref().unwrap_or_default())
        {
            return false;
        }
        let stop = matches!(
            crate::errors::with_exceptions(policy.evaluate(&target), &target),
            PrivacyDecision::Block(_)
        );
        if stop {
            report(host.to_owned());
        }
        stop
    });
}

const DEBUG_LOG_JS: &str = include_str!("../ui/debug_log.js");
const LIVE_CROP_JS: &str = include_str!("../ui/live_crop.js");
const LIVE_PICKER_JS: &str = include_str!("../ui/live_picker.js");

/// A script injected in web pages with the texts it shows (`__BH_TEXT__`: the catalog keys it
/// uses, in the current language). Set when the view is created: a change of language reaches
/// the tabs opened afterwards.
fn page_script(js: &str, keys: &[&str]) -> String {
    let cat = crate::i18n::catalog();
    let texts: serde_json::Map<String, serde_json::Value> =
        keys.iter().map(|k| ((*k).to_owned(), cat.get(k).into())).collect();
    let literal = serde_json::Value::Object(texts).to_string().replace('<', "\\u003c");
    js.replace("__BH_TEXT__", &literal)
}

/// The live area picker, injected on demand in the page in front.
pub fn live_picker_js() -> String {
    page_script(LIVE_PICKER_JS, &["page.pick_area"])
}

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

/// Per-navigation user agent: `None` keeps WebKit's default (see `page_view`).
#[cfg(target_os = "macos")]
fn user_agent_for(url: &str) -> Option<String> {
    let host = Url::parse(url).ok()?.host_str()?.to_owned();
    (host != "docs.google.com").then(|| MACOS_USER_AGENT.to_owned())
}

/// Shell colour, visible before a page paints.
const SHELL_BG: (u8, u8, u8, u8) = (16, 15, 23, 255);
const CLEAR: (u8, u8, u8, u8) = (0, 0, 0, 0);
/// Pages that leave their canvas transparent must render on white, like in any browser.
const PAGE_BG: (u8, u8, u8, u8) = (255, 255, 255, 255);

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    // Fractional view frames make WKWebView rasterise its layer off-grid (blurry canvas pages).
    Rect {
        position: LogicalPosition::new(x.round(), y.round()).into(),
        size: LogicalSize::new(w.round().max(1.0), h.round().max(1.0)).into(),
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
        .with_initialization_script_for_main_only(KEYS_JS, true)
        .with_initialization_script_for_main_only(i18n_script(), true)
        .with_initialization_script_for_main_only(THEME_JS, true)
        .with_initialization_script_for_main_only(ICONS_JS, true)
        .with_initialization_script_for_main_only(SHORTCUTS_JS, true)
        .with_ipc_handler(move |request| {
            if let Ok(command) = serde_json::from_str::<UiCommand>(request.body()) {
                mailbox.push(Msg::Ui(command));
            }
        })
        .with_navigation_handler(|url| matches!(url.split(':').next(), Some("about" | "data")))
        .build_as_child(window)
}

/// Own page filling a secondary window (Settings, Easel) as a child view, so live views can be
/// stacked above it; no shortcut forwarding, the page handles its own keys. Never `build()`:
/// on macOS it replaces the window's content view, and winit then panics (abort) when that
/// window loses the focus (`windowDidResignKey` reads the content view as its own `WinitView`).
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
        .with_initialization_script_for_main_only(KEYS_JS, true)
        .with_initialization_script_for_main_only(i18n_script(), true)
        .with_initialization_script_for_main_only(THEME_JS, true)
        .with_initialization_script_for_main_only(ICONS_JS, true)
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
    let guard = guard_script(policy);
    #[cfg(any(target_os = "macos", windows))]
    let native_policy = policy.clone();
    let shields = shields_script(policy);
    let crop = LIVE_CROP_JS.replace(
        "__SELECTOR__",
        &serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_owned()),
    );
    let (nav, popups) = (mailbox.clone(), mailbox.clone());
    let policy = policy.clone();
    let view = builder(ctx)
        .with_url(url.as_str())
        .with_bounds(rect(0.0, 0.0, 1.0, 1.0))
        .with_visible(false)
        .with_background_color(PAGE_BG)
        .with_initialization_script(guard)
        .with_initialization_script(shields)
        .with_initialization_script_for_main_only(crop, true)
        // A hidden or captured view never prompts: the challenge is cancelled (401 page).
        .with_http_auth_handler(|_, responder| drop(responder))
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
        .build_as_child(window)?;
    #[cfg(target_os = "macos")]
    block_natively(&view, &native_policy);
    #[cfg(windows)]
    block_natively(&view, &native_policy, |_| {});
    Ok(view)
}

/// Shows the autofill menu under the focused login field: `{names, locked, generate}`
/// (usernames only, never a password).
pub fn autofill_show(view: &WebView, menu: &serde_json::Value) {
    let _ = view.evaluate_script(&format!(
        "window.__bhAutofill&&window.__bhAutofill.show({menu})"
    ));
}

/// Fills the focused login form with the credentials the user picked.
pub fn autofill_fill(view: &WebView, username: &str, password: &str) {
    let (u, p) = (
        serde_json::to_string(username).unwrap_or_default(),
        zeroize::Zeroizing::new(serde_json::to_string(password).unwrap_or_default()),
    );
    let script = zeroize::Zeroizing::new(format!(
        "window.__bhAutofill&&window.__bhAutofill.fill({u},{})",
        p.as_str()
    ));
    let _ = view.evaluate_script(&script);
}

/// Writes a generated password into the new-password fields of the focused form.
pub fn autofill_generated(view: &WebView, password: &str) {
    let p = zeroize::Zeroizing::new(serde_json::to_string(password).unwrap_or_default());
    let script = zeroize::Zeroizing::new(format!(
        "window.__bhAutofill&&window.__bhAutofill.fillNew({})",
        p.as_str()
    ));
    let _ = view.evaluate_script(&script);
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
        window,
        ctx,
        &slot,
        Some(url),
        bounds,
        policy,
        mailbox,
        host,
        store,
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
    /// A black hole tab (its id): a session of its own, destroyed with the page.
    Burner(u64),
}

/// The engine profile of a black hole tab: unique to this run, so the folders left by earlier
/// runs can be told apart and removed (`remove_burner_profiles`). WebKit's ephemeral stores
/// leave nothing on disk: Windows only.
#[cfg(windows)]
pub fn burner_prefix() -> String {
    static RUN: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let run = RUN.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    });
    format!("{}{run}-", blackhole_infrastructure::BURNER_PROFILE)
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
/// It loads nothing at all: a blank page would stay first in the tab's history, and Back on the
/// tab's first page would land on it.
pub fn spare_tab_view(
    window: &Window,
    ctx: &mut WebContext,
    policy: &PrivacyPolicy,
    mailbox: &Mailbox,
    host: Option<&blackhole_macos_webext::ExtensionHost>,
    store: Store,
) -> wry::Result<(TabSlot, WebView)> {
    let slot = TabSlot::new(AtomicU64::new(0));
    let bounds = rect(0.0, 0.0, 1.0, 1.0);
    let view = build_tab(
        window, ctx, &slot, None, bounds, policy, mailbox, host, store,
    )?;
    Ok((slot, view))
}

#[allow(clippy::too_many_arguments)]
fn build_tab(
    window: &Window,
    ctx: &mut WebContext,
    slot: &TabSlot,
    url: Option<&Url>,
    bounds: Rect,
    policy: &PrivacyPolicy,
    mailbox: &Mailbox,
    host: Option<&blackhole_macos_webext::ExtensionHost>,
    store: Store,
) -> wry::Result<WebView> {
    let id = |slot: &TabSlot| TabId(slot.load(Ordering::Relaxed));
    let (s_ipc, s_nav, s_title, s_load) = (slot.clone(), slot.clone(), slot.clone(), slot.clone());
    let (s_auth, auth) = (slot.clone(), mailbox.clone());
    let s_pop = slot.clone();
    let (s_fail, fail, s_trust, trust) = (slot.clone(), mailbox.clone(), slot.clone(), mailbox.clone());
    let guard = guard_script(policy);
    #[cfg(any(target_os = "macos", windows))]
    let native_policy = policy.clone();
    let shields = shields_script(policy);
    let (events, nav, popups, title, load) = (
        mailbox.clone(),
        mailbox.clone(),
        mailbox.clone(),
        mailbox.clone(),
        mailbox.clone(),
    );
    let (dl_start, dl_done, s_dl) = (mailbox.clone(), mailbox.clone(), slot.clone());
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
            // Each ephemeral store is a new one: two black hole tabs share nothing.
            Store::Private | Store::Burner(_) => DataStore::Ephemeral,
        };
        match (host, data) {
            (Some(host), data) => builder.with_webview_configuration(host.view_configuration(data)),
            (None, DataStore::Identified(id)) => builder.with_data_store_identifier(id),
            (None, DataStore::Ephemeral) => builder.with_incognito(true),
            (None, DataStore::Default) => builder,
        }
    };
    // The bare WKWebView user agent lacks the Safari token, so Google Meet and others reject it.
    // Google Docs renders its canvas blurry when it sees that token, so it keeps WebKit's own.
    #[cfg(target_os = "macos")]
    let builder = {
        let _ = wry::USER_AGENT_RESOLVER.set(user_agent_for);
        builder.with_user_agent(MACOS_USER_AGENT)
    };
    // WebKit kills the process of a page that uses too much memory (heavy documents, long
    // calls): without this the tab would stay blank until the user reloads it.
    #[cfg(target_os = "macos")]
    let builder = {
        use wry::WebViewBuilderExtDarwin;
        let (s_crash, crash) = (slot.clone(), mailbox.clone());
        builder.with_on_web_content_process_terminate_handler(move || {
            let tab = id(&s_crash);
            if tab.0 != 0 {
                crash.push(Msg::Crashed(tab));
            }
        })
    };
    // What the content rule lists block is counted like what `page_guard.js` blocks elsewhere.
    #[cfg(target_os = "macos")]
    let builder = {
        use wry::WebViewBuilderExtDarwin;
        let (s_blocked, blocked) = (slot.clone(), mailbox.clone());
        builder.with_content_blocked_handler(move |raw| {
            let tab = id(&s_blocked);
            let host = Url::parse(&raw).ok().and_then(|u| u.host_str().map(str::to_owned));
            if let (true, Some(host)) = (tab.0 != 0, host) {
                blocked.push(Msg::Page(tab, PageEvent::Blocked { host }));
            }
        })
    };
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
        // An InPrivate profile of its own: nothing shared with the space, nothing kept; the
        // shell deletes the profile when the tab closes (`delete_profile`).
        Store::Burner(tab) => {
            use wry::WebViewBuilderExtWindows;
            builder
                .with_profile_name(format!("{}{tab}", burner_prefix()))
                .with_incognito(true)
        }
        _ => builder,
    };
    #[cfg(not(windows))]
    let _ = store;
    // WebView2 keeps Ctrl+F, Ctrl+G, Ctrl+P, F5, Alt+←/→… for its own browser features and the
    // page never sees them, so `shortcuts.js` could not forward them: find in page did nothing.
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_browser_accelerator_keys(false)
    };
    let builder = match url {
        Some(url) => builder.with_url(url.as_str()),
        None => builder,
    };
    let view = builder
        .with_bounds(bounds)
        .with_visible(false)
        .with_background_color(PAGE_BG)
        // Web pages can be inspected (F12): elements and styles, console, network.
        .with_devtools(true)
        .with_back_forward_navigation_gestures(true)
        .with_initialization_script(guard)
        .with_initialization_script(shields)
        .with_initialization_script_for_main_only(SHORTCUTS_JS, true)
        .with_initialization_script_for_main_only(
            page_script(
                AUTOFILL_JS,
                &["page.saved_passwords", "page.unlock_passwords", "page.strong_password", "settings.no_username"],
            ),
            true,
        )
        .with_initialization_script_for_main_only(FAVICON_JS, true)
        .with_initialization_script_for_main_only(MEDIA_DEVICES_JS, true)
        .with_initialization_script_for_main_only(
            page_script(FIND_JS, &["page.no_match", "palette.find", "page.previous", "page.next", "window.close"]),
            true,
        )
        .with_initialization_script_for_main_only(HIGHLIGHTS_JS, true)
        .with_initialization_script_for_main_only(
            if cfg!(debug_assertions) { DEBUG_LOG_JS } else { "" },
            false,
        )
        .with_ipc_handler(move |request| {
            crate::trace(|| {
                let body: String = request.body().chars().take(400).collect();
                if body.contains("\"media\"") || body.contains("login_submitted") || body.contains("autofill") {
                    String::new()
                } else {
                    format!("page event tab={} {body}", id(&s_ipc).0)
                }
            });
            if let Ok(event) = serde_json::from_str::<PageEvent>(request.body()) {
                events.push(Msg::Page(id(&s_ipc), event));
            }
        })
        .with_navigation_handler(move |raw| {
            crate::trace(|| format!("navigate tab={} main={} {raw}", id(&s_nav).0, wry::navigating_main_frame()));
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
            let host = target.host_str().unwrap_or_default().to_owned();
            match crate::errors::with_exceptions(policy.evaluate(&target), &target) {
                PrivacyDecision::Allow => true,
                PrivacyDecision::Rewrite(url) => {
                    let tab = id(&s_nav);
                    // The automatic https upgrade: if the secure version fails, the error page
                    // says the site has none.
                    if target.scheme() == "http" && url.scheme() == "https" {
                        nav.push(Msg::Upgraded(tab, target.clone()));
                    }
                    if policy.shields().clean_links
                        && blackhole_domain::clean_link::clean(&target).is_some()
                    {
                        nav.push(Msg::LinkCleaned);
                    }
                    nav.push(Msg::Redirect(tab, url));
                    false
                }
                PrivacyDecision::Block(why) => {
                    let tab = id(&s_nav);
                    // A blocked tracker opened as the page itself (not one of its frames).
                    if tab.0 != 0 && wry::navigating_main_frame() && why != "unsupported scheme" {
                        nav.push(Msg::BlockedPage(tab, target.clone(), why));
                    }
                    nav.push(Msg::Blocked { why, host });
                    false
                }
            }
        })
        .with_new_window_req_handler(move |raw, features| {
            // Script popups (OAuth/SSO logins, Google Identity Services) need `window.opener` and
            // `window.close()`; a plain tab would lose the link with the page and the login would
            // never come back. `Allow` opens an engine popup on the opener's configuration: same
            // profile (`Store`, cookies), closed by the page's `window.close()` (wry's
            // `webViewDidClose:` on macOS, WebView2's own popup window on Windows).
            let sized = features.size.is_some() || features.position.is_some();
            crate::trace(|| format!("new window sized={sized} -> {:?} {raw}", popup::new_window(&raw, sized)));
            match popup::new_window(&raw, sized) {
                NewWindow::Popup => return NewWindowResponse::Allow,
                NewWindow::Tab => {
                    match Url::parse(&raw) {
                        // A window asked by an extension from a page (Bitwarden's unlock, from
                        // its menu in a login form): the shell shows it as the extension's
                        // popup when it is one (`Msg::OpenTab`), never with a referrer.
                        Ok(url) if blackhole_domain::is_extension_page(&url) => {
                            popups.push(Msg::OpenTab(url));
                        }
                        Ok(url) => popups.push(Msg::OpenTabFrom(id(&s_pop), url)),
                        Err(_) => {}
                    }
                }
                NewWindow::Deny => {}
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
                tab: id(&s_dl),
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
                    PageLoadEvent::Started => {
                        crate::trace(|| format!("load start tab={} {raw}", tab.0));
                        load.push(Msg::LoadStart(tab))
                    }
                    PageLoadEvent::Finished => load.push(Msg::Loaded(tab, url.clone())),
                    PageLoadEvent::Failed => {
                        load.push(Msg::Failed(tab, url.clone()));
                        return;
                    }
                }
                load.push(Msg::Url(tab, url));
            }
        })
        // HTTP authentication (a NAS, a router): the shell asks for the credentials.
        .with_http_auth_handler(move |challenge, responder| {
            auth.push(Msg::Auth(id(&s_auth), challenge, responder));
        })
        // Why a load failed, or its HTTP error status: picks the error page.
        .with_load_failure_handler(move |failure| {
            let tab = id(&s_fail);
            if tab.0 != 0 {
                fail.push(Msg::LoadFailure(tab, failure));
            }
        })
        // A refused certificate goes on only for a host the user chose in the shell's menu.
        .with_server_trust_handler(move |challenge| {
            let allowed = crate::errors::cert_allowed(&challenge.host);
            let tab = id(&s_trust);
            if challenge.error != 0 && tab.0 != 0 {
                trust.push(Msg::ServerTrust(tab, challenge));
            }
            allowed
        })
        .build_as_child(window)?;
    #[cfg(target_os = "macos")]
    block_natively(&view, &native_policy);
    #[cfg(windows)]
    {
        let (s_blocked, blocked) = (slot.clone(), mailbox.clone());
        block_natively(&view, &native_policy, move |host| {
            let tab = id(&s_blocked);
            if tab.0 != 0 {
                blocked.push(Msg::Page(tab, PageEvent::Blocked { host }));
            }
        });
        // The renderer of the page died: the shell reloads the tab (see `on_crashed`).
        let (s_crash, crash) = (slot.clone(), mailbox.clone());
        blackhole_macos_webext::on_renderer_gone(&view, move || {
            let tab = id(&s_crash);
            if tab.0 != 0 {
                crash.push(Msg::Crashed(tab));
            }
        });
    }
    Ok(view)
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

/// "Back" on BlackHole's error page: the page tells the shell its history length
/// (`PageEvent::ErrorBack`), the shell picks how far to go.
pub fn error_back(view: &WebView) {
    let _ = view.evaluate_script("window.__bhBack?window.__bhBack():history.back()");
}

pub fn go_forward(view: &WebView) {
    let _ = view.go_forward();
}

/// Shell-themed page shown in a tab whose load failed (`ui/error.html`, one variant per
/// `kind`). The error page exposes `__bhRetry`, since reloading its own (blank-origin)
/// document would not retry the failed URL.
pub fn error_page(config: &serde_json::Value) -> String {
    let mut config = config.clone();
    config["i18n"] = crate::i18n::error_strings();
    // `<` is escaped so no text of the site can close the script block.
    let literal = config.to_string().replace('<', "\\u003c");
    include_str!("../ui/error.html").replace("__BH_ERROR__", &literal)
}

#[cfg(test)]
pub fn unreachable_page(url: &str) -> String {
    error_page(&serde_json::json!({ "kind": "unreachable", "url": url }))
}

pub fn reload(view: &WebView) {
    let _ = view.evaluate_script("window.__bhRetry?window.__bhRetry():location.reload()");
}

/// Hard refresh: drops service workers and Cache Storage, refetches the page bypassing the
/// HTTP cache, then reloads. Cookies and storage are kept so the user stays signed in.
pub fn hard_reload(view: &WebView) {
    let _ = view.evaluate_script(
        "if(window.__bhRetry){window.__bhRetry()}else(async()=>{try{for(const r of await navigator.serviceWorker.getRegistrations())await r.unregister()}catch(e){}\
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
/// A video that buffers still plays (no `readyState` test): the card's ▶/⏸ no longer blinks.
/// A video the site muted itself counts when it plays, does not loop and fills a quarter of the
/// window or more (a Twitch stream starts muted); hover previews and small autoplays do not.
pub const MEDIA_PROBE_JS: &str = "(()=>{const a=innerWidth*innerHeight;const big=v=>!v.paused&&!v.ended&&!v.loop&&(r=>r.width*r.height>=a*.25)(v.getBoundingClientRect());const m=[...document.querySelectorAll('video,audio')].filter(v=>!v.muted||v.dataset.bhm||big(v));const on=m.filter(v=>!v.paused&&!v.ended);return {playing:on.length>0,has:on.length>0||m.some(v=>v.currentTime>0&&!v.ended&&v.duration>1),muted:m.length>0&&m.every(v=>v.muted),watching:[...document.querySelectorAll('video,audio')].some(v=>!v.paused&&!v.ended&&!v.loop&&v.readyState>2&&(!isFinite(v.duration)||v.duration>30)),pip:!!document.pictureInPictureElement||[...document.querySelectorAll('video')].some(v=>v.webkitPresentationMode==='picture-in-picture'),...(window.__bhRtc?window.__bhRtc.state():{})}})()";

/// Pauses what plays, or resumes the media that was being watched.
pub const MEDIA_TOGGLE_JS: &str = "(()=>{const m=[...document.querySelectorAll('video,audio')];const on=m.filter(v=>!v.paused&&!v.ended);if(on.length){on.forEach(v=>v.pause());return}const v=m.find(v=>v.currentTime>0&&!v.ended)||m[0];if(v)v.play().catch(()=>{})})()";

/// Restores the sound BlackHole muted; else mutes what is audible; else (nothing audible, nothing
/// muted by us) unmutes what plays: the stream the site started muted.
pub const MEDIA_MUTE_JS: &str = "(()=>{const m=[...document.querySelectorAll('video,audio')];const ours=m.filter(v=>v.dataset.bhm);if(ours.length){ours.forEach(v=>{delete v.dataset.bhm;v.muted=false});return}const loud=m.filter(v=>!v.muted);if(loud.length){loud.forEach(v=>{v.dataset.bhm='1';v.muted=true});return}m.filter(v=>!v.paused&&!v.ended&&!v.loop).forEach(v=>{v.muted=false})})()";

/// Switches the camera or the microphone (`video` / `audio`) of a call on or off.
pub fn rtc_toggle_js(kind: &str) -> String {
    format!("window.__bhRtc&&window.__bhRtc.toggle('{kind}')")
}

/// Closes the player of the sidebar card: pauses everything and brings a floating video back
/// into its page.
pub const MEDIA_CLOSE_JS: &str = "(()=>{for(const v of document.querySelectorAll('video,audio')){v.pause();if(v.webkitPresentationMode==='picture-in-picture')v.webkitSetPresentationMode('inline')}if(document.pictureInPictureElement)document.exitPictureInPicture().catch(()=>{})})()";

#[cfg(all(test, target_os = "macos"))]
mod user_agent_tests {
    use super::{user_agent_for, MACOS_USER_AGENT};

    #[test]
    fn google_docs_keeps_the_webkit_agent_and_the_rest_gets_safari() {
        assert_eq!(user_agent_for("https://docs.google.com/document/d/x/edit"), None);
        assert_eq!(user_agent_for("https://docs.google.com/spreadsheets/d/x"), None);
        let safari = Some(MACOS_USER_AGENT.to_owned());
        assert_eq!(user_agent_for("https://meet.google.com/abc-defg-hij"), safari);
        assert_eq!(user_agent_for("https://x.atlassian.net/jira/software/projects/A/boards/1"), safari);
        // Unparsable or host-less URLs keep the default rather than guessing.
        assert_eq!(user_agent_for("about:blank"), None);
    }
}

#[cfg(test)]
mod unreachable_tests {
    use super::unreachable_page;

    #[test]
    fn url_cannot_break_out_of_the_script() {
        let page = unreachable_page("https://x.test/</script><script>alert(1)</script>");
        assert!(!page.contains("</script><script>alert"));
        assert!(page.contains("\\u003c/script>"));
        assert!(!page.contains("__BH_ERROR__"));
    }
}

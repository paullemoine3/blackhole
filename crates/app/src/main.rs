//! BlackHole composition root: window, native web views and message routing.
//! Business rules live in the inner crates; this file only wires them to the platform web engine
//! (WKWebView on macOS, WebView2 on Windows) through `wry`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod backdrop;
#[cfg(feature = "devtools")]
mod devtools;
mod extensions;
mod favicons;
mod ipc;
mod sidebar;
mod views;

use blackhole_application::{BrowserService, NavigationOutcome, WorkspaceRepository};
use blackhole_domain::{BlockLog, Easels, FolderId, SearchEngine, Settings, SpaceId, TabId};
use blackhole_infrastructure::catalog;
use blackhole_infrastructure::peers;
use blackhole_infrastructure::updater::{self, Release};
use blackhole_infrastructure::{
    default_privacy_policy, extensions_dir, load_extensions, JsonWorkspaceRepository,
};
use blackhole_infrastructure::{
    import,
    store::{BookmarkStore, EaselStore, Vault},
};
use ipc::{Mailbox, Msg, PageEvent, UiCommand, Wake};
use serde_json::json;
use sidebar::Sidebar;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use url::Url;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};
use wry::{WebContext, WebView};

const CHROME_HTML: &str = include_str!("../ui/index.html");
const HOME_HTML: &str = include_str!("../ui/home.html");
const BAR_HTML: &str = include_str!("../ui/bar.html");
const PALETTE_HTML: &str = include_str!("../ui/palette.html");
const SPLITBAR_HTML: &str = include_str!("../ui/splitbar.html");
const PEEK_HTML: &str = include_str!("../ui/peek.html");
const HOTZONE_HTML: &str = include_str!("../ui/hotzone.html");
#[cfg(windows)]
const EXT_POPUP_HTML: &str = include_str!("../ui/extpopup.html");
const SETTINGS_HTML: &str = include_str!("../ui/settings.html");
/// What a page says about its audio and video (see `views::MEDIA_PROBE_JS`).
#[derive(Clone, Copy, Default, serde::Deserialize)]
struct MediaProbe {
    playing: bool,
    has: bool,
    muted: bool,
}

/// How often the event loop wakes to refresh this window's place in the registry, look for
/// incoming tabs, spares to build and media to probe.
const PEERS_EVERY: Duration = Duration::from_millis(400);
/// How often background tabs are asked whether they play audio or video.
const MEDIA_EVERY: Duration = Duration::from_millis(1500);
/// Spare page views kept ready, and how long the app must stay quiet before building them.
const SPARES: usize = 2;
const SPARES_DELAY: Duration = Duration::from_millis(900);
const EASEL_HTML: &str = include_str!("../ui/easel.html");
const LIBRARY_HTML: &str = include_str!("../ui/library.html");
/// Gap between the two panes of a split view.
const SPLIT_GAP: f64 = 8.0;
/// Height of the strip above a split view that carries each pane's title and close button.
const SPLIT_HEADER: f64 = 30.0;
/// Reserved id of the peek page; real tabs start at 1.
const PEEK_TAB: TabId = TabId(0);
const SPACE_COLORS: [&str; 5] = ["#7c5cff", "#ff5c8a", "#2fd4a0", "#ffb020", "#3aa0ff"];
/// Logical height of the top bar that holds the window controls and the address field.
const BAR_HEIGHT: f64 = 38.0;

/// Margin between the page card and the window frame (the card floats above the tinted frame).
const CARD_GAP: f64 = 8.0;

/// The sidebar keeps its full width and slides out to the left as `width` shrinks to zero.
/// The view spans the rest of the window so its tint also paints the frame around the card.
fn chrome_rect(sidebar: &Sidebar, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    let x = sidebar.width() - sidebar.full();
    views::rect(
        x,
        BAR_HEIGHT,
        (window.width - x).max(1.0),
        (window.height - BAR_HEIGHT).max(1.0),
    )
}

/// Invisible view over the page while the sidebar is hidden. Page views swallow mouse events, so
/// it is the one that notices the cursor: a thin strip on the left edge reveals the sidebar, and
/// once the flyout is out it covers the rest of the page to hide it when the cursor leaves.
const HOTZONE_WIDTH: f64 = 12.0;

fn hotzone_rect(sidebar: &Sidebar, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    let (x, width) = if sidebar.flyout_shown() {
        let x = sidebar.full();
        (x, (window.width - x).max(1.0))
    } else {
        (0.0, HOTZONE_WIDTH)
    };
    views::rect(x, BAR_HEIGHT, width, (window.height - BAR_HEIGHT).max(1.0))
}

/// The floating sidebar: full width, sliding in from the left edge below the top bar.
fn flyout_rect(sidebar: &Sidebar, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    views::rect(
        sidebar.flyout_width() - sidebar.full(),
        BAR_HEIGHT,
        sidebar.full(),
        (window.height - BAR_HEIGHT).max(1.0),
    )
}

/// Where the page lives: a card inset from the sidebar and the window edges.
/// The gap on its left narrows while a sidebar is beside it.
fn card_rect(sidebar: &Sidebar, window: &winit::dpi::LogicalSize<f64>) -> (f64, f64, f64, f64) {
    let open = (sidebar.width() / sidebar.full()).clamp(0.0, 1.0);
    let x = sidebar.width() + CARD_GAP * (1.0 - open / 2.0);
    (
        x,
        BAR_HEIGHT,
        (window.width - x - CARD_GAP).max(1.0),
        (window.height - BAR_HEIGHT - CARD_GAP).max(1.0),
    )
}
const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(15);
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// Where the self-update stands; shown as a button at the bottom of the sidebar.
enum UpdateState {
    Idle,
    Available(Release),
    Downloading(Release),
    Failed(Release, String),
}

enum UpdateMsg {
    Found(Release),
    Downloaded(PathBuf),
    Failed(String),
}
const FRAME: Duration = Duration::from_millis(16);

/// Floating preview of a link (Arc's "Peek"): a dimming scrim plus the page itself.
struct Peek {
    scrim: WebView,
    page: WebView,
}

/// An extension's toolbar popup, shown under its button; a click anywhere else closes it.
#[cfg(windows)]
#[allow(dead_code)] // held only to keep the views alive
struct ExtPopup {
    scrim: WebView,
    page: WebView,
    /// Right edge of the popup: it grows leftwards from its toolbar button.
    right: f64,
    size: (f64, f64),
    probes_left: u8,
    /// After the first fit the popup only grows: shrinking toggles scrollbars and makes it flicker.
    fitted: bool,
    next_probe: Instant,
    measured: std::sync::mpsc::Receiver<String>,
    measure: std::sync::mpsc::Sender<String>,
}

/// Popup limits: it never gets narrower than the usual toolbar popup, nor larger than Chromium's maximum.
#[cfg(windows)]
const POPUP_MIN: (f64, f64) = (300.0, 120.0);
#[cfg(windows)]
const POPUP_MAX: (f64, f64) = (800.0, 600.0);

/// The settings window. The view is declared first so it is dropped before its window.
struct SettingsWindow {
    view: WebView,
    window: Window,
}

/// The Easel window, laid out like the settings window.
struct EaselWindow {
    view: WebView,
    window: Window,
}

/// A page snapshot read by `capture_to_easel`: address from the engine, the rest from the page.
struct Capture {
    url: String,
    page: String,
}

/// A download shown in the sidebar while it runs, and for a few seconds after it ends.
struct DownloadLive {
    name: String,
    path: PathBuf,
    received: u64,
    total: Option<u64>,
    finished: Option<(bool, Instant)>,
}

/// How long a finished download stays in the sidebar.
const DOWNLOAD_LINGER: Duration = Duration::from_secs(4);
const DOWNLOAD_POLL: Duration = Duration::from_millis(500);

/// Size of the file a download is writing: browsers use a temporary name until it completes.
fn partial_size(path: &std::path::Path) -> u64 {
    let size = |p: &std::path::Path| std::fs::metadata(p).map_or(0, |m| m.len());
    let with = |ext: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(ext);
        PathBuf::from(name)
    };
    size(path)
        .max(size(&with(".crdownload")))
        .max(size(&with(".partial")))
        .max(size(&with(".download")))
}

struct Running {
    chrome: WebView,
    /// Copy of the sidebar floating over the page while the real one is hidden.
    flyout: WebView,
    hotzone: WebView,
    bar: WebView,
    home: WebView,
    tabs: HashMap<TabId, WebView>,
    /// Blank hidden page views ready to take the next tabs (see `views::spare_tab_view`).
    spares: Vec<(views::TabSlot, WebView)>,
    spares_due: Option<Instant>,
    /// Logins offered by the last autofill query of each tab (passwords stay in the shell).
    autofill: HashMap<TabId, Vec<import::Login>>,
    last_active: HashMap<TabId, Instant>,
    /// Tab whose page was on screen at the last `sync` (drives automatic Picture in Picture).
    shown: Option<TabId>,
    peers_due: Instant,
    media: Arc<Mutex<HashMap<u64, MediaProbe>>>,
    media_due: Instant,
    media_seen: serde_json::Value,
    /// Tabs whose media card was closed; they come back when the tab plays again.
    media_dismissed: HashSet<u64>,
    svc: BrowserService<JsonWorkspaceRepository>,
    mailbox: Mailbox,
    sidebar: Sidebar,
    web_context: WebContext,
    blocked: BlockLog,
    /// Section requested for the settings window; `settings_seq` changes with each request.
    settings_tab: Option<String>,
    bookmarks: BookmarkStore,
    bookmark_list: Vec<import::Bookmark>,
    import_sources: Vec<import::Source>,
    import_msg: String,
    app_ext: Vec<blackhole_macos_webext::AppExtension>,
    bar_exts: serde_json::Value,
    bar_exts_at: Instant,
    vault_count: usize,
    settings_seq: u32,
    home_visible: bool,
    /// "What's new" shown on the start page until dismissed; empty when there is nothing to show.
    release_notes: Vec<serde_json::Value>,
    /// Command bar overlay; rebuilt whenever a view is added so it stays on top.
    splitbar: WebView,
    palette: WebView,
    /// Context menu being shown by the overlay (position and items).
    menu: Option<serde_json::Value>,
    palette_ready: bool,
    palette_open: bool,
    palette_mode: String,
    /// Space or folder whose emoji the picker is editing.
    icon_target: Option<(String, u64)>,
    peek: Option<Peek>,
    #[cfg(windows)]
    ext_popup: Option<ExtPopup>,
    data: std::path::PathBuf,
    /// Where installed extensions are read from: the profile itself, or, in a private window
    /// that allows extensions, the main profile.
    ext_src: std::path::PathBuf,
    favicons: favicons::Favicons,
    /// Native WebExtension host (macOS 15.4+); `None` where unsupported.
    webext: Option<blackhole_macos_webext::ExtensionHost>,
    installing: std::collections::HashSet<String>,
    /// Outdated extensions found by the last "Check for updates"; `None` until it is pressed.
    catalog_checked: Option<usize>,
    update: UpdateState,
    update_tx: std::sync::mpsc::Sender<UpdateMsg>,
    update_rx: std::sync::mpsc::Receiver<UpdateMsg>,
    next_update_check: Option<Instant>,
    quit: bool,
    maximized: bool,
    install_errors: HashMap<String, String>,
    install_tx: std::sync::mpsc::Sender<(String, Result<std::path::PathBuf, String>)>,
    install_rx: std::sync::mpsc::Receiver<(String, Result<std::path::PathBuf, String>)>,
    import_tx: std::sync::mpsc::Sender<ImportDone>,
    import_rx: std::sync::mpsc::Receiver<ImportDone>,
    webext_sig: String,
    settings: Option<SettingsWindow>,
    settings_wanted: bool,
    easels: Easels,
    easel_store: EaselStore,
    easel_current: Option<u64>,
    /// Live views of the current easel's playing cards (children of the easel window, so they
    /// must be dropped before it: declared first).
    live: HashMap<u64, WebView>,
    live_slots: HashMap<u64, ipc::LiveSlot>,
    /// Tab whose area picker is armed.
    live_pick: Option<TabId>,
    /// The Easel page shown in the content area, in place of a web page, while `easel_tab`.
    easel_panel: WebView,
    easel_tab: bool,
    /// The Library page, over the whole window below the bar, while `library`.
    library_panel: WebView,
    library: bool,
    downloads: Vec<blackhole_infrastructure::store::DownloadRec>,
    dl_live: HashMap<u64, DownloadLive>,
    dl_polled: Instant,
    dl_size_tx: std::sync::mpsc::Sender<(u64, u64)>,
    dl_size_rx: std::sync::mpsc::Receiver<(u64, u64)>,
    archive: Vec<blackhole_infrastructure::store::ArchivedTab>,
    thumbs: std::cell::RefCell<HashMap<u64, Option<String>>>,
    easel: Option<EaselWindow>,
    easel_wanted: bool,
    capture_tx: std::sync::mpsc::Sender<Capture>,
    capture_rx: std::sync::mpsc::Receiver<Capture>,
    #[cfg(target_os = "macos")]
    _menu: muda::Menu,
    #[cfg(feature = "devtools")]
    dev: Option<devtools::Dev>,
    #[cfg(windows)]
    backdrop: Option<backdrop::Backdrop>,
    // Declared last: web views are children of the window and must be dropped first.
    window: Rc<Window>,
}

/// The Edit menu is what routes Cmd+C/V/X/A/Z to the focused web view on macOS.
#[cfg(target_os = "macos")]
fn install_menu() -> muda::Menu {
    use muda::{Menu, PredefinedMenuItem as Item, Submenu};
    let menu = Menu::new();
    let app = Submenu::new("BlackHole", true);
    let edit = Submenu::new("Edit", true);
    let _ = app.append_items(&[
        &Item::about(None, None),
        &Item::separator(),
        &Item::hide(None),
        &Item::hide_others(None),
        &Item::separator(),
        &Item::quit(None),
    ]);
    let _ = edit.append_items(&[
        &Item::undo(None),
        &Item::redo(None),
        &Item::separator(),
        &Item::cut(None),
        &Item::copy(None),
        &Item::paste(None),
        &Item::select_all(None),
    ]);
    let _ = menu.append_items(&[&app, &edit]);
    menu.init_for_nsapp();
    menu
}

impl Running {
    fn new(event_loop: &ActiveEventLoop, mailbox: Mailbox) -> Self {
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(if private_mode() {
                "BlackHole (Private)"
            } else {
                "BlackHole"
            })
            .with_theme(Some(winit::window::Theme::Dark))
            .with_maximized(true)
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(720.0, 480.0));
        // The top bar draws its own minimize / maximize / close buttons.
        #[cfg(windows)]
        {
            use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
            attrs = attrs.with_decorations(false).with_undecorated_shadow(true);
            // Resource 1 is the icon embedded by build.rs.
            if let Ok(icon) = winit::window::Icon::from_resource(1, None) {
                attrs = attrs
                    .with_window_icon(Some(icon.clone()))
                    .with_taskbar_icon(Some(icon));
            }
        }
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true);
        }
        let window = Rc::new(event_loop.create_window(attrs).expect("window"));
        #[cfg(windows)]
        let backdrop = backdrop::Backdrop::new(&window);

        // Dev builds use a scratch profile so scripted runs never touch the user's data.
        #[cfg(feature = "devtools")]
        let profile = devtools::scratch().join("bh_data");
        #[cfg(not(feature = "devtools"))]
        let profile = blackhole_infrastructure::data_dir();
        let data = if private_mode() {
            private_data_dir()
        } else {
            profile.clone()
        };
        let mut web_context = WebContext::new(Some(data.join("webview")));
        let mut svc = BrowserService::start(
            JsonWorkspaceRepository::new(data.clone()),
            default_privacy_policy(),
        );
        // A private window has no extensions unless the main profile's settings allow them.
        let ext_src = if private_mode() {
            let shared = JsonWorkspaceRepository::new(profile.clone())
                .load_settings()
                .ok()
                .flatten()
                .filter(|s| s.extensions_in_private);
            match shared {
                Some(s) => {
                    for id in &s.disabled_extensions {
                        svc.set_enabled_flag(id, false);
                    }
                    Some(profile.clone())
                }
                None => None,
            }
        } else {
            Some(data.clone())
        };
        let ext_src_dir = ext_src.clone().unwrap_or_else(|| data.clone());
        if let Some(src) = &ext_src {
            svc.set_extensions(load_extensions(&extensions_dir(src)));
        }
        views::set_hardware_acceleration(svc.settings().hardware_acceleration);
        let sidebar = Sidebar::new(f64::from(svc.settings().sidebar_width));

        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        let chrome_bounds = chrome_rect(&sidebar, &size);
        let bar_bounds = views::rect(0.0, 0.0, size.width, BAR_HEIGHT);
        let (cx, cy, cw, ch) = card_rect(&sidebar, &size);
        let home_bounds = views::rect(cx, cy, cw, ch);
        let chrome = views::ui_view(
            &window,
            &mut web_context,
            CHROME_HTML,
            chrome_bounds,
            true,
            false,
            &mailbox,
        )
        .expect("sidebar view");
        let bar = views::ui_view(
            &window,
            &mut web_context,
            BAR_HTML,
            bar_bounds,
            true,
            false,
            &mailbox,
        )
        .expect("top bar view");
        let home = views::ui_view(
            &window,
            &mut web_context,
            HOME_HTML,
            home_bounds,
            true,
            false,
            &mailbox,
        )
        .expect("start page view");
        let easel_panel = views::ui_view(
            &window,
            &mut web_context,
            EASEL_HTML,
            home_bounds,
            false,
            false,
            &mailbox,
        )
        .expect("easel tab view");
        let library_panel = views::ui_view(
            &window,
            &mut web_context,
            LIBRARY_HTML,
            views::rect(
                0.0,
                BAR_HEIGHT,
                size.width,
                (size.height - BAR_HEIGHT).max(1.0),
            ),
            false,
            false,
            &mailbox,
        )
        .expect("library view");
        let splitbar = views::ui_view(
            &window,
            &mut web_context,
            SPLITBAR_HTML,
            views::rect(cx, cy, cw, SPLIT_HEADER),
            false,
            false,
            &mailbox,
        )
        .expect("split bar view");
        let hotzone = views::ui_view(
            &window,
            &mut web_context,
            HOTZONE_HTML,
            hotzone_rect(&sidebar, &size),
            !sidebar.is_open(),
            true,
            &mailbox,
        )
        .expect("sidebar hotzone view");
        let flyout = views::ui_view(
            &window,
            &mut web_context,
            CHROME_HTML,
            flyout_rect(&sidebar, &size),
            false,
            false,
            &mailbox,
        )
        .expect("sidebar flyout view");
        let palette = views::ui_view(
            &window,
            &mut web_context,
            PALETTE_HTML,
            views::rect(0.0, 0.0, size.width, size.height),
            false,
            true,
            &mailbox,
        )
        .expect("command bar view");

        let (install_tx, install_rx) = std::sync::mpsc::channel();
        let (update_tx, update_rx) = std::sync::mpsc::channel();
        let (import_tx, import_rx) = std::sync::mpsc::channel();
        let (dl_size_tx, dl_size_rx) = std::sync::mpsc::channel();
        let (capture_tx, capture_rx) = std::sync::mpsc::channel();
        let easel_store = EaselStore::new(&data);
        let easels = easel_store.load();
        let webext = blackhole_macos_webext::ExtensionHost::new();
        let app_ext = blackhole_macos_webext::discover_app_extensions();
        #[cfg(windows)]
        if let Some(host) = &webext {
            host.attach(&chrome);
        }
        if let (Some(host), Some(src)) = (&webext, &ext_src) {
            for e in app_ext.iter().filter(|e| app_extension_enabled(src, &e.id)) {
                host.load(&e.id, &e.path);
            }
            for entry in catalog::CATALOG {
                if catalog::is_installed(src, entry.id) && svc.extension_enabled(entry.id) {
                    host.load(entry.id, &catalog::package_path(src, entry.id));
                }
            }
        }

        // Bookmarks saved by older versions become folders and tabs once.
        let filed = data.join("bookmarks.filed");
        if !filed.exists() {
            file_bookmarks(&mut svc, BookmarkStore::new(&data).load());
            let _ = std::fs::write(&filed, b"1");
        }

        let release_notes: Vec<serde_json::Value> = svc
            .take_release_notes(env!("CARGO_PKG_VERSION"))
            .into_iter()
            .map(|(version, items)| json!({"version": version, "items": items}))
            .collect();

        Self {
            release_notes,
            chrome,
            flyout,
            hotzone,
            bar,
            home,
            tabs: HashMap::new(),
            spares: Vec::new(),
            spares_due: Some(Instant::now() + SPARES_DELAY),
            last_active: HashMap::new(),
            shown: None,
            peers_due: Instant::now(),
            media: Arc::default(),
            media_due: Instant::now(),
            media_seen: serde_json::Value::Null,
            media_dismissed: HashSet::new(),
            svc,
            mailbox,
            sidebar,
            web_context,
            blocked: BlockLog::default(),
            settings_tab: None,
            bookmark_list: BookmarkStore::new(&data).load(),
            bookmarks: BookmarkStore::new(&data),
            favicons: favicons::Favicons::load(&data),
            import_sources: import::detect(),
            import_msg: String::new(),
            app_ext,
            bar_exts: json!([]),
            bar_exts_at: Instant::now(),
            autofill: HashMap::new(),
            vault_count: 0,
            settings_seq: 0,
            home_visible: true,
            splitbar,
            palette,
            menu: None,
            palette_ready: false,
            palette_open: false,
            palette_mode: String::new(),
            icon_target: None,
            peek: None,
            #[cfg(windows)]
            ext_popup: None,
            webext,
            installing: Default::default(),
            catalog_checked: None,
            update: UpdateState::Idle,
            update_tx,
            update_rx,
            next_update_check: updates_enabled().then(|| Instant::now() + FIRST_UPDATE_CHECK),
            quit: false,
            maximized: false,
            install_errors: HashMap::new(),
            install_tx,
            install_rx,
            import_tx,
            import_rx,
            webext_sig: String::new(),
            downloads: blackhole_infrastructure::store::DownloadStore::new(&data).load(),
            dl_live: HashMap::new(),
            dl_polled: Instant::now(),
            dl_size_tx,
            dl_size_rx,
            archive: blackhole_infrastructure::store::ArchiveStore::new(&data).load(),
            data,
            ext_src: ext_src_dir,
            settings: None,
            settings_wanted: false,
            easel_current: easels.all().first().map(|e| e.id),
            easels,
            easel_store,
            live: HashMap::new(),
            live_slots: HashMap::new(),
            live_pick: None,
            easel_panel,
            easel_tab: false,
            library_panel,
            library: false,
            thumbs: std::cell::RefCell::default(),
            easel: None,
            easel_wanted: false,
            capture_tx,
            capture_rx,
            #[cfg(target_os = "macos")]
            _menu: install_menu(),
            #[cfg(feature = "devtools")]
            dev: devtools::Dev::load(),
            #[cfg(windows)]
            backdrop,
            window,
        }
    }

    fn scale(&self) -> f64 {
        self.window.scale_factor()
    }

    /// The start page is shown at launch, via the Home button, and whenever no tab is active.
    fn home_shown(&self) -> bool {
        self.library
            || self.easel_tab
            || self.home_visible
            || self.svc.workspace().active_tab().is_none()
    }

    fn active_view(&self) -> &WebView {
        if self.library {
            return &self.library_panel;
        }
        if self.easel_tab {
            return &self.easel_panel;
        }
        if self.home_shown() {
            return &self.home;
        }
        self.svc
            .workspace()
            .active_tab()
            .and_then(|t| self.tabs.get(&t.id))
            .unwrap_or(&self.home)
    }

    fn window_size(&self) -> winit::dpi::LogicalSize<f64> {
        self.window.inner_size().to_logical::<f64>(self.scale())
    }

    /// The pane of the active tab, and the second pane when a split view is showing.
    fn content_rects(&self) -> (wry::Rect, Option<wry::Rect>) {
        let size = self.window_size();
        let (offset, _, width, height) = card_rect(&self.sidebar, &size);
        if self.split_tab().is_none() {
            return (views::rect(offset, BAR_HEIGHT, width, height), None);
        }
        let half = ((width - SPLIT_GAP) / 2.0).max(1.0);
        let (top, height) = (BAR_HEIGHT + SPLIT_HEADER, (height - SPLIT_HEADER).max(1.0));
        (
            views::rect(offset, top, half, height),
            Some(views::rect(offset + half + SPLIT_GAP, top, half, height)),
        )
    }

    fn full_content_bounds(&self) -> wry::Rect {
        let (x, y, w, h) = card_rect(&self.sidebar, &self.window_size());
        views::rect(x, y, w, h)
    }

    fn peek_bounds(&self) -> wry::Rect {
        let size = self.window_size();
        views::rect(
            size.width * 0.1,
            size.height * 0.07,
            size.width * 0.8,
            size.height * 0.84,
        )
    }

    /// The tab shown beside the active one, if the split view is valid right now.
    fn split_tab(&self) -> Option<TabId> {
        if self.home_shown() {
            return None;
        }
        self.svc.workspace().split_tab()
    }

    fn view_of(&self, tab: TabId) -> Option<&WebView> {
        if tab == PEEK_TAB {
            return self.peek.as_ref().map(|p| &p.page);
        }
        self.tabs.get(&tab)
    }

    /// Positions the views; `all` also resizes background tabs (window resize, tab switch).
    fn layout(&self, all: bool) {
        let size = self.window_size();
        let _ = self
            .bar
            .set_bounds(views::rect(0.0, 0.0, size.width, BAR_HEIGHT));
        let _ = self.chrome.set_bounds(chrome_rect(&self.sidebar, &size));
        let _ = self.flyout.set_bounds(flyout_rect(&self.sidebar, &size));
        let _ = self
            .flyout
            .set_visible(self.sidebar.flyout_shown() && !self.library);
        let _ = self.hotzone.set_bounds(hotzone_rect(&self.sidebar, &size));
        let _ = self
            .hotzone
            .set_visible(!self.sidebar.is_open() && !self.library);
        let _ = self
            .palette
            .set_bounds(views::rect(0.0, 0.0, size.width, size.height));
        if let Some(peek) = &self.peek {
            let _ = peek
                .scrim
                .set_bounds(views::rect(0.0, 0.0, size.width, size.height));
            let _ = peek.page.set_bounds(self.peek_bounds());
        }
        if all {
            let _ = self.home.set_bounds(self.full_content_bounds());
            let _ = self.easel_panel.set_bounds(self.full_content_bounds());
            for view in self.tabs.values() {
                let _ = view.set_bounds(self.full_content_bounds());
            }
        }
        let (cx, cy, cw, _) = card_rect(&self.sidebar, &size);
        let _ = self
            .splitbar
            .set_bounds(views::rect(cx, cy, cw, SPLIT_HEADER));
        let (main, side) = self.content_rects();
        let _ = self.active_view().set_bounds(main);
        let _ = self.library_panel.set_bounds(views::rect(
            0.0,
            BAR_HEIGHT,
            size.width,
            (size.height - BAR_HEIGHT).max(1.0),
        ));
        for item in self.live.keys().copied().collect::<Vec<_>>() {
            self.apply_slot(item);
        }
        if let (Some(id), Some(rect)) = (self.split_tab(), side) {
            if let Some(view) = self.tabs.get(&id) {
                let _ = view.set_bounds(rect);
            }
        }
    }

    fn set_sidebar_width(&mut self, width: u32) {
        self.svc.set_sidebar_width(width);
        self.sidebar
            .set_full(f64::from(self.svc.settings().sidebar_width));
        self.layout(true);
    }

    /// The sidebar is either hidden or fully open, like Arc's.
    fn toggle_sidebar(&mut self) {
        self.sidebar.toggle();
    }

    fn downloads_json(&self) -> serde_json::Value {
        let mut live: Vec<_> = self.dl_live.iter().collect();
        live.sort_by_key(|(id, _)| **id);
        json!(live
            .into_iter()
            .map(|(id, d)| json!({
                "id": id, "name": d.name, "received": d.received, "total": d.total,
                "state": d.finished.map(|(ok, _)| if ok { "ok" } else { "bad" }),
            }))
            .collect::<Vec<_>>())
    }

    /// Refreshes the sidebar's download cards from the size of the files being written.
    fn poll_downloads(&mut self) {
        if self.dl_live.is_empty() {
            return;
        }
        let mut changed = false;
        while let Ok((id, total)) = self.dl_size_rx.try_recv() {
            if let Some(d) = self.dl_live.get_mut(&id) {
                d.total = Some(total);
                changed = true;
            }
        }
        if self.dl_polled.elapsed() >= DOWNLOAD_POLL {
            self.dl_polled = Instant::now();
            for d in self.dl_live.values_mut().filter(|d| d.finished.is_none()) {
                let size = partial_size(&d.path);
                if size != d.received {
                    d.received = size;
                    changed = true;
                }
            }
            let before = self.dl_live.len();
            self.dl_live.retain(|_, d| {
                d.finished
                    .is_none_or(|(_, at)| at.elapsed() < DOWNLOAD_LINGER)
            });
            changed |= before != self.dl_live.len();
        }
        if changed {
            let list = self.downloads_json();
            self.ui(format!("window.bh&&bh.downloads({list})"));
        }
    }

    fn tick_sidebar(&mut self) {
        if self.sidebar.moving() {
            self.sidebar.tick();
            self.layout(false);
        }
    }

    // ---- state → UI -------------------------------------------------------------------

    fn ui(&self, js: String) {
        let _ = self.chrome.evaluate_script(&js);
        let _ = self
            .flyout
            .evaluate_script(&format!("window.bh&&(bh.role='flyout');{js}"));
    }

    /// The space an easel shows in: its folder's, else the one it was created in.
    fn easel_space(&self, easel: &blackhole_domain::easel::Easel) -> u64 {
        let ws = self.svc.workspace();
        let first = ws.spaces().first().map_or(0, |s| s.id.0);
        let in_folder = easel.folder.and_then(|f| {
            ws.spaces()
                .iter()
                .find(|s| s.folders.iter().any(|x| x.id.0 == f))
        });
        in_folder.map_or_else(|| easel.space.unwrap_or(first), |s| s.id.0)
    }

    fn ui_state(&self) {
        let ws = self.svc.workspace();
        let space = ws.active_space();
        let state = json!({
            "os": std::env::consts::OS,
            "sidebarWidth": self.sidebar.full(),
            "sidebarOpen": self.sidebar.is_open(),
            "engine": self.svc.settings().search_engine.id(),
            "animations": self.svc.settings().animations,
            "tabSleep": self.svc.settings().tab_sleep_minutes,
            "tabSleepChoices": Settings::SLEEP_CHOICES,
            "engines": SearchEngine::ALL.iter().map(|e| json!({"id": e.id(), "label": e.label()})).collect::<Vec<_>>(),
            "spaces": ws.spaces().iter().map(|s| json!({"id": s.id.0, "name": s.name, "color": s.color, "icon": s.icon})).collect::<Vec<_>>(),
            "activeSpace": space.id.0,
            "color": space.color,
            "activeTab": if self.home_shown() { None } else { space.active_tab.map(|t| t.0) },
            "home": self.home_shown(),
            "blocked": self.blocked.total(),
            "version": env!("CARGO_PKG_VERSION"),
            "update": self.update_json(),
            "downloads": self.downloads_json(),
            "folders": space.folders.iter().map(|f| json!({"id": f.id.0, "name": f.name, "collapsed": f.collapsed, "icon": f.icon})).collect::<Vec<_>>(),
            "favicons": self.favicons.all(),
            "media": self.media_json(),
            "easels": self.easels.all().iter().filter(|e| self.easel_space(e) == space.id.0).map(|e| json!({"id": e.id, "name": e.name, "folder": e.folder, "count": e.items.len(), "open": self.easel_tab && self.easel_current == Some(e.id)})).collect::<Vec<_>>(),
            "tabs": space.tabs.iter().map(|t| json!({
                "id": t.id.0, "title": t.title, "url": t.url.as_str(), "pinned": t.pinned,
                "folder": t.folder.map(|f| f.0),
                "split": Some(t.id) == self.split_tab() || (self.split_tab().is_some() && Some(t.id) == space.active_tab),
                "asleep": !self.tabs.contains_key(&t.id),
            })).collect::<Vec<_>>(),
        });
        self.ui(format!("window.bh&&bh.render({state})"));
        self.ui_settings();
        self.ui_easel();
        self.ui_library();
        self.ui_bar();
        self.ui_splitbar();
        if self.home_shown() {
            self.ui_home();
        }
    }

    /// Titles and visibility of the strip above the split panes.
    fn ui_splitbar(&self) {
        let ws = self.svc.workspace();
        let (active, other) = (ws.active_tab(), self.split_tab());
        let pane = |t: &blackhole_domain::Tab| json!({"id": t.id.0, "title": t.title, "url": t.url.as_str()});
        let other = other.and_then(|id| ws.active_space().tabs.iter().find(|t| t.id == id));
        let _ = self
            .splitbar
            .set_visible(active.is_some() && other.is_some());
        if let (Some(left), Some(right)) = (active, other) {
            let state =
                json!({"left": pane(left), "right": pane(right), "color": ws.active_space().color});
            let _ = self
                .splitbar
                .evaluate_script(&format!("window.bh&&bh.render({state})"));
        }
    }

    fn ui_settings(&self) {
        let Some(win) = &self.settings else { return };
        let settings = self.svc.settings();
        let state = json!({
            "os": std::env::consts::OS,
            "version": env!("CARGO_PKG_VERSION"),
            "engine": settings.search_engine.id(),
            "engines": SearchEngine::ALL.iter().map(|e| json!({"id": e.id(), "label": e.label()})).collect::<Vec<_>>(),
            "sidebarWidth": settings.sidebar_width,
            "sidebarWidths": Settings::SIDEBAR_WIDTH_CHOICES,
            "animations": settings.animations,
            "hardwareAcceleration": settings.hardware_acceleration,
            "extensionsInPrivate": settings.extensions_in_private,
            "tabSleep": settings.tab_sleep_minutes,
            "tabSleepChoices": Settings::SLEEP_CHOICES,
            "blocked": self.blocked.total(),
            "blockedLog": self.blocked.entries().iter().take(100).map(|b| json!({"host": b.host, "count": b.count, "site": b.site})).collect::<Vec<_>>(),
            "tab": self.settings_tab.clone(),
            "tabSeq": self.settings_seq,
            "catalog": self.catalog_state(),
            "catalogChecked": self.catalog_checked,
            "appExtensions": self.app_ext.iter().map(|e| {
                let on = app_extension_enabled(&self.data, &e.id);
                let state = self.webext.as_ref().and_then(|h| h.state(&e.id));
                json!({
                    "id": e.id, "name": e.name, "enabled": on, "supported": self.webext.is_some(),
                    "status": match state {
                        Some(blackhole_macos_webext::LoadState::Loading) => "loading",
                        Some(blackhole_macos_webext::LoadState::Loaded) => "active",
                        Some(blackhole_macos_webext::LoadState::Failed(_)) => "failed",
                        None => "off",
                    },
                    "error": match state { Some(blackhole_macos_webext::LoadState::Failed(w)) => Some(w), _ => None },
                })
            }).collect::<Vec<_>>(),
            "import": json!({
                "sources": self.import_sources.iter().map(|s| json!({"id": s.id, "browser": s.browser, "profile": s.profile})).collect::<Vec<_>>(),
                "message": self.import_msg,
                "bookmarks": self.bookmarks.load().len(),
                "logins": self.vault_count,
            }),
            "extensionsDir": extensions_dir(&self.data).display().to_string(),
            "extensions": self.svc.extensions().iter().map(|e| json!({
                "id": e.id, "name": e.name, "version": e.version, "description": e.description,
                "enabled": self.svc.extension_enabled(&e.id),
                "sites": e.scripts.iter().flat_map(|s| s.matches.iter().cloned()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        let _ = win
            .view
            .evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    /// Imported bookmarks are ordinary tabs (drag & drop, close, folders) filed in folders.
    fn file_bookmarks(&mut self, list: Vec<import::Bookmark>) {
        file_bookmarks(&mut self.svc, list);
    }

    fn store_dropped(&mut self, list: Vec<import::Bookmark>) -> String {
        let space = self.svc.workspace().active_space().id.0;
        let (msg, list) = store_bookmarks(&self.bookmarks, list, "Glisser-déposer", space);
        self.bookmark_list = self.bookmarks.load();
        self.file_bookmarks(list);
        self.ui_state();
        self.ui(format!("window.bh&&bh.toast({})", json!(msg)));
        msg
    }

    /// Runs blocking import work (file dialog, Keychain) off the UI thread: modal panels
    /// opened inside the event-loop callback crash AppKit.
    fn spawn_import(
        &self,
        job: impl FnOnce(&BookmarkStore, &Vault) -> Option<(String, Vec<import::Bookmark>)>
            + Send
            + 'static,
    ) {
        let (tx, data) = (self.import_tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let (bookmarks, vault) = (BookmarkStore::new(&data), Vault::new(&data));
            let (msg, list) = match job(&bookmarks, &vault) {
                Some((m, l)) => (Some(m), l),
                None => (None, Vec::new()),
            };
            let _ = tx.send((msg, vault.count(), list));
        });
    }

    fn catalog_state(&self) -> Vec<serde_json::Value> {
        catalog::CATALOG
            .iter()
            .map(|e| {
                let installed = catalog::is_installed(&self.data, e.id);
                let loaded = self.webext.as_ref().and_then(|h| h.state(e.id));
                let status = if self.installing.contains(e.id) {
                    "installing"
                } else {
                    match loaded {
                        Some(blackhole_macos_webext::LoadState::Loading) => "loading",
                        Some(blackhole_macos_webext::LoadState::Loaded) => "active",
                        Some(blackhole_macos_webext::LoadState::Failed(_)) => "failed",
                        None if installed => "disabled",
                        None => "available",
                    }
                };
                let error = match self.webext.as_ref().and_then(|h| h.state(e.id)) {
                    Some(blackhole_macos_webext::LoadState::Failed(why)) => Some(why),
                    _ => self.install_errors.get(e.id).cloned(),
                };
                json!({
                    "id": e.id, "name": e.name, "description": e.description, "version": e.version,
                    "homepage": e.homepage, "installed": installed, "status": status, "error": error,
                    "installedVersion": catalog::installed_version(&self.data, e.id),
                    "outdated": catalog::needs_update(&self.data, e),
                    "enabled": self.svc.extension_enabled(e.id),
                    "supported": self.webext.is_some(),
                })
            })
            .collect()
    }

    /// Applies finished downloads and refreshes the settings window when a state changed.
    fn poll_webext(&mut self) {
        self.poll_update();
        self.poll_downloads();
        self.poll_captures();
        while let Ok((msg, count, list)) = self.import_rx.try_recv() {
            if let Some(m) = msg {
                self.import_msg = m;
            }
            self.vault_count = count;
            self.bookmark_list = self.bookmarks.load();
            self.file_bookmarks(list);
            self.ui_state();
        }
        while let Ok((id, result)) = self.install_rx.try_recv() {
            self.installing.remove(&id);
            match result {
                Ok(path) => {
                    self.install_errors.remove(&id);
                    self.svc.set_enabled_flag(&id, true);
                    if let Some(host) = &self.webext {
                        host.load(&id, &path);
                    }
                }
                Err(why) => {
                    self.install_errors.insert(id, why);
                }
            }
        }
        self.sync_extension_tab();
        if let Some(host) = &self.webext {
            if self.bar_exts_at.elapsed() > Duration::from_millis(800) {
                self.bar_exts_at = Instant::now();
                let list = json!(host
                    .actions()
                    .iter()
                    .filter(|a| a.enabled)
                    .map(|a| json!({
                        "id": a.id, "label": a.label, "badge": a.badge, "icon": a.icon,
                    }))
                    .collect::<Vec<_>>());
                if list != self.bar_exts {
                    self.bar_exts = list;
                    self.ui_bar();
                }
            }
        }
        let sig = serde_json::to_string(&json!([
            self.catalog_state(),
            self.app_ext
                .iter()
                .map(|e| self
                    .webext
                    .as_ref()
                    .and_then(|h| h.state(&e.id))
                    .map(|s| format!("{s:?}")))
                .collect::<Vec<_>>()
        ]))
        .unwrap_or_default();
        if sig != self.webext_sig {
            self.webext_sig = sig;
            self.ui_settings();
        }
    }

    /// Tells the extension host which tab is in front, so extensions see a real page.
    fn sync_extension_tab(&self) {
        let Some(host) = &self.webext else { return };
        let tab = if self.home_shown() {
            None
        } else {
            self.svc.workspace().active_tab()
        };
        let Some(tab) = tab else {
            host.set_active_tab(None, "", "");
            return;
        };
        let view = self.tabs.get(&tab.id).and_then(web_view_ptr);
        host.set_active_tab(view, tab.url.as_str(), &tab.title);
    }

    fn update_json(&self) -> serde_json::Value {
        match &self.update {
            UpdateState::Idle => json!(null),
            UpdateState::Available(r) => json!({"state": "available", "version": r.version}),
            UpdateState::Downloading(r) => json!({"state": "downloading", "version": r.version}),
            UpdateState::Failed(r, why) => {
                json!({"state": "error", "version": r.version, "error": why})
            }
        }
    }

    /// Runs the periodic release check and applies what the background threads report.
    fn poll_update(&mut self) {
        if let Some(at) = self.next_update_check {
            if Instant::now() >= at {
                self.next_update_check = Some(Instant::now() + UPDATE_CHECK_EVERY);
                let tx = self.update_tx.clone();
                std::thread::spawn(move || match updater::check(env!("CARGO_PKG_VERSION")) {
                    Ok(Some(release)) => {
                        let _ = tx.send(UpdateMsg::Found(release));
                    }
                    Ok(None) => {}
                    Err(why) => tracing::warn!("update check failed: {why}"),
                });
            }
        }
        while let Ok(msg) = self.update_rx.try_recv() {
            match (msg, std::mem::replace(&mut self.update, UpdateState::Idle)) {
                (UpdateMsg::Found(release), UpdateState::Idle | UpdateState::Available(_)) => {
                    self.update = UpdateState::Available(release);
                }
                (UpdateMsg::Downloaded(path), UpdateState::Downloading(release)) => {
                    match updater::apply(&path) {
                        Ok(()) => {
                            self.quit = true;
                            self.update = UpdateState::Downloading(release);
                        }
                        Err(why) => self.update = UpdateState::Failed(release, why),
                    }
                }
                (UpdateMsg::Failed(why), UpdateState::Downloading(release)) => {
                    self.update = UpdateState::Failed(release, why);
                }
                (_, state) => self.update = state,
            }
            self.ui_state();
        }
    }

    /// Sidebar button: downloads the new version, then the installer takes over and restarts.
    fn start_update(&mut self) {
        let release = match &self.update {
            UpdateState::Available(r) | UpdateState::Failed(r, _) => r.clone(),
            _ => return,
        };
        self.update = UpdateState::Downloading(release.clone());
        let (tx, data) = (self.update_tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let _ = tx.send(match updater::download(&data, &release) {
                Ok(path) => UpdateMsg::Downloaded(path),
                Err(why) => UpdateMsg::Failed(why),
            });
        });
        self.ui_state();
    }

    fn outdated_extensions(&self) -> Vec<&'static str> {
        catalog::CATALOG
            .iter()
            .filter(|e| catalog::needs_update(&self.data, e))
            .map(|e| e.id)
            .collect()
    }

    fn install_catalog(&mut self, id: &str) {
        let Some(entry) = catalog::find(id) else {
            return;
        };
        if self.webext.is_none() || !self.installing.insert(id.to_owned()) {
            return;
        }
        self.install_errors.remove(id);
        let (tx, data) = (self.install_tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let _ = tx.send((entry.id.to_owned(), catalog::install(&data, entry)));
        });
    }

    /// Opens (or focuses) the settings window; called from the event loop, which creates windows.
    fn open_settings(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(win) = &self.settings {
            win.window.set_minimized(false);
            win.window.focus_window();
            return;
        }
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title("BlackHole Settings")
            .with_theme(Some(winit::window::Theme::Dark))
            .with_inner_size(LogicalSize::new(820.0, 600.0))
            .with_min_inner_size(LogicalSize::new(640.0, 440.0));
        // Owned by the main window so it always stays above it.
        #[cfg(windows)]
        {
            use winit::{
                platform::windows::WindowAttributesExtWindows,
                raw_window_handle::{HasWindowHandle, RawWindowHandle},
            };
            if let Ok(handle) = self.window.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    attrs = attrs.with_owner_window(h.hwnd.get());
                }
            }
        }
        let Ok(window) = event_loop.create_window(attrs) else {
            return;
        };
        window.focus_window();
        match views::settings_view(&window, &mut self.web_context, SETTINGS_HTML, &self.mailbox) {
            Ok(view) => self.settings = Some(SettingsWindow { view, window }),
            Err(err) => tracing::error!("cannot create settings window: {err}"),
        }
    }

    fn close_settings(&mut self) {
        self.settings = None;
    }

    fn settings_window_id(&self) -> Option<WindowId> {
        self.settings.as_ref().map(|w| w.window.id())
    }

    // ---- easels ---------------------------------------------------------------------------

    /// Opens (or focuses) the Easel window; called from the event loop, which creates windows.
    fn open_easel(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(win) = &self.easel {
            win.window.set_minimized(false);
            win.window.focus_window();
            return;
        }
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title("BlackHole Easel")
            .with_theme(Some(winit::window::Theme::Dark))
            .with_inner_size(LogicalSize::new(1100.0, 720.0))
            .with_min_inner_size(LogicalSize::new(640.0, 440.0));
        #[cfg(windows)]
        {
            use winit::{
                platform::windows::WindowAttributesExtWindows,
                raw_window_handle::{HasWindowHandle, RawWindowHandle},
            };
            if let Ok(handle) = self.window.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    attrs = attrs.with_owner_window(h.hwnd.get());
                }
            }
        }
        let Ok(window) = event_loop.create_window(attrs) else {
            return;
        };
        window.focus_window();
        match views::panel_view(&window, &mut self.web_context, EASEL_HTML, &self.mailbox) {
            Ok(view) => self.easel = Some(EaselWindow { view, window }),
            Err(err) => tracing::error!("cannot create easel window: {err}"),
        }
    }

    fn close_easel(&mut self) {
        if !self.easel_tab {
            self.live.clear();
            self.live_slots.clear();
        }
        self.easel = None;
        self.ui_state();
    }

    /// Shows an easel in the content area like a web page, with its own controls.
    fn open_easel_tab(&mut self, id: u64) {
        if self.easels.get(id).is_none() {
            return;
        }
        self.close_palette();
        self.close_peek();
        if !self.easel_tab || self.easel_current != Some(id) {
            self.live.clear();
            self.live_slots.clear();
        }
        self.easel_current = Some(id);
        self.easel_tab = true;
        self.library = false;
        self.sync();
    }

    fn close_easel_tab(&mut self) {
        if !self.easel_tab {
            return;
        }
        self.live.clear();
        self.live_slots.clear();
        self.easel_tab = false;
        self.library = false;
        self.sync();
    }

    /// Keeps the easel page filling its window; live views follow `EaselLayout`.
    fn easel_resized(&self) {
        if let Some(win) = &self.easel {
            let size = win
                .window
                .inner_size()
                .to_logical::<f64>(win.window.scale_factor());
            let _ = win
                .view
                .set_bounds(views::rect(0.0, 0.0, size.width, size.height));
        }
    }

    /// Drops the live views whose card is gone (removed, other easel, no longer live).
    fn sync_live(&mut self) {
        let current = self.easel_current.and_then(|id| self.easels.get(id));
        let keep: Vec<u64> = current
            .map(|e| {
                e.items
                    .iter()
                    .filter(|i| i.kind == blackhole_domain::EaselItemKind::Live)
                    .map(|i| i.id)
                    .collect()
            })
            .unwrap_or_default();
        self.live.retain(|id, _| keep.contains(id));
        self.live_slots.retain(|id, _| keep.contains(id));
    }

    /// The card's page, cropped to its region, in the card's slot of the easel window.
    fn live_play(&mut self, item: u64) {
        if self.live.contains_key(&item) {
            return;
        }
        let card = self
            .easel_current
            .and_then(|id| self.easels.get(id))
            .and_then(|e| e.items.iter().find(|i| i.id == item))
            .filter(|i| i.kind == blackhole_domain::EaselItemKind::Live)
            .and_then(|i| Some((Url::parse(i.url.as_deref()?).ok()?, i.selector.clone())));
        let host = if self.easel_tab {
            Some(&*self.window)
        } else {
            self.easel.as_ref().map(|w| &w.window)
        };
        let (Some((url, selector)), Some(host)) = (card, host) else {
            return;
        };
        match views::live_view(
            host,
            &mut self.web_context,
            &url,
            &selector,
            self.svc.policy(),
            &self.mailbox,
        ) {
            Ok(view) => {
                self.live.insert(item, view);
                self.apply_slot(item);
            }
            Err(err) => {
                tracing::error!("cannot create live view: {err}");
                self.toast("Could not start the live view");
            }
        }
        self.ui_easel();
    }

    fn apply_slot(&self, item: u64) {
        let (Some(view), Some(slot)) = (self.live.get(&item), self.live_slots.get(&item)) else {
            return;
        };
        // Slots are relative to the Easel page: the content area when it is a tab.
        let (ox, oy) = if self.easel_tab {
            let (x, y, _, _) = card_rect(&self.sidebar, &self.window_size());
            (x, y)
        } else {
            (0.0, 0.0)
        };
        let _ = view.set_bounds(views::rect(ox + slot.x, oy + slot.y, slot.w, slot.h));
        let covered = self.palette_open || self.peek.is_some();
        let _ = view.set_visible(slot.visible && !covered && slot.w >= 2.0 && slot.h >= 2.0);
    }

    /// Arms the picker on the page in front.
    fn live_pick_start(&mut self) {
        let tab = self.svc.workspace().active_tab().map(|t| t.id);
        let (Some(tab), false) = (tab, self.home_shown()) else {
            self.toast("Open a web page to capture an area");
            return;
        };
        let Some(view) = self.tabs.get(&tab) else {
            return;
        };
        if view.evaluate_script(views::LIVE_PICKER_JS).is_ok() {
            self.live_pick = Some(tab);
        }
    }

    /// Adds a live card for `selector` of the page of `tab`; the address comes from the engine.
    fn capture_live(&mut self, tab: TabId, selector: &str, size: (f64, f64)) {
        let page = self.tabs.get(&tab).and_then(|v| v.url().ok());
        let title = self
            .svc
            .workspace()
            .active_tab()
            .filter(|t| t.id == tab)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let (Some(url), Some(easel)) = (page, self.easel_or_create()) else {
            self.toast("This page cannot be captured");
            return;
        };
        match self
            .easels
            .capture_live(easel, &url, &title, selector, size)
        {
            Ok(_) => {
                self.easel_changed(true);
                self.toast("Live area added to Easel");
            }
            Err(why) => self.toast(why),
        }
    }

    fn easel_window_id(&self) -> Option<WindowId> {
        self.easel.as_ref().map(|w| w.window.id())
    }

    /// The easel being edited; falls back to the first one when the selection is gone.
    fn current_easel(&mut self) -> Option<u64> {
        if !self
            .easel_current
            .is_some_and(|id| self.easels.get(id).is_some())
        {
            self.easel_current = self.easels.all().first().map(|e| e.id);
        }
        self.easel_current
    }

    fn tag_easel_space(&mut self) {
        if let Some(id) = self.easel_current {
            let space = self.svc.workspace().active_space().id.0;
            self.easels.set_space(id, Some(space));
        }
    }

    /// The current easel, created on first use so a capture never fails for lack of a board.
    fn easel_or_create(&mut self) -> Option<u64> {
        if self.current_easel().is_none() {
            self.easel_current = self.easels.create("");
            self.tag_easel_space();
        }
        self.easel_current
    }

    /// Everything the Library page shows; it picks the section and filters client-side.
    fn ui_library(&self) {
        if !self.library {
            return;
        }
        let ws = self.svc.workspace();
        let active = ws.active_space();
        let exists = |d: &blackhole_infrastructure::store::DownloadRec| {
            std::path::Path::new(&d.path).exists()
        };
        let downloads: Vec<_> = self.downloads.iter().rev().collect();
        let mut thumbs = self.thumbs.borrow_mut();
        let mut budget = 60;
        let media: Vec<_> = downloads
            .iter()
            .filter(|d| d.done && d.ok && blackhole_infrastructure::media_kind(&d.name).is_some())
            .filter(|d| exists(d))
            .map(|d| {
                let kind = blackhole_infrastructure::media_kind(&d.name);
                let thumb = if kind == Some("image") && budget > 0 {
                    budget -= 1;
                    thumbs
                        .entry(d.id)
                        .or_insert_with(|| {
                            blackhole_infrastructure::image_data_url(
                                std::path::Path::new(&d.path),
                                1_500_000,
                            )
                        })
                        .clone()
                } else {
                    None
                };
                json!({"id": d.id, "name": d.name, "kind": kind, "thumb": thumb, "at": d.at})
            })
            .collect();
        let state = json!({
            "accent": active.color,
            "media": media,
            "downloads": downloads.iter().map(|d| json!({
                "id": d.id, "name": d.name, "url": d.url, "at": d.at,
                "done": d.done, "ok": d.ok, "exists": exists(d),
            })).collect::<Vec<_>>(),
            "easels": self.easels.all().iter().filter(|e| self.easel_space(e) == active.id.0).map(|e| json!({
                "id": e.id, "name": e.name, "count": e.items.len(),
                "preview": e.items.iter().map(|i| i.text.as_str()).find(|t| !t.is_empty()).unwrap_or(""),
            })).collect::<Vec<_>>(),
            "spaces": ws.spaces().iter().map(|sp| json!({
                "id": sp.id.0, "name": sp.name, "color": sp.color, "icon": sp.icon,
                "active": sp.id == active.id,
                "folders": sp.folders.iter().map(|f| json!({"id": f.id.0, "name": f.name})).collect::<Vec<_>>(),
                "tabs": sp.tabs.iter().map(|t| json!({
                    "id": t.id.0, "title": t.title, "url": t.url.as_str(),
                    "folder": t.folder.map(|f| f.0), "pinned": t.pinned,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "archive": self.archive.iter().enumerate().rev().map(|(i, a)| json!({
                "index": i, "title": a.title, "url": a.url, "at": a.at,
            })).collect::<Vec<_>>(),
            "now": now_secs(),
        });
        let _ = self
            .library_panel
            .evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    fn open_library(&mut self) {
        self.close_palette();
        self.close_peek();
        self.close_easel_tab();
        self.library = true;
        self.sync();
        self.ui_library();
        let _ = self.library_panel.evaluate_script("window.bh&&bh.intro()");
    }

    fn close_library(&mut self) {
        if self.library {
            self.library = false;
            self.sync();
        }
    }

    fn save_downloads(&self) {
        let _ =
            blackhole_infrastructure::store::DownloadStore::new(&self.data).save(&self.downloads);
    }

    fn save_archive(&self) {
        let _ = blackhole_infrastructure::store::ArchiveStore::new(&self.data).save(&self.archive);
    }

    /// Remembers a tab that is about to be closed so the Library can bring it back.
    fn archive_tab(&mut self, id: TabId) {
        let ws = self.svc.workspace();
        let space = ws.active_space();
        let Some(tab) = space.tabs.iter().find(|t| t.id == id) else {
            return;
        };
        if !matches!(tab.url.scheme(), "http" | "https") {
            return;
        }
        let rec = blackhole_infrastructure::store::ArchivedTab {
            title: tab.title.clone(),
            url: tab.url.to_string(),
            space: space.id.0,
            at: now_secs(),
        };
        self.archive
            .retain(|a| !(a.url == rec.url && a.space == rec.space));
        self.archive.push(rec);
        self.save_archive();
    }

    fn ui_easel(&self) {
        let active = self.svc.workspace().active_space().id.0;
        let state = |embedded: bool| {
            json!({
                "embedded": embedded,
                "current": self.easel_current,
                "easels": self.easels.all().iter().filter(|e| self.easel_space(e) == active).map(|e| json!({
                    "id": e.id,
                    "name": e.name,
                    "items": e.items.iter().map(|i| json!({
                        "id": i.id, "kind": i.kind.id(), "x": i.x, "y": i.y, "w": i.w, "h": i.h,
                        "text": i.text, "title": i.title, "url": i.url, "color": i.color,
                        "selector": i.selector, "playing": self.live.contains_key(&i.id),
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "colors": blackhole_domain::easel::COLORS,
            })
        };
        if let Some(win) = &self.easel {
            let _ = win
                .view
                .evaluate_script(&format!("window.bh&&bh.render({})", state(false)));
        }
        if self.easel_tab {
            let _ = self
                .easel_panel
                .evaluate_script(&format!("window.bh&&bh.render({})", state(true)));
        }
    }

    /// Persists after a change and refreshes the window; `false` changes nothing.
    fn easel_changed(&mut self, changed: bool) {
        if changed {
            self.easel_persist();
            self.sync_live();
            if self.easel_tab && self.current_easel().is_none() {
                self.close_easel_tab();
            }
            self.ui_state();
        }
    }

    fn easel_persist(&mut self) {
        if let Err(e) = self.easel_store.save(&self.easels) {
            tracing::error!("cannot save easels: {e}");
            self.toast("Could not save your easels");
        }
    }

    /// Reads the page in front. The address comes from the engine; the title and the selection
    /// come from the page and are only ever stored as text.
    fn capture_to_easel(&mut self) {
        let Some(view) = (if self.home_visible {
            None
        } else {
            self.svc
                .workspace()
                .active_tab()
                .and_then(|t| self.tabs.get(&t.id))
        }) else {
            self.toast("Open a web page to capture it");
            return;
        };
        let Ok(url) = view.url() else { return };
        let tx = self.capture_tx.clone();
        let result = view.evaluate_script_with_callback(
            "({title:document.title,text:String(getSelection()||'').slice(0,2000)})",
            move |page| {
                let _ = tx.send(Capture {
                    url: url.clone(),
                    page,
                });
            },
        );
        if result.is_err() {
            self.toast("This page cannot be captured");
        }
    }

    fn poll_captures(&mut self) {
        while let Ok(cap) = self.capture_rx.try_recv() {
            let page: serde_json::Value = serde_json::from_str(&cap.page).unwrap_or_default();
            let text = |key| page.get(key).and_then(|v| v.as_str()).unwrap_or_default();
            let Some(easel) = self.easel_or_create() else {
                self.toast("Too many easels");
                continue;
            };
            match self
                .easels
                .capture(easel, &cap.url, text("title"), text("text"))
            {
                Ok(_) => {
                    let name = self
                        .easels
                        .get(easel)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    self.easel_changed(true);
                    self.toast(&format!("Captured to {name}"));
                }
                Err(why) => self.toast(why),
            }
        }
    }

    fn handle_easel(&mut self, command: UiCommand) {
        match command {
            UiCommand::OpenEasel { id } => {
                if let Some(id) = id.filter(|id| self.easels.get(*id).is_some()) {
                    self.easel_current = Some(id);
                }
                self.easel_or_create();
                self.easel_changed(true);
                self.easel_wanted = true;
            }
            UiCommand::CloseEasel => self.close_easel(),
            UiCommand::OpenEaselTab { id } => self.open_easel_tab(id),
            UiCommand::CloseEaselTab => self.close_easel_tab(),
            UiCommand::OpenLibrary => self.open_library(),
            UiCommand::CloseLibrary => self.close_library(),
            UiCommand::LibraryOpenFile { id } => {
                if let Some(d) = self.downloads.iter().find(|d| d.id == id) {
                    open_download(std::path::Path::new(&d.path));
                }
            }
            UiCommand::LibraryDeleteDownload { id } => {
                self.downloads.retain(|d| d.id != id);
                self.thumbs.borrow_mut().remove(&id);
                self.save_downloads();
                self.ui_library();
            }
            UiCommand::LibraryClear { kind } => {
                match kind.as_str() {
                    "downloads" => {
                        self.downloads.clear();
                        self.thumbs.borrow_mut().clear();
                        self.save_downloads();
                    }
                    "archive" => {
                        self.archive.clear();
                        self.save_archive();
                    }
                    _ => {}
                }
                self.ui_library();
            }
            UiCommand::ReopenArchived { index } => {
                if index < self.archive.len() {
                    let rec = self.archive.remove(index);
                    self.save_archive();
                    self.open(&rec.url);
                }
            }
            UiCommand::DeleteArchived { index } => {
                if index < self.archive.len() {
                    self.archive.remove(index);
                    self.save_archive();
                    self.ui_library();
                }
            }
            UiCommand::LibraryOpenTab { space, id } => {
                self.svc.switch_space(SpaceId(space));
                self.svc.activate_tab(TabId(id));
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.sync();
            }
            UiCommand::ClearTabs => {
                let loose: Vec<TabId> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| !t.pinned && t.folder.is_none())
                    .map(|t| t.id)
                    .collect();
                for id in loose {
                    self.archive_tab(id);
                    self.svc.close_tab(id);
                    self.tabs.remove(&id);
                }
                self.sync();
            }
            UiCommand::NewEasel { name } => {
                match self.easels.create(&name) {
                    Some(id) => {
                        self.easel_current = Some(id);
                        self.tag_easel_space();
                    }
                    None => self.toast("Too many easels"),
                }
                self.easel_changed(true);
            }
            UiCommand::SelectEasel { id } => {
                if self.easels.get(id).is_some() {
                    self.easel_current = Some(id);
                    self.sync_live();
                    self.ui_state();
                }
            }
            UiCommand::RenameEasel { id, name } => {
                let ok = self.easels.rename(id, &name);
                self.easel_changed(ok);
            }
            UiCommand::MoveEasel { id, folder } => {
                let known = folder.is_none()
                    || folder.is_some_and(|f| {
                        self.svc
                            .workspace()
                            .spaces()
                            .iter()
                            .any(|s| s.folders.iter().any(|x| x.id.0 == f))
                    });
                let ok = known && self.easels.set_folder(id, folder);
                self.easel_changed(ok);
            }
            UiCommand::DeleteEasel { id } => {
                let ok = self.easels.delete(id);
                self.easel_changed(ok);
            }
            UiCommand::CaptureToEasel => self.capture_to_easel(),
            UiCommand::LivePick => self.live_pick_start(),
            UiCommand::CaptureLive { selector, w, h } => {
                let tab = self.svc.workspace().active_tab().map(|t| t.id);
                match tab {
                    Some(tab) if !self.home_shown() => self.capture_live(tab, &selector, (w, h)),
                    _ => self.toast("Open a web page to capture an area"),
                }
            }
            UiCommand::EaselLivePlay { item } => self.live_play(item),
            UiCommand::EaselLiveStop { item } => {
                self.live.remove(&item);
                self.ui_easel();
            }
            UiCommand::EaselLayout { slots, embedded } if embedded == self.easel_tab => {
                self.live_slots = slots.into_iter().map(|s| (s.item, s)).collect();
                for item in self.live.keys().copied().collect::<Vec<_>>() {
                    self.apply_slot(item);
                }
            }
            UiCommand::EaselLayout { .. } => {}
            UiCommand::EaselAddNote { x, y } => {
                if let Some(easel) = self.easel_or_create() {
                    let at = x.zip(y);
                    let added = self.easels.add_note(easel, at).is_some();
                    self.easel_changed(added);
                }
            }
            UiCommand::EaselOpenLink { item } => {
                let url = self
                    .current_easel()
                    .and_then(|id| self.easels.get(id))
                    .and_then(|e| e.items.iter().find(|i| i.id == item))
                    .and_then(|i| i.url.clone());
                if let Some(url) = url {
                    self.open(&url);
                    self.toast("Opened in a new tab");
                }
            }
            other => {
                let Some(easel) = self.current_easel() else {
                    return;
                };
                // Text, moves and resizes are already on screen: re-rendering would steal the
                // caret or interrupt a drag.
                let silent = match &other {
                    UiCommand::EaselSetText { item, text } => {
                        self.easels.set_text(easel, *item, text)
                    }
                    UiCommand::EaselMove { item, x, y } => {
                        self.easels.move_item(easel, *item, *x, *y)
                    }
                    UiCommand::EaselResize { item, w, h } => {
                        self.easels.resize_item(easel, *item, *w, *h)
                    }
                    _ => false,
                };
                if silent {
                    self.easel_persist();
                    return;
                }
                let changed = match other {
                    UiCommand::EaselColor { item, color } => {
                        self.easels.set_color(easel, item, &color)
                    }
                    UiCommand::EaselRemove { item } => self.easels.remove_item(easel, item),
                    _ => false,
                };
                self.easel_changed(changed);
            }
        }
    }

    fn reload_extensions(&mut self) {
        self.svc
            .set_extensions(load_extensions(&extensions_dir(&self.ext_src)));
        self.reapply_extensions();
        self.ui_state();
    }

    /// Injects the enabled extensions into every open page (also clears the CSS of disabled ones).
    fn reapply_extensions(&self) {
        for tab in self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
        {
            if let Some(view) = self.tabs.get(&tab.id) {
                let injection = self.svc.injection_for(&tab.url);
                views::apply_injection(view, &injection.css, &injection.js);
            }
        }
    }

    fn ui_bar(&self) {
        let ws = self.svc.workspace();
        let url = if self.home_shown() {
            String::new()
        } else {
            ws.active_tab()
                .map(|t| t.url.to_string())
                .unwrap_or_default()
        };
        let state = json!({
            "os": std::env::consts::OS,
            "maximized": self.window.is_maximized(),
            "private": private_mode(),
            "url": url,
            "color": ws.active_space().color,
            "engine": self.svc.settings().search_engine.label(),
            "extensions": self.bar_exts,
        });
        let _ = self
            .bar
            .evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    fn ui_home(&self) {
        let state = json!({
            "engine": self.svc.settings().search_engine.label(),
            "animations": self.svc.settings().animations,
            "favicons": self.favicons.all(),
            "favorites": self.svc.favorites().iter()
                .map(|f| json!({
                    "name": f.name, "url": f.url.as_str(),
                    // Only tabs of the current space can be unpinned from here.
                    "tab": f.tab.filter(|id| self.svc.workspace().active_space().tabs.iter().any(|t| t.id == *id)).map(|id| id.0),
                }))
                .collect::<Vec<_>>(),
            "recent": self.svc.recent_activity(12).iter()
                .map(|e| json!({"title": e.title, "url": e.url.as_str(), "at": e.visited_at}))
                .collect::<Vec<_>>(),
            "releaseNotes": self.release_notes,
        });
        let _ = self
            .home
            .evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    fn toast(&self, message: &str) {
        self.ui(format!("window.bh&&bh.toast({})", json!(message)));
    }

    // ---- tabs -------------------------------------------------------------------------

    fn ensure_active_view(&mut self) {
        if self.home_shown() {
            return;
        }
        let Some(tab) = self.svc.workspace().active_tab().cloned() else {
            return;
        };
        self.ensure_view(&tab);
        let split = self.split_tab().and_then(|id| {
            self.svc
                .workspace()
                .active_space()
                .tabs
                .iter()
                .find(|t| t.id == id)
                .cloned()
        });
        if let Some(tab) = split {
            self.ensure_view(&tab);
        }
    }

    fn ensure_view(&mut self, tab: &blackhole_domain::Tab) {
        if self.tabs.contains_key(&tab.id) {
            return;
        }
        let bounds = self.full_content_bounds();
        if let Some((slot, view)) = self.spares.pop() {
            slot.store(tab.id.0, std::sync::atomic::Ordering::Relaxed);
            let _ = view.set_bounds(bounds);
            let _ = view.load_url(tab.url.as_str());
            self.tabs.insert(tab.id, view);
            self.spares_due = Some(Instant::now() + SPARES_DELAY);
            return;
        }
        match views::tab_view(
            &self.window,
            &mut self.web_context,
            tab.id,
            &tab.url,
            bounds,
            self.svc.policy(),
            &self.mailbox,
            self.webext.as_ref(),
        ) {
            Ok(view) => {
                self.tabs.insert(tab.id, view);
                self.raise_palette();
                self.spares_due = Some(Instant::now() + SPARES_DELAY);
            }
            Err(error) => tracing::error!("cannot create web view: {error}"),
        }
    }

    /// Builds the spare views while nothing is going on. They all sit below the command bar,
    /// which is recreated once on top of them, so the next tabs open without any view creation.
    fn refill_spares(&mut self) {
        let Some(due) = self.spares_due else { return };
        if Instant::now() < due {
            return;
        }
        if self.palette_open || self.peek.is_some() || !self.spares.is_empty() {
            self.spares_due = Some(Instant::now() + SPARES_DELAY);
            return;
        }
        self.spares_due = None;
        for _ in 0..SPARES {
            match views::spare_tab_view(
                &self.window,
                &mut self.web_context,
                self.svc.policy(),
                &self.mailbox,
                self.webext.as_ref(),
            ) {
                Ok(spare) => self.spares.push(spare),
                Err(error) => {
                    tracing::error!("cannot create spare view: {error}");
                    break;
                }
            }
        }
        self.raise_palette();
        self.sync();
    }

    /// Native views stack in creation order, so the command bar is recreated above new tabs.
    fn raise_palette(&mut self) {
        let size = self.window_size();
        match views::ui_view(
            &self.window,
            &mut self.web_context,
            HOTZONE_HTML,
            hotzone_rect(&self.sidebar, &size),
            !self.sidebar.is_open(),
            true,
            &self.mailbox,
        ) {
            Ok(view) => self.hotzone = view,
            Err(error) => tracing::error!("cannot create sidebar hotzone: {error}"),
        }
        match views::ui_view(
            &self.window,
            &mut self.web_context,
            CHROME_HTML,
            flyout_rect(&self.sidebar, &size),
            self.sidebar.flyout_shown(),
            false,
            &self.mailbox,
        ) {
            Ok(view) => self.flyout = view,
            Err(error) => tracing::error!("cannot create sidebar flyout: {error}"),
        }
        let bounds = views::rect(0.0, 0.0, size.width, size.height);
        match views::ui_view(
            &self.window,
            &mut self.web_context,
            PALETTE_HTML,
            bounds,
            false,
            true,
            &self.mailbox,
        ) {
            Ok(view) => {
                self.palette = view;
                self.palette_ready = false;
                self.palette_open = false;
            }
            Err(error) => tracing::error!("cannot create command bar: {error}"),
        }
    }

    /// Audio/video state of the active space's tabs, for the speaker icon and the media card.
    fn media_json(&self) -> serde_json::Value {
        let Ok(probes) = self.media.lock() else {
            return json!([]);
        };
        let tabs = &self.svc.workspace().active_space().tabs;
        let list: Vec<_> = tabs
            .iter()
            .filter_map(|t| {
                let p = probes.get(&t.id.0)?;
                let shown = (p.playing || p.has) && !self.media_dismissed.contains(&t.id.0);
                shown.then(|| json!({"id": t.id.0, "playing": p.playing, "muted": p.muted}))
            })
            .collect();
        json!(list)
    }

    /// Asks every loaded tab about its media, and refreshes the sidebar when something changed.
    fn service_media(&mut self) {
        if Instant::now() >= self.media_due {
            self.media_due = Instant::now() + MEDIA_EVERY;
            if let Ok(mut probes) = self.media.lock() {
                probes.retain(|id, _| self.tabs.contains_key(&TabId(*id)));
            }
            for (id, view) in &self.tabs {
                let sink = Arc::clone(&self.media);
                let id = id.0;
                let _ = view.evaluate_script_with_callback(views::MEDIA_PROBE_JS, move |raw| {
                    if let (Ok(probe), Ok(mut probes)) =
                        (serde_json::from_str::<MediaProbe>(&raw), sink.lock())
                    {
                        probes.insert(id, probe);
                    }
                });
            }
        }
        if let Ok(probes) = self.media.lock() {
            self.media_dismissed
                .retain(|id| !probes.get(id).is_some_and(|p| p.playing));
        }
        let now = self.media_json();
        if now != self.media_seen {
            self.media_seen = now;
            self.ui_state();
        }
    }

    fn media_ctl(&mut self, id: TabId, action: &str) {
        let Some(view) = self.tabs.get(&id) else {
            return;
        };
        let js = match action {
            "toggle" => views::MEDIA_TOGGLE_JS,
            "mute" => views::MEDIA_MUTE_JS,
            "pip" => views::PIP_TOGGLE_JS,
            "close" => {
                self.media_dismissed.insert(id.0);
                views::MEDIA_PAUSE_JS
            }
            _ => return,
        };
        let _ = view.evaluate_script(js);
        self.media_due = Instant::now() + Duration::from_millis(250);
        if action == "close" {
            self.ui_state();
        }
    }

    /// This window's rectangle on the screen, in logical pixels.
    fn screen_rect(&self) -> Option<(f64, f64, f64, f64)> {
        let scale = self.scale();
        let pos = self.window.outer_position().ok()?.to_logical::<f64>(scale);
        let size = self.window.outer_size().to_logical::<f64>(scale);
        Some((pos.x, pos.y, size.width, size.height))
    }

    /// Keeps this window listed for the others and picks up tabs they sent here.
    fn service_peers(&mut self) {
        if Instant::now() < self.peers_due {
            return;
        }
        self.peers_due = Instant::now() + PEERS_EVERY;
        let dir = peers_dir();
        if let Some((x, y, w, h)) = self.screen_rect() {
            peers::publish(
                &dir,
                &peers::Peer {
                    pid: std::process::id(),
                    private: private_mode(),
                    x,
                    y,
                    w,
                    h,
                    updated: 0,
                },
            );
        }
        let incoming = peers::receive(&dir, std::process::id());
        if incoming.is_empty() {
            return;
        }
        for transfer in incoming {
            self.open(&transfer.url);
        }
        self.window.focus_window();
    }

    /// A tab released outside its sidebar: hands it to the window under the pointer, if any.
    fn drop_tab_out(&mut self, id: TabId, x: f64, y: f64) {
        if self
            .screen_rect()
            .is_some_and(|(wx, wy, w, h)| x >= wx && x < wx + w && y >= wy && y < wy + h)
        {
            return;
        }
        let dir = peers_dir();
        let others = peers::peers(&dir, std::process::id());
        let target = peers::at(&others, x, y).map(|p| p.pid);
        let Some(tab) = self
            .svc
            .workspace()
            .active_space()
            .tabs
            .iter()
            .find(|t| t.id == id)
        else {
            return;
        };
        let transfer = peers::Transfer {
            url: tab.url.to_string(),
            title: tab.title.clone(),
        };
        let moved = match target {
            Some(pid) => peers::send(&dir, pid, &transfer),
            // Released over nothing: the tab becomes a window of its own (same privacy).
            None => open_window(private_mode(), Some(&transfer.url)),
        };
        if moved {
            self.handle_ui(UiCommand::Close { id: id.0 });
        }
    }

    /// Frees the engine view of tabs left in the background; they reload when selected again.
    fn discard_idle_tabs(&mut self) {
        let now = Instant::now();
        if !self.home_shown() {
            let active = self.svc.workspace().active_tab().map(|t| t.id);
            for id in active.into_iter().chain(self.split_tab()) {
                self.last_active.insert(id, now);
            }
        }
        let limit = Duration::from_secs(u64::from(self.svc.settings().tab_sleep_minutes) * 60);
        let idle: Vec<TabId> = self
            .tabs
            .keys()
            .filter(|id| {
                let seen = self.last_active.get(id).copied().unwrap_or(now);
                now.duration_since(seen) >= limit
            })
            .copied()
            .collect();
        if idle.is_empty() {
            return;
        }
        for id in idle {
            self.tabs.remove(&id);
            self.last_active.remove(&id);
        }
        self.ui_state();
    }

    fn sync(&mut self) {
        self.ensure_active_view();
        let active = if self.home_shown() {
            None
        } else {
            self.svc.workspace().active_tab().map(|t| t.id)
        };
        if let Some(id) = active {
            self.last_active.insert(id, Instant::now());
        }
        if self.shown != active {
            if let Some(left) = self.shown.and_then(|id| self.tabs.get(&id)) {
                let _ = left.evaluate_script(views::PIP_ENTER_JS);
            }
            if let Some(back) = active.and_then(|id| self.tabs.get(&id)) {
                let _ = back.evaluate_script(views::PIP_EXIT_JS);
            }
            self.shown = active;
        }
        let _ = self.easel_panel.set_visible(self.easel_tab);
        if !self.library {
            // Keep the page blank while hidden so the next opening fades in without a flash.
            let _ = self.library_panel.evaluate_script("window.bh&&bh.prep()");
        }
        let _ = self.library_panel.set_visible(self.library);
        let split = self.split_tab();
        for (id, view) in &self.tabs {
            let _ = view.set_visible(Some(*id) == active || Some(*id) == split);
        }
        let _ = self
            .home
            .set_visible(active.is_none() && !self.easel_tab && !self.library);
        self.layout(true);
        self.apply_focus();
        self.ui_state();
    }

    fn apply_focus(&self) {
        let _ = if self.palette_open {
            self.palette.focus()
        } else if let Some(peek) = &self.peek {
            peek.page.focus()
        } else {
            self.active_view().focus()
        };
    }

    fn focus_content(&mut self) {
        self.close_palette();
        self.apply_focus();
    }

    fn open(&mut self, input: &str) {
        self.close_palette();
        self.close_peek();
        match self.svc.resolve(input) {
            NavigationOutcome::Blocked(why) => self.toast(why),
            NavigationOutcome::Go(_) => {
                self.svc.open_tab(input);
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.sync();
            }
        }
    }

    fn navigate(&mut self, input: &str) {
        self.close_palette();
        let active = if self.home_shown() {
            None
        } else {
            // A pinned tab is immutable: a new search opens another tab.
            self.svc
                .workspace()
                .active_tab()
                .filter(|t| !t.pinned)
                .map(|t| t.id)
        };
        match (active, self.svc.resolve(input)) {
            (Some(id), NavigationOutcome::Go(url)) => {
                if let Some(view) = self.tabs.get(&id) {
                    let _ = view.load_url(url.as_str());
                }
                self.focus_content();
            }
            (None, _) => self.open(input),
            (_, NavigationOutcome::Blocked(why)) => self.toast(why),
        }
    }

    fn close_active_tab(&mut self) {
        if self.easel_tab {
            self.close_easel_tab();
            return;
        }
        if self.home_shown() {
            return;
        }
        if let Some(id) = self.svc.workspace().active_tab().map(|t| t.id) {
            self.archive_tab(id);
            self.svc.close_tab(id);
            self.tabs.remove(&id);
            self.sync();
        }
    }

    /// A new tab shows the start page behind the search bar, which opens straight away.
    fn new_tab(&mut self) {
        self.close_palette();
        self.close_peek();
        self.home_visible = true;
        self.easel_tab = false;
        self.library = false;
        self.sync();
        self.open_palette("new");
    }

    // ---- command bar --------------------------------------------------------------------

    fn open_palette(&mut self, mode: &str) {
        self.close_peek();
        self.palette_mode = mode.to_owned();
        self.palette_open = true;
        let _ = self.palette.set_visible(true);
        let _ = self.palette.focus();
        if self.palette_ready {
            self.push_palette();
        }
    }

    fn push_palette(&self) {
        let state = self.palette_state();
        let _ = self
            .palette
            .evaluate_script(&format!("window.bh&&bh.open({state})"));
    }

    fn close_palette(&mut self) {
        if !self.palette_open {
            return;
        }
        self.palette_open = false;
        let _ = self.palette.evaluate_script("window.bh&&bh.hide()");
        let _ = self.palette.set_visible(false);
        self.apply_focus();
    }

    fn active_site(&self) -> Option<String> {
        if self.home_shown() {
            return None;
        }
        self.svc
            .workspace()
            .active_tab()?
            .url
            .host_str()
            .map(str::to_owned)
    }

    fn palette_state(&self) -> serde_json::Value {
        let ws = self.svc.workspace();
        let space = ws.active_space();
        let active = if self.home_shown() {
            None
        } else {
            ws.active_tab()
        };
        let site = self.active_site();
        let easel_actions: Vec<_> = self
            .easels
            .all()
            .iter()
            .map(|e| json!({"id": format!("easel:{}", e.id), "label": format!("Easel: {}", e.name), "hint": "Easel"}))
            .collect();
        let mut actions = vec![
            json!({"id": "new_space", "label": "New Space"}),
            json!({"id": "new_folder", "label": "New Folder"}),
            json!({"id": "icon_space", "label": format!("Set emoji for space {}", space.name), "hint": "Emoji"}),
            json!({"id": "toggle_sidebar", "label": "Toggle sidebar mode", "hint": "⌘B"}),
            json!({"id": "new_easel", "label": "New Easel", "hint": "Easel"}),
            json!({"id": "open_easel", "label": "Open Easel", "hint": "⌘⇧E"}),
            json!({"id": "open_library", "label": "Open Library", "hint": "Media, downloads, archive"}),
            json!({"id": "settings", "label": "Open Settings"}),
            json!({"id": "default_browser", "label": "Make BlackHole the default browser", "hint": "System"}),
            json!({"id": "clear_history", "label": "Clear browsing history"}),
        ];
        actions.extend(easel_actions);
        for folder in &space.folders {
            actions.push(json!({
                "id": format!("icon_folder:{}", folder.id.0),
                "label": format!("Set emoji for folder {}", folder.name),
                "hint": "Emoji",
            }));
        }
        if self.split_tab().is_some() {
            actions
                .push(json!({"id": "close_split", "label": "Close Split View", "hint": "Split"}));
        }
        if let Some(current) = active {
            for tab in space.tabs.iter().filter(|t| t.id != current.id) {
                let name = if tab.title.is_empty() {
                    tab.url.as_str()
                } else {
                    &tab.title
                };
                actions.push(json!({
                    "id": format!("split:{}", tab.id.0),
                    "label": format!("Split View with {name}"),
                    "hint": "Split",
                }));
            }
        }
        if active.is_some() {
            actions.push(json!({"id": "pip", "label": "Picture in Picture", "hint": "Video"}));
            actions.push(
                json!({"id": "capture_easel", "label": "Capture this page to Easel", "hint": "⌘⇧S"}),
            );
            actions.push(
                json!({"id": "capture_live", "label": "Live capture: select an area for Easel", "hint": "⌘⇧L"}),
            );
        }
        if let Some(site) = &site {
            actions.push(json!({"id": "boost", "label": format!("Boost {site}"), "hint": "Boost"}));
            let routed = self.svc.route_for(site);
            for other in ws.spaces().iter().filter(|s| Some(s.id) != routed) {
                actions.push(json!({
                    "id": format!("route:{}", other.id.0),
                    "label": format!("Always open {site} in {}", other.name),
                    "hint": "Air Traffic",
                }));
            }
            if routed.is_some() {
                actions.push(json!({"id": "route_clear", "label": format!("Stop routing {site}"), "hint": "Air Traffic"}));
            }
        }
        let url = match (self.palette_mode.as_str(), active) {
            ("url", Some(tab)) => tab.url.to_string(),
            _ => String::new(),
        };
        let target = self.icon_target.as_ref().and_then(|(kind, id)| {
            let (name, icon) = if kind == "space" {
                let s = ws.spaces().iter().find(|s| s.id.0 == *id)?;
                (&s.name, &s.icon)
            } else {
                let f = space.folders.iter().find(|f| f.id.0 == *id)?;
                (&f.name, &f.icon)
            };
            Some(json!({"kind": kind, "id": id, "name": name, "icon": icon}))
        });
        json!({
            "target": target,
            "mode": self.palette_mode,
            "animations": self.svc.settings().animations,
            "url": url,
            "engine": self.svc.settings().search_engine.label(),
            "color": space.color,
            "menu": self.menu,
            "boostHost": site,
            "boost": site.as_deref().and_then(|h| self.svc.boost_for(h)),
            "tabs": space.tabs.iter().map(|t| json!({"id": t.id.0, "title": t.title, "url": t.url.as_str()})).collect::<Vec<_>>(),
            "favorites": self.svc.favorites().iter().map(|f| json!({"name": f.name, "url": f.url.as_str()})).collect::<Vec<_>>(),
            "bookmarks": self.bookmarks.load().iter().map(|b| json!({"title": b.title, "url": b.url})).collect::<Vec<_>>(),
            "recent": self.svc.recent_activity(30).iter().map(|e| json!({"title": e.title, "url": e.url.as_str()})).collect::<Vec<_>>(),
            "actions": actions,
        })
    }

    fn run_action(&mut self, id: &str) {
        let site = self.active_site();
        if id == "boost" {
            self.palette_mode = "boost".to_owned();
            self.push_palette();
            return;
        }
        self.close_palette();
        match id {
            "new_space" => {
                let n = self.svc.workspace().spaces().len();
                self.handle_ui(UiCommand::NewSpace {
                    name: format!("Space {}", n + 1),
                    color: SPACE_COLORS[n % SPACE_COLORS.len()].to_owned(),
                });
            }
            "new_folder" => self.handle_ui(UiCommand::NewFolder),
            "pip" => {
                let _ = self.active_view().evaluate_script(views::PIP_TOGGLE_JS);
            }
            "toggle_sidebar" => self.handle_ui(UiCommand::Lock),
            "settings" => self.settings_wanted = true,
            "new_easel" => {
                self.handle_ui(UiCommand::NewEasel {
                    name: String::new(),
                });
                if let Some(id) = self.easel_current {
                    self.open_easel_tab(id);
                }
            }
            "open_easel" => self.handle_ui(UiCommand::OpenEasel { id: None }),
            "open_library" => self.handle_ui(UiCommand::OpenLibrary),
            other if other.starts_with("easel:") => {
                if let Ok(id) = other["easel:".len()..].parse() {
                    self.handle_ui(UiCommand::OpenEaselTab { id });
                }
            }
            "capture_easel" => self.handle_ui(UiCommand::CaptureToEasel),
            "capture_live" => self.handle_ui(UiCommand::LivePick),
            "clear_history" => {
                self.svc.clear_history();
                self.toast("History cleared");
            }
            "default_browser" => {
                let message = if blackhole_macos_url::set_default_browser() {
                    "Confirm in the system dialog to make BlackHole your default browser"
                } else {
                    "Install the BlackHole.app bundle first (scripts/package-macos.sh)"
                };
                self.toast(message);
            }
            "icon_space" => {
                let id = self.svc.workspace().active_space().id.0;
                self.handle_ui(UiCommand::PickIcon {
                    kind: "space".to_owned(),
                    id,
                });
            }
            "close_split" => {
                self.svc.clear_split();
                self.sync();
            }
            "route_clear" => {
                if let Some(site) = site {
                    self.svc.set_route(&site, None);
                    self.toast(&format!("{site} is no longer routed"));
                }
            }
            other => {
                if let Some(n) = other
                    .strip_prefix("icon_folder:")
                    .and_then(|n| n.parse().ok())
                {
                    self.handle_ui(UiCommand::PickIcon {
                        kind: "folder".to_owned(),
                        id: n,
                    });
                } else if let Some(n) = other.strip_prefix("split:").and_then(|n| n.parse().ok()) {
                    self.svc.set_split(TabId(n));
                    self.sync();
                } else if let (Some(n), Some(site)) = (
                    other.strip_prefix("route:").and_then(|n| n.parse().ok()),
                    site,
                ) {
                    self.svc.set_route(&site, Some(SpaceId(n)));
                    self.toast(&format!("{site} will open in its own space"));
                }
            }
        }
    }

    fn save_boost(&mut self, css: &str) {
        let Some(site) = self.active_site() else {
            return;
        };
        self.svc.set_boost(&site, css);
        self.close_palette();
        let key = blackhole_domain::site_key(&site);
        let matching: Vec<TabId> = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .filter(|t| {
                t.url
                    .host_str()
                    .is_some_and(|h| blackhole_domain::site_key(h) == key)
            })
            .map(|t| t.id)
            .collect();
        for id in matching {
            if let Some(view) = self.tabs.get(&id) {
                views::apply_boost(view, css);
            }
        }
    }

    // ---- peek ---------------------------------------------------------------------------

    fn open_peek(&mut self, url: &Url) {
        let NavigationOutcome::Go(url) = self.svc.evaluate(url) else {
            return;
        };
        self.close_palette();
        self.close_peek();
        let size = self.window_size();
        let scrim_bounds = views::rect(0.0, 0.0, size.width, size.height);
        let scrim = views::ui_view(
            &self.window,
            &mut self.web_context,
            PEEK_HTML,
            scrim_bounds,
            true,
            true,
            &self.mailbox,
        );
        let peek_bounds = self.peek_bounds();
        let page = views::tab_view(
            &self.window,
            &mut self.web_context,
            PEEK_TAB,
            &url,
            peek_bounds,
            self.svc.policy(),
            &self.mailbox,
            self.webext.as_ref(),
        );
        match (scrim, page) {
            (Ok(scrim), Ok(page)) => {
                let _ = page.set_visible(true);
                let _ = page.focus();
                self.peek = Some(Peek { scrim, page });
            }
            (scrim, page) => {
                tracing::error!("cannot create peek: {:?} {:?}", scrim.err(), page.err());
            }
        }
    }

    fn close_peek(&mut self) {
        if self.peek.take().is_some() {
            self.apply_focus();
        }
    }

    /// Shows an extension's native popover under `rect`; false when it has none to show.
    #[cfg(target_os = "macos")]
    fn show_native_popup(&self, id: &str, rect: (f64, f64, f64, f64)) -> bool {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let (Some(host), Ok(handle)) = (&self.webext, self.window.window_handle()) else {
            return false;
        };
        match handle.as_raw() {
            RawWindowHandle::AppKit(view) => host.show_popup(id, view.ns_view, rect),
            _ => false,
        }
    }

    /// Opens the popup of an extension under its toolbar button (`rect` is the button).
    #[cfg(windows)]
    fn open_ext_popup(&mut self, url: &str, rect: (f64, f64, f64, f64)) {
        let was_open = self.ext_popup.is_some();
        self.close_ext_popup();
        if !was_open {
            self.show_ext_popup(url, rect);
        }
    }

    #[cfg(windows)]
    fn show_ext_popup(&mut self, url: &str, rect: (f64, f64, f64, f64)) {
        let size = self.window_size();
        let (width, height) = (
            380.0_f64.min(size.width - 16.0),
            600.0_f64.min(size.height - BAR_HEIGHT - 16.0),
        );
        let x = (rect.0 + rect.2 - width).clamp(8.0, (size.width - width - 8.0).max(8.0));
        let scrim = views::ui_view(
            &self.window,
            &mut self.web_context,
            EXT_POPUP_HTML,
            views::rect(0.0, 0.0, size.width, size.height),
            true,
            true,
            &self.mailbox,
        );
        let page = views::extension_popup(
            &self.window,
            &mut self.web_context,
            url,
            views::rect(x, BAR_HEIGHT - 4.0, width, height),
            &self.mailbox,
        );
        let (measure, measured) = std::sync::mpsc::channel();
        match (scrim, page) {
            (Ok(scrim), Ok(page)) => {
                let _ = page.focus();
                self.ext_popup = Some(ExtPopup {
                    scrim,
                    page,
                    right: x + width,
                    size: (width, height),
                    probes_left: 8,
                    fitted: false,
                    next_probe: Instant::now() + Duration::from_millis(300),
                    measured,
                    measure,
                });
            }
            (scrim, page) => {
                tracing::error!(
                    "cannot open extension popup: {:?} {:?}",
                    scrim.err(),
                    page.err()
                );
            }
        }
    }

    /// Asks the popup how big its content is and applies the answers that came back.
    #[cfg(windows)]
    fn probe_ext_popup(&mut self) {
        let Some(popup) = &mut self.ext_popup else {
            return;
        };
        let answers: Vec<String> = popup.measured.try_iter().collect();
        for raw in answers {
            self.fit_ext_popup(&raw);
        }
        let Some(popup) = &mut self.ext_popup else {
            return;
        };
        if popup.probes_left == 0 || Instant::now() < popup.next_probe {
            return;
        }
        popup.probes_left -= 1;
        popup.next_probe = Instant::now() + Duration::from_millis(250);
        let measure = popup.measure.clone();
        let _ = popup.page.evaluate_script_with_callback(
            "(()=>{const b=document.body,d=document.documentElement;if(!b)return '';const w=Math.max(b.scrollWidth,d.scrollWidth>innerWidth?d.scrollWidth:0),h=Math.max(b.scrollHeight,d.scrollHeight>innerHeight?d.scrollHeight:0);return Math.ceil(w+innerWidth-d.clientWidth)+'x'+Math.ceil(h+innerHeight-d.clientHeight)})()",
            move |raw| {
                let _ = measure.send(raw);
            },
        );
    }

    /// Resizes the popup to its content (within the limits), keeping its right edge.
    #[cfg(windows)]
    fn fit_ext_popup(&mut self, raw: &str) {
        let max_h = (self.window_size().height - BAR_HEIGHT - 16.0).min(POPUP_MAX.1);
        let Some(popup) = &mut self.ext_popup else {
            return;
        };
        let Some((w, h)) = raw.trim_matches('"').split_once('x') else {
            return;
        };
        let (Ok(w), Ok(h)) = (w.parse::<f64>(), h.parse::<f64>()) else {
            return;
        };
        let w = w.clamp(POPUP_MIN.0, POPUP_MAX.0);
        let h = h.clamp(POPUP_MIN.1, max_h.max(POPUP_MIN.1));
        let (w, h) = if popup.fitted {
            (w.max(popup.size.0), h.max(popup.size.1))
        } else {
            (w, h)
        };
        if (w - popup.size.0).abs() < 4.0 && (h - popup.size.1).abs() < 4.0 {
            return;
        }
        popup.fitted = true;
        popup.size = (w, h);
        let x = (popup.right - w).max(8.0);
        let _ = popup
            .page
            .set_bounds(views::rect(x, BAR_HEIGHT - 4.0, w, h));
    }

    #[cfg(windows)]
    fn close_ext_popup(&mut self) {
        if self.ext_popup.take().is_some() {
            self.apply_focus();
        }
    }

    fn peek_to_tab(&mut self) {
        let url = self.peek.as_ref().and_then(|p| p.page.url().ok());
        self.close_peek();
        if let Some(url) = url {
            self.open(&url);
        }
    }

    fn with_active_tab(&self, f: impl FnOnce(&WebView)) {
        if self.home_shown() {
            return;
        }
        if let Some(view) = self
            .svc
            .workspace()
            .active_tab()
            .and_then(|t| self.tabs.get(&t.id))
        {
            f(view);
        }
    }

    fn handle_ui(&mut self, command: UiCommand) {
        match command {
            UiCommand::Ready => self.sync(),
            UiCommand::Open { input } => self.open(&input),
            UiCommand::Home => {
                self.close_palette();
                self.close_peek();
                self.home_visible = true;
                self.easel_tab = false;
                self.library = false;
                self.sync();
            }
            UiCommand::NewTab => self.new_tab(),
            UiCommand::Palette { mode } => {
                if matches!(mode.as_str(), "url" | "new") {
                    self.open_palette(&mode);
                }
            }
            UiCommand::PaletteReady => {
                self.palette_ready = true;
                if self.palette_open {
                    self.push_palette();
                }
            }
            UiCommand::PaletteClose => self.close_palette(),
            UiCommand::Action { id } => self.run_action(&id),
            UiCommand::SaveBoost { css } => self.save_boost(&css),
            UiCommand::ClosePeek => self.close_peek(),
            UiCommand::SetTabSleep { minutes } => {
                self.svc.set_tab_sleep_minutes(minutes);
                self.ui_state();
            }
            UiCommand::SetHardwareAcceleration { on } => {
                self.svc.set_hardware_acceleration(on);
                self.ui_state();
            }
            UiCommand::SetExtensionsInPrivate { on } => {
                self.svc.set_extensions_in_private(on);
                self.ui_state();
            }
            UiCommand::SetAnimations { on } => {
                self.svc.set_animations(on);
                self.ui_state();
            }
            UiCommand::ClearHistory => {
                self.svc.clear_history();
                self.ui_state();
            }
            UiCommand::Navigate { input } => self.navigate(&input),
            UiCommand::Activate { id } => {
                self.close_palette();
                self.close_peek();
                self.svc.activate_tab(TabId(id));
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.sync();
            }
            UiCommand::Close { id } => {
                self.archive_tab(TabId(id));
                self.svc.close_tab(TabId(id));
                self.tabs.remove(&TabId(id));
                self.sync();
            }
            UiCommand::Split { id } => {
                if self.svc.set_split(TabId(id)) {
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                }
                self.sync();
            }
            UiCommand::Menu {
                origin,
                x,
                y,
                items,
            } => {
                let dx = if origin == "home" {
                    card_rect(&self.sidebar, &self.window_size()).0
                } else {
                    0.0
                };
                self.menu = Some(json!({"x": x + dx, "y": y + BAR_HEIGHT, "items": items}));
                self.open_palette("menu");
            }
            UiCommand::DeleteSpace { id } => {
                if let Some(tabs) = self.svc.delete_space(SpaceId(id)) {
                    for tab in tabs {
                        self.tabs.remove(&tab);
                    }
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                    self.sync();
                }
            }
            UiCommand::ClosePane { side } => {
                if side == "left" {
                    if let Some(other) = self.split_tab() {
                        self.svc.activate_tab(other);
                    }
                }
                self.svc.clear_split();
                self.sync();
            }
            UiCommand::PinSite { url } => {
                self.open(&url);
                if let Some(id) = self.svc.workspace().active_tab().map(|t| t.id) {
                    self.svc.toggle_pin(id);
                }
                self.sync();
            }
            UiCommand::DismissReleaseNotes => {
                self.release_notes.clear();
                self.ui_home();
            }
            UiCommand::HideFavorite { url } => {
                if let Ok(url) = Url::parse(&url) {
                    self.svc.hide_favorite(&url);
                    self.ui_home();
                }
            }
            UiCommand::CloseSplit => {
                self.svc.clear_split();
                self.sync();
            }
            UiCommand::PickIcon { kind, id } => {
                self.icon_target = Some((kind, id));
                self.open_palette("emoji");
            }
            UiCommand::SetSpaceIcon { id, icon } => {
                self.svc.set_space_icon(SpaceId(id), &icon);
                self.ui_state();
            }
            UiCommand::SetFolderIcon { id, icon } => {
                self.svc.set_folder_icon(FolderId(id), &icon);
                self.ui_state();
            }
            UiCommand::Pin { id } => {
                self.svc.toggle_pin(TabId(id));
                self.ui_state();
            }
            UiCommand::OpenAt {
                url,
                before,
                folder,
                pinned,
            } => {
                self.open(&url);
                if let Some(id) = self.svc.workspace().active_tab().map(|t| t.id) {
                    self.svc
                        .move_tab(id, before.map(TabId), folder.map(FolderId), pinned);
                }
                self.sync();
            }
            UiCommand::MoveTab {
                id,
                before,
                folder,
                pinned,
            } => {
                self.svc
                    .move_tab(TabId(id), before.map(TabId), folder.map(FolderId), pinned);
                self.ui_state();
            }
            UiCommand::TabDropOut { id, x, y } => self.drop_tab_out(TabId(id), x, y),
            UiCommand::MediaCtl { id, action } => self.media_ctl(TabId(id), &action),
            UiCommand::NewFolder => {
                self.svc.add_folder("New folder");
                self.ui_state();
            }
            UiCommand::RenameFolder { id, name } => {
                self.svc.rename_folder(FolderId(id), &name);
                self.ui_state();
            }
            UiCommand::RenameSpace { id, name } => {
                self.svc.rename_space(SpaceId(id), &name);
                self.ui_state();
            }
            UiCommand::EditSpace { id } => self.ui(format!("window.bh&&bh.editSpace({id})")),
            UiCommand::ToggleFolder { id } => {
                self.svc.toggle_folder(FolderId(id));
                self.ui_state();
            }
            UiCommand::MoveFolder { id, before } => {
                self.svc.move_folder(FolderId(id), before.map(FolderId));
                self.ui_state();
            }
            UiCommand::DeleteFolder { id } => {
                // The folder takes its tabs with it (they go to the archive like any closed tab).
                let inside: Vec<TabId> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| t.folder == Some(FolderId(id)))
                    .map(|t| t.id)
                    .collect();
                for tab in inside {
                    self.archive_tab(tab);
                    self.svc.close_tab(tab);
                    self.tabs.remove(&tab);
                }
                self.svc.delete_folder(FolderId(id));
                self.sync();
            }
            UiCommand::Space { id } => {
                self.svc.switch_space(SpaceId(id));
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.sync();
            }
            UiCommand::MoveSpace { id, before } => {
                self.svc.move_space(SpaceId(id), before.map(SpaceId));
                self.ui_state();
            }
            UiCommand::NewSpace { name, color } => {
                let name = if name.trim().is_empty() {
                    "Space"
                } else {
                    name.as_str()
                };
                self.svc.add_space(name, &color);
                self.sync();
            }
            UiCommand::SetEngine { id } => {
                if let Some(engine) = SearchEngine::from_id(&id) {
                    self.svc.set_search_engine(engine);
                    self.ui_state();
                }
            }
            UiCommand::Lock => self.toggle_sidebar(),
            UiCommand::Peek { on } => {
                self.sidebar.peek(on);
                self.layout(false);
            }
            UiCommand::SetSidebarWidth { width } => self.set_sidebar_width(width),
            UiCommand::FocusContent => self.focus_content(),
            UiCommand::DragWindow => {
                let _ = self.window.drag_window();
            }
            UiCommand::WindowMinimize => self.window.set_minimized(true),
            UiCommand::WindowMaximize => self.window.set_maximized(!self.window.is_maximized()),
            UiCommand::WindowClose => self.quit = true,
            UiCommand::Shortcut { key } => self.shortcut(&key),
            UiCommand::Back => self.with_active_tab(views::go_back),
            UiCommand::Forward => self.with_active_tab(views::go_forward),
            UiCommand::Reload => self.with_active_tab(views::reload),
            UiCommand::OpenSettings { tab } => {
                self.settings_tab = tab;
                self.settings_seq += 1;
                self.settings_wanted = true;
            }
            UiCommand::ClearBlocked => {
                self.blocked.clear();
                self.ui_state();
            }
            UiCommand::CloseSettings => self.close_settings(),
            command @ (UiCommand::OpenEasel { .. }
            | UiCommand::CloseEasel
            | UiCommand::OpenEaselTab { .. }
            | UiCommand::CloseEaselTab
            | UiCommand::NewEasel { .. }
            | UiCommand::SelectEasel { .. }
            | UiCommand::RenameEasel { .. }
            | UiCommand::DeleteEasel { .. }
            | UiCommand::MoveEasel { .. }
            | UiCommand::CaptureToEasel
            | UiCommand::LivePick
            | UiCommand::CaptureLive { .. }
            | UiCommand::EaselLivePlay { .. }
            | UiCommand::EaselLiveStop { .. }
            | UiCommand::EaselLayout { .. }
            | UiCommand::EaselAddNote { .. }
            | UiCommand::EaselSetText { .. }
            | UiCommand::EaselMove { .. }
            | UiCommand::EaselResize { .. }
            | UiCommand::EaselColor { .. }
            | UiCommand::EaselRemove { .. }
            | UiCommand::EaselOpenLink { .. }
            | UiCommand::OpenLibrary
            | UiCommand::CloseLibrary
            | UiCommand::LibraryOpenFile { .. }
            | UiCommand::LibraryDeleteDownload { .. }
            | UiCommand::LibraryClear { .. }
            | UiCommand::ReopenArchived { .. }
            | UiCommand::DeleteArchived { .. }
            | UiCommand::LibraryOpenTab { .. }
            | UiCommand::ClearTabs) => self.handle_easel(command),
            UiCommand::InstallCatalog { id } => self.install_catalog(&id),
            UiCommand::CheckCatalogUpdates => {
                self.catalog_checked = Some(self.outdated_extensions().len());
                self.ui_state();
            }
            UiCommand::UpdateAllCatalog => {
                for id in self.outdated_extensions() {
                    self.install_catalog(id);
                }
                self.catalog_checked = Some(0);
                self.ui_state();
            }
            UiCommand::ApplyUpdate => self.start_update(),
            UiCommand::ExtAction { id, x, y, w, h: hh } => {
                #[cfg(target_os = "macos")]
                self.show_native_popup(&id, (x, y, w, hh));
                #[cfg(windows)]
                {
                    let url = self.webext.as_ref().and_then(|host| host.popup_url(&id));
                    if let Some(url) = url {
                        self.open_ext_popup(&url, (x, y, w, hh));
                    }
                }
            }
            UiCommand::CloseExtPopup => {
                #[cfg(windows)]
                self.close_ext_popup();
            }
            UiCommand::SetAppExtension { id, on } => {
                if let Some(e) = self.app_ext.iter().find(|e| e.id == id).cloned() {
                    set_app_extension_enabled(&self.data, &id, on);
                    if let Some(host) = &self.webext {
                        if on {
                            host.load(&id, &e.path);
                        } else {
                            host.unload(&id);
                        }
                    }
                    self.ui_settings();
                }
            }
            UiCommand::ImportScan => {
                self.import_sources = import::detect();
                self.ui_settings();
                self.spawn_import(|_, _| None);
            }
            UiCommand::ImportBookmarks { source } => {
                let found = self.import_sources.iter().find(|s| s.id == source).cloned();
                let space = self.svc.workspace().active_space().id.0;
                self.spawn_import(move |bookmarks, _| {
                    Some(match found {
                        Some(s) => match import::read_bookmarks(&s) {
                            Ok(list) => store_bookmarks(bookmarks, list, &s.browser, space),
                            Err(e) => (format!("{} : {e}", s.browser), Vec::new()),
                        },
                        None => ("Source introuvable".into(), Vec::new()),
                    })
                });
            }
            UiCommand::AddBookmark { title, url, folder } => {
                let allowed =
                    url::Url::parse(&url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
                self.import_msg = if allowed {
                    let title = if title.trim().is_empty() {
                        url.clone()
                    } else {
                        title
                    };
                    self.store_dropped(vec![import::Bookmark {
                        title,
                        url,
                        folder,
                        space: None,
                    }])
                } else {
                    String::new()
                };
            }
            UiCommand::ImportHtml { text } => {
                let list = import::parse_html(&text);
                self.import_msg = self.store_dropped(list);
            }
            UiCommand::ImportFile { kind } => {
                let space = self.svc.workspace().active_space().id.0;
                self.spawn_import(move |bookmarks, vault| {
                    Some(import_file(bookmarks, vault, &kind, space))
                });
            }
            UiCommand::RemoveCatalog { id } => {
                if let Some(host) = &self.webext {
                    host.unload(&id);
                }
                catalog::remove(&self.data, &id);
                self.ui_state();
            }
            UiCommand::SetExtensionEnabled { id, enabled } if catalog::find(&id).is_some() => {
                self.svc.set_enabled_flag(&id, enabled);
                if let Some(host) = &self.webext {
                    if enabled && catalog::is_installed(&self.data, &id) {
                        host.load(&id, &catalog::package_path(&self.data, &id));
                    } else {
                        host.unload(&id);
                    }
                }
                self.ui_state();
            }
            UiCommand::SetExtensionEnabled { id, enabled } => {
                self.svc.set_extension_enabled(&id, enabled);
                self.reapply_extensions();
                self.ui_state();
            }
            UiCommand::ReloadExtensions => self.reload_extensions(),
            UiCommand::OpenExtensionsFolder => extensions::reveal(&extensions_dir(&self.data)),
            UiCommand::InstallSampleExtension => {
                if extensions::install_sample(&extensions_dir(&self.data)).is_ok() {
                    self.reload_extensions();
                }
            }
        }
    }

    /// Split view with the first other tab of the space, or closes the current split.
    fn toggle_split(&mut self) {
        if self.home_shown() {
            return;
        }
        if self.split_tab().is_some() {
            self.handle_ui(UiCommand::CloseSplit);
            return;
        }
        let ws = self.svc.workspace();
        let active = ws.active_tab().map(|t| t.id);
        let other = ws
            .active_space()
            .tabs
            .iter()
            .find(|t| Some(t.id) != active && !t.pinned)
            .map(|t| t.id);
        match other {
            Some(id) => self.handle_ui(UiCommand::Split { id: id.0 }),
            None => self.toast("Open another tab first to use Split View"),
        }
    }

    fn shortcut(&mut self, key: &str) {
        match key {
            "escape" => {
                if self.palette_open {
                    self.close_palette();
                } else {
                    self.close_peek();
                }
                return;
            }
            "enter" => {
                if self.peek.is_some() {
                    self.peek_to_tab();
                } else if self.palette_open && self.palette_mode == "boost" {
                    let _ = self
                        .palette
                        .evaluate_script("window.bh&&bh.save&&bh.save()");
                }
                return;
            }
            _ if self.peek.is_some() || (self.palette_open && !matches!(key, "l" | "t")) => return,
            _ => {}
        }
        match key {
            "l" => self.open_palette("url"),
            "t" => self.new_tab(),
            "n" => {
                open_window(false, None);
            }
            "shift+n" => {
                open_window(true, None);
            }
            "w" => self.close_active_tab(),
            "r" => self.with_active_tab(views::reload),
            "b" => self.handle_ui(UiCommand::Lock),
            "[" => self.handle_ui(UiCommand::Back),
            "]" => self.handle_ui(UiCommand::Forward),
            "," => self.settings_wanted = true,
            "shift+e" => self.handle_ui(UiCommand::OpenEasel { id: None }),
            "shift+s" => self.handle_ui(UiCommand::CaptureToEasel),
            "shift+l" => self.handle_ui(UiCommand::LivePick),
            "\\" => self.toggle_split(),
            _ => {}
        }
    }

    /// Re-reads the address from the engine; page scripts cannot dictate what the bar shows.
    fn refresh_url(&mut self, tab: TabId) {
        let current = self.tabs.get(&tab).and_then(|v| v.url().ok());
        if let Some(url) = current.and_then(|raw| Url::parse(&raw).ok()) {
            self.page_url(tab, url);
        }
    }

    fn page_url(&mut self, tab: TabId, url: Url) {
        if url.scheme() == "about" || tab == PEEK_TAB {
            return;
        }
        let boost = url
            .host_str()
            .and_then(|h| self.svc.boost_for(h))
            .map(str::to_owned);
        if let (Some(css), Some(view)) = (boost, self.tabs.get(&tab)) {
            views::apply_boost(view, &css);
        }
        self.svc.page_changed(tab, Some(url), None);
        self.ui_state();
    }

    fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Ui(command) => self.handle_ui(command),
            Msg::Page(tab, PageEvent::Shortcut { key }) => {
                let active = self.svc.workspace().active_tab().map(|t| t.id);
                if Some(tab) == active || Some(tab) == self.split_tab() || tab == PEEK_TAB {
                    self.shortcut(&key);
                }
            }
            Msg::Page(_, PageEvent::Peek { url }) => {
                if let Ok(url) = Url::parse(&url) {
                    self.open_peek(&url);
                }
            }
            Msg::Page(tab, PageEvent::LivePick { selector, w, h }) => {
                if self.live_pick.take() == Some(tab) {
                    self.capture_live(tab, &selector, (w, h));
                }
            }
            Msg::Page(tab, PageEvent::AutofillQuery) => self.autofill_query(tab),
            Msg::Page(tab, PageEvent::AutofillFill { index }) => self.autofill_fill(tab, index),
            Msg::Page(tab, PageEvent::Location) => self.refresh_url(tab),
            Msg::Page(tab, PageEvent::Favicon { url }) => {
                let page = self.tabs.get(&tab).and_then(|v| v.url().ok());
                if let Some(page) = page.and_then(|raw| Url::parse(&raw).ok()) {
                    if self.favicons.set(&page, &url) {
                        self.ui_state();
                    }
                }
            }
            Msg::Page(tab, PageEvent::Blocked { host }) => {
                let site = self.site_of(tab);
                self.on_blocked("", &host, &site);
            }
            Msg::Url(tab, url) => self.page_url(tab, url),
            Msg::Loaded(tab, url) => {
                if tab != PEEK_TAB && url.scheme() != "about" {
                    let injection = self.svc.injection_for(&url);
                    if let (false, Some(view)) = (injection.is_empty(), self.tabs.get(&tab)) {
                        views::apply_injection(view, &injection.css, &injection.js);
                    }
                }
            }
            Msg::Title(tab, _) if tab == PEEK_TAB => {}
            Msg::Title(tab, title) => {
                self.svc.page_changed(tab, None, Some(title));
                self.ui_state();
            }
            // An extension asking for a window (Bitwarden's unlock) gets a popup, not a tab.
            #[cfg(windows)]
            Msg::OpenTab(url) if blackhole_domain::is_extension_popout(&url) => {
                self.close_ext_popup();
                let width = self.window_size().width;
                self.show_ext_popup(url.as_str(), (width, BAR_HEIGHT, 0.0, 0.0));
            }
            #[cfg(target_os = "macos")]
            Msg::OpenTab(url)
                if blackhole_domain::is_extension_popout(&url)
                    && self.show_native_popup(
                        url.host_str().unwrap_or_default(),
                        (self.window_size().width, BAR_HEIGHT, 0.0, 0.0),
                    ) => {}
            Msg::OpenTab(url) => self.open(url.as_str()),
            Msg::External(url) => {
                self.window.set_minimized(false);
                self.window.focus_window();
                self.open(url.as_str());
            }
            Msg::Redirect(tab, url) => {
                if let Some(view) = self.view_of(tab) {
                    let _ = view.load_url(url.as_str());
                }
            }
            Msg::DownloadStarted { url, path } => {
                let id = self.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1;
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let remote = url.clone();
                self.downloads
                    .push(blackhole_infrastructure::store::DownloadRec {
                        id,
                        name,
                        path: path.to_string_lossy().into_owned(),
                        url,
                        at: now_secs(),
                        done: false,
                        ok: false,
                    });
                self.save_downloads();
                self.dl_live.insert(
                    id,
                    DownloadLive {
                        name: self
                            .downloads
                            .last()
                            .map(|d| d.name.clone())
                            .unwrap_or_default(),
                        path: path.clone(),
                        received: 0,
                        total: None,
                        finished: None,
                    },
                );
                self.dl_polled = Instant::now();
                let tx = self.dl_size_tx.clone();
                std::thread::spawn(move || {
                    if let Some(total) = blackhole_infrastructure::remote_size(&remote) {
                        let _ = tx.send((id, total));
                    }
                });
                let list = self.downloads_json();
                self.ui(format!("window.bh&&bh.downloads({list})"));
                self.toast("Download started");
                self.ui_library();
            }
            Msg::DownloadDone { url, path, ok } => {
                if let Some(d) = self
                    .downloads
                    .iter_mut()
                    .rev()
                    .find(|d| d.url == url && !d.done)
                {
                    d.done = true;
                    d.ok = ok;
                    if let Some(p) = path {
                        d.path = p.to_string_lossy().into_owned();
                    }
                    if let Some(live) = self.dl_live.get_mut(&d.id) {
                        live.finished = Some((ok, Instant::now()));
                        live.path = PathBuf::from(&d.path);
                        if ok {
                            let size = std::fs::metadata(&live.path).map_or(0, |m| m.len());
                            live.received = size;
                            live.total = Some(size.max(1));
                        }
                    }
                }
                self.save_downloads();
                let list = self.downloads_json();
                self.ui(format!("window.bh&&bh.downloads({list})"));
                self.toast(if ok {
                    "Download complete"
                } else {
                    "Download failed"
                });
                self.ui_library();
            }
            Msg::Blocked { why, host } => self.on_blocked(why, &host, ""),
        }
    }

    /// Offers the vault logins saved for the tab's site. Only https (or localhost) pages of the
    /// foreground tab are served, and only usernames are sent to the page.
    fn autofill_query(&mut self, tab: TabId) {
        let active = self.svc.workspace().active_tab().map(|t| t.id);
        if Some(tab) != active && Some(tab) != self.split_tab() {
            return;
        }
        // A loaded password-manager extension owns the login fields: two menus would collide.
        let manager_active = self.webext.as_ref().is_some_and(|h| {
            matches!(
                h.state("bitwarden"),
                Some(blackhole_macos_webext::LoadState::Loaded)
            )
        });
        if manager_active {
            return;
        }
        let host = self.site_of(tab);
        let secure = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .find(|t| t.id == tab)
            .is_some_and(|t| t.url.scheme() == "https" || host == "localhost");
        let logins = if secure && !host.is_empty() {
            Vault::new(&self.data).for_host(&host).unwrap_or_default()
        } else {
            Vec::new()
        };
        let names: Vec<String> = logins.iter().map(|l| l.username.clone()).collect();
        if let Some(view) = self.tabs.get(&tab) {
            views::autofill_show(view, &names);
        }
        self.autofill.insert(tab, logins);
    }

    fn autofill_fill(&mut self, tab: TabId, index: usize) {
        let host = self.site_of(tab);
        let Some(login) = self.autofill.get(&tab).and_then(|l| l.get(index)) else {
            return;
        };
        // The site may have changed since the query.
        if !blackhole_infrastructure::store::same_site(&login.url, &host) {
            return;
        }
        if let Some(view) = self.tabs.get(&tab) {
            views::autofill_fill(view, &login.username, &login.password);
        }
    }

    fn site_of(&self, tab: TabId) -> String {
        let ws = self.svc.workspace();
        let url = ws
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .find(|t| t.id == tab)
            .map(|t| &t.url);
        url.and_then(|u| u.host_str())
            .unwrap_or_default()
            .to_owned()
    }

    fn on_blocked(&mut self, why: &str, host: &str, site: &str) {
        self.blocked.record(host, site);
        if !why.is_empty() {
            self.toast(why);
        }
        self.ui_state();
    }

    fn service_windows(&mut self, event_loop: &ActiveEventLoop) {
        if std::mem::take(&mut self.settings_wanted) {
            self.open_settings(event_loop);
            self.ui_state();
        }
        if std::mem::take(&mut self.easel_wanted) {
            self.open_easel(event_loop);
            self.ui_easel();
        }
    }

    fn pump(&mut self) {
        while let Some(msg) = self.mailbox.pop() {
            self.handle_msg(msg);
        }
    }
}

enum App {
    Initial(Mailbox),
    Running(Box<Running>),
}

impl ApplicationHandler<Wake> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Self::Initial(mailbox) = self {
            let mut running = Running::new(event_loop, mailbox.clone());
            running.pump();
            if let Some(url) = launch_url() {
                running.open(&url);
            }
            *self = Self::Running(Box::new(running));
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[allow(unused_mut)]
        let mut wake = PEERS_EVERY;
        if let Self::Running(app) = self {
            app.service_windows(event_loop);
            if app.quit {
                event_loop.exit();
                return;
            }
            app.poll_webext();
            app.discard_idle_tabs();
            app.refill_spares();
            app.service_peers();
            app.service_media();
            if !app.dl_live.is_empty() {
                wake = wake.min(DOWNLOAD_POLL);
            }
            #[cfg(windows)]
            if app.ext_popup.is_some() {
                app.probe_ext_popup();
                wake = wake.min(Duration::from_millis(250));
            }
            if app.sidebar.moving() {
                app.tick_sidebar();
                wake = FRAME;
            }
            #[cfg(feature = "devtools")]
            {
                app.dev_tick(event_loop);
                if app.dev_active() {
                    wake = wake.min(Duration::from_millis(30));
                }
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wake));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: Wake) {
        if let Self::Running(app) = self {
            app.pump();
            app.service_windows(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Self::Running(app) = self else { return };
        app.pump();
        app.service_windows(event_loop);
        if app.settings_window_id() == Some(id) {
            if matches!(event, WindowEvent::CloseRequested) {
                app.close_settings();
            }
            return;
        }
        if app.easel_window_id() == Some(id) {
            match event {
                WindowEvent::CloseRequested => app.close_easel(),
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    app.easel_resized();
                }
                _ => {}
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                #[cfg(windows)]
                if let Some(backdrop) = &mut app.backdrop {
                    backdrop.paint();
                }
                app.layout(true);
                #[cfg(windows)]
                app.close_ext_popup();
                if app.window.is_maximized() != app.maximized {
                    app.maximized = !app.maximized;
                    app.ui_bar();
                }
            }
            #[cfg(windows)]
            WindowEvent::RedrawRequested => {
                if let Some(backdrop) = &mut app.backdrop {
                    backdrop.paint();
                }
            }
            WindowEvent::Focused(true) => app.apply_focus(),
            _ => {}
        }
    }
}

fn app_extensions_file(data: &std::path::Path) -> std::path::PathBuf {
    data.join("app_extensions.json")
}

fn enabled_app_extensions(data: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(app_extensions_file(data))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn app_extension_enabled(data: &std::path::Path, id: &str) -> bool {
    enabled_app_extensions(data).iter().any(|e| e == id)
}

fn set_app_extension_enabled(data: &std::path::Path, id: &str, on: bool) {
    let mut list = enabled_app_extensions(data);
    list.retain(|e| e != id);
    if on {
        list.push(id.to_owned());
    }
    let _ = std::fs::create_dir_all(data);
    let _ = std::fs::write(
        app_extensions_file(data),
        serde_json::to_vec(&list).unwrap_or_default(),
    );
}

#[cfg(target_os = "macos")]
fn web_view_ptr(view: &WebView) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    use wry::WebViewExtMacOS;
    let web_view = view.webview();
    std::ptr::NonNull::new(std::ptr::from_ref(&*web_view).cast_mut().cast())
}

#[cfg(not(target_os = "macos"))]
fn web_view_ptr(_view: &WebView) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
    None
}

/// Result of an import job: message, vault size, and the bookmarks to file as tabs.
type ImportDone = (Option<String>, usize, Vec<import::Bookmark>);

fn file_bookmarks(svc: &mut BrowserService<JsonWorkspaceRepository>, list: Vec<import::Bookmark>) {
    let first = svc.workspace().spaces().first().map_or(0, |s| s.id.0);
    let mut by_space: HashMap<u64, Vec<(String, String, url::Url)>> = HashMap::new();
    for b in list {
        let Ok(u) = url::Url::parse(&b.url) else {
            continue;
        };
        if matches!(u.scheme(), "http" | "https") {
            by_space
                .entry(b.space.unwrap_or(first))
                .or_default()
                .push((b.folder, b.title, u));
        }
    }
    for (space, links) in by_space {
        svc.import_tabs(blackhole_domain::SpaceId(space), links);
    }
}

fn store_bookmarks(
    store: &BookmarkStore,
    mut list: Vec<import::Bookmark>,
    from: &str,
    space: u64,
) -> (String, Vec<import::Bookmark>) {
    list.iter_mut().for_each(|b| b.space = Some(space));
    let total = list.len();
    match store.merge(list.clone()) {
        Ok(new) => (
            format!(
                "{from} : {new} favoris ajoutés ({} déjà présents)",
                total - new
            ),
            list,
        ),
        Err(e) => (format!("{from} : {e}"), Vec::new()),
    }
}

fn import_file(
    bookmarks: &BookmarkStore,
    vault: &Vault,
    kind: &str,
    space: u64,
) -> (String, Vec<import::Bookmark>) {
    let (label, exts): (&str, &[&str]) = if kind == "logins" {
        ("CSV", &["csv"])
    } else {
        ("HTML", &["html", "htm"])
    };
    let Some(path) = rfd::FileDialog::new().add_filter(label, exts).pick_file() else {
        return (String::new(), Vec::new());
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ("Fichier illisible".into(), Vec::new());
    };
    if kind == "logins" {
        let msg = match import::parse_logins_csv(&text).and_then(|l| {
            let total = l.len();
            vault.merge(l).map(|new| (new, total))
        }) {
            Ok((new, total)) => format!("{new} mots de passe ajoutés au coffre chiffré ({} déjà présents). Supprimez le fichier CSV : il contient vos mots de passe en clair.", total - new),
            Err(e) => e,
        };
        return (msg, Vec::new());
    }
    store_bookmarks(bookmarks, import::parse_html(&text), "Fichier HTML", space)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Opens a downloaded file with its default app; programs are only revealed, never run.
fn open_download(path: &std::path::Path) {
    let risky = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "exe"
                | "msi"
                | "bat"
                | "cmd"
                | "ps1"
                | "com"
                | "scr"
                | "app"
                | "command"
                | "sh"
                | "jar"
                | "vbs"
                | "js"
                | "lnk"
                | "pkg"
                | "dmg"
        )
    });
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if risky {
            cmd.arg("-R");
        }
        let _ = cmd.arg(path).spawn();
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer");
        if risky {
            cmd.arg(format!("/select,{}", path.display()));
        } else {
            cmd.arg(path);
        }
        let _ = cmd.spawn();
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (path, risky);
}

/// Scripted devtools runs and cargo builds never look for (or install) updates.
fn updates_enabled() -> bool {
    !cfg!(feature = "devtools") && !private_mode() && updater::supported()
}

/// A private window runs in its own process with a throwaway profile (cookies, history, tabs,
/// bookmarks), deleted when it closes.
fn private_mode() -> bool {
    static PRIVATE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PRIVATE.get_or_init(|| std::env::args().any(|a| a == "--private"))
}

/// Dev builds keep their window registry in the scratch folder, away from the real app.
fn peers_dir() -> PathBuf {
    #[cfg(feature = "devtools")]
    {
        devtools::scratch().join("windows")
    }
    #[cfg(not(feature = "devtools"))]
    {
        peers::dir()
    }
}

fn private_data_dir() -> PathBuf {
    std::env::temp_dir().join(format!("BlackHole-private-{}", std::process::id()))
}

/// Each window is its own process: a private one gets `--private` and a temporary profile.
/// A window given a page opens it right away (`--open <url>`).
fn open_window(private: bool, url: Option<&str>) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut cmd = std::process::Command::new(exe);
    if private {
        cmd.arg("--private");
    }
    if let Some(url) = url {
        cmd.args(["--open", url]);
    }
    cmd.spawn().is_ok()
}

/// The page this window was asked to open at launch.
fn launch_url() -> Option<String> {
    let mut args = std::env::args();
    args.find(|a| a == "--open")?;
    args.next()
        .filter(|u| matches!(Url::parse(u), Ok(u) if matches!(u.scheme(), "http" | "https")))
}

fn main() {
    let event_loop = EventLoop::with_user_event().build().expect("event loop");
    let mailbox = Mailbox::new(event_loop.create_proxy());
    // Registered before the loop starts so a link that launches the app is not lost.
    let links = mailbox.clone();
    blackhole_macos_url::register_url_handler(move |link| {
        if let Some(url) = Url::parse(&link)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"))
        {
            links.push(Msg::External(url));
        }
    });
    let mut app = App::Initial(mailbox);
    event_loop.run_app(&mut app).expect("run");
    drop(app);
    peers::withdraw(&peers_dir(), std::process::id());
    if private_mode() {
        let _ = std::fs::remove_dir_all(private_data_dir());
    }
}

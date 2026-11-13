//! BlackHole composition root: window, native web views and message routing.
//! Business rules live in the inner crates; this file only wires them to the platform web engine
//! (WKWebView on macOS, WebView2 on Windows) through `wry`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod backdrop;
mod bridges;
#[cfg(feature = "devtools")]
mod devtools;
mod errors;
mod extensions;
mod favicons;
mod ipc;
mod media;
mod compare;
mod meet;
mod notices;
mod handoff;
mod highlights;
mod i18n;
mod watching;
mod passwords;
mod pilot;
mod recall;
mod sidebar;
mod stats;
mod views;

use i18n::{message, t, tf, tn};
use blackhole_application::{BrowserService, NavigationOutcome, WorkspaceRepository};
use blackhole_domain::http_auth;
use blackhole_domain::verbs::{self, Target, Verb};
use blackhole_domain::{
    BlockLog, Easels, FolderId, Removed, SearchEngine, Settings, SpaceId, TabId,
};
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
    /// A long or live video/audio is playing, muted or not (not an ambient looping clip).
    #[serde(default)]
    watching: bool,
    /// A video of the page is floating in Picture in Picture.
    #[serde(default)]
    pip: bool,
    /// Camera / microphone opened by a call, and whether each is currently on.
    #[serde(default)]
    cam: bool,
    #[serde(default)]
    mic: bool,
    /// A live WebRTC connection (a call), even when no camera or microphone is open.
    #[serde(default)]
    call: bool,
    #[serde(default, rename = "camOn")]
    cam_on: bool,
    #[serde(default, rename = "micOn")]
    mic_on: bool,
}

/// How often the event loop wakes to refresh this window's place in the registry, look for
/// incoming tabs, spares to build and media to probe.
const PEERS_EVERY: Duration = Duration::from_millis(400);
/// A window's listing file is only rewritten this often (peers consider it stale after 5 s).
const PEERS_PUBLISH_EVERY: Duration = Duration::from_secs(2);
/// A new tab is covered by the loading screen until its page loads, at most this long.
const SPLASH_MAX: Duration = Duration::from_secs(5);
/// A page that never reports its end (error, cancelled) stops showing progress after this.
const LOADING_MAX: Duration = Duration::from_secs(30);
/// A new tab whose server never starts answering is reported unreachable after this long.
const CONNECT_MAX: Duration = Duration::from_secs(20);
/// How often background tabs are asked whether they play audio or video.
const MEDIA_EVERY: Duration = Duration::from_millis(1500);
/// How long a tab that was in a call or playing media is still treated as such.
const BUSY_GRACE: Duration = Duration::from_secs(90);
/// How long a tab is probed after it said its media may have changed.
const MEDIA_HINT_FOR: Duration = Duration::from_secs(5);
/// A page whose engine process dies again this soon after a reload is not reloaded again.
const CRASH_LOOP: Duration = Duration::from_secs(30);
/// Spare page views kept ready, and how long the app must stay quiet before building them.
const SPARES_DELAY: Duration = Duration::from_millis(900);
const EASEL_HTML: &str = include_str!("../ui/easel.html");
const LIBRARY_HTML: &str = include_str!("../ui/library.html");
/// The sections of the Library page, in its rail's order.
const LIBRARY_SECTIONS: [&str; 8] =
    ["media", "downloads", "easels", "spaces", "history", "highlights", "watches", "archive"];
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
    // Shown by the shell (a download): the strip stays thin, the page keeps its clicks.
    let (x, width) = if sidebar.flyout_shown() && !sidebar.held() {
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
/// The page while the sidebar opens or hides: it keeps the width it has with the sidebar hidden
/// and only its left edge moves (the window clips the rest), so the site is not laid out again
/// on every frame (what made the motion stutter); its final size is set once, at the end.
fn sliding(main: wry::Rect, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    let scale = 1.0;
    let at = main.position.to_logical::<f64>(scale);
    let size = main.size.to_logical::<f64>(scale);
    views::rect(at.x, at.y, (window.width - 2.0 * CARD_GAP).max(1.0), size.height)
}

/// Where the command bar lives: the whole window while open, a parked 1x1 view otherwise. A hidden
/// full-window view still wins the drag destination lookup and would swallow in-page drags.
fn palette_rect(open: bool, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    if open {
        views::rect(0.0, 0.0, window.width, window.height)
    } else {
        parked()
    }
}

/// Bounds of a hidden view that must not catch drags (see `palette_rect`).
fn parked() -> wry::Rect {
    views::rect(0.0, 0.0, 1.0, 1.0)
}

/// The Library covers the whole window under the command bar.
fn library_rect(window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    views::rect(
        0.0,
        BAR_HEIGHT,
        window.width,
        (window.height - BAR_HEIGHT).max(1.0),
    )
}
/// Quiet time after a tab creation before the overlays (palette, flyout) are rebuilt on top.
const RAISE_DELAY: Duration = Duration::from_millis(400);
const BLUR_DELAY: Duration = Duration::from_millis(350);
/// A site needs this many loose tabs to be suggested as a folder; the sidebar only suggests a
/// tidy-up from `TIDY_FROM` loose tabs.
const TIDY_MIN: usize = 3;
/// Black hole tabs open at once: each keeps its page alive (never put to sleep) until closed.
const MAX_BURNERS: usize = 4;
const TIDY_FROM: usize = 6;
/// How long the Undo of a deleted space or folder stays possible (the toast shows for 8 s).
const UNDO_FOR: Duration = Duration::from_secs(15);
/// How often an unfocused but still visible window is checked for being covered.
const PIP_RECHECK: Duration = Duration::from_millis(1000);
/// A closed media card stays closed this long whatever the probes say (one may have left before
/// the pause): longer than the probe interval.
const DISMISS_GRACE: Duration = Duration::from_millis(2000);
/// How long after the shell brings a floating video back its "left PiP" is taken as an echo.
const PIP_SHELL_EXIT: Duration = Duration::from_millis(1500);
/// Share of the browser window another window must hide before a video floats.
const COVERED_SHARE: f64 = 0.5;

/// Share of the rectangle `own` (`x, y, w, h`) that `other` covers.
fn overlap_share(own: (f64, f64, f64, f64), other: (f64, f64, f64, f64)) -> f64 {
    let w = (own.0 + own.2).min(other.0 + other.2) - own.0.max(other.0);
    let h = (own.1 + own.3).min(other.1 + other.3) - own.1.max(other.1);
    if own.2 <= 0.0 || own.3 <= 0.0 || w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    w * h / (own.2 * own.3)
}
const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(15);
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// After a failed check (offline, GitHub unreachable), try again well before `UPDATE_CHECK_EVERY`.
const UPDATE_RETRY: Duration = Duration::from_secs(30 * 60);

/// Where the self-update stands; shown as a button at the bottom of the sidebar.
enum UpdateState {
    Idle,
    Available(Release),
    Downloading(Release),
    Failed(Release, String),
}

enum UpdateMsg {
    Found(Release),
    /// The check found nothing newer.
    Current,
    CheckFailed(String),
    Downloaded(PathBuf),
    Failed(String),
}
/// One frame of the sidebar's motion (~120 per second): paced with a precise sleep, since the
/// system timer behind `WaitUntil` (15.6 ms on Windows) made the frames uneven.
const FRAME: Duration = Duration::from_millis(8);
/// How long a rarely used page of the shell (library, easel tab, split bar) stays loaded once hidden.
const PANEL_KEEP: Duration = Duration::from_secs(60);

/// A page of the shell created on first use and released after `PANEL_KEEP` hidden: each one
/// is a renderer process, and most sessions never open them.
struct LazyView {
    view: Option<WebView>,
    shown: Instant,
}

impl LazyView {
    fn new() -> Self {
        Self {
            view: None,
            shown: Instant::now(),
        }
    }
}

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

/// Popup limits: a narrow popup keeps its own width (uBlock Origin Lite's is 273 px wide: a wider
/// view showed an empty strip beside it), and none gets larger than Chromium's maximum.
#[cfg(windows)]
const POPUP_MIN: (f64, f64) = (200.0, 80.0);
/// Corner radius of an extension popup, in logical pixels.
#[cfg(windows)]
const POPUP_RADIUS: f64 = 10.0;
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
const DOWNLOAD_LINGER: Duration = Duration::from_secs(8);
const DOWNLOAD_POLL: Duration = Duration::from_millis(500);
/// Debug builds only, with `BH_TRACE` set: one line on stderr for what a page asks the shell
/// (navigations, new windows, page events), to follow a click that misbehaves. Never in release.
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

/// How long after a download started the end of its navigation may still be reported.
const DOWNLOAD_NAV: Duration = Duration::from_secs(10);
/// How long a hidden sidebar shows itself for a download card.
const DOWNLOAD_REVEAL: Duration = Duration::from_secs(4);
/// How long a hidden sidebar shows itself when a link opens a tab in the background (middle
/// click): without it nothing on screen says the tab is there.
const LINK_REVEAL: Duration = Duration::from_millis(2500);
/// How long an aborted navigation waits for the download that would explain it.
const ABORT_GRACE: Duration = Duration::from_millis(600);

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
    spares: Vec<(views::Store, views::TabSlot, WebView)>,
    spares_due: Option<Instant>,
    /// The password wallet: autofill, save offers, Settings › Passwords.
    passwords: passwords::Passwords,
    last_active: HashMap<TabId, Instant>,
    /// Page zoom per site of black hole tabs (percent), for the session only: other tabs use
    /// `Settings::site_zoom`, remembered between launches.
    zoom: HashMap<String, u32>,
    /// Tabs whose view currently has a zoom other than 100%.
    zoomed: HashSet<TabId>,
    /// Set when a tab view was created above the overlays; they are rebuilt once it is due.
    raise_due: Option<Instant>,
    /// Tab whose page was on screen at the last `sync` (drives automatic Picture in Picture).
    shown: Option<TabId>,
    /// Tab floated because the user left it for another tab (its "back to tab" button returns).
    pip_tab: Option<TabId>,
    /// The probe has seen the floating video of `pip_tab`: its disappearance means it came back.
    pip_seen: bool,
    /// `pip_tab` when the probe stopped seeing its floating video, and when. WebKit changes
    /// the video's mode as soon as the window's "back to tab" button is pressed but only tells
    /// the page once the window has shrunk back: the page's `pip_left` arrives after the probe
    /// and must still count as a video the shell floated (`pip::LEFT_EVENT_GRACE`).
    pip_faded: Option<(TabId, Instant)>,
    /// Last time a tab was seen in a call or playing media: it keeps its page for a while after.
    busy_seen: HashMap<TabId, Instant>,
    /// Tab floated because the window lost focus; it returns when the window gets focus back.
    pip_on_blur: Option<TabId>,
    /// When a focus loss of the window becomes a real blur, unless focus came back before.
    blur_due: Option<Instant>,
    /// Left edge of the page card inside the sidebar page, as last sent to its shadow.
    card_left: std::cell::Cell<f64>,
    bar_left: std::cell::Cell<f64>,
    /// What the top bar was last told about notices (`sync_bar_notice`).
    bar_notice: std::cell::Cell<(bool, usize)>,
    sampler: stats::Sampler,
    usage: Option<stats::Usage>,
    stats_tabs: usize,
    peers_due: Instant,
    peers_published: Instant,
    /// Tabs whose page is loading (drives the progress bar), with the time they started.
    loading: HashMap<TabId, Instant>,
    /// New tabs still waiting for their first page: the loading screen covers them.
    splash: HashMap<TabId, Instant>,
    /// Tabs opened but not yet answering at all (no commit), with the URL asked for.
    awaiting: HashMap<TabId, (Instant, Url)>,
    /// The address typed for a tab that already shows a page, until that page starts, fails or
    /// turns into a download: the bar shows it at once, with the progress line. A slow server
    /// (a device of the local network) otherwise left the old address for many seconds, as if
    /// nothing had been asked.
    pending: HashMap<TabId, (Instant, Url)>,
    /// Error pages of the tabs and what led to them (`errors.rs`).
    errors: errors::Errors,
    /// HTTP authentication prompts waiting for the user, their engine answers, and the one shown.
    auth: http_auth::AuthPrompts,
    auth_replies: HashMap<u64, wry::HttpAuthResponder>,
    auth_shown: Option<u64>,
    /// The exact origin of each prompt (read from the engine), for the logins saved for it.
    auth_origins: HashMap<u64, String>,
    /// Cards of the sidebar that stay until answered (`bridges.rs`, `domain::signals`).
    signals: blackhole_domain::signals::Signals,
    /// The last tab received from another device: from whom, its address, when (start page).
    received: Option<(String, String, i64)>,
    #[cfg(target_os = "macos")]
    cards_rounded: Instant,
    media: Arc<Mutex<HashMap<u64, MediaProbe>>>,
    media_due: Instant,
    /// When each tab last said its media may have changed (see `PageEvent::Media`).
    media_hint: HashMap<u64, Instant>,
    /// When the engine process of a tab last died, to reload it once and not in a loop.
    crashed: HashMap<TabId, Instant>,
    media_seen: serde_json::Value,
    /// Last report with media per tab: a short gap in the probes keeps the card on screen.
    media_hold: media::Linger<MediaProbe>,
    /// The next address typed in the command bar opens in a black hole tab.
    burner_next: bool,
    /// Pilot split: the tab whose links open in the other pane (session only).
    pilot: Option<TabId>,
    /// Links cleaned on the way this session (the privacy report's count).
    links_cleaned: u32,
    /// The space or folder deleted last, for the toast's Undo.
    undo: Option<(Instant, Removed)>,
    /// Library section asked for by the command bar ("Open History"), with a request number.
    library_jump: (u32, &'static str),
    /// Whether the Settings and Easel windows are maximized, last time their page was told.
    panels_maxed: (bool, bool),
    /// When the sidebar's last frame was drawn (motion pacing).
    last_frame: Instant,
    /// A live capture area waiting for the user to choose its Easel.
    live_pending: Option<(TabId, String, (f64, f64))>,
    /// The tab about to be opened from a page: its address and the `Referer` of its first load.
    referrer_next: Option<(Url, String)>,
    /// The picker was armed to watch the area (page menu), not to ask where it goes.
    pick_watches: bool,
    /// Google Calendar sync and the meeting card (see meet.rs).
    calendar: meet::Calendar,
    /// Paired devices and "Continue on…" (see handoff.rs).
    nearby: handoff::Nearby,
    /// Watched areas of pages (see watching.rs).
    watching: watching::Watching,
    /// Recall: the pages read, searchable by what they said (see recall.rs).
    recall: recall::Recall,
    /// Highlights and notes on pages (see highlights.rs).
    marks: highlights::Marks,
    /// The tab comparator (see compare.rs).
    comparing: compare::Comparing,
    /// The last tidy-up, for its Undo; whether the suggestion was dismissed this session.
    tidied: Option<blackhole_domain::Tidied>,
    tidy_dismissed: bool,
    /// Tabs whose media card was closed, and when; they come back when the tab plays again.
    media_dismissed: HashMap<u64, Instant>,
    /// Tabs whose floating video the shell itself just brought back (their "left PiP" is no
    /// request from the user).
    pip_shell_exits: HashMap<u64, Instant>,
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
    settings_seq: u32,
    home_visible: bool,
    /// "What's new" shown on the start page until dismissed; empty when there is nothing to show.
    release_notes: Vec<serde_json::Value>,
    /// Command bar overlay; rebuilt whenever a view is added so it stays on top.
    splitbar: LazyView,
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
    /// Left edge and width of each extension's button in the command bar (`ExtAnchors`).
    #[cfg(windows)]
    ext_anchors: HashMap<String, (f64, f64)>,
    data: std::path::PathBuf,
    /// Where installed extensions are read from: the profile itself, or, in a private window
    /// that allows extensions, the main profile.
    ext_src: std::path::PathBuf,
    favicons: favicons::Favicons,
    /// The icon table is only sent to the sidebar / start page when it changed.
    icons_for_ui: std::cell::Cell<bool>,
    icons_for_home: std::cell::Cell<bool>,
    /// Page events only mark the UI stale; it is refreshed once per event-loop turn.
    ui_stale: bool,
    last_flush: Instant,
    /// Native WebExtension host (macOS 15.4+); `None` where unsupported.
    webext: Option<blackhole_macos_webext::ExtensionHost>,
    installing: std::collections::HashSet<String>,
    /// Outdated extensions found by the last "Check for updates"; `None` until it is pressed.
    catalog_checked: Option<usize>,
    update: UpdateState,
    update_tx: std::sync::mpsc::Sender<UpdateMsg>,
    update_rx: std::sync::mpsc::Receiver<UpdateMsg>,
    next_update_check: Option<Instant>,
    /// A release check is running (background or Settings > Updates).
    update_checking: bool,
    /// What the last finished check said: up to date, or why it failed (Settings > Updates).
    update_checked: Option<Result<(), String>>,
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
    easel_panel: LazyView,
    easel_tab: bool,
    /// An Easel shown in the right pane beside a tab (a meeting's notes beside its Meet call),
    /// as `(tab, easel)`: on screen while that tab is the one shown and that easel the current.
    easel_beside: Option<(TabId, u64)>,
    /// The Library page, over the whole window below the bar, while `library`.
    library_panel: LazyView,
    library: bool,
    downloads: Vec<blackhole_infrastructure::store::DownloadRec>,
    dl_live: HashMap<u64, DownloadLive>,
    /// Tabs opened for a file alone (no page shown), by download URL: closed once it is done
    /// (closing the view earlier would cancel the download).
    dl_only_tabs: HashMap<String, TabId>,
    /// Tabs whose navigation just became a download: the engine ends that navigation as a
    /// failure, which is no error to show.
    dl_navs: HashMap<TabId, Instant>,
    /// Navigations ended as "connection aborted": an error page unless a download follows.
    aborted: HashMap<TabId, (Instant, Url)>,
    dl_polled: Instant,
    dl_size_tx: std::sync::mpsc::Sender<(u64, u64)>,
    dl_size_rx: std::sync::mpsc::Receiver<(u64, u64)>,
    archive: Vec<blackhole_infrastructure::store::ArchivedTab>,
    thumbs: std::cell::RefCell<HashMap<u64, Option<String>>>,
    easel: Option<EaselWindow>,
    easel_wanted: bool,
    /// Settings / Easel windows just closed: hidden at once, dropped on the next pass of the
    /// loop. Dropping a winit window while AppKit still treats it as the key window crashes on
    /// the next change of key window (macOS, `resignKeyWindow` on a freed delegate).
    closed_windows: Vec<Window>,
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

/// The main window's title (shown by the system: taskbar, window list).
fn main_title() -> String {
    if private_mode() {
        i18n::t("window.private")
    } else {
        "BlackHole".to_owned()
    }
}

/// The Edit menu is what routes Cmd+C/V/X/A/Z to the focused web view on macOS.
#[cfg(target_os = "macos")]
fn install_menu() -> muda::Menu {
    use i18n::t;
    use muda::{Menu, PredefinedMenuItem as Item, Submenu};
    let menu = Menu::new();
    let app = Submenu::new("BlackHole", true);
    let edit = Submenu::new(t("native.edit"), true);
    let _ = app.append_items(&[
        &Item::about(Some(&t("native.about")), None),
        &Item::separator(),
        &Item::hide(Some(&t("native.hide"))),
        &Item::hide_others(Some(&t("native.hide_others"))),
        &Item::separator(),
        &Item::quit(Some(&t("native.quit"))),
    ]);
    let _ = edit.append_items(&[
        &Item::undo(Some(&t("native.undo"))),
        &Item::redo(Some(&t("native.redo"))),
        &Item::separator(),
        &Item::cut(Some(&t("native.cut"))),
        &Item::copy(Some(&t("native.copy"))),
        &Item::paste(Some(&t("native.paste"))),
        &Item::select_all(Some(&t("native.select_all"))),
    ]);
    let _ = menu.append_items(&[&app, &edit]);
    menu.init_for_nsapp();
    menu
}

impl Running {
    fn new(event_loop: &ActiveEventLoop, mailbox: Mailbox) -> Self {
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(main_title())
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
        // Profiles of the black hole tabs of earlier runs (WebView2 keeps a folder per profile).
        #[cfg(windows)]
        {
            let (engine, keep) = (data.join("webview"), views::burner_prefix());
            std::thread::spawn(move || {
                blackhole_infrastructure::remove_burner_profiles(&engine, &keep);
            });
        }
        // Session, icons and extensions live in separate files: load them side by side.
        let (mut svc, favicons, preloaded_ext) = std::thread::scope(|scope| {
            let icons = scope.spawn(|| favicons::Favicons::load(&data));
            let exts =
                (!private_mode()).then(|| scope.spawn(|| load_extensions(&extensions_dir(&data))));
            let svc = BrowserService::start(
                JsonWorkspaceRepository::new(data.clone()),
                default_privacy_policy(),
            );
            (
                svc,
                icons.join().expect("favicons"),
                exts.map(|h| h.join().unwrap_or_default()),
            )
        });
        views::prepare_blocking(svc.policy());
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
            svc.set_extensions(
                preloaded_ext.unwrap_or_else(|| load_extensions(&extensions_dir(src))),
            );
        }
        views::set_hardware_acceleration(svc.settings().hardware_acceleration);
        // Before any page is created: each one receives the catalog of this language.
        i18n::apply(&svc.settings().language);
        window.set_title(&main_title());
        let sidebar = Sidebar::starting(
            f64::from(svc.settings().sidebar_width),
            svc.settings().sidebar_open,
        );

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
            palette_rect(false, &size),
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
            zoom: HashMap::new(),
            zoomed: HashSet::new(),
            raise_due: None,
            shown: None,
            pip_tab: None,
            pip_seen: false,
            pip_faded: None,
            busy_seen: HashMap::new(),
            pip_on_blur: None,
            blur_due: None,
            card_left: std::cell::Cell::new(f64::NAN),
            bar_left: std::cell::Cell::new(f64::NAN),
            bar_notice: std::cell::Cell::new((true, 0)),
            sampler: stats::Sampler::start(),
            usage: None,
            stats_tabs: 0,
            peers_due: Instant::now(),
            peers_published: Instant::now() - PEERS_PUBLISH_EVERY,
            loading: HashMap::new(),
            splash: HashMap::new(),
            awaiting: HashMap::new(),
            pending: HashMap::new(),
            errors: errors::Errors::default(),
            auth: http_auth::AuthPrompts::default(),
            auth_replies: HashMap::new(),
            auth_shown: None,
            auth_origins: HashMap::new(),
            signals: Default::default(),
            received: None,
            #[cfg(target_os = "macos")]
            cards_rounded: Instant::now(),
            media: Arc::default(),
            media_due: Instant::now(),
            media_hint: HashMap::new(),
            crashed: HashMap::new(),
            media_seen: serde_json::Value::Null,
            media_hold: media::Linger::new(media::LINGER),
            burner_next: false,
            pilot: None,
            links_cleaned: 0,
            undo: None,
            library_jump: (0, ""),
            panels_maxed: (false, false),
            last_frame: Instant::now(),
            live_pending: None,
            referrer_next: None,
            pick_watches: false,
            calendar: meet::Calendar::new(&data),
            nearby: handoff::Nearby::new(&data, private_mode()),
            watching: watching::Watching::new(&data),
            recall: recall::Recall::new(&data, private_mode()),
            marks: highlights::Marks::new(&data, private_mode()),
            comparing: compare::Comparing::new(),
            tidied: None,
            tidy_dismissed: false,
            media_dismissed: HashMap::new(),
            pip_shell_exits: HashMap::new(),
            svc,
            mailbox,
            sidebar,
            web_context,
            blocked: BlockLog::default(),
            settings_tab: None,
            bookmark_list: BookmarkStore::new(&data).load(),
            bookmarks: BookmarkStore::new(&data),
            favicons,
            icons_for_ui: std::cell::Cell::new(true),
            icons_for_home: std::cell::Cell::new(true),
            ui_stale: false,
            last_flush: Instant::now(),
            import_sources: import::detect(),
            import_msg: String::new(),
            app_ext,
            bar_exts: json!([]),
            bar_exts_at: Instant::now(),
            passwords: passwords::Passwords::new(&profile, private_mode()),
            settings_seq: 0,
            home_visible: true,
            splitbar: LazyView::new(),
            palette,
            menu: None,
            palette_ready: false,
            palette_open: false,
            palette_mode: String::new(),
            icon_target: None,
            peek: None,
            #[cfg(windows)]
            ext_popup: None,
            #[cfg(windows)]
            ext_anchors: HashMap::new(),
            webext,
            installing: Default::default(),
            catalog_checked: None,
            update: UpdateState::Idle,
            update_tx,
            update_rx,
            next_update_check: updates_enabled().then(|| Instant::now() + FIRST_UPDATE_CHECK),
            update_checking: false,
            update_checked: None,
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
            dl_only_tabs: HashMap::new(),
            dl_navs: HashMap::new(),
            aborted: HashMap::new(),
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
            easel_panel: LazyView::new(),
            easel_tab: false,
            easel_beside: None,
            library_panel: LazyView::new(),
            library: false,
            thumbs: std::cell::RefCell::default(),
            easel: None,
            easel_wanted: false,
            closed_windows: Vec::new(),
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
            return self.library_panel.view.as_ref().unwrap_or(&self.home);
        }
        if self.easel_tab {
            return self.easel_panel.view.as_ref().unwrap_or(&self.home);
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
        if self.split_tab().is_none() && !self.easel_side() {
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
        if self.home_shown() || self.easel_side() {
            return None;
        }
        self.svc.workspace().split_tab()
    }

    /// Whether an Easel fills the right pane beside the tab shown (`easel_beside`).
    fn easel_side(&self) -> bool {
        !self.home_shown()
            && self.easel_beside.is_some_and(|(tab, easel)| {
                self.easel_current == Some(easel)
                    && self.svc.workspace().active_tab().is_some_and(|t| t.id == tab)
            })
    }

    /// Where the embedded Easel page starts in the window (live views are placed from there).
    fn easel_origin(&self) -> (f64, f64) {
        let (x, y, width, _) = card_rect(&self.sidebar, &self.window_size());
        if self.easel_side() {
            let half = ((width - SPLIT_GAP) / 2.0).max(1.0);
            (x + half + SPLIT_GAP, y + SPLIT_HEADER)
        } else {
            (x, y)
        }
    }

    fn view_of(&self, tab: TabId) -> Option<&WebView> {
        if tab == PEEK_TAB {
            return self.peek.as_ref().map(|p| &p.page);
        }
        self.tabs.get(&tab)
    }

    /// Positions the views; `all` also resizes background tabs (window resize, tab switch).
    fn layout(&self, all: bool) {
        self.sync_bar_notice();
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
            .set_bounds(palette_rect(self.palette_open, &size));
        if let Some(peek) = &self.peek {
            let _ = peek
                .scrim
                .set_bounds(views::rect(0.0, 0.0, size.width, size.height));
            let _ = peek.page.set_bounds(self.peek_bounds());
        }
        if all {
            let _ = self.home.set_bounds(self.full_content_bounds());
            for view in self.tabs.values() {
                let _ = view.set_bounds(self.full_content_bounds());
            }
        }
        let (cx, cy, cw, _) = card_rect(&self.sidebar, &size);
        if let Some(bar) = &self.splitbar.view {
            let _ = bar.set_bounds(views::rect(cx, cy, cw, SPLIT_HEADER));
        }
        let (main, side) = self.content_rects();
        let main = if self.sidebar.moving() && side.is_none() {
            sliding(main, &size)
        } else {
            main
        };
        let _ = self.active_view().set_bounds(main);
        self.place_panels();
        self.apply_slots();
        if let (Some(id), Some(rect)) = (self.split_tab(), side) {
            if let Some(view) = self.tabs.get(&id) {
                let _ = view.set_bounds(rect);
            }
        }
        self.round_cards(all);
        self.place_card_shadow();
    }

    /// Sizes the Library and Easel panels, or parks them while hidden: they stay alive a minute
    /// after closing, and a hidden full-size view would still swallow the drags of the page
    /// under it (e.g. moving a Jira ticket), like the command bar did.
    fn place_panels(&self) {
        if let Some(panel) = &self.easel_panel.view {
            let rect = match self.content_rects() {
                (main, _) if self.easel_tab => main,
                (_, Some(side)) if self.easel_side() => side,
                _ => parked(),
            };
            let _ = panel.set_bounds(rect);
        }
        if let Some(panel) = &self.library_panel.view {
            let rect = if self.library { library_rect(&self.window_size()) } else { parked() };
            let _ = panel.set_bounds(rect);
        }
    }

    /// Rounds the page views that fill the card (the others are rounded when they are shown).
    fn round_cards(&self, all: bool) {
        let scale = self.scale();
        let mut cards = vec![&self.home, self.active_view()];
        cards.extend(self.easel_panel.view.as_ref());
        if let Some(side) = self.split_tab().and_then(|id| self.tabs.get(&id)) {
            cards.push(side);
        }
        if all {
            cards.extend(self.tabs.values());
        }
        for view in cards {
            views::round_corners(view, scale);
        }
    }

    /// Tells the sidebar page where the card starts so the shadow under it follows the card.
    fn place_card_shadow(&self) {
        let size = self.window_size();
        let card_x = card_rect(&self.sidebar, &size).0;
        let moved = self.bar_left.get().is_nan() || (self.bar_left.get() - card_x).abs() >= 0.5;
        if moved {
            self.bar_left.set(card_x);
            let _ = self.bar.evaluate_script(&format!(
                "document.documentElement.style.setProperty('--cl','{card_x}px')"
            ));
        }
        let left = card_x - (self.sidebar.width() - self.sidebar.full());
        if (self.card_left.get() - left).abs() < 0.5 {
            return;
        }
        self.card_left.set(left);
        let origin = self.sidebar.full() - self.sidebar.width();
        let _ = self.chrome.evaluate_script(&format!(
            "{{const s=document.documentElement.style;s.setProperty('--cl','{left}px');s.setProperty('--ox','{origin}px')}}"
        ));
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
        self.svc.set_sidebar_open(self.sidebar.is_open());
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

    /// Sends the live tab count and the browser's memory / CPU use to the sidebar.
    fn service_stats(&mut self) {
        let tabs = self.tabs.len();
        let mut changed = false;
        // The count is shown at once; the sampler waits for the engine to return its memory.
        if tabs != self.stats_tabs {
            self.stats_tabs = tabs;
            self.sampler.poke();
            changed = true;
        }
        if let Some(usage) = self.sampler.latest() {
            changed |= self.usage != Some(usage);
            self.usage = Some(usage);
        }
        let (true, Some(usage)) = (changed, self.usage) else {
            return;
        };
        self.ui(format!(
            "window.bh&&bh.stats&&bh.stats({tabs},{},{:.0})",
            usage.memory_mb, usage.cpu
        ));
    }

    /// A hidden sidebar slides out by itself for a moment: what just happened shows in it.
    fn reveal_sidebar(&mut self, time: Duration) {
        if !self.library && self.sidebar.reveal(Instant::now(), time) {
            self.layout(false);
        }
    }

    /// The navigation of `tab` just turned into a download.
    fn download_took(&self, tab: TabId) -> bool {
        self.dl_navs.get(&tab).is_some_and(|at| at.elapsed() < DOWNLOAD_NAV)
    }

    fn tick_sidebar(&mut self) {
        if self.sidebar.moving() {
            self.sidebar.tick();
            self.layout(false);
        }
    }

    // ---- state → UI -------------------------------------------------------------------

    pub(crate) fn ui(&self, js: String) {
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
        self.sync_bar_notice();
        let ws = self.svc.workspace();
        let space = ws.active_space();
        let mut state = json!({
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
            "collapsed": self.svc.settings().collapsed_sections,
            "color": space.color,
            "activeTab": if self.home_shown() { None } else { space.active_tab.map(|t| t.0) },
            "home": self.home_shown(),
            "blocked": self.blocked.total(),
            "version": env!("CARGO_PKG_VERSION"),
            "update": self.update_json(),
            "downloads": self.downloads_json(),
            "folders": space.folders.iter().map(|f| json!({"id": f.id.0, "name": f.name, "collapsed": f.collapsed, "icon": f.icon, "parent": f.parent.map(|p| p.0)})).collect::<Vec<_>>(),
            "media": self.media_json(),
            "meeting": self.calendar.card(),
            "signals": self.signals_json(),
            "devices": self.nearby.menu_json(),
            "tidy": self.tidy_json(),
            "easels": self.easels.all().iter().filter(|e| self.easel_space(e) == space.id.0).map(|e| json!({"id": e.id, "name": e.name, "folder": e.folder, "count": e.items.len(), "open": self.easel_tab && self.easel_current == Some(e.id)})).collect::<Vec<_>>(),
            "tabs": space.tabs.iter().map(|t| json!({
                "id": t.id.0, "title": t.title, "url": t.url.as_str(), "pinned": t.pinned,
                "folder": t.folder.map(|f| f.0),
                "split": Some(t.id) == self.split_tab() || (self.split_tab().is_some() && Some(t.id) == space.active_tab),
                "asleep": !self.tabs.contains_key(&t.id),
                "away": t.away(),
                "burner": ws.is_burner(t.id),
                // Its menu offers "Continue on …" (`domain::verbs`, the rule of every menu).
                "send": verbs::allows(Target::Tab, verbs::Context {
                    web: matches!(t.url.scheme(), "http" | "https"),
                    burner: ws.is_burner(t.id),
                    paired: self.paired(),
                    ..Default::default()
                }, Verb::Send),
            })).collect::<Vec<_>>(),
        });
        if self.icons_for_ui.replace(false) {
            state["favicons"] = json!(self.favicons.all());
        }
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
        let pilot = self.pilot_panes().map(|(pilot, _)| pilot);
        let pane = |t: &blackhole_domain::Tab| json!({"id": t.id.0, "title": t.title, "url": t.url.as_str(), "pilot": pilot == Some(t.id)});
        let other = other.and_then(|id| ws.active_space().tabs.iter().find(|t| t.id == id));
        let Some(splitbar) = &self.splitbar.view else {
            return;
        };
        // The Easel beside the tab is the right pane.
        let easel = self
            .easel_beside
            .filter(|_| self.easel_side())
            .and_then(|(_, id)| self.easels.get(id))
            .map(|e| json!({"id": e.id, "title": e.name, "url": "", "easel": true}));
        let right = other.map(pane).or(easel);
        let _ = splitbar.set_visible(active.is_some() && right.is_some());
        if let (Some(left), Some(right)) = (active, right) {
            let state =
                json!({"left": pane(left), "right": right, "color": ws.active_space().color});
            let _ = splitbar.evaluate_script(&format!("window.bh&&bh.render({state})"));
        }
    }

    fn ui_settings(&self) {
        let Some(win) = &self.settings else { return };
        let settings = self.svc.settings();
        let mut state = json!({
            "os": std::env::consts::OS,
            // The window's own top bar: the space's color and the maximize button's state.
            "color": self.svc.workspace().active_space().color,
            "maximized": win.window.is_maximized(),
            "version": env!("CARGO_PKG_VERSION"),
            "engine": settings.search_engine.id(),
            "engines": SearchEngine::ALL.iter().map(|e| json!({"id": e.id(), "label": e.label()})).collect::<Vec<_>>(),
            "sidebarWidth": settings.sidebar_width,
            "sidebarWidths": Settings::SIDEBAR_WIDTH_CHOICES,
            "animations": settings.animations,
            "hardwareAcceleration": settings.hardware_acceleration,
            "extensionsInPrivate": settings.extensions_in_private,
            "trackerBlocking": settings.tracker_blocking,
            "calendar": self.calendar_json(),
            "devices": self.devices_json(),
            "adBlocking": settings.ad_blocking,
            "shields": {
                "hideAds": settings.shields.hide_ads,
                "videoAds": settings.shields.video_ads,
                "cookieBanners": settings.shields.cookie_banners,
                "cleanLinks": settings.shields.clean_links,
            },
            "tabSleep": settings.tab_sleep_minutes,
            "tabSleepChoices": Settings::SLEEP_CHOICES,
            "maxLiveTabs": settings.max_live_tabs,
            "maxLiveTabsChoices": Settings::LIVE_TABS_CHOICES,
            "spareViews": settings.spare_views,
            "spareChoices": Settings::SPARE_CHOICES,
            "blocked": self.blocked.total(),
            "blockedLog": self.blocked.entries().iter().take(100).map(|b| json!({"host": b.host, "count": b.count, "site": b.site})).collect::<Vec<_>>(),
            "tab": self.settings_tab.clone(),
            "tabSeq": self.settings_seq,
            "catalog": self.catalog_state(),
            "catalogChecked": self.catalog_checked,
            "appUpdate": self.app_update_json(),
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
                "message": i18n::render(&self.import_msg),
                "bookmarks": self.bookmarks.load().len(),
                "logins": self.passwords.count(),
            }),
            "passwords": self.passwords.state_json(settings),
            "extensionsDir": extensions_dir(&self.data).display().to_string(),
            "extensions": self.svc.extensions().iter().map(|e| json!({
                "id": e.id, "name": e.name, "version": e.version, "description": e.description,
                "enabled": self.svc.extension_enabled(&e.id),
                "sites": e.scripts.iter().flat_map(|s| s.matches.iter().cloned()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        // General > Language: the setting, the languages in their own words, and what "System"
        // gives on this computer.
        state["recall"] = self.recall_state();
        state["language"] = json!(settings.language);
        state["languages"] = json!(blackhole_domain::i18n::LANGUAGES
            .iter()
            .map(|(id, label)| json!({"id": id, "label": label}))
            .collect::<Vec<_>>());
        state["systemLanguage"] = json!(blackhole_domain::i18n::resolve(
            blackhole_domain::i18n::SYSTEM,
            &i18n::system_languages()
        ));
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
        let (msg, list) = store_bookmarks(&self.bookmarks, list, &message("settings.import_from_drop", &[]), space);
        self.bookmark_list = self.bookmarks.load();
        self.file_bookmarks(list);
        self.ui_state();
        self.toast(&msg);
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
                // The catalog's descriptions are translated by extension id (`extdesc.<id>`).
                let key = format!("extdesc.{}", e.id);
                let description = Some(t(&key)).filter(|d| *d != key).unwrap_or_else(|| e.description.to_owned());
                json!({
                    "id": e.id, "name": e.name, "description": description, "version": e.version,
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
        self.poll_passwords();
        while let Ok((msg, _count, list)) = self.import_rx.try_recv() {
            if let Some(m) = msg.filter(|m| !m.is_empty()) {
                self.import_msg = m;
            }
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

    /// Sidebar button: only for an installation that can replace itself.
    fn update_json(&self) -> serde_json::Value {
        if !can_self_update() {
            return json!(null);
        }
        match &self.update {
            UpdateState::Idle => json!(null),
            UpdateState::Available(r) => json!({"state": "available", "version": r.version}),
            UpdateState::Downloading(r) => json!({"state": "downloading", "version": r.version}),
            UpdateState::Failed(r, why) => {
                json!({"state": "error", "version": r.version, "error": why})
            }
        }
    }

    /// Settings > Updates: the check and the install, step by step.
    fn app_update_json(&self) -> serde_json::Value {
        let (state, version, error) = match (&self.update, &self.update_checked) {
            (UpdateState::Downloading(r), _) => ("downloading", Some(&r.version), None),
            _ if self.update_checking => ("checking", None, None),
            (UpdateState::Available(r), _) => ("available", Some(&r.version), None),
            (UpdateState::Failed(r, why), _) => ("install_failed", Some(&r.version), Some(why)),
            (UpdateState::Idle, None) => ("idle", None, None),
            (UpdateState::Idle, Some(Ok(()))) => ("current", None, None),
            (UpdateState::Idle, Some(Err(why))) => ("check_failed", None, Some(why)),
        };
        json!({
            "state": state,
            "version": version,
            "error": error,
            "installable": can_self_update(),
            "page": format!("https://github.com/{}/releases/latest", updater::REPO),
        })
    }

    /// Looks for a newer release off the UI thread; the answer comes back through `update_rx`.
    fn check_for_update(&mut self) {
        if self.update_checking || matches!(self.update, UpdateState::Downloading(_)) {
            return;
        }
        self.update_checking = true;
        let tx = self.update_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(match updater::check(env!("CARGO_PKG_VERSION")) {
                Ok(Some(release)) => UpdateMsg::Found(release),
                Ok(None) => UpdateMsg::Current,
                Err(why) => UpdateMsg::CheckFailed(why),
            });
        });
    }

    /// Runs the periodic release check and applies what the background threads report.
    fn poll_update(&mut self) {
        if let Some(at) = self.next_update_check {
            if Instant::now() >= at {
                self.next_update_check = Some(Instant::now() + UPDATE_CHECK_EVERY);
                self.check_for_update();
            }
        }
        while let Ok(msg) = self.update_rx.try_recv() {
            match msg {
                UpdateMsg::Found(release) => {
                    self.update_checking = false;
                    self.update_checked = Some(Ok(()));
                    if !matches!(self.update, UpdateState::Downloading(_)) {
                        self.update = UpdateState::Available(release);
                    }
                }
                UpdateMsg::Current => {
                    self.update_checking = false;
                    self.update_checked = Some(Ok(()));
                }
                UpdateMsg::CheckFailed(why) => {
                    tracing::warn!("update check failed: {why}");
                    self.update_checking = false;
                    self.update_checked = Some(Err(why));
                    // A failed check no longer waits for the next 6-hour round.
                    let retry = Instant::now() + UPDATE_RETRY;
                    if let Some(at) = &mut self.next_update_check {
                        *at = (*at).min(retry);
                    }
                }
                UpdateMsg::Downloaded(path) => {
                    if let UpdateState::Downloading(release) =
                        std::mem::replace(&mut self.update, UpdateState::Idle)
                    {
                        match updater::apply(&path) {
                            Ok(()) => {
                                self.quit = true;
                                self.update = UpdateState::Downloading(release);
                            }
                            Err(why) => self.update = UpdateState::Failed(release, why),
                        }
                    }
                }
                UpdateMsg::Failed(why) => {
                    if let UpdateState::Downloading(release) =
                        std::mem::replace(&mut self.update, UpdateState::Idle)
                    {
                        self.update = UpdateState::Failed(release, why);
                    }
                }
            }
            self.ui_state();
        }
    }

    /// Sidebar button: downloads the new version, then the installer takes over and restarts.
    fn start_update(&mut self) {
        if !can_self_update() {
            return;
        }
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
        let attrs = self.panel_window_attributes(&i18n::t("window.settings"), (820.0, 600.0));
        let Ok(window) = event_loop.create_window(attrs) else {
            return;
        };
        window.focus_window();
        match views::panel_view(&window, &mut self.web_context, SETTINGS_HTML, &self.mailbox) {
            Ok(view) => self.settings = Some(SettingsWindow { view, window }),
            Err(err) => tracing::error!("cannot create settings window: {err}"),
        }
    }

    fn close_settings(&mut self) {
        if let Some(SettingsWindow { view, window }) = self.settings.take() {
            drop(view);
            self.retire_window(window);
        }
    }

    /// Hides a secondary window now (AppKit hands the key window over while it is still alive)
    /// and drops it on the next pass of the event loop.
    fn retire_window(&mut self, window: Window) {
        window.set_visible(false);
        self.closed_windows.push(window);
    }

    /// A secondary window (Settings, Easel) dressed like the main one: its page draws the top
    /// bar (title, and on Windows the minimize / maximize / close buttons). Owned by the main
    /// window on Windows so it always stays above it.
    fn panel_window_attributes(
        &self,
        title: &str,
        (width, height): (f64, f64),
    ) -> winit::window::WindowAttributes {
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(title)
            .with_theme(Some(winit::window::Theme::Dark))
            .with_inner_size(LogicalSize::new(width, height))
            .with_min_inner_size(LogicalSize::new(640.0, 440.0));
        #[cfg(windows)]
        {
            use winit::{
                platform::windows::{IconExtWindows, WindowAttributesExtWindows},
                raw_window_handle::{HasWindowHandle, RawWindowHandle},
            };
            attrs = attrs.with_decorations(false).with_undecorated_shadow(true);
            if let Ok(icon) = winit::window::Icon::from_resource(1, None) {
                attrs = attrs.with_window_icon(Some(icon));
            }
            if let Ok(handle) = self.window.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    attrs = attrs.with_owner_window(h.hwnd.get());
                }
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
        attrs
    }

    /// The buttons and drag area of a secondary window's top bar.
    fn panel_window_action(&mut self, which: &str, action: &str) {
        let window = match which {
            "settings" => self.settings.as_ref().map(|w| &w.window),
            "easel" => self.easel.as_ref().map(|w| &w.window),
            _ => None,
        };
        let Some(window) = window else { return };
        match action {
            "drag" => {
                let _ = window.drag_window();
            }
            "minimize" => window.set_minimized(true),
            "maximize" => {
                window.set_maximized(!window.is_maximized());
            }
            "close" => match which {
                "settings" => self.close_settings(),
                _ => self.close_easel(),
            },
            _ => {}
        }
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
        let attrs = self.panel_window_attributes(&i18n::t("window.easel"), (1100.0, 720.0));
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
        if let Some(EaselWindow { view, window }) = self.easel.take() {
            drop(view);
            self.retire_window(window);
        }
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

    /// Shows a tab with an easel in the right pane beside it (a meeting's notes beside the
    /// call). It replaces the tab's split view; going back to that tab shows the easel again.
    pub(crate) fn open_easel_side(&mut self, tab: TabId, id: u64) {
        let known = self.svc.workspace().active_space().tabs.iter().any(|t| t.id == tab);
        if self.easels.get(id).is_none() || !known {
            return;
        }
        self.close_palette();
        self.close_peek();
        if self.easel_tab || self.easel_current != Some(id) {
            self.live.clear();
            self.live_slots.clear();
        }
        self.easel_current = Some(id);
        self.easel_tab = false;
        self.library = false;
        self.home_visible = false;
        self.svc.activate_tab(tab);
        self.svc.clear_split();
        self.easel_beside = Some((tab, id));
        self.sync();
    }

    fn close_easel_side(&mut self) {
        if self.easel_beside.take().is_none() {
            return;
        }
        if !self.easel_tab {
            self.live.clear();
            self.live_slots.clear();
        }
        self.sync();
    }

    /// Keeps the easel page filling its window; live views follow `EaselLayout`.
    fn settings_resized(&self) {
        if let Some(win) = &self.settings {
            let size = win
                .window
                .inner_size()
                .to_logical::<f64>(win.window.scale_factor());
            let _ = win
                .view
                .set_bounds(views::rect(0.0, 0.0, size.width, size.height));
        }
    }

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
        let host = if self.easel_tab || self.easel_side() {
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
                self.notify(crate::notices::Notice::Problem, &t("toast.live_failed"), serde_json::Value::Null);
            }
        }
        self.ui_easel();
    }

    fn apply_slot(&self, item: u64) {
        let (Some(view), Some(slot)) = (self.live.get(&item), self.live_slots.get(&item)) else {
            return;
        };
        // Slots are relative to the Easel page: the content area when it is a tab, the right
        // pane when it is beside a tab.
        let (ox, oy) = if self.easel_tab || self.easel_side() {
            self.easel_origin()
        } else {
            (0.0, 0.0)
        };
        let _ = view.set_bounds(views::rect(ox + slot.x, oy + slot.y, slot.w, slot.h));
        // The palette and the peek only cover the main window: an Easel window of its own keeps
        // its live views.
        let embedded = self.easel_tab || self.easel_side();
        let covered = embedded && (self.palette_open || self.peek.is_some());
        let _ = view.set_visible(slot.visible && !covered && slot.w >= 2.0 && slot.h >= 2.0);
    }

    /// Re-applies every live view's place and visibility (an overlay opened or closed).
    fn apply_slots(&self) {
        for item in self.live.keys() {
            self.apply_slot(*item);
        }
    }

    /// Arms the picker on the page in front.
    fn live_pick_start(&mut self) {
        let tab = self.svc.workspace().active_tab().map(|t| t.id);
        let (Some(tab), false) = (tab, self.home_shown()) else {
            self.pick_watches = false;
            self.toast(&t("toast.open_page_for_area"));
            return;
        };
        // A live area and a watched area keep a trace of the page: never for a black hole
        // tab (`domain::verbs`, the same rule as the menus).
        if !self.front_page_allows(Verb::LiveArea) {
            self.pick_watches = false;
            let key = if self.svc.workspace().is_burner(tab) { "toast.burner_keeps_nothing" } else { "toast.open_page_for_area" };
            self.toast(&t(key));
            return;
        }
        let Some(view) = self.tabs.get(&tab) else {
            return;
        };
        if view.evaluate_script(&views::live_picker_js()).is_ok() {
            self.live_pick = Some(tab);
        } else {
            self.pick_watches = false;
        }
    }

    /// An area was picked: ask which Easel of the space it goes to (or a new one), or whether
    /// to watch it for changes.
    fn live_picked(&mut self, tab: TabId, selector: String, size: (f64, f64)) {
        // Picked from "Watch an area…": nothing more to ask.
        if std::mem::take(&mut self.pick_watches) {
            self.watch_area(tab, &selector);
            return;
        }
        let space = self.svc.workspace().active_space().id.0;
        let easels: Vec<(u64, String)> = self
            .easels
            .all()
            .iter()
            .filter(|e| self.easel_space(e) == space)
            .map(|e| (e.id, e.name.clone()))
            .collect();
        self.live_pending = Some((tab, selector, size));
        let current = self.easel_current;
        let mut items = vec![json!({"head": t("menu.live_add_to")})];
        items.extend(easels.iter().map(|(id, name)| {
            json!({
                "icon": if Some(*id) == current { "check" } else { "easel" },
                "label": name, "run": {"cmd": "live_capture_to", "easel": id},
            })
        }));
        items.push(json!({"sep": true}));
        items.push(json!({"icon": "plus", "label": t("menu.new_easel"), "run": {"cmd": "live_capture_to", "easel": null}}));
        items.push(json!({"sep": true}));
        items.push(json!({"icon": "watch", "label": t("menu.watch_area"), "run": {"cmd": "watch_area"}}));
        // In the middle of the page, where the area was just picked.
        let (x, y) = self.view_origin(tab);
        let size = self.window_size();
        let at = (x + (size.width - x) / 2.0 - 120.0, y + 120.0);
        self.show_menu(at, json!(items));
    }

    /// Top-left corner of a page view in the window (logical pixels).
    fn view_origin(&self, tab: TabId) -> (f64, f64) {
        let scale = self.scale();
        self.view_of(tab)
            .and_then(|v| v.bounds().ok())
            .map(|b| {
                let p = b.position.to_logical::<f64>(scale);
                (p.x, p.y)
            })
            .unwrap_or((card_rect(&self.sidebar, &self.window_size()).0, BAR_HEIGHT))
    }

    /// Shows a menu (drawn by the command bar overlay) at a point of the window.
    fn show_menu(&mut self, (x, y): (f64, f64), items: serde_json::Value) {
        self.menu = Some(json!({"x": x, "y": y, "items": items}));
        self.open_palette("menu");
    }

    /// Right click on a link of a page: open it in a new tab, a new window, a private window,
    /// preview it, or copy it. On selected text (`text`, untrusted): search it with the engine
    /// of the Settings, or open it when it is a web address, or copy it.
    fn link_menu(&mut self, tab: TabId, raw: &str, text: &str, mark: u64, image: &str, (x, y): (f64, f64)) {
        use blackhole_domain::selection::{
            clean_selection, excerpt, selection_action, SelectionAction,
        };
        let link = Url::parse(raw)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        let selection = clean_selection(text);
        let mut items = Vec::new();
        if let Some(text) = &selection {
            let short = excerpt(text, 30);
            let action = selection_action(text, self.svc.settings().search_engine);
            let (icon, label) = match &action {
                SelectionAction::Open(_) => ("open", tf("menu.open_address", &[("text", &short)])),
                SelectionAction::Search(_) => ("search", tf("menu.search_for", &[("text", &short)])),
            };
            let url = action.url().as_str();
            // Ctrl/Cmd+click or middle click on the entry: in the background, like a link.
            items.push(json!({"icon": icon, "label": label,
                "run": {"cmd": "open_link", "url": url, "target": "front"},
                "background": {"cmd": "open_link", "url": url, "target": "tab"}}));
            items.push(json!({"icon": "copy", "label": t("menu.copy"), "copy": text}));
            let can = verbs::verbs(Target::Selection, self.verb_context(tab));
            if can.contains(&Verb::Send) {
                items.extend(self.beam_menu_items(text));
            }
            // The selection is read now, while it is there: the entry only confirms it.
            if can.contains(&Verb::Highlight) && self.can_mark(tab) {
                self.mark_capture(tab, false);
                items.push(json!({"icon": "marker", "label": t("menu.highlight"), "run": {"cmd": "highlight_selection", "tab": tab.0}}));
            }
        }
        let on_mark = if mark == 0 { Vec::new() } else { self.mark_menu_items(tab, mark) };
        if !on_mark.is_empty() {
            if !items.is_empty() {
                items.push(json!({"sep": true}));
            }
            items.extend(on_mark);
        }
        // "Copy link" copies the clean address: what is pasted elsewhere tracks nobody.
        let clean = link
            .as_ref()
            .filter(|_| self.svc.settings().shields.clean_links)
            .and_then(blackhole_domain::clean_link::clean);
        if let Some(url) = &link {
            let link = url.as_str();
            let copied = clean.as_ref().map_or(link, Url::as_str);
            if !items.is_empty() {
                items.push(json!({"sep": true}));
            }
            items.extend([
                json!({"icon": "plus", "label": t("menu.link_new_tab"), "run": {"cmd": "open_link", "url": link, "target": "tab"}}),
                json!({"icon": "window", "label": t("menu.link_new_window"), "run": {"cmd": "open_link", "url": link, "target": "window"}}),
                json!({"icon": "sleep", "label": t("menu.link_private_window"), "run": {"cmd": "open_link", "url": link, "target": "private"}}),
                json!({"icon": "hole", "label": t("menu.link_burner"), "run": {"cmd": "open_link", "url": link, "target": "burner"}}),
                json!({"sep": true}),
                json!({"icon": "open", "label": t("menu.link_peek"), "run": {"cmd": "open_peek", "url": link}}),
                json!({"icon": "link", "label": t(if clean.is_some() { "menu.copy_clean_link" } else { "menu.copy_link" }), "copy": copied}),
            ]);
            // Keep it, send it: the same rule as every other menu (`domain::verbs`).
            let ctx = verbs::Context { web: true, ..self.verb_context(tab) };
            for verb in verbs::verbs(Target::Link, ctx) {
                match verb {
                    Verb::Keep => items.push(json!({"icon": "easel", "label": t("menu.link_to_easel"),
                        "run": {"cmd": "link_to_easel", "url": copied}})),
                    Verb::Send => items.extend(self.send_items("menu.link_send_to", |d| json!({"cmd": "send_link", "device": d, "url": copied}))),
                    _ => {}
                }
            }
        }
        let image = Url::parse(image)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        if let Some(image) = &image {
            let image = image.as_str();
            if !items.is_empty() {
                items.push(json!({"sep": true}));
            }
            items.extend([
                json!({"icon": "image", "label": t("menu.image_new_tab"),
                    "run": {"cmd": "open_link", "url": image, "target": "front"},
                    "background": {"cmd": "open_link", "url": image, "target": "tab"}}),
                json!({"icon": "link", "label": t("menu.copy_image_address"), "copy": image}),
            ]);
        }
        // Off a link and off a selection: the page's own entries. On one, BlackHole's tools
        // (Easel, live area, watched area) still follow, so the menu offers them wherever the
        // click lands.
        let more = if link.is_none() && selection.is_none() {
            self.page_menu_items(tab)
        } else {
            self.page_tool_items(tab)
        };
        if !items.is_empty() && !more.is_empty() {
            items.push(json!({"sep": true}));
        }
        items.extend(more);
        if items.is_empty() {
            return;
        }
        let (ox, oy) = self.view_origin(tab);
        self.show_menu((ox + x, oy + y), json!(items));
    }

    /// BlackHole's own tools for the page in front: clip it to the Easel, capture a live area,
    /// watch an area. None for another pane, a Peek, an error page, or a black hole tab
    /// (nothing of it is kept).
    fn page_tool_items(&self, tab: TabId) -> Vec<serde_json::Value> {
        verbs::verbs(Target::Page, self.verb_context(tab))
            .into_iter()
            .filter_map(|verb| match verb {
                Verb::Keep => Some(json!({"icon": "easel", "label": t("menu.page_to_easel"), "run": {"cmd": "capture_to_easel"}})),
                Verb::LiveArea => Some(json!({"icon": "eye", "label": t("menu.live_capture"), "run": {"cmd": "live_pick"}})),
                Verb::Watch => Some(json!({"icon": "watch", "label": t("menu.watch_pick"), "run": {"cmd": "watch_pick"}})),
                _ => None,
            })
            .collect()
    }

    /// "Send this page to …": the tab goes to a paired device (`domain::verbs`, Send).
    fn page_send_items(&self, tab: TabId) -> Vec<serde_json::Value> {
        if !verbs::allows(Target::Page, self.verb_context(tab), Verb::Send) {
            return Vec::new();
        }
        self.send_items("menu.page_send_to", |d| json!({"cmd": "continue_on", "id": tab.0, "device": d}))
    }

    /// The entries of a right click on the page itself: history, the Easel, a live area, a
    /// watched area, the address, find, print, the inspector. Those that act on "the page in
    /// front" are only offered for it (not for the other pane of a split, nor for a Peek).
    fn page_menu_items(&self, tab: TabId) -> Vec<serde_json::Value> {
        let ws = self.svc.workspace();
        let front = ws.active_tab().is_some_and(|t| t.id == tab) && !self.home_shown();
        let address = self
            .view_of(tab)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok())
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        let mut items = Vec::new();
        if front {
            let can = |f: fn(&WebView) -> wry::Result<bool>| {
                self.tabs.get(&tab).is_some_and(|v| f(v).unwrap_or(false))
            };
            if can(WebView::can_go_back) {
                items.push(json!({"icon": "back", "label": t("menu.page_back"), "run": {"cmd": "back"}}));
            }
            if can(WebView::can_go_forward) {
                items.push(json!({"icon": "forward", "label": t("menu.page_forward"), "run": {"cmd": "forward"}}));
            }
            items.push(json!({"icon": "reload", "label": t("menu.page_reload"), "run": {"cmd": "reload"}}));
            let tools = self.page_tool_items(tab);
            if !tools.is_empty() {
                items.push(json!({"sep": true}));
                items.extend(tools);
            }
            items.push(json!({"sep": true}));
        }
        if let Some(url) = &address {
            let clean = self
                .svc
                .settings()
                .shields
                .clean_links
                .then(|| blackhole_domain::clean_link::clean(url))
                .flatten();
            items.push(json!({"icon": "link", "label": t("menu.copy_page_link"),
                "copy": clean.as_ref().map_or(url.as_str(), Url::as_str)}));
            items.extend(self.page_send_items(tab));
        }
        if front {
            items.extend([
                json!({"icon": "search", "label": t("menu.find_in_page"), "run": {"cmd": "shortcut", "key": "f"}}),
                json!({"icon": "print", "label": t("menu.print"), "run": {"cmd": "shortcut", "key": "p"}}),
                json!({"sep": true}),
                json!({"icon": "code", "label": t("menu.inspect"), "run": {"cmd": "shortcut", "key": "devtools"}}),
            ]);
        }
        while items.last().is_some_and(|i| i["sep"] == json!(true)) {
            items.pop();
        }
        items
    }

    /// Middle click / Ctrl+click on a link, or "Open link in new tab": the tab opens at the top
    /// of the list and starts loading, while the page shown stays on screen.
    fn open_in_background(&mut self, raw: &str) {
        let Ok(url) = Url::parse(raw) else { return };
        if !matches!(url.scheme(), "http" | "https") {
            return;
        }
        let ws = self.svc.workspace();
        let (space, shown) = (ws.active_space().id, ws.active_tab().map(|t| t.id));
        let Some(id) = self.svc.open_tab(url.as_str()) else {
            return;
        };
        // Air Traffic may file it in another space: it goes there, the user stays here.
        self.svc.switch_space(space);
        if let Some(prev) = shown.filter(|prev| *prev != id) {
            self.svc.activate_tab(prev);
        }
        let tab = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .find(|t| t.id == id)
            .cloned();
        if let Some(tab) = tab {
            self.ensure_view(&tab);
        }
        self.sync();
        self.reveal_sidebar(LINK_REVEAL);
    }

    /// Adds a live card for `selector` of the page of `tab` to `easel` (`None`: a new Easel);
    /// the address comes from the engine.
    fn capture_live(&mut self, tab: TabId, selector: &str, size: (f64, f64), easel: Option<u64>) {
        let page = self.tabs.get(&tab).and_then(|v| v.url().ok());
        let title = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .find(|t| t.id == tab)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let easel = match easel.filter(|id| self.easels.get(*id).is_some()) {
            Some(id) => {
                self.easel_current = Some(id);
                Some(id)
            }
            None => {
                self.easel_current = self.create_easel("");
                self.tag_easel_space();
                self.easel_current
            }
        };
        let (Some(url), Some(easel)) = (page, easel) else {
            self.notify(crate::notices::Notice::Problem, &t("toast.cannot_capture"), serde_json::Value::Null);
            return;
        };
        match self
            .easels
            .capture_live(easel, &url, &title, selector, size)
        {
            Ok(_) => {
                self.easel_changed(true);
                self.notify(notices::Notice::Easel, &t("toast.live_added"), serde_json::Value::Null);
            }
            Err(why) => self.toast(&t(why)),
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

    /// A new Easel; a blank name becomes "Easel N" in the interface language.
    fn create_easel(&mut self, name: &str) -> Option<u64> {
        let name = if name.trim().is_empty() {
            tf("easel.default_name", &[("n", &(self.easels.all().len() + 1).to_string())])
        } else {
            name.to_owned()
        };
        self.easels.create(&name)
    }

    /// The current easel, created on first use so a capture never fails for lack of a board.
    fn easel_or_create(&mut self) -> Option<u64> {
        if self.current_easel().is_none() {
            self.easel_current = self.create_easel("");
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
            "history": self.svc.recent_activity(usize::MAX).iter().map(|e| json!({
                "title": e.title, "url": e.url.as_str(), "at": e.visited_at,
            })).collect::<Vec<_>>(),
            "now": now_secs(),
            // A section to show, applied once per request (the page keeps its own choice after).
            "jump": json!([self.library_jump.0, self.library_jump.1]),
            "animations": self.svc.settings().animations,
            "watches": self.watches_json(),
            // The Easel ticked in the "Add to an Easel" menus.
            "easelCurrent": self.easel_current,
            // Paired devices: a downloaded file can be sent to one (the Wormhole).
            "devices": self.devices_brief(),
            "highlights": self.marks_json(),
        });
        if let Some(panel) = &self.library_panel.view {
            let _ = panel.evaluate_script(&format!("window.bh&&bh.render({state})"));
        }
    }

    fn open_library(&mut self, section: Option<&str>) {
        // Only the Library's own sections: the page is told which one to show.
        if let Some(name) = section.and_then(|s| LIBRARY_SECTIONS.iter().find(|n| **n == s)) {
            self.library_jump = (self.library_jump.0 + 1, name);
        }
        self.close_palette();
        self.close_peek();
        self.close_easel_tab();
        self.library = true;
        let fresh = self.library_panel.view.is_none();
        self.sync();
        self.ui_library();
        // A page still loading plays its intro when it reports `library_ready`.
        if !fresh {
            self.library_intro();
        }
    }

    fn library_intro(&self) {
        if let Some(panel) = &self.library_panel.view {
            let _ = panel.evaluate_script("window.bh&&bh.intro()");
        }
    }

    fn close_library(&mut self) {
        if self.library {
            self.marks_edit_done();
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
    /// Works for any space: deleting a space that is not shown archives its tabs too.
    fn archive_tab(&mut self, id: TabId) {
        let ws = self.svc.workspace();
        let Some((space, tab)) = ws
            .spaces()
            .iter()
            .find_map(|s| s.tabs.iter().find(|t| t.id == id).map(|t| (s, t)))
        else {
            return;
        };
        // A black hole tab leaves nothing behind, not even in the archive.
        if !matches!(tab.url.scheme(), "http" | "https") || ws.is_burner(id) {
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
                        "watched": i.kind == blackhole_domain::EaselItemKind::Live
                            && i.url.as_deref().is_some_and(|u| self.is_watched(u, &i.selector)),
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "colors": blackhole_domain::easel::COLORS,
                // The Easel window's own top bar (not drawn when embedded as a tab).
                "os": std::env::consts::OS,
                "spaceColor": self.svc.workspace().active_space().color,
                "maximized": self.easel.as_ref().is_some_and(|w| w.window.is_maximized()),
            })
        };
        if let Some(win) = &self.easel {
            let _ = win
                .view
                .evaluate_script(&format!("window.bh&&bh.render({})", state(false)));
        }
        if self.easel_tab || self.easel_side() {
            if let Some(panel) = &self.easel_panel.view {
                let _ = panel.evaluate_script(&format!("window.bh&&bh.render({})", state(true)));
            }
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
            if self.easel_beside.is_some_and(|(_, id)| self.easels.get(id).is_none()) {
                self.close_easel_side();
            }
            self.ui_state();
        }
    }

    fn easel_persist(&mut self) {
        if let Err(e) = self.easel_store.save(&self.easels) {
            tracing::error!("cannot save easels: {e}");
            self.toast(&t("toast.easels_not_saved"));
        }
    }

    /// Reads the page in front. The address comes from the engine; the title and the selection
    /// come from the page and are only ever stored as text.
    fn capture_to_easel(&mut self) {
        // Nothing of a black hole tab is kept (`domain::verbs`), whatever led here.
        let burner = self.svc.workspace().active_tab().is_some_and(|t| self.svc.workspace().is_burner(t.id));
        if burner && !self.home_shown() {
            self.toast(&t("toast.burner_keeps_nothing"));
            return;
        }
        let Some(view) = (if self.home_visible {
            None
        } else {
            self.svc
                .workspace()
                .active_tab()
                .and_then(|t| self.tabs.get(&t.id))
        }) else {
            self.toast(&t("toast.open_page_to_capture"));
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
            self.notify(crate::notices::Notice::Problem, &t("toast.cannot_capture"), serde_json::Value::Null);
        }
    }

    fn poll_captures(&mut self) {
        while let Ok(cap) = self.capture_rx.try_recv() {
            let page: serde_json::Value = serde_json::from_str(&cap.page).unwrap_or_default();
            let text = |key| page.get(key).and_then(|v| v.as_str()).unwrap_or_default();
            let Some(easel) = self.easel_or_create() else {
                self.toast(&t("toast.too_many_easels"));
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
                    self.notify(crate::notices::Notice::Easel, &tf("toast.captured_to", &[("name", &name)]), serde_json::Value::Null);
                }
                Err(why) => self.toast(&t(why)),
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
            // The easel beside a tab closes its pane; the tab stays.
            UiCommand::CloseEaselTab if self.easel_side() => self.close_easel_side(),
            UiCommand::CloseEaselTab => self.close_easel_tab(),
            UiCommand::OpenLibrary { section } => self.open_library(section.as_deref()),
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
                    "history" => {
                        self.svc.clear_history();
                        self.recall_forget(None);
                    }
                    "highlights" => self.marks_clear(),
                    _ => {}
                }
                self.ui_library();
            }
            UiCommand::ReopenArchived { index } => {
                if index < self.archive.len() {
                    let rec = self.archive.remove(index);
                    self.save_archive();
                    // Back in the space it was closed from, when that space still exists.
                    let space = SpaceId(rec.space);
                    if self.svc.workspace().spaces().iter().any(|s| s.id == space) {
                        self.svc.switch_space(space);
                    }
                    self.open(&rec.url);
                }
            }
            UiCommand::ForgetHistory { url } => {
                if let Ok(url) = Url::parse(&url) {
                    self.svc.forget_history(&url);
                    self.recall_forget(Some(url.as_str()));
                    self.ui_library();
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
                    self.drop_view(id);
                }
                self.sync();
            }
            UiCommand::NewEasel { name } => {
                match self.create_easel(&name) {
                    Some(id) => {
                        self.easel_current = Some(id);
                        self.tag_easel_space();
                    }
                    None => self.toast(&t("toast.too_many_easels")),
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
            UiCommand::LivePick => {
                self.pick_watches = false;
                self.live_pick_start();
            }
            UiCommand::CaptureLive { selector, w, h } => {
                let tab = self.svc.workspace().active_tab().map(|t| t.id);
                match tab {
                    Some(tab) if !self.home_shown() => {
                        self.live_picked(tab, selector, (w, h));
                    }
                    _ => self.toast(&t("toast.open_page_for_area")),
                }
            }
            UiCommand::EaselLivePlay { item } => self.live_play(item),
            UiCommand::EaselLiveStop { item } => {
                self.live.remove(&item);
                self.ui_easel();
            }
            UiCommand::EaselLayout { slots, embedded }
                if embedded == (self.easel_tab || self.easel_side()) =>
            {
                self.live_slots = slots.into_iter().map(|s| (s.item, s)).collect();
                self.apply_slots();
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
                    self.toast(&t("toast.opened_in_tab"));
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
        let asked = ws
            .active_tab()
            .filter(|_| !self.home_shown())
            .and_then(|t| self.pending.get(&t.id))
            .map(|(_, url)| url.to_string());
        let url = if self.home_shown() {
            String::new()
        } else {
            asked.clone().unwrap_or_else(|| {
                ws.active_tab()
                    .map(|t| t.url.to_string())
                    .unwrap_or_default()
            })
        };
        let nav = |can: fn(&WebView) -> wry::Result<bool>| {
            // The start page has no history: its arrows would act on the hidden tab.
            !self.home_shown()
                && ws
                    .active_tab()
                    .and_then(|t| self.tabs.get(&t.id))
                    .is_some_and(|v| can(v).unwrap_or(false))
        };
        let zoom = ws
            .active_tab()
            .filter(|_| !self.home_shown())
            .and_then(|t| t.url.host_str().map(|h| self.zoom_of(t.id, h)))
            .unwrap_or(100);
        let loading = asked.is_some()
            || (!self.home_shown()
                && ws
                    .active_tab()
                    .is_some_and(|t| self.loading.contains_key(&t.id) || self.splash.contains_key(&t.id)));
        let state = json!({
            "loading": loading,
            "zoom": zoom,
            "canBack": nav(WebView::can_go_back),
            "canForward": nav(WebView::can_go_forward),
            "os": std::env::consts::OS,
            "maximized": self.window.is_maximized(),
            "private": private_mode(),
            "insecure": Url::parse(&url).is_ok_and(|u| errors::insecure(&u)),
            "url": url,
            "color": ws.active_space().color,
            "engine": self.svc.settings().search_engine.label(),
            "extensions": self.bar_exts,
            "animations": self.svc.settings().animations,
            "side": self.bar_notice_state().0,
            "waiting": self.bar_notice_state().1,
        });
        self.bar_notice.set(self.bar_notice_state());
        let _ = self
            .bar
            .evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    /// The start page doubles as the loading screen of a new tab until its page is there.
    fn ui_wait(&self) {
        let site = self
            .svc
            .workspace()
            .active_tab()
            .filter(|t| !self.home_shown() && self.splash.contains_key(&t.id))
            .map(|t| {
                let host = t.url.host_str().unwrap_or_default().trim_start_matches("www.");
                json!({ "host": host, "url": t.url.as_str() })
            });
        let _ = self
            .home
            .evaluate_script(&format!("window.bh&&bh.wait({})", json!(site)));
    }

    fn show_unreachable(&mut self, tab: TabId, url: Url) {
        self.show_error(tab, url, blackhole_domain::error_page::ErrorPage::Unreachable);
    }

    /// Ends the loading screen of a tab, and the progress of a page that went quiet.
    fn service_loading(&mut self) -> bool {
        let now = Instant::now();
        let before = (self.splash.len(), self.loading.len() + self.pending.len());
        self.splash
            .retain(|id, since| self.tabs.contains_key(id) && now.duration_since(*since) < SPLASH_MAX);
        self.loading
            .retain(|id, since| self.tabs.contains_key(id) && now.duration_since(*since) < LOADING_MAX);
        self.awaiting.retain(|id, _| self.tabs.contains_key(id));
        self.pending
            .retain(|id, (since, _)| self.tabs.contains_key(id) && now.duration_since(*since) < LOADING_MAX);
        let late: Vec<_> = self
            .aborted
            .iter()
            .filter(|(_, (since, _))| now.duration_since(*since) >= ABORT_GRACE)
            .map(|(id, _)| *id)
            .collect();
        for id in late {
            if let Some((_, url)) = self.aborted.remove(&id) {
                self.on_failed(id, url);
            }
        }
        let stalled: Vec<_> = self
            .awaiting
            .iter()
            .filter(|(_, (since, _))| now.duration_since(*since) >= CONNECT_MAX)
            .map(|(id, _)| *id)
            .collect();
        for id in stalled {
            if let Some((_, url)) = self.awaiting.remove(&id) {
                self.show_unreachable(id, url);
            }
        }
        if before != (self.splash.len(), self.loading.len() + self.pending.len()) {
            self.sync();
        }
        !self.splash.is_empty()
            || !self.loading.is_empty()
            || !self.pending.is_empty()
            || !self.awaiting.is_empty()
            || !self.aborted.is_empty()
    }

    fn ui_home(&self) {
        let mut state = json!({
            "engine": self.svc.settings().search_engine.label(),
            "animations": self.svc.settings().animations,
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
            "now": self.home_now_json(),
        });
        if self.icons_for_home.replace(false) {
            state["favicons"] = json!(self.favicons.all());
        }
        let _ = self
            .home
            .evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    /// The interface language changed: every open page of BlackHole gets the new catalog and
    /// re-renders, the shell's own texts (menus, titles, state) follow. Error pages already
    /// shown keep their language until the next error.
    fn language_changed(&mut self) {
        i18n::apply(&self.svc.settings().language);
        self.close_palette();
        let mut views: Vec<&WebView> = vec![
            &self.chrome,
            &self.flyout,
            &self.hotzone,
            &self.bar,
            &self.home,
            &self.palette,
        ];
        views.extend(self.peek.as_ref().map(|p| &p.scrim));
        views.extend(self.splitbar.view.as_ref());
        views.extend(self.library_panel.view.as_ref());
        views.extend(self.easel_panel.view.as_ref());
        views.extend(self.settings.as_ref().map(|w| &w.view));
        views.extend(self.easel.as_ref().map(|w| &w.view));
        for view in views {
            views::send_language(view);
        }
        self.window.set_title(&main_title());
        if let Some(w) = &self.settings {
            w.window.set_title(&i18n::t("window.settings"));
        }
        if let Some(w) = &self.easel {
            w.window.set_title(&i18n::t("window.easel"));
        }
        #[cfg(target_os = "macos")]
        {
            self._menu = install_menu();
        }
        self.ui_state();
    }

    /// A short message; `message` may come from another layer (`i18n::message`).
    pub(crate) fn toast(&self, message: &str) {
        self.notify(notices::Notice::Info, message, serde_json::Value::Null);
    }

    /// A toast with an Undo button, for the space or folder just deleted (`removed`).
    fn toast_undo(&mut self, message: &str, removed: Removed) {
        self.undo = Some((Instant::now(), removed));
        self.notify(notices::Notice::Undo, message, json!([{"label": t("toast.undo"), "run": {"cmd": "undo_removal"}}]));
    }

    /// The toast's Undo: the deleted space or folder comes back with its tabs (asleep), and they
    /// leave the archive they had been put in.
    fn undo_removal(&mut self) {
        let Some((at, removed)) = self.undo.take() else {
            return;
        };
        if at.elapsed() > UNDO_FOR {
            return;
        }
        let space = match &removed {
            Removed::Space { space, .. } => space.id.0,
            Removed::Folder { space, .. } => space.0,
        };
        let urls: HashSet<String> = removed.tabs().iter().map(|t| t.url.to_string()).collect();
        if self.svc.undo_removal(removed) {
            self.archive
                .retain(|a| !(a.space == space && urls.contains(&a.url)));
            self.save_archive();
            self.sync();
            self.toast(&t("toast.restored"));
        }
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

    /// The next tab opened for `target` comes from a page of `opener`: its first request says so
    /// (`popup::referrer`), as the link would have in that page.
    fn opened_from(&mut self, opener: TabId, target: &Url) {
        self.referrer_next = self
            .view_of(opener)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok())
            .and_then(|page| blackhole_domain::popup::referrer(&page, target))
            .map(|referrer| (target.clone(), referrer));
    }

    fn ensure_view(&mut self, tab: &blackhole_domain::Tab) {
        if self.tabs.contains_key(&tab.id) {
            return;
        }
        let url = &tab.url;
        let bounds = self.full_content_bounds();
        let store = self.store_of_tab(tab.id);
        let referrer = self
            .referrer_next
            .take_if(|(target, _)| target == url)
            .and_then(|(_, r)| wry::http::HeaderValue::from_str(&r).ok());
        // Headers go with a load, not with a view's creation: a view without a page is made
        // first when none is in reserve.
        if referrer.is_some() && !self.spares.iter().any(|(s, ..)| *s == store) {
            if let Ok((slot, view)) = views::spare_tab_view(
                &self.window,
                &mut self.web_context,
                self.svc.policy(),
                &self.mailbox,
                self.webext.as_ref(),
                store,
            ) {
                self.spares.push((store, slot, view));
                self.raise_palette();
            }
        }
        if let Some(at) = self.spares.iter().position(|(s, ..)| *s == store) {
            let (_, slot, view) = self.spares.swap_remove(at);
            slot.store(tab.id.0, std::sync::atomic::Ordering::Relaxed);
            let _ = view.set_bounds(bounds);
            // A native load (not `location.replace`) so that a failure reaches the load handler.
            let _ = match referrer {
                // WebView2 drops a header given to the navigation at the first redirect: the
                // view's document requests are stamped instead, until the page shows.
                #[cfg(windows)]
                Some(value) => {
                    blackhole_macos_webext::referrer_once(&view, value.to_str().unwrap_or_default());
                    view.load_url(url.as_str())
                }
                #[cfg(not(windows))]
                Some(value) => {
                    let mut headers = wry::http::HeaderMap::new();
                    headers.insert(wry::http::header::REFERER, value);
                    view.load_url_with_headers(url.as_str(), headers)
                }
                None => view.load_url(url.as_str()),
            };
            self.tabs.insert(tab.id, view);
            self.splash.insert(tab.id, Instant::now());
            self.awaiting.insert(tab.id, (Instant::now(), url.clone()));
            self.spares_due = Some(Instant::now() + SPARES_DELAY);
            self.refuse_restricted_port(tab.id, url);
            return;
        }
        match views::tab_view(
            &self.window,
            &mut self.web_context,
            tab.id,
            url,
            bounds,
            self.svc.policy(),
            &self.mailbox,
            self.webext.as_ref(),
            store,
        ) {
            Ok(view) => {
                self.tabs.insert(tab.id, view);
                self.splash.insert(tab.id, Instant::now());
                self.awaiting.insert(tab.id, (Instant::now(), url.clone()));
                self.raise_palette();
                self.spares_due = Some(Instant::now() + SPARES_DELAY);
                self.refuse_restricted_port(tab.id, url);
            }
            Err(error) => tracing::error!("cannot create web view: {error}"),
        }
    }

    /// The cookie jar of a space: the original "Personal" space (id 1, whatever its position)
    /// keeps the browser's main one so existing logins survive, every other space has its own,
    /// and a private window keeps nothing.
    fn store_of_space(&self, space: SpaceId) -> views::Store {
        if private_mode() {
            views::Store::Private
        } else if space == SpaceId(1) {
            views::Store::Main
        } else {
            views::Store::Space(space.0)
        }
    }

    fn store_of_tab(&self, tab: TabId) -> views::Store {
        if self.svc.workspace().is_burner(tab) {
            return views::Store::Burner(tab.0);
        }
        let owner = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .find(|s| s.tabs.iter().any(|t| t.id == tab))
            .map_or_else(|| self.svc.workspace().active_space().id, |s| s.id);
        self.store_of_space(owner)
    }

    /// Drops every spare view; `refill_spares` rebuilds them for the configured count.
    fn trim_spares(&mut self) {
        if !self.spares.is_empty() {
            self.spares.clear();
            self.raise_palette();
        }
    }

    /// Builds the spare views while nothing is going on. They all sit below the command bar,
    /// which is recreated once on top of them, so the next tabs open without any view creation.
    fn refill_spares(&mut self) {
        let Some(due) = self.spares_due else { return };
        if Instant::now() < due {
            return;
        }
        let wanted = self.svc.settings().spare_views as usize;
        if wanted == 0 {
            self.spares_due = None;
            return;
        }
        // Spares belong to a space's login store: only the active space's are worth keeping.
        let store = self.store_of_space(self.svc.workspace().active_space().id);
        if self.palette_open || self.peek.is_some() || self.spares.iter().any(|(s, ..)| *s == store)
        {
            self.spares_due = Some(Instant::now() + SPARES_DELAY);
            return;
        }
        self.spares_due = None;
        self.spares.retain(|(s, ..)| *s == store);
        for _ in 0..wanted {
            match views::spare_tab_view(
                &self.window,
                &mut self.web_context,
                self.svc.policy(),
                &self.mailbox,
                self.webext.as_ref(),
                store,
            ) {
                Ok((slot, view)) => self.spares.push((store, slot, view)),
                Err(error) => {
                    tracing::error!("cannot create spare view: {error}");
                    break;
                }
            }
        }
        self.raise_palette();
        self.sync();
    }

    /// Native views stack in creation order, so the overlays are recreated above new tabs. Burst
    /// of tab creations (session restore) share a single recreation.
    fn raise_palette(&mut self) {
        self.raise_due = Some(Instant::now() + RAISE_DELAY);
    }

    /// Recreates the overlays now if a tab was created since; true while still waiting.
    fn flush_raise(&mut self, force: bool) -> bool {
        let Some(due) = self.raise_due else {
            return false;
        };
        if self.palette_open || (!force && Instant::now() < due) {
            return !self.palette_open;
        }
        self.raise_due = None;
        self.raise_overlays();
        false
    }

    fn raise_overlays(&mut self) {
        // Restacking the existing overlays: rebuilding them made the sidebar flash.
        if views::raise_views(&[&self.hotzone, &self.flyout, &self.palette]) {
            self.apply_focus();
            return;
        }
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
        let bounds = palette_rect(false, &size);
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
        let now = Instant::now();
        let tabs = &self.svc.workspace().active_space().tabs;
        let list: Vec<_> = tabs
            .iter()
            .filter(|t| !self.media_dismissed.contains_key(&t.id.0))
            .filter_map(|t| {
                let p = self.media_hold.shown(t.id.0, now)?;
                Some(json!({
                    "id": t.id.0, "playing": p.playing, "muted": p.muted,
                    "cam": p.cam, "mic": p.mic, "camOn": p.cam_on, "micOn": p.mic_on,
                    "call": p.cam || p.mic || p.call,
                }))
            })
            .collect();
        json!(list)
    }

    /// Asks every loaded tab about its media, and refreshes the sidebar when something changed.
    fn service_media(&mut self) {
        if Instant::now() >= self.media_due {
            self.media_due = Instant::now()
                + if self.pip_seen {
                    Duration::from_millis(500)
                } else {
                    MEDIA_EVERY
                };
            if let Ok(mut probes) = self.media.lock() {
                probes.retain(|id, _| self.tabs.contains_key(&TabId(*id)));
            }
            // Only the tabs with something going on are asked: a script run in every live tab
            // every second and a half kept the idle ones (documents, sheets) awake for nothing.
            let now = Instant::now();
            self.media_hint
                .retain(|id, at| self.tabs.contains_key(&TabId(*id)) && now.duration_since(*at) < MEDIA_HINT_FOR);
            let active: Vec<u64> = self.media.lock().map_or_else(
                |_| Vec::new(),
                |probes| {
                    probes
                        .iter()
                        .filter(|(_, p)| p.playing || p.pip || p.cam || p.mic || p.call)
                        .map(|(id, _)| *id)
                        .collect()
                },
            );
            let floating = self.pip_tab.map(|t| t.0);
            for (id, view) in &self.tabs {
                let sink = Arc::clone(&self.media);
                let id = id.0;
                if !self.media_hint.contains_key(&id) && !active.contains(&id) && floating != Some(id) {
                    continue;
                }
                let _ = view.evaluate_script_with_callback(views::MEDIA_PROBE_JS, move |raw| {
                    if let (Ok(probe), Ok(mut probes)) =
                        (serde_json::from_str::<MediaProbe>(&raw), sink.lock())
                    {
                        probes.insert(id, probe);
                    }
                });
            }
        }
        let now = Instant::now();
        if let Ok(probes) = self.media.lock() {
            // A probe sent before the card was closed still says "playing": only a later one
            // can bring the card back.
            self.media_dismissed.retain(|id, at| {
                at.elapsed() < DISMISS_GRACE || !probes.get(id).is_some_and(|p| p.playing)
            });
            for (id, p) in probes.iter() {
                let shown = p.playing || p.has || p.cam || p.mic;
                self.media_hold.note(*id, now, shown.then_some(*p));
            }
        }
        self.media_hold
            .prune(now, |id| self.tabs.contains_key(&TabId(id)));
        // Fallback for engines that do not tell the page when its floating window is left.
        let floating = self
            .pip_tab
            .and_then(|tab| self.media.lock().ok()?.get(&tab.0).copied().map(|p| (tab, p)));
        if let Some((tab, probe)) = floating {
            if probe.pip {
                self.pip_seen = true;
            } else if self.pip_seen {
                self.pip_faded = Some((tab, Instant::now()));
                self.pip_returned(tab);
            }
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
        let rtc;
        let js = match action {
            "cam" | "mic" => {
                rtc = views::rtc_toggle_js(if action == "cam" { "video" } else { "audio" });
                rtc.as_str()
            }
            "toggle" => views::MEDIA_TOGGLE_JS,
            "mute" => views::MEDIA_MUTE_JS,
            "pip" => views::PIP_TOGGLE_JS,
            // Closing the player also closes its floating window.
            "close" => {
                self.media_dismissed.insert(id.0, Instant::now());
                if self.pip_tab == Some(id) {
                    self.pip_tab = None;
                    self.pip_seen = false;
                }
                views::MEDIA_CLOSE_JS
            }
            _ => return,
        };
        // The card of a call (camera, microphone or a live connection): closing it leaves the
        // call too, instead of only pausing its tiles.
        if action == "close"
            && self
                .media
                .lock()
                .ok()
                .and_then(|probes| probes.get(&id.0).copied())
                .is_some_and(|p| p.cam || p.mic || p.call)
        {
            let _ = view.evaluate_script(views::CALL_HANG_UP_JS);
        }
        let _ = view.evaluate_script(js);
        self.media_hint.insert(id.0, Instant::now());
        match action {
            "pip" => self.pip_toggled(id),
            "close" => {
                self.pip_shell_exits.insert(id.0, Instant::now());
            }
            _ => {}
        }
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
        let publish = self.peers_published.elapsed() >= PEERS_PUBLISH_EVERY;
        if let Some((x, y, w, h)) = self.screen_rect().filter(|_| publish) {
            self.peers_published = Instant::now();
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
        self.ensure_panels();
        let now = Instant::now();
        if !self.home_shown() {
            let active = self.svc.workspace().active_tab().map(|t| t.id);
            for id in active.into_iter().chain(self.split_tab()) {
                self.last_active.insert(id, now);
            }
        }
        let limit = Duration::from_secs(u64::from(self.svc.settings().tab_sleep_minutes) * 60);
        // A tab in a call or playing media is never freed just because the user browsed elsewhere;
        // a short grace period covers a probe that is late or a stream that is restarting.
        let now_busy: Vec<TabId> = self.media.lock().map_or_else(
            |_| Vec::new(),
            |probes| {
                self.tabs
                    .keys()
                    .filter(|id| {
                        probes
                            .get(&id.0)
                            .is_some_and(|p| p.playing || p.watching || p.cam || p.mic || p.call)
                    })
                    .copied()
                    .collect()
            },
        );
        for id in now_busy {
            self.busy_seen.insert(id, now);
        }
        // Releasing the page of a black hole tab would end its session: it stays alive until
        // the tab is closed (their number is capped, see `open_burner`).
        let ws = self.svc.workspace();
        let holes: Vec<TabId> = self.tabs.keys().filter(|id| ws.is_burner(**id)).copied().collect();
        for id in holes {
            self.busy_seen.insert(id, now);
        }
        self.busy_seen
            .retain(|id, seen| self.tabs.contains_key(id) && now.duration_since(*seen) < BUSY_GRACE);
        let busy: Vec<TabId> = self.busy_seen.keys().copied().collect();
        // The sleep delay only starts once the call or the media has stopped.
        for id in &busy {
            self.last_active.insert(*id, now);
        }
        let mut idle: Vec<TabId> = self
            .tabs
            .keys()
            .filter(|id| !busy.contains(id))
            .filter(|id| {
                let seen = self.last_active.get(id).copied().unwrap_or(now);
                now.duration_since(seen) >= limit
            })
            .copied()
            .collect();
        // Cap on live views, independent of the delay: each one is a renderer process.
        // Tabs never shown sort first, then the least recently used.
        let live: Vec<(TabId, Option<Instant>)> = self
            .tabs
            .keys()
            .filter(|id| !idle.contains(id))
            .map(|id| (*id, self.last_active.get(id).copied()))
            .collect();
        let mut visible: Vec<TabId> = self
            .svc
            .workspace()
            .active_tab()
            .map(|t| t.id)
            .into_iter()
            .chain(self.split_tab())
            .collect();
        visible.extend(&busy);
        idle.extend(blackhole_domain::overflow_to_release(
            &live,
            &visible,
            self.svc.settings().max_live_tabs as usize,
        ));
        if idle.is_empty() {
            return;
        }
        for id in idle {
            self.tabs.remove(&id);
            self.last_active.remove(&id);
            self.svc.go_home(id);
        }
        self.ui_state();
    }

    /// Creates the shell pages that are about to be needed and releases the ones left unused.
    fn ensure_panels(&mut self) {
        let now = Instant::now();
        let side = self.easel_side();
        let split = self.split_tab().is_some() || side;
        let size = self.window_size();
        let (cx, cy, cw, ch) = card_rect(&self.sidebar, &size);
        let mut created = false;
        for (needed, which) in [(self.library, 0), (self.easel_tab || side, 1), (split, 2)] {
            let (panel, html, bounds) = match which {
                0 => (
                    &mut self.library_panel,
                    LIBRARY_HTML,
                    views::rect(
                        0.0,
                        BAR_HEIGHT,
                        size.width,
                        (size.height - BAR_HEIGHT).max(1.0),
                    ),
                ),
                1 => (
                    &mut self.easel_panel,
                    EASEL_HTML,
                    views::rect(cx, cy, cw, ch),
                ),
                _ => (
                    &mut self.splitbar,
                    SPLITBAR_HTML,
                    views::rect(cx, cy, cw, SPLIT_HEADER),
                ),
            };
            if needed {
                panel.shown = now;
                if panel.view.is_none() {
                    match views::ui_view(
                        &self.window,
                        &mut self.web_context,
                        html,
                        bounds,
                        false,
                        false,
                        &self.mailbox,
                    ) {
                        Ok(view) => {
                            panel.view = Some(view);
                            created = true;
                        }
                        Err(error) => tracing::error!("cannot create panel: {error}"),
                    }
                }
            } else if panel.view.is_some() && now.duration_since(panel.shown) >= PANEL_KEEP {
                panel.view = None;
            }
        }
        if created {
            self.raise_palette();
        }
    }

    pub(crate) fn sync(&mut self) {
        self.pilot_sync();
        self.icons_for_ui.set(true);
        self.icons_for_home.set(true);
        self.ensure_active_view();
        let active = if self.home_shown() {
            None
        } else {
            self.svc.workspace().active_tab().map(|t| t.id)
        };
        if let Some(id) = active {
            self.last_active.insert(id, Instant::now());
        }
        // Back on the tab with an easel beside it: that easel again, unless the Easel window
        // shows another one.
        if let Some((tab, easel)) = self.easel_beside {
            if active == Some(tab) && !self.easel_tab && self.easel.is_none() {
                self.easel_current = Some(easel);
            }
        }
        self.ensure_panels();
        if let Some(panel) = &self.easel_panel.view {
            let _ = panel.set_visible(self.easel_tab || self.easel_side());
        }
        // The easel beside a tab went off screen (another tab, the start page): its live views
        // were on the main window and have no page under them any more.
        if self.easel_beside.is_some() && !self.easel_side() && !self.easel_tab && self.easel.is_none() {
            self.live.clear();
            self.live_slots.clear();
        }
        if self.shown != active {
            // The other pane of a split stays on screen: its video keeps playing in place.
            let left = self.shown.filter(|id| Some(*id) != self.split_tab());
            if let Some(id) = left.filter(|id| self.tabs.contains_key(id)) {
                // Never during a call: the first video found there is some participant's tile.
                if let Some(view) = self.tabs.get(&id).filter(|_| !self.in_call(id)) {
                    let _ = view.evaluate_script(views::PIP_ENTER_JS);
                }
                // A video already floating keeps its "back to tab" button working unless the
                // tab just left is known to play something that replaces it.
                let floating = self.pip_seen
                    && self
                        .pip_tab
                        .is_some_and(|t| t != id && self.tabs.contains_key(&t));
                if !floating || self.is_playing(id) {
                    self.pip_tab = Some(id);
                    self.pip_seen = false;
                }
            }
            if let Some(back) = active.and_then(|id| self.tabs.get(&id)) {
                let _ = back.evaluate_script(views::PIP_EXIT_JS);
                if let Some(id) = active {
                    self.pip_shell_exits.insert(id.0, Instant::now());
                }
                if self.pip_tab == active {
                    self.pip_tab = None;
                    self.pip_seen = false;
                }
            }
            self.shown = active;
            if let Some(id) = active {
                self.reveal_tab(id);
            }
        }
        if !self.library {
            // Keep the page blank while hidden so the next opening fades in without a flash.
            if let Some(panel) = &self.library_panel.view {
                let _ = panel.evaluate_script("window.bh&&bh.prep()");
            }
        }
        if let Some(panel) = &self.library_panel.view {
            let _ = panel.set_visible(self.library);
        }
        self.place_panels();
        let split = self.split_tab();
        let waiting = active.filter(|id| self.splash.contains_key(id));
        for (id, view) in &self.tabs {
            let shown = (Some(*id) == active && waiting.is_none()) || Some(*id) == split;
            let _ = view.set_visible(shown);
            views::set_background(view, !shown);
        }
        let _ = self
            .home
            .set_visible((active.is_none() || waiting.is_some()) && !self.easel_tab && !self.library);
        self.ui_wait();
        self.layout(true);
        self.apply_focus();
        self.ui_state();
    }

    /// Unfolds the section that hides a tab in the sidebar, so the tab that becomes active, e.g.
    /// from a search, is always visible. Its folders stay as the user left them: like Arc, a
    /// closed folder shows the pages open under it (`ui/index.html`), so nothing else unfolds.
    fn reveal_tab(&mut self, id: TabId) {
        let space = self.svc.workspace().active_space();
        let Some(tab) = space.tabs.iter().find(|t| t.id == id) else {
            return;
        };
        let section = if tab.pinned {
            "pinned"
        } else if tab.folder.is_some() {
            "folders"
        } else {
            return;
        };
        if self.svc.settings().collapsed_sections.contains(section) {
            self.svc.set_section_collapsed(section, false);
        }
    }

    /// A video left its floating window. The window's close button pauses it: nothing changes;
    /// its "back to tab" button leaves it playing: its tab is shown, as in Safari. WebView2
    /// cannot pause a live stream on close (`stream`): for those nothing is switched on
    /// Windows. Only a video the shell floated counts, and not when the shell itself brought
    /// it back.
    fn pip_left(&mut self, tab: TabId, paused: bool, stream: bool) {
        let now = Instant::now();
        self.pip_shell_exits.retain(|_, at| now.duration_since(*at) < PIP_SHELL_EXIT);
        // The probe may have seen the video leave before the page said so (WebKit tells the
        // page only once its window has shrunk back): it was still floated by the shell.
        let faded = self.pip_faded.take().is_some_and(|(id, at)| {
            id == tab && now.duration_since(at) < blackhole_domain::pip::LEFT_EVENT_GRACE
        });
        let left = blackhole_domain::pip::PipLeft {
            floated_by_shell: self.pip_tab == Some(tab) || self.pip_on_blur == Some(tab) || faded,
            left_by_shell: self.pip_shell_exits.contains_key(&tab.0),
            paused,
            tab_shown: self.shown == Some(tab) && !self.home_shown(),
            close_pauses: cfg!(target_os = "macos") || !stream,
        };
        self.pip_returned(tab);
        if self.pip_on_blur == Some(tab) {
            self.pip_on_blur = None;
        }
        if blackhole_domain::pip::back_to_tab(left) && self.tabs.contains_key(&tab) {
            self.window.set_minimized(false);
            self.window.focus_window();
            self.handle_ui(UiCommand::Activate { id: tab.0 });
        }
    }

    /// The shell's PiP button (card, command bar) toggled the video of `tab`: floating, it comes
    /// back by the shell's hand (no request to show the tab); in its page, the shell floats it.
    fn pip_toggled(&mut self, tab: TabId) {
        let floating = self
            .media
            .lock()
            .is_ok_and(|probes| probes.get(&tab.0).is_some_and(|p| p.pip));
        if floating {
            self.pip_shell_exits.insert(tab.0, Instant::now());
        } else {
            self.pip_tab = Some(tab);
            self.pip_seen = false;
        }
    }

    /// The floating video of `tab` is gone (left, or no longer seen by the probe): the shell
    /// stops tracking it.
    fn pip_returned(&mut self, tab: TabId) {
        if self.pip_tab == Some(tab) {
            self.pip_tab = None;
            self.pip_seen = false;
        }
    }

    /// The window lost focus (another app, a dialog): a playing video floats so it stays visible.
    /// Moving the window or clicking into a page view also drops the window's own focus, so the
    /// loss is only acted on when it lasts and the whole application is no longer frontmost.
    fn pip_on_blur(&mut self) {
        let Some(due) = self.blur_due else { return };
        if Instant::now() < due {
            return;
        }
        // Still the frontmost app: focus only moved inside it. Keep watching, since switching
        // to another app later raises no further event.
        #[cfg(any(target_os = "macos", windows))]
        if blackhole_macos_webext::app_is_active() {
            self.blur_due = Some(Instant::now() + BLUR_DELAY);
            return;
        }
        // The browser is still on screen (e.g. the user works in another window on a second
        // monitor): nothing to rescue. Look again later in case that window is moved over it.
        if !self.window_is_covered() {
            self.blur_due = Some(Instant::now() + PIP_RECHECK);
            return;
        }
        self.blur_due = None;
        let Some(id) = self.shown else { return };
        // A call is left alone: sharing a screen is precisely working in another window.
        if self.in_call(id) {
            return;
        }
        if let Some(view) = self.tabs.get(&id) {
            let _ = view.evaluate_script(views::PIP_ENTER_JS);
            self.pip_on_blur = Some(id);
        }
    }

    /// Whether most of the browser window is hidden: minimised, or under the window of another
    /// application that now has focus.
    fn window_is_covered(&self) -> bool {
        if self.window.is_minimized().unwrap_or(false) || !self.window.is_visible().unwrap_or(true)
        {
            return true;
        }
        #[cfg(any(target_os = "macos", windows))]
        {
            let (Ok(pos), size) = (self.window.outer_position(), self.window.outer_size()) else {
                return true;
            };
            let Some(front) = blackhole_macos_webext::frontmost_foreign_window() else {
                return false;
            };
            // winit reports pixels; macOS window lists use points.
            let scale = if cfg!(target_os = "macos") { self.window.scale_factor() } else { 1.0 };
            let own = (
                f64::from(pos.x) / scale,
                f64::from(pos.y) / scale,
                f64::from(size.width) / scale,
                f64::from(size.height) / scale,
            );
            return overlap_share(own, front) >= COVERED_SHARE;
        }
        #[allow(unreachable_code)]
        true
    }

    /// Focus is back: the video floated by `pip_on_blur` returns into its page.
    fn pip_back_on_focus(&mut self) {
        self.blur_due = None;
        if let Some(id) = self.pip_on_blur.take() {
            if let Some(view) = self.tabs.get(&id) {
                let _ = view.evaluate_script(views::PIP_EXIT_JS);
                self.pip_shell_exits.insert(id.0, Instant::now());
            }
        }
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

    /// A black hole tab on `input`: a tab with a session of its own (no cookie of the space,
    /// nothing kept), destroyed when it closes.
    fn open_burner(&mut self, input: &str) {
        self.close_palette();
        self.close_peek();
        if self.svc.workspace().burner_count() >= MAX_BURNERS {
            self.toast(&t("toast.too_many_burners"));
            return;
        }
        match self.svc.open_burner(input) {
            Some(_) => {
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.sync();
            }
            None => self.toast(&t("blocked.unsupported_scheme")),
        }
    }

    fn open(&mut self, input: &str) {
        if std::mem::take(&mut self.burner_next) {
            return self.open_burner(input);
        }
        self.close_palette();
        self.close_peek();
        match self.svc.resolve(input) {
            NavigationOutcome::Blocked(why) => self.toast(why),
            NavigationOutcome::Go(url) => {
                let typed = blackhole_domain::resolve_input(input, self.svc.settings().search_engine);
                let tab = self.svc.open_tab(input);
                if let (Some(tab), "http", "https") = (tab, typed.scheme(), url.scheme()) {
                    self.on_upgraded(tab, typed);
                }
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.sync();
            }
        }
    }

    fn navigate(&mut self, input: &str) {
        if self.burner_next {
            return self.open(input);
        }
        self.close_palette();
        let typed = blackhole_domain::resolve_input(input, self.svc.settings().search_engine);
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
                if typed.scheme() == "http" && url.scheme() == "https" {
                    self.on_upgraded(id, typed);
                }
                if self.refuse_restricted_port(id, &url) {
                    // Nothing to load: WebKit would refuse it without a word.
                } else if let Some(view) = self.tabs.get(&id) {
                    if view.load_url(url.as_str()).is_ok() {
                        self.pending.insert(id, (Instant::now(), url));
                        self.ui_bar();
                    }
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
        let Some((id, kept)) = self
            .svc
            .workspace()
            .active_tab()
            .map(|t| (t.id, t.pinned || t.folder.is_some()))
        else {
            return;
        };
        // Like Arc: a pinned or folder tab is only put to sleep, it keeps its place.
        if kept {
            self.svc.release_tab(id);
        } else {
            self.archive_tab(id);
            self.svc.close_tab(id);
        }
        self.drop_view(id);
        self.sync();
    }

    /// Releases the page view of a tab and everything that followed it (media probe, floating
    /// video bookkeeping), so no state outlives the view.
    pub(crate) fn drop_view(&mut self, id: TabId) {
        // The end of a black hole tab's page is the end of its session: on Windows the engine
        // profile is deleted (WebKit's ephemeral store dies with the view).
        #[cfg(windows)]
        if let Some(view) = self.tabs.get(&id).filter(|_| self.svc.workspace().is_burner(id)) {
            blackhole_macos_webext::delete_profile(view);
        }
        self.tabs.remove(&id);
        // Like Arc: a quick access or folder tab that wandered wakes up at its own address.
        self.svc.go_home(id);
        self.marks_push(id, false);
        self.last_active.remove(&id);
        if let Ok(mut probes) = self.media.lock() {
            probes.remove(&id.0);
        }
        self.media_hold.forget(id.0);
        self.media_hint.remove(&id.0);
        self.crashed.remove(&id);
        if self.pip_tab == Some(id) {
            self.pip_tab = None;
            self.pip_seen = false;
        }
        if self.pip_on_blur == Some(id) {
            self.pip_on_blur = None;
        }
    }

    /// ⌘⇧T: brings back the tab closed last, in the space it was closed from.
    fn reopen_closed(&mut self) {
        match self.archive.len().checked_sub(1) {
            Some(index) => self.handle_easel(UiCommand::ReopenArchived { index }),
            None => self.toast(&t("toast.nothing_to_reopen")),
        }
    }

    /// Ctrl+Tab and ⌘⌥↑/↓ (Ctrl+Alt on Windows): show another tab of the space.
    fn switch_tab(&mut self, key: &str) {
        let ws = self.svc.workspace();
        let target = match key {
            // From the start page, the tab just left is the one to go back to.
            "recent_tab" if self.home_shown() && ws.active_tab().is_some() => {
                ws.active_tab().map(|t| t.id)
            }
            "recent_tab" => ws.previous_tab(),
            "next_tab" => ws.sibling_tab(1),
            _ => ws.sibling_tab(-1),
        };
        if let Some(id) = target {
            self.handle_ui(UiCommand::Activate { id: id.0 });
        }
    }

    /// The page says its media may have changed: it is probed soon, and for a few seconds.
    fn media_hinted(&mut self, tab: TabId) {
        if !self.tabs.contains_key(&tab) {
            return;
        }
        let now = Instant::now();
        self.media_hint.insert(tab.0, now);
        self.media_due = self.media_due.min(now + Duration::from_millis(250));
    }

    /// The engine process of a page died. A tab on screen is reloaded once (a second death
    /// within `CRASH_LOOP` leaves it alone, with a word to the user); a hidden one gives its
    /// view back and loads again when it is shown.
    fn on_crashed(&mut self, tab: TabId) {
        if !self.tabs.contains_key(&tab) {
            return;
        }
        let on_screen = self.shown == Some(tab) || self.split_tab() == Some(tab);
        if !on_screen {
            self.drop_view(tab);
            self.ui_state();
            return;
        }
        let now = Instant::now();
        let again = self
            .crashed
            .insert(tab, now)
            .is_some_and(|before| now.duration_since(before) < CRASH_LOOP);
        self.loading.remove(&tab);
        if again {
            self.notify(crate::notices::Notice::Problem, &t("toast.page_crashed"), serde_json::Value::Null);
            self.ui_bar();
        } else if let Some(view) = self.tabs.get(&tab) {
            let _ = view.reload();
        }
    }

    /// Whether the last probe of a tab heard it play something.
    fn is_playing(&self, tab: TabId) -> bool {
        self.media
            .lock()
            .is_ok_and(|probes| probes.get(&tab.0).is_some_and(|p| p.playing))
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
        // Another use of the palette replaces the sign-in prompt: it is cancelled.
        if mode != "auth" {
            if let Some(id) = self.auth_shown.take() {
                self.reply_auth(id, None);
            }
        }
        self.close_peek();
        self.flush_raise(true);
        self.palette_mode = mode.to_owned();
        self.palette_open = true;
        self.apply_slots();
        let _ = self
            .palette
            .set_bounds(palette_rect(true, &self.window_size()));
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
        self.burner_next = false;
        if !self.palette_open {
            return;
        }
        self.palette_open = false;
        self.apply_slots();
        let _ = self.palette.evaluate_script("window.bh&&bh.hide()");
        let _ = self.palette.set_visible(false);
        let _ = self
            .palette
            .set_bounds(palette_rect(false, &self.window_size()));
        self.apply_focus();
        // Closing the sign-in prompt any other way (Esc, click outside, another palette)
        // cancels it: the server's own 401 page shows.
        if self.palette_mode == "auth" {
            if let Some(id) = self.auth_shown.take() {
                self.answer_auth(id, None);
            }
        }
    }

    // ---- HTTP authentication --------------------------------------------------------------

    /// A server asks for a user name and password. The tab is not unreachable: it waits for
    /// the prompt, shown once the tab is on screen.
    fn on_auth(&mut self, tab: TabId, challenge: &wry::HttpAuthChallenge, reply: wry::HttpAuthResponder) {
        if !self.auth_tab_alive(tab) {
            return; // `reply` dropped: cancelled.
        }
        self.awaiting.remove(&tab);
        self.pending.remove(&tab);
        let realm = http_auth::realm_of(&challenge.challenge);
        let id = self
            .auth
            .push(tab, &challenge.host, &realm, challenge.secure, challenge.retry);
        self.auth_replies.insert(id, reply);
        // The logins saved for exactly this origin (https or localhost) can answer it.
        let origin = Url::parse(&challenge.url)
            .ok()
            .and_then(|u| blackhole_domain::passwords::origin_of(&u));
        if let Some(origin) = origin {
            self.auth_origins.insert(id, origin);
        }
        if self.splash.remove(&tab).is_some() {
            self.sync();
        }
        self.service_auth();
    }

    /// Peek has no prompt (the palette would close it): its page gets the 401, opening it as
    /// a tab asks.
    fn auth_tab_alive(&self, tab: TabId) -> bool {
        tab.0 != 0 && tab != PEEK_TAB && self.tabs.contains_key(&tab)
    }

    /// Tabs whose page is on screen: the active one and the other pane of a split.
    fn auth_tabs_shown(&self) -> Vec<TabId> {
        let mut shown = Vec::new();
        if !self.home_shown() {
            shown.extend(self.svc.workspace().active_tab().map(|t| t.id));
        }
        shown.extend(self.split_tab());
        shown
    }

    /// Shows the oldest prompt of a tab on screen, unless one is up; drops those of closed tabs.
    fn service_auth(&mut self) {
        if self.auth.is_empty() {
            return;
        }
        let gone: Vec<TabId> = self
            .auth_replies
            .keys()
            .filter_map(|id| self.auth.get(*id).map(|c| c.tab))
            .filter(|tab| !self.auth_tab_alive(*tab))
            .collect();
        for tab in gone {
            for id in self.auth.drop_tab(tab) {
                self.auth_replies.remove(&id);
                if self.auth_shown == Some(id) {
                    self.auth_shown = None;
                    if self.palette_mode == "auth" {
                        self.close_palette();
                    }
                }
            }
        }
        if self.auth_shown.is_some() {
            return;
        }
        let Some((id, _)) = self.auth.next_for(&self.auth_tabs_shown()) else {
            return;
        };
        self.auth_shown = Some(id);
        self.open_palette("auth");
    }

    /// Sends the answer to the engine (credentials are never kept nor logged), for every
    /// challenge of the same site, then shows the next prompt if any.
    fn answer_auth(&mut self, id: u64, credentials: Option<(String, String)>) {
        if self.auth_shown == Some(id) {
            self.auth_shown = None;
            if self.palette_mode == "auth" {
                self.close_palette();
            }
        }
        self.reply_auth(id, credentials);
        self.service_auth();
    }

    fn reply_auth(&mut self, id: u64, credentials: Option<(String, String)>) {
        for each in self.auth.answer(id, credentials.is_some()) {
            self.auth_origins.remove(&each);
            if let Some(reply) = self.auth_replies.remove(&each) {
                reply.respond(credentials.clone());
            }
        }
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
            .map(|e| json!({"id": format!("easel:{}", e.id), "label": tf("palette.open_easel_named", &[("name", &e.name)]), "hint": "Easel"}))
            .collect();
        // Labels: sentence case, the words of the menus. `keys` is a shortcut the palette spells
        // the way the system does (see ui/keys.js); `confirm` asks for a second Enter.
        let mut actions = vec![
            json!({"id": "new_space", "label": t("palette.new_space")}),
            json!({"id": "new_folder", "label": t("palette.new_folder")}),
            json!({"id": "icon_space", "label": tf("palette.space_emoji", &[("name", &space.name)]), "hint": t("palette.hint_emoji")}),
            json!({"id": "toggle_sidebar", "label": t("palette.toggle_sidebar"), "keys": "mod+b"}),
            json!({"id": "new_easel", "label": t("menu.new_easel"), "hint": "Easel"}),
            json!({"id": "open_easel", "label": t("palette.open_easel"), "keys": "mod+shift+e"}),
            json!({"id": "open_library", "label": t("palette.open_library"), "hint": t("palette.hint_library_all")}),
            json!({"id": "settings", "label": t("palette.open_settings"), "keys": "mod+,"}),
            json!({"id": "default_browser", "label": t("palette.default_browser"), "hint": t("palette.hint_system")}),
            json!({"id": "clear_history", "label": t("palette.clear_history"), "confirm": t("palette.clear_history_confirm")}),
            json!({"id": "open_history", "label": t("palette.open_history"), "hint": t("palette.hint_library")}),
            json!({"id": "recall", "label": t("palette.recall"), "keys": "mod+shift+f"}),
            json!({"id": "tidy", "label": t("palette.tidy"), "hint": t("palette.hint_tabs")}),
            json!({"id": "open_watches", "label": t("palette.watched_areas"), "hint": t("palette.hint_library")}),
            json!({"id": "open_highlights", "label": t("palette.highlights"), "hint": t("palette.hint_library")}),
            json!({"id": "compare", "label": t("palette.compare"), "hint": t("palette.hint_tabs")}),
            json!({"id": "key:shift+t", "label": t("palette.reopen_closed"), "keys": "mod+shift+t"}),
            json!({"id": "key:n", "label": t("palette.new_window"), "keys": "mod+n"}),
            json!({"id": "key:shift+n", "label": t("palette.new_private_window"), "keys": "mod+shift+n"}),
            json!({"id": "burner_tab", "label": t("palette.new_burner_tab"), "hint": t("palette.hint_burner")}),
        ];
        actions.extend(easel_actions);
        actions.extend(self.meeting_actions());
        actions.extend(self.continue_actions());
        actions.extend(self.beam_actions());
        for folder in &space.folders {
            actions.push(json!({
                "id": format!("icon_folder:{}", folder.id.0),
                "label": tf("palette.folder_emoji", &[("name", &folder.name)]),
                "hint": t("palette.hint_emoji"),
            }));
        }
        if self.split_tab().is_some() {
            actions
                .push(json!({"id": "close_split", "label": t("palette.close_split"), "keys": "mod+\\"}));
            let label = if self.pilot_panes().is_some() { "palette.pilot_off" } else { "palette.pilot_on" };
            actions.push(json!({"id": "pilot", "label": t(label), "hint": t("palette.hint_split")}));
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
                    "label": tf("palette.split_with", &[("name", name)]),
                    "hint": t("palette.hint_split"),
                }));
            }
        }
        if let Some(current_tab) = active.map(|t| t.id) {
            actions.extend([
                json!({"id": "key:f", "label": t("palette.find"), "keys": "mod+f"}),
                json!({"id": "key:p", "label": t("palette.print"), "keys": "mod+p"}),
                json!({"id": "key:zoom_in", "label": t("palette.zoom_in"), "keys": "mod+plus"}),
                json!({"id": "key:zoom_out", "label": t("palette.zoom_out"), "keys": "mod+minus"}),
                json!({"id": "key:zoom_reset", "label": t("palette.zoom_reset"), "keys": "mod+0"}),
                json!({"id": "key:r", "label": t("palette.hard_reload"), "keys": "mod+r"}),
                json!({
                    "id": "key:devtools", "label": t("palette.devtools"),
                    "keys": if cfg!(target_os = "macos") { "mod+alt+i" } else { "F12" },
                }),
            ]);
            actions.push(json!({"id": "pip", "label": t("palette.pip"), "hint": t("palette.hint_video")}));
            if self.can_mark(current_tab) {
                actions.push(json!({"id": "highlight", "label": t("palette.highlight"), "hint": t("palette.hint_page")}));
            }
            // Only what the page menu would offer for it (`domain::verbs`): nothing is kept of
            // a black hole tab.
            if self.front_page_allows(Verb::Keep) {
                actions.push(
                    json!({"id": "capture_easel", "label": t("palette.capture_easel"), "keys": "mod+shift+s"}),
                );
            }
            if self.front_page_allows(Verb::LiveArea) {
                actions.push(
                    json!({"id": "capture_live", "label": t("palette.capture_live"), "keys": "mod+shift+l"}),
                );
            }
        }
        if let Some(site) = &site {
            if self.recall_on() && !blackhole_domain::recall::excluded(site, &self.svc.settings().recall_exclude) {
                actions.push(json!({"id": "recall_exclude", "label": tf("palette.recall_exclude", &[("site", site)]), "hint": "Recall"}));
            }
            actions.push(json!({"id": "boost", "label": tf("palette.boost_site", &[("site", site)]), "hint": "Boost"}));
            let routed = self.svc.route_for(site);
            for other in ws.spaces().iter().filter(|s| Some(s.id) != routed) {
                actions.push(json!({
                    "id": format!("route:{}", other.id.0),
                    "label": tf("palette.route_to", &[("site", site), ("space", &other.name)]),
                    "hint": "Air Traffic",
                }));
            }
            if routed.is_some() {
                actions.push(json!({"id": "route_clear", "label": tf("palette.route_clear", &[("site", site)]), "hint": "Air Traffic"}));
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
            "burner": self.burner_next,
            "recall": self.recall_state(),
            "animations": self.svc.settings().animations,
            "url": url,
            "engine": self.svc.settings().search_engine.label(),
            "color": space.color,
            "menu": self.menu,
            "compare": self.comparing.result,
            "auth": self.auth_shown.and_then(|id| self.auth.get(id)).map(|c| json!({
                "id": self.auth_shown,
                "host": c.host,
                "realm": c.realm,
                "secure": c.secure,
                "retry": c.retry,
                // User names only: the password goes from the wallet to the engine.
                "saved": self.auth_shown.map(|id| self.auth_saved_users(id)).unwrap_or_default(),
            })),
            // The highlights, watched areas, archive and downloads the search also looks in.
            "found": if matches!(self.palette_mode.as_str(), "new" | "url") { self.found_json() } else { json!(null) },
            "boostHost": site,
            "boost": site.as_deref().and_then(|h| self.svc.boost_for(h)),
            "tabs": space.tabs.iter().map(|t| json!({"id": t.id.0, "title": t.title, "url": t.url.as_str()})).collect::<Vec<_>>(),
            "favorites": self.svc.favorites().iter().map(|f| json!({"name": f.name, "url": f.url.as_str()})).collect::<Vec<_>>(),
            "bookmarks": self.bookmarks.load().iter().map(|b| json!({"title": b.title, "url": b.url})).collect::<Vec<_>>(),
            "recent": self.svc.recent_activity(usize::MAX).iter().map(|e| json!({"title": e.title, "url": e.url.as_str()})).collect::<Vec<_>>(),
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
        if id == "recall" {
            self.palette_mode = "recall".to_owned();
            self.push_palette();
            return;
        }
        self.close_palette();
        match id {
            "new_space" => {
                let n = self.svc.workspace().spaces().len();
                self.handle_ui(UiCommand::NewSpace {
                    name: tf("sidebar.default_space", &[("n", &(n + 1).to_string())]),
                    color: SPACE_COLORS[n % SPACE_COLORS.len()].to_owned(),
                });
            }
            "new_folder" => self.handle_ui(UiCommand::NewFolder { parent: None }),
            "pip" => {
                let _ = self.active_view().evaluate_script(views::PIP_TOGGLE_JS);
                if let Some(id) = self.shown {
                    self.pip_toggled(id);
                }
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
            "open_library" => self.handle_ui(UiCommand::OpenLibrary { section: None }),
            "new_meet" => self.open_meet(None),
            "tidy" => self.handle_ui(UiCommand::Tidy),
            "pilot" => self.toggle_pilot(None),
            "burner_tab" => {
                self.new_tab();
                self.burner_next = true;
                self.push_palette();
            }
            "compare" => self.compare_start(),
            other if other.starts_with("meet:") => self.open_meet(Some(other["meet:".len()..].to_owned())),
            other if other.starts_with("beam_clip:") => {
                self.handle_devices(UiCommand::BeamClipboard { device: other["beam_clip:".len()..].to_owned() });
            }
            other if other.starts_with("beam_file:") => {
                self.handle_devices(UiCommand::BeamFile { device: other["beam_file:".len()..].to_owned() });
            }
            other if other.starts_with("continue:") => {
                let tab = self.svc.workspace().active_tab().map(|t| t.id.0);
                if let Some(tab) = tab {
                    self.continue_on(tab, other["continue:".len()..].to_owned());
                }
            }
            "open_history" => {
                self.handle_ui(UiCommand::OpenLibrary { section: Some("history".into()) });
            }
            "open_watches" => {
                self.handle_ui(UiCommand::OpenLibrary { section: Some("watches".into()) });
            }
            "open_highlights" => {
                self.handle_ui(UiCommand::OpenLibrary { section: Some("highlights".into()) });
            }
            "highlight" => self.mark_selection(None),
            "recall_exclude" => {
                if let Some(site) = site {
                    self.handle_recall(UiCommand::RecallExclude { site, on: true });
                }
            }
            // The commands that have a shortcut run exactly as the shortcut does.
            other if other.starts_with("key:") => self.shortcut(&other["key:".len()..]),
            other if other.starts_with("easel:") => {
                if let Ok(id) = other["easel:".len()..].parse() {
                    self.handle_ui(UiCommand::OpenEaselTab { id });
                }
            }
            "capture_easel" => self.handle_ui(UiCommand::CaptureToEasel),
            "capture_live" => self.handle_ui(UiCommand::LivePick),
            "clear_history" => {
                self.svc.clear_history();
                self.recall_forget(None);
                self.toast(&t("toast.history_cleared"));
            }
            "default_browser" => {
                let message = if blackhole_macos_url::set_default_browser() {
                    t("toast.default_browser_confirm")
                } else {
                    t("toast.default_browser_bundle")
                };
                self.toast(&message);
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
                    self.toast(&tf("toast.route_cleared", &[("site", &site)]));
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
                    self.toast(&tf("toast.route_set", &[("site", &site)]));
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
        let peek_store = self.store_of_space(self.svc.workspace().active_space().id);
        let page = views::tab_view(
            &self.window,
            &mut self.web_context,
            PEEK_TAB,
            &url,
            peek_bounds,
            self.svc.policy(),
            &self.mailbox,
            self.webext.as_ref(),
            peek_store,
        );
        match (scrim, page) {
            (Ok(scrim), Ok(page)) => {
                let _ = page.set_visible(true);
                let _ = page.focus();
                self.peek = Some(Peek { scrim, page });
                self.apply_slots();
            }
            (scrim, page) => {
                tracing::error!("cannot create peek: {:?} {:?}", scrim.err(), page.err());
            }
        }
    }

    fn close_peek(&mut self) {
        if self.peek.take().is_some() {
            self.apply_slots();
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
                self.round_ext_popup(&page);
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

    /// Where the window an extension asked for by itself goes: under that extension's button
    /// in the command bar, or at the right edge of the window when the bar has not drawn it.
    #[cfg(windows)]
    fn ext_anchor(&self, page: &Url) -> (f64, f64, f64, f64) {
        let button = self.webext.as_ref().and_then(|host| {
            host.actions().into_iter().find_map(|a| {
                let popup = Url::parse(&host.popup_url(&a.id)?).ok()?;
                (popup.host_str() == page.host_str())
                    .then(|| self.ext_anchors.get(&a.id).copied())
                    .flatten()
            })
        });
        match button {
            Some((x, w)) => (x, 0.0, w, BAR_HEIGHT),
            None => (self.window_size().width, BAR_HEIGHT, 0.0, 0.0),
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
        if let Some(popup) = &self.ext_popup {
            self.round_ext_popup(&popup.page);
        }
    }

    /// Rounded corners for the popup, like the menus; the region has the size of the view, so
    /// this follows every resize.
    #[cfg(windows)]
    fn round_ext_popup(&self, page: &WebView) {
        let radius = (POPUP_RADIUS * self.window.scale_factor()).round() as i32;
        blackhole_macos_webext::round_view(page, radius);
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
            UiCommand::LibraryReady => {
                self.ui_library();
                if self.library {
                    self.library_intro();
                }
            }
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
            UiCommand::AuthAnswer { id, user, password } => {
                self.answer_auth(id, Some((user, password)));
            }
            UiCommand::AuthCancel { id } => self.answer_auth(id, None),
            UiCommand::ErrorProceed { tab } => self.error_proceed(TabId(tab)),
            UiCommand::Action { id } => self.run_action(&id),
            UiCommand::SaveBoost { css } => self.save_boost(&css),
            UiCommand::ClosePeek => self.close_peek(),
            UiCommand::SetTabSleep { minutes } => {
                self.svc.set_tab_sleep_minutes(minutes);
                self.ui_state();
            }
            UiCommand::SetMaxLiveTabs { tabs } => {
                self.svc.set_max_live_tabs(tabs);
                self.discard_idle_tabs();
                self.ui_state();
            }
            UiCommand::SetSpareViews { count } => {
                self.svc.set_spare_views(count);
                self.trim_spares();
                self.spares_due = Some(Instant::now());
                self.ui_state();
            }
            UiCommand::SetHardwareAcceleration { on } => {
                self.svc.set_hardware_acceleration(on);
                self.ui_state();
            }
            UiCommand::SetTrackerBlocking { on } => {
                self.svc.set_tracker_blocking(on);
                // Spare views carry the blocklist of their creation time.
                self.spares.clear();
                self.spares_due = Some(Instant::now() + SPARES_DELAY);
                self.ui_state();
            }
            UiCommand::SetShield { name, on } => {
                if self.svc.set_shield(&name, on) {
                    // Spare views carry the lists and page scripts of their creation time.
                    self.spares.clear();
                    self.spares_due = Some(Instant::now() + SPARES_DELAY);
                    self.ui_state();
                }
            }
            UiCommand::SetSectionCollapsed { section, collapsed } => {
                self.svc.set_section_collapsed(&section, collapsed);
            }
            UiCommand::SetExtensionsInPrivate { on } => {
                self.svc.set_extensions_in_private(on);
                self.ui_state();
            }
            UiCommand::SetAnimations { on } => {
                self.svc.set_animations(on);
                self.ui_state();
            }
            UiCommand::SetLanguage { language } => {
                if self.svc.set_language(&language) {
                    self.language_changed();
                }
            }
            UiCommand::ClearHistory => {
                self.svc.clear_history();
                self.recall_forget(None);
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
            // A quick access tab is never taken off the list by closing it: it only stops.
            UiCommand::Close { id }
                if self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .any(|t| t.id.0 == id && t.pinned) =>
            {
                self.handle_ui(UiCommand::CloseInstance { id });
            }
            UiCommand::Close { id } => {
                self.archive_tab(TabId(id));
                self.svc.close_tab(TabId(id));
                self.drop_view(TabId(id));
                self.sync();
            }
            UiCommand::CloseInstance { id } => {
                let id = TabId(id);
                self.drop_view(id);
                if self.split_tab() == Some(id) {
                    self.svc.clear_split();
                }
                // The tab seen before takes over, as when it is closed; the start page otherwise.
                if self.svc.workspace().active_tab().map(|t| t.id) == Some(id)
                    && !self.svc.release_tab(id)
                {
                    self.home_visible = true;
                    self.easel_tab = false;
                    self.library = false;
                }
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
                let dx = if origin == "home" || origin == "library" {
                    card_rect(&self.sidebar, &self.window_size()).0
                } else {
                    0.0
                };
                self.menu = Some(json!({"x": x + dx, "y": y + BAR_HEIGHT, "items": items}));
                self.open_palette("menu");
            }
            UiCommand::DeleteSpace { id } => {
                // Its tabs go to the archive, like those of a deleted folder: nothing is lost.
                let doomed: Vec<TabId> = self
                    .svc
                    .workspace()
                    .spaces()
                    .iter()
                    .find(|s| s.id.0 == id)
                    .map(|s| s.tabs.iter().map(|t| t.id).collect())
                    .unwrap_or_default();
                if self.svc.workspace().spaces().len() > 1 {
                    for &tab in &doomed {
                        self.archive_tab(tab);
                    }
                }
                let snapshot = self.svc.workspace().snapshot_space(SpaceId(id));
                if let Some(tabs) = self.svc.delete_space(SpaceId(id)) {
                    for tab in tabs {
                        self.drop_view(tab);
                    }
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                    self.sync();
                    if let Some(snapshot @ Removed::Space { .. }) = snapshot {
                        let name = match &snapshot {
                            Removed::Space { space, .. } => space.name.clone(),
                            Removed::Folder { .. } => String::new(),
                        };
                        self.toast_undo(&tf("toast.space_deleted", &[("name", &name)]), snapshot);
                    }
                }
            }
            UiCommand::UndoRemoval => self.undo_removal(),
            UiCommand::NoticeDone => self.notice_done(),
            UiCommand::PrivacyReport { x, y } => self.privacy_report((x, y + BAR_HEIGHT)),
            UiCommand::WatchPick => {
                self.live_pick_start();
                self.pick_watches = self.live_pick.is_some();
            }
            UiCommand::WatchArea => {
                if let Some((tab, selector, _)) = self.live_pending.take() {
                    self.watch_area(tab, &selector);
                }
            }
            command @ (UiCommand::RecallSearch { .. }
            | UiCommand::SetRecall { .. }
            | UiCommand::RecallExclude { .. }
            | UiCommand::RecallForget { .. }
            | UiCommand::RecallClear) => self.handle_recall(command),
            command @ (UiCommand::HighlightSelection { .. }
            | UiCommand::HighlightRemove { .. }
            | UiCommand::HighlightNote { .. }
            | UiCommand::HighlightEdit { .. }
            | UiCommand::HighlightOpen { .. }
            | UiCommand::HighlightToEasel { .. }) => self.handle_marks(command),
            command @ (UiCommand::SignalAct { .. }
            | UiCommand::SignalDismiss { .. }
            | UiCommand::LinkToEasel { .. }
            | UiCommand::FoundToEasel { .. }
            | UiCommand::WatchToEasel { .. }
            | UiCommand::CompareToEasel { .. }
            | UiCommand::BeamToEasel { .. }
            | UiCommand::EaselWatch { .. }
            | UiCommand::AuthSaved { .. }) => self.handle_bridges(command),
            command @ (UiCommand::WatchRemove { .. }
            | UiCommand::WatchCheck { .. }
            | UiCommand::WatchesSeen) => self.handle_watch(command),
            UiCommand::Tidy => {
                let groups = self.svc.workspace().tidy_suggestion(TIDY_MIN);
                if groups.is_empty() {
                    self.toast(&t("toast.nothing_to_tidy"));
                    return;
                }
                let done = self.svc.tidy(&groups);
                let message = tn(
                    "toast.tidied",
                    groups.len(),
                    &[("tabs", &done.tabs().to_string())],
                );
                self.tidied = Some(done);
                self.sync();
                self.notify(notices::Notice::Undo, &message, json!([{"label": t("toast.undo"), "run": {"cmd": "untidy"}}]));
            }
            UiCommand::Untidy => {
                if let Some(done) = self.tidied.take() {
                    self.svc.untidy(&done);
                    self.sync();
                }
            }
            UiCommand::DismissTidy => {
                self.tidy_dismissed = true;
                self.ui_state();
            }
            UiCommand::DismissDownload { id } => {
                // A download still running keeps its card.
                if self.dl_live.get(&id).is_some_and(|d| d.finished.is_some()) {
                    self.dl_live.remove(&id);
                    let list = self.downloads_json();
                    self.ui(format!("window.bh&&bh.downloads({list})"));
                }
            }
            command @ (UiCommand::OpenMeet { .. }
            | UiCommand::MeetingNotes { .. }
            | UiCommand::MeetingDismiss { .. }
            | UiCommand::CalendarAdd { .. }
            | UiCommand::CalendarRemove { .. }
            | UiCommand::CalendarSync
            | UiCommand::SetMeetingOption { .. }) => self.handle_calendar(command),
            command @ (UiCommand::DeviceShowCode
            | UiCommand::DeviceJoin { .. }
            | UiCommand::DeviceCancelPairing
            | UiCommand::DeviceForget { .. }
            | UiCommand::DeviceSync { .. }
            | UiCommand::DeviceSyncNow { .. }
            | UiCommand::SyncAnswer { .. }
            | UiCommand::BeamText { .. }
            | UiCommand::BeamClipboard { .. }
            | UiCommand::BeamFile { .. }
            | UiCommand::BeamAnswer { .. }
            | UiCommand::BeamCopy { .. }
            | UiCommand::SendLink { .. }
            | UiCommand::SendDownload { .. }
            | UiCommand::ContinueOn { .. }) => self.handle_devices(command),
            UiCommand::OpenLink { url, target } => match target.as_str() {
                "window" | "private" => {
                    if matches!(Url::parse(&url), Ok(u) if matches!(u.scheme(), "http" | "https")) {
                        open_window(target == "private", Some(&url));
                    }
                }
                "burner" => {
                    if matches!(Url::parse(&url), Ok(u) if matches!(u.scheme(), "http" | "https")) {
                        self.open_burner(&url);
                    }
                }
                // A new tab shown at once (the selection's search or address).
                "front" => {
                    if matches!(Url::parse(&url), Ok(u) if matches!(u.scheme(), "http" | "https")) {
                        self.open(&url);
                    }
                }
                _ => self.open_in_background(&url),
            },
            UiCommand::OpenPeek { url } => {
                if let Ok(url) = Url::parse(&url) {
                    self.open_peek(&url);
                }
            }
            UiCommand::LiveCaptureTo { easel } => {
                if let Some((tab, selector, size)) = self.live_pending.take() {
                    self.capture_live(tab, &selector, size, easel);
                }
            }
            UiCommand::SetSpaceColor { id, color } => {
                if self.svc.set_space_color(SpaceId(id), &color) {
                    self.sync();
                }
            }
            UiCommand::TabHome { id } => {
                let id = TabId(id);
                if self.svc.go_home(id) {
                    let home = self
                        .svc
                        .workspace()
                        .spaces()
                        .iter()
                        .flat_map(|s| s.tabs.iter())
                        .find(|t| t.id == id)
                        .map(|t| t.url.clone());
                    if let (Some(url), Some(view)) = (home, self.tabs.get(&id)) {
                        let _ = view.load_url(url.as_str());
                    }
                    self.sync();
                }
            }
            UiCommand::TabSetHome { id } => {
                if self.svc.set_home(TabId(id)) {
                    self.ui_state();
                }
            }
            UiCommand::ReloadTab { id } => {
                if let Some(view) = self.tabs.get(&TabId(id)) {
                    views::reload(view);
                }
            }
            // Not `open`: opening an address shows the tab already on it instead of a copy.
            UiCommand::DuplicateTab { id } => {
                let ws = self.svc.workspace();
                if ws.is_burner(TabId(id)) && ws.burner_count() >= MAX_BURNERS {
                    self.toast(&t("toast.too_many_burners"));
                    return;
                }
                if self.svc.duplicate_tab(TabId(id)).is_some() {
                    self.close_palette();
                    self.close_peek();
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                    self.sync();
                }
            }
            UiCommand::MoveTabToSpace { id, space } => {
                let name = self
                    .svc
                    .workspace()
                    .spaces()
                    .iter()
                    .find(|s| s.id.0 == space)
                    .map(|s| s.name.clone());
                // Spaces have their own cookies: the view is rebuilt on the new space's profile
                // the next time the tab is shown.
                if self.svc.move_tab_to_space(TabId(id), SpaceId(space)) {
                    self.drop_view(TabId(id));
                    self.sync();
                    self.toast(&tf("toast.moved_to", &[("name", &name.unwrap_or_default())]));
                }
            }
            UiCommand::CloseOtherTabs { id } => {
                let others: Vec<TabId> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| t.id.0 != id && !t.pinned && t.folder.is_none())
                    .map(|t| t.id)
                    .collect();
                for &tab in &others {
                    self.archive_tab(tab);
                    self.svc.close_tab(tab);
                    self.drop_view(tab);
                }
                self.svc.activate_tab(TabId(id));
                self.home_visible = false;
                self.sync();
                let n = others.len();
                self.toast(&tn("toast.tabs_closed", n, &[]));
            }
            UiCommand::ClosePane { side } if self.easel_side() => {
                // Closing the tab's pane leaves the easel alone, in the content area.
                let easel = self.easel_beside.map(|(_, id)| id);
                self.close_easel_side();
                if let Some(id) = easel.filter(|_| side == "left") {
                    self.open_easel_tab(id);
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
                // The site may already be open as a quick access tab: keep it pinned.
                let tab = self.svc.workspace().active_tab().filter(|t| !t.pinned).map(|t| t.id);
                if let Some(id) = tab {
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
            UiCommand::Pilot { id } => self.toggle_pilot(id.map(TabId)),
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
            UiCommand::NewFolder { parent } => {
                match parent {
                    Some(p) => {
                        let made = self.svc.add_subfolder(&t("palette.new_folder"), FolderId(p));
                        // Show the new subfolder: a collapsed parent would hide it.
                        if self
                            .svc
                            .workspace()
                            .active_space()
                            .folders
                            .iter()
                            .any(|f| f.id == FolderId(p) && f.collapsed)
                        {
                            self.svc.toggle_folder(FolderId(p));
                        }
                        self.ui_state();
                        if let Some(id) = made {
                            self.ui(format!("window.bh&&bh.editFolderId({})", id.0));
                        }
                        return;
                    }
                    None => {
                        self.svc.add_folder(&t("palette.new_folder"));
                    }
                }
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
            UiCommand::RenameTab { id, name } => {
                self.svc.rename_tab(TabId(id), &name);
                self.ui_state();
            }
            UiCommand::EditTab { id } => self.ui(format!("window.bh&&bh.editTab({id})")),
            UiCommand::EditFolder { id } => {
                self.ui(format!("window.bh&&bh.editFolderId({id})"))
            }
            UiCommand::ToggleFolder { id } => {
                self.svc.toggle_folder(FolderId(id));
                self.ui_state();
            }
            UiCommand::MoveFolder { id, parent, before } => {
                self.svc
                    .move_folder(FolderId(id), parent.map(FolderId), before.map(FolderId));
                self.ui_state();
            }
            UiCommand::DeleteFolder { id } => {
                // The folder takes its tabs with it (they go to the archive like any closed tab).
                let doomed = self.svc.workspace().folder_tree(FolderId(id));
                let inside: Vec<TabId> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| t.folder.is_some_and(|f| doomed.contains(&f)))
                    .map(|t| t.id)
                    .collect();
                let snapshot = self.svc.workspace().snapshot_folder(FolderId(id));
                for tab in inside {
                    self.archive_tab(tab);
                    self.svc.close_tab(tab);
                    self.drop_view(tab);
                }
                if self.svc.delete_folder(FolderId(id)) {
                    if let Some(snapshot) = snapshot {
                        let name = match &snapshot {
                            Removed::Folder { folders, .. } => {
                                folders.first().map(|(_, f)| f.name.clone()).unwrap_or_default()
                            }
                            Removed::Space { .. } => String::new(),
                        };
                        self.toast_undo(&tf("toast.folder_deleted", &[("name", &name)]), snapshot);
                    }
                }
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
                    t("sidebar.space")
                } else {
                    name
                };
                self.svc.add_space(&name, &color);
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
            UiCommand::PanelWindow { window, action } => {
                self.panel_window_action(&window, &action)
            }
            UiCommand::Shortcut { key } => self.shortcut(&key),
            UiCommand::Back => {
                // On our error page, "back" is the page seen before the failure (the entry
                // just before is the address that failed).
                let on_error = self.svc.workspace().active_tab().is_some_and(|t| self.error_shown(t.id));
                self.with_active_tab(if on_error { views::error_back } else { views::go_back });
            }
            UiCommand::Forward => self.with_active_tab(views::go_forward),
            UiCommand::Reload => self.with_active_tab(views::reload),
            UiCommand::Passwords { action } => self.password_action(action),
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
            | UiCommand::OpenLibrary { .. }
            | UiCommand::CloseLibrary
            | UiCommand::LibraryOpenFile { .. }
            | UiCommand::LibraryDeleteDownload { .. }
            | UiCommand::LibraryClear { .. }
            | UiCommand::ReopenArchived { .. }
            | UiCommand::DeleteArchived { .. }
            | UiCommand::ForgetHistory { .. }
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
            UiCommand::CheckAppUpdate => {
                self.check_for_update();
                self.ui_state();
            }
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
            UiCommand::ExtAnchors { anchors } => {
                #[cfg(windows)]
                {
                    self.ext_anchors = anchors;
                }
                #[cfg(not(windows))]
                let _ = anchors;
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
                            Err(e) => (message("settings.import_failed", &[("from", &s.browser), ("why", &e.to_string())]), Vec::new()),
                        },
                        None => (message("settings.import_no_source", &[]), Vec::new()),
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
                let logins = self.passwords.import_sender();
                self.spawn_import(move |bookmarks, _| {
                    Some(import_file(bookmarks, &logins, &kind, space))
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
            None => self.toast(&t("toast.split_needs_tab")),
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
            "shift+t" => self.reopen_closed(),
            "recent_tab" | "next_tab" | "prev_tab" => self.switch_tab(key),
            "r" => self.with_active_tab(views::hard_reload),
            "reload" => self.handle_ui(UiCommand::Reload),
            "b" => self.handle_ui(UiCommand::Lock),
            "[" => self.handle_ui(UiCommand::Back),
            "]" => self.handle_ui(UiCommand::Forward),
            "," => self.settings_wanted = true,
            "shift+e" => self.handle_ui(UiCommand::OpenEasel { id: None }),
            "shift+s" => self.handle_ui(UiCommand::CaptureToEasel),
            "shift+l" => self.handle_ui(UiCommand::LivePick),
            "shift+f" => self.open_palette("recall"),
            "\\" => self.toggle_split(),
            "zoom_in" | "zoom_out" | "zoom_reset" => self.zoom_active(key),
            "f" => self.with_active_tab(|v| {
                let _ = v.evaluate_script("window.__bhFind&&__bhFind()");
            }),
            "g" | "shift+g" => self.with_active_tab(|v| {
                let call = if key == "g" { "next" } else { "prev" };
                let _ = v.evaluate_script(&format!("window.__bhFind&&__bhFind('{call}')"));
            }),
            "p" => self.with_active_tab(|v| {
                let _ = v.print();
            }),
            // The engine's inspector for the page shown: elements and styles, console, network.
            "devtools" => self.with_active_tab(WebView::open_devtools),
            _ => {}
        }
    }

    /// The zoom of `host`'s site for `tab`, in percent: remembered in the settings, or for the
    /// session only in a black hole tab (it keeps nothing).
    fn zoom_of(&self, tab: TabId, host: &str) -> u32 {
        if self.svc.workspace().is_burner(tab) {
            self.zoom.get(&blackhole_domain::site_key(host)).copied().unwrap_or(100)
        } else {
            self.svc.settings().zoom_of(host)
        }
    }

    /// Zooms the active page like other browsers do: the level belongs to the site (`www.`
    /// included) and is remembered between launches.
    fn zoom_active(&mut self, action: &str) {
        let Some(tab) = self.svc.workspace().active_tab().map(|t| t.id) else {
            return;
        };
        let host = self
            .tabs
            .get(&tab)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok())
            .and_then(|u| u.host_str().map(str::to_owned));
        let Some(host) = host else { return };
        let current = self.zoom_of(tab, &host);
        let next = match action {
            "zoom_in" => blackhole_domain::next_zoom(current, true),
            "zoom_out" => blackhole_domain::next_zoom(current, false),
            _ => 100,
        };
        if self.svc.workspace().is_burner(tab) {
            let site = blackhole_domain::site_key(&host);
            if next == 100 {
                self.zoom.remove(&site);
            } else {
                self.zoom.insert(site, next);
            }
        } else {
            self.svc.set_site_zoom(&host, next);
        }
        // Every tab of this window showing the site follows, as in other browsers.
        let ids: Vec<TabId> = self.tabs.keys().copied().collect();
        for id in ids {
            self.apply_zoom(id);
        }
        self.ui_bar();
        self.toast(&tf("toast.zoom", &[("n", &next.to_string())]));
    }

    /// Gives a tab the zoom of the site it shows (100% when the site has none).
    fn apply_zoom(&mut self, tab: TabId) {
        let Some(view) = self.tabs.get(&tab) else { return };
        let host = view
            .url()
            .ok()
            .and_then(|raw| Url::parse(&raw).ok())
            .and_then(|u| u.host_str().map(str::to_owned));
        let wanted = host.map_or(100, |h| self.zoom_of(tab, &h));
        let zoomed = self.zoomed.contains(&tab);
        if wanted == 100 && !zoomed {
            return;
        }
        let _ = view.zoom(f64::from(wanted) / 100.0);
        if wanted == 100 {
            self.zoomed.remove(&tab);
        } else {
            self.zoomed.insert(tab);
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
        self.apply_zoom(tab);
        if let (Some(css), Some(view)) = (boost, self.tabs.get(&tab)) {
            views::apply_boost(view, &css);
        }
        self.svc.page_changed(tab, Some(url), None);
        self.ui_stale = true;
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
                    self.live_picked(tab, selector, (w, h));
                }
            }
            Msg::Page(tab, PageEvent::OpenLink { url }) => {
                if let Ok(target) = Url::parse(&url) {
                    self.opened_from(tab, &target);
                }
                self.open_in_background(&url);
                self.referrer_next = None;
            }
            Msg::Page(tab, PageEvent::PilotLink { url }) => self.pilot_link(tab, &url),
            Msg::Page(tab, PageEvent::LinkMenu { url, text, mark, image, x, y }) => {
                self.link_menu(tab, &url, &text, mark, &image, (x, y))
            }
            Msg::Page(tab, PageEvent::DebugLog { msg }) => {
                if cfg!(debug_assertions) {
                    eprintln!("[page {}] {}", tab.0, msg.chars().take(600).collect::<String>());
                }
            }
            Msg::Page(tab, PageEvent::PipLeft { paused, stream }) => self.pip_left(tab, paused, stream),
            Msg::Page(tab, PageEvent::AutofillQuery { fresh }) => self.autofill_query(tab, fresh),
            Msg::Page(tab, PageEvent::AutofillFill { index, token }) => {
                self.autofill_fill(tab, index, &token)
            }
            Msg::Page(tab, PageEvent::AutofillUnlock { token }) => self.autofill_unlock(tab, &token),
            Msg::Page(tab, PageEvent::AutofillGenerate { token }) => {
                self.autofill_generate(tab, &token)
            }
            Msg::Page(tab, PageEvent::LoginSubmitted { username, password }) => {
                self.login_submitted(tab, &username, &password)
            }
            Msg::Page(tab, PageEvent::Location) => {
                self.recall_seen(tab);
                self.marks_push(tab, false);
                self.pilot_mark(tab);
                self.media_hinted(tab);
                self.refresh_url(tab)
            }
            Msg::Page(tab, PageEvent::Favicon { url }) => {
                let page = self.tabs.get(&tab).and_then(|v| v.url().ok());
                if let Some(page) = page.and_then(|raw| Url::parse(&raw).ok()) {
                    if self.favicons.set(&page, &url) {
                        self.icons_for_ui.set(true);
                        self.icons_for_home.set(true);
                        self.ui_state();
                    }
                }
            }
            Msg::Page(tab, PageEvent::Blocked { host }) => {
                let site = self.site_of(tab);
                self.on_blocked("", &host, &site);
            }
            Msg::Url(tab, url) => {
                self.pilot_mark(tab);
                self.page_url(tab, url);
            }
            Msg::LoadStart(tab) => {
                self.aborted.remove(&tab);
                self.dl_navs.remove(&tab);
                self.errors_load_start(tab);
                self.awaiting.remove(&tab);
                self.pending.remove(&tab);
                self.loading.insert(tab, Instant::now());
                self.ui_bar();
            }
            Msg::Loaded(tab, url) => {
                self.recall_seen(tab);
                self.marks_push(tab, true);
                self.pilot_mark(tab);
                self.media_hinted(tab);
                self.errors_loaded(tab, &url);
                self.auth.loaded(tab);
                self.loading.remove(&tab);
                self.pending.remove(&tab);
                if self.splash.remove(&tab).is_some() {
                    self.sync();
                } else {
                    self.ui_bar();
                }
                if tab != PEEK_TAB && url.scheme() != "about" {
                    let injection = self.svc.injection_for(&url);
                    if let (false, Some(view)) = (injection.is_empty(), self.tabs.get(&tab)) {
                        views::apply_injection(view, &injection.css, &injection.js);
                    }
                }
            }
            Msg::Failed(tab, url) => {
                trace(|| format!("failed tab={} {url}", tab.0));
                if self.download_took(tab) {
                    self.ui_bar();
                } else if self.failure_is_abort(tab) {
                    // WebView2 ends a navigation that becomes a download as "connection
                    // aborted", just before it says a download starts: wait for that word.
                    self.aborted.insert(tab, (Instant::now(), url));
                } else {
                    self.on_failed(tab, url);
                }
            }
            Msg::Crashed(tab) => self.on_crashed(tab),
            Msg::Page(tab, PageEvent::Media { .. }) => self.media_hinted(tab),
            Msg::LoadFailure(tab, failure) => {
                trace(|| format!("load failure tab={} {} {} {}", tab.0, failure.domain, failure.code, failure.url));
                if failure.http_status > 0 || !self.download_took(tab) {
                    self.on_load_failure(tab, failure);
                }
            }
            Msg::ServerTrust(tab, challenge) => self.on_server_trust(tab, challenge),
            Msg::Upgraded(tab, plain) => self.on_upgraded(tab, plain),
            Msg::BlockedPage(tab, url, why) => self.on_blocked_page(tab, url, why),
            Msg::LinkCleaned => self.links_cleaned = self.links_cleaned.saturating_add(1),
            Msg::Page(tab, PageEvent::ErrorMenu { x, y }) => self.error_menu(tab, x, y),
            Msg::Page(tab, PageEvent::ErrorBack { length }) => self.error_back(tab, length),
            Msg::Page(tab, PageEvent::ErrorRetry) => self.error_retry(tab),
            Msg::Auth(tab, challenge, reply) => {
                self.errors_auth_asked(tab);
                self.on_auth(tab, &challenge, reply);
            }
            Msg::Title(tab, _) if tab == PEEK_TAB => {}
            Msg::Title(tab, title) => {
                self.svc.page_changed(tab, None, Some(title));
                self.ui_stale = true;
            }
            // An extension asking for a window (Bitwarden's unlock) gets a popup, not a tab.
            #[cfg(windows)]
            Msg::OpenTab(url) if blackhole_domain::is_extension_popout(&url) => {
                self.close_ext_popup();
                let rect = self.ext_anchor(&url);
                self.show_ext_popup(url.as_str(), rect);
            }
            #[cfg(target_os = "macos")]
            Msg::OpenTab(url)
                if blackhole_domain::is_extension_popout(&url)
                    && self.show_native_popup(
                        url.host_str().unwrap_or_default(),
                        (self.window_size().width, BAR_HEIGHT, 0.0, 0.0),
                    ) => {}
            Msg::OpenTab(url) => {
                trace(|| format!("open tab {url}"));
                self.open(url.as_str());
            }
            Msg::OpenTabFrom(opener, url) => {
                trace(|| format!("open tab {url} from tab {}", opener.0));
                self.opened_from(opener, &url);
                self.open(url.as_str());
                self.referrer_next = None;
            }
            Msg::External(url) => {
                self.window.set_minimized(false);
                self.window.focus_window();
                self.open(url.as_str());
            }
            Msg::Redirect(tab, url) => {
                if self.refuse_restricted_port(tab, &url) {
                    // Nothing to load: WebKit would refuse it without a word.
                } else if let Some(view) = self.view_of(tab) {
                    let _ = view.load_url(url.as_str());
                }
            }
            Msg::DownloadStarted { tab, url, path } => {
                trace(|| format!("download started tab={} {url}", tab.0));
                // The navigation became a download: the tab is not waiting for a page any more
                // (else "Site injoignable" after CONNECT_MAX). A tab opened for the file alone,
                // that never showed a page, closes once the file is done, as in other browsers.
                if self.awaiting.remove(&tab).is_some() && self.splash.contains_key(&tab) {
                    self.dl_only_tabs.insert(url.clone(), tab);
                }
                self.dl_navs.retain(|_, at| at.elapsed() < DOWNLOAD_NAV);
                self.dl_navs.insert(tab, Instant::now());
                if self.aborted.remove(&tab).is_some() {
                    self.forget_failure(tab);
                }
                self.loading.remove(&tab);
                self.pending.remove(&tab);
                self.reveal_sidebar(DOWNLOAD_REVEAL);
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
                self.sync_bar_notice();
                self.notify(notices::Notice::Download, &t("toast.download_started"), serde_json::Value::Null);
                self.ui_library();
            }
            Msg::DownloadDone { url, path, ok } => {
                if let Some(tab) = self.dl_only_tabs.remove(&url) {
                    if self.tabs.contains_key(&tab) {
                        self.handle_ui(UiCommand::Close { id: tab.0 });
                    }
                }
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
                self.reveal_sidebar(DOWNLOAD_REVEAL);
                self.sync_bar_notice();
                let (kind, key) = if ok {
                    (notices::Notice::Done, "toast.download_complete")
                } else {
                    (notices::Notice::Problem, "toast.download_failed")
                };
                self.notify(kind, &t(key), serde_json::Value::Null);
                self.ui_library();
            }
            Msg::Blocked { why, host } => self.on_blocked(why, &host, ""),
        }
    }

    /// The sidebar's tidy-up suggestion: from `TIDY_FROM` loose tabs, the sites that would get
    /// a folder (`null` when there is none, or when dismissed this session).
    fn tidy_json(&self) -> serde_json::Value {
        let ws = self.svc.workspace();
        let loose = ws.active_space().tabs.iter().filter(|t| !t.pinned && t.folder.is_none()).count();
        if self.tidy_dismissed || loose < TIDY_FROM {
            return serde_json::Value::Null;
        }
        let groups = ws.tidy_suggestion(TIDY_MIN);
        if groups.is_empty() {
            return serde_json::Value::Null;
        }
        json!({
            "tabs": groups.iter().map(|g| g.tabs.len()).sum::<usize>(),
            "groups": groups.iter().map(|g| json!({"name": g.name, "count": g.tabs.len()})).collect::<Vec<_>>(),
        })
    }

    /// The shield's report: for the site shown, what was blocked this session (trackers and
    /// ads apart, the hosts tried most) and a grade; on the start page, the session total.
    fn privacy_report(&mut self, at: (f64, f64)) {
        let mut items = Vec::new();
        match self.active_site() {
            Some(site) => {
                let hosts = self.blocked.for_site(&site);
                let policy = self.svc.policy();
                let is_ad = |host: &str| {
                    Url::parse(&format!("https://{host}/")).is_ok_and(|u| {
                        policy.evaluate(&u) == blackhole_domain::PrivacyDecision::Block("ad blocked")
                    })
                };
                let (mut trackers, mut ads) = (0, 0);
                for (host, n) in &hosts {
                    if is_ad(host) {
                        ads += n;
                    } else {
                        trackers += n;
                    }
                }
                let grade = blackhole_domain::privacy_grade(trackers, ads);
                items.push(json!({"head": tf("menu.privacy_grade", &[("site", &site), ("grade", &grade.to_string())])}));
                let shown = self.svc.workspace().active_tab().map(|t| t.url.clone());
                if shown.is_some_and(|u| errors::insecure(&u)) {
                    items.push(json!({"icon": "close", "label": t("menu.insecure_exception")}));
                }
                items.push(json!({"icon": "shield", "label": tn("menu.trackers_blocked", trackers as usize, &[])}));
                items.push(json!({"icon": "shield", "label": tn("menu.ads_blocked", ads as usize, &[])}));
                if hosts.is_empty() {
                    items.push(json!({"icon": "check", "label": t("menu.nothing_followed")}));
                } else {
                    items.push(json!({"sep": true}));
                    items.push(json!({"head": t("menu.blocked_hosts")}));
                    items.extend(hosts.iter().take(8).map(|(host, n)| {
                        json!({"icon": if is_ad(host) { "close" } else { "eye" }, "label": format!("{host} · {n}")})
                    }));
                }
            }
            None => {
                items.push(json!({"head": t("menu.this_session")}));
                items.push(json!({"icon": "shield", "label": tn("menu.requests_blocked", self.blocked.total() as usize, &[])}));
            }
        }
        if self.links_cleaned > 0 {
            items.push(json!({"icon": "link", "label": tn("menu.links_cleaned", self.links_cleaned as usize, &[])}));
        }
        items.push(json!({"sep": true}));
        items.push(json!({"icon": "settings", "label": t("menu.privacy_settings"), "run": {"cmd": "open_settings", "tab": "privacy"}}));
        self.show_menu(at, json!(items));
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
        // `why` is the domain's reason ("tracker blocked"…), shown in the interface language.
        if !why.is_empty() {
            self.toast(&t(&format!("blocked.{}", why.replace(' ', "_"))));
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
        let mut animating = false;
        if let Self::Running(app) = self {
            // Closed last pass: no longer the key window, safe to destroy.
            app.closed_windows.clear();
            app.service_windows(event_loop);
            if std::mem::take(&mut app.ui_stale) {
                app.ui_state();
            }
            if app.last_flush.elapsed() >= Duration::from_secs(1) {
                app.svc.flush();
                app.last_flush = Instant::now();
            }
            if app.quit {
                event_loop.exit();
                return;
            }
            app.poll_webext();
            // WebKit can reset the layer of a page view when it resizes or reappears.
            #[cfg(target_os = "macos")]
            if app.cards_rounded.elapsed() >= Duration::from_millis(300) {
                app.cards_rounded = Instant::now();
                // Only the cards on screen: the hidden tabs are rounded when they are shown.
                app.round_cards(false);
            }
            app.discard_idle_tabs();
            if app.flush_raise(false) {
                wake = wake.min(RAISE_DELAY);
            }
            app.refill_spares();
            if app.service_loading() {
                wake = wake.min(Duration::from_millis(250));
            }
            app.service_peers();
            app.service_media();
            app.service_stats();
            app.service_calendar();
            app.service_auth();
            app.service_nearby();
            app.service_watches();
            app.service_signals();
            app.service_marks();
            if let Some(soon) = app.service_recall() {
                wake = wake.min(soon.max(Duration::from_millis(20)));
            }
            app.service_compare();
            app.service_errors();
            if app.blur_due.is_some() {
                app.pip_on_blur();
                wake = wake.min(BLUR_DELAY);
            }
            if !app.dl_live.is_empty() {
                wake = wake.min(DOWNLOAD_POLL);
            }
            #[cfg(windows)]
            if app.ext_popup.is_some() {
                app.probe_ext_popup();
                wake = wake.min(Duration::from_millis(250));
            }
            if app.sidebar.held() {
                if app.sidebar.release_due(Instant::now()) {
                    app.layout(false);
                }
                wake = wake.min(Duration::from_millis(200));
            }
            if app.sidebar.moving() {
                let since = app.last_frame.elapsed();
                if since < FRAME {
                    std::thread::sleep(FRAME - since);
                }
                app.last_frame = Instant::now();
                app.tick_sidebar();
                animating = true;
            }
            #[cfg(feature = "devtools")]
            {
                app.dev_tick(event_loop);
                if app.dev_active() {
                    wake = wake.min(Duration::from_millis(30));
                }
            }
        }
        // While the sidebar moves, the loop runs frame after frame (paced above).
        event_loop.set_control_flow(if animating {
            ControlFlow::Poll
        } else {
            ControlFlow::WaitUntil(Instant::now() + wake)
        });
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
            match event {
                WindowEvent::CloseRequested => app.close_settings(),
                // The page is a child view: it follows the window's size by hand. The top bar's
                // maximize button follows the window too.
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    app.settings_resized();
                    let maxed = app.settings.as_ref().is_some_and(|w| w.window.is_maximized());
                    if maxed != app.panels_maxed.0 {
                        app.panels_maxed.0 = maxed;
                        app.ui_settings();
                    }
                }
                _ => {}
            }
            return;
        }
        if app.easel_window_id() == Some(id) {
            match event {
                WindowEvent::CloseRequested => app.close_easel(),
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    app.easel_resized();
                    let maxed = app.easel.as_ref().is_some_and(|w| w.window.is_maximized());
                    if maxed != app.panels_maxed.1 {
                        app.panels_maxed.1 = maxed;
                        app.ui_easel();
                    }
                }
                _ => {}
            }
            return;
        }
        // A Settings / Easel window being retired (or already gone): its last events (focus
        // lost, close, destroyed) must not be read as the main window's.
        if id != app.window.id() {
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
            WindowEvent::Focused(true) => {
                app.apply_focus();
                app.pip_back_on_focus();
            }
            WindowEvent::Focused(false) => app.blur_due = Some(Instant::now() + BLUR_DELAY),
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
            message(
                "settings.import_done",
                &[("from", from), ("n", &new.to_string()), ("known", &(total - new).to_string())],
            ),
            list,
        ),
        Err(e) => (message("settings.import_failed", &[("from", from), ("why", &e.to_string())]), Vec::new()),
    }
}

fn import_file(
    bookmarks: &BookmarkStore,
    logins: &std::sync::mpsc::Sender<Vec<import::Login>>,
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
        return (message("settings.import_unreadable", &[]), Vec::new());
    };
    if kind == "logins" {
        // Added to the wallet on the UI thread, which sets the message.
        let msg = match import::parse_logins_csv(&text) {
            Ok(list) => {
                let _ = logins.send(list);
                String::new()
            }
            Err(e) => e,
        };
        return (msg, Vec::new());
    }
    store_bookmarks(bookmarks, import::parse_html(&text), &message("settings.import_from_html", &[]), space)
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
    !cfg!(feature = "devtools") && can_self_update()
}

/// Whether this installation may download and install a release (Settings can still check).
fn can_self_update() -> bool {
    !private_mode() && updater::supported()
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
    blackhole_infrastructure::flush_writes();
    peers::withdraw(&peers_dir(), std::process::id());
    if private_mode() {
        let _ = std::fs::remove_dir_all(private_data_dir());
    }
}

#[cfg(test)]
mod tests {
    use super::overlap_share;

    #[test]
    fn overlap_share_is_the_hidden_part_of_the_window() {
        let own = (0.0, 0.0, 1000.0, 800.0);
        assert_eq!(overlap_share(own, (1200.0, 0.0, 800.0, 600.0)), 0.0);
        assert_eq!(overlap_share(own, (-10.0, -10.0, 2000.0, 1000.0)), 1.0);
        assert!((overlap_share(own, (500.0, 0.0, 1000.0, 800.0)) - 0.5).abs() < 1e-9);
    }
}

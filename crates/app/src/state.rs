//! The state of a window of the app (`Running`) and its small accessors.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use blackhole_application::BrowserService;
use blackhole_domain::{BlockLog, Easels, http_auth, Removed, TabId};
use blackhole_infrastructure::{import, JsonWorkspaceRepository, market};
use blackhole_infrastructure::store::{BookmarkStore, EaselStore};
use url::Url;
use winit::window::Window;
use wry::{WebContext, WebView};

use crate::{ipc, views};
use crate::features::{
    compare,
    github_ext,
    handoff,
    highlights,
    meet,
    passwords,
    recall,
    stats,
    suggesting,
    trackers,
    trash,
    watching,
};
use crate::features::easel::{Capture, EaselWindow};
use crate::features::imports::ImportDone;
use crate::features::updates::{UpdateMsg, UpdateState};
use crate::ipc::Mailbox;
use crate::shell::pane_motion;
use crate::shell::panels::LazyView;
use crate::shell::peek::{Peek, PEEK_TAB};
use crate::shell::sidebar::Sidebar;
use crate::tabs::{errors, favicons, media};
use crate::tabs::downloads::DownloadLive;
use crate::tabs::media::MediaProbe;

#[cfg(feature = "devtools")]
use crate::devtools;
#[cfg(windows)]
use crate::backdrop;
#[cfg(windows)]
use crate::shell::ext_popup::ExtPopup;

pub(crate) struct Running {
    pub(crate) chrome: WebView,
    /// Copy of the sidebar floating over the page while the real one is hidden.
    /// The overlays (hotzone, flyout, command bar) are created just after the window shows its
    /// first views (`raise_due` at launch), not before: three engine views less before the window
    /// has content. `None` until then (or if the engine refused one).
    pub(crate) flyout: Option<WebView>,
    pub(crate) hotzone: Option<WebView>,
    pub(crate) bar: WebView,
    pub(crate) home: WebView,
    pub(crate) tabs: HashMap<TabId, WebView>,
    /// Blank hidden page views ready to take the next tabs (see `views::spare_tab_view`).
    pub(crate) spares: Vec<(views::Store, views::TabSlot, WebView)>,
    pub(crate) spares_due: Option<Instant>,
    /// When the anchored tabs are woken at launch (once, after the window settles).
    pub(crate) anchors_due: Option<Instant>,
    /// The password wallet: autofill, save offers, Settings › Passwords.
    pub(crate) passwords: passwords::Passwords,
    pub(crate) last_active: HashMap<TabId, Instant>,
    /// Page zoom per site of black hole tabs (percent), for the session only: other tabs use
    /// `Settings::site_zoom`, remembered between launches.
    pub(crate) zoom: HashMap<String, u32>,
    /// Tabs whose view currently has a zoom other than 100%.
    pub(crate) zoomed: HashSet<TabId>,
    /// Set when a tab view was created above the overlays; they are rebuilt once it is due.
    pub(crate) raise_due: Option<Instant>,
    /// Tab whose page was on screen at the last `sync` (drives automatic Picture in Picture).
    pub(crate) shown: Option<TabId>,
    /// Tab floated because the user left it for another tab (its "back to tab" button returns).
    pub(crate) pip_tab: Option<TabId>,
    /// The probe has seen the floating video of `pip_tab`: its disappearance means it came back.
    pub(crate) pip_seen: bool,
    /// `pip_tab` when the probe stopped seeing its floating video, and when. WebKit changes
    /// the video's mode as soon as the window's "back to tab" button is pressed but only tells
    /// the page once the window has shrunk back: the page's `pip_left` arrives after the probe
    /// and must still count as a video the shell floated (`pip::LEFT_EVENT_GRACE`).
    pub(crate) pip_faded: Option<(TabId, Instant)>,
    /// Last time a tab was seen in a call or playing media: it keeps its page for a while after.
    pub(crate) busy_seen: HashMap<TabId, Instant>,
    /// Tab floated because the window lost focus; it returns when the window gets focus back.
    pub(crate) pip_on_blur: Option<TabId>,
    /// When a focus loss of the window becomes a real blur, unless focus came back before.
    pub(crate) blur_due: Option<Instant>,
    /// Left edge of the page card inside the sidebar page, as last sent to its shadow.
    pub(crate) card_left: std::cell::Cell<f64>,
    pub(crate) bar_left: std::cell::Cell<f64>,
    /// What the top bar was last told about notices (`sync_bar_notice`).
    pub(crate) bar_notice: std::cell::Cell<(bool, usize)>,
    pub(crate) sampler: stats::Sampler,
    pub(crate) usage: Option<stats::Usage>,
    pub(crate) stats_tabs: usize,
    pub(crate) peers_due: Instant,
    pub(crate) peers_published: Instant,
    /// Tabs whose page is loading (drives the progress bar), with the time they started.
    pub(crate) loading: HashMap<TabId, Instant>,
    /// New tabs still waiting for their first page: the loading screen covers them.
    pub(crate) splash: HashMap<TabId, Instant>,
    /// Tabs opened but not yet answering at all (no commit), with the URL asked for.
    pub(crate) awaiting: HashMap<TabId, (Instant, Url)>,
    /// The address typed for a tab that already shows a page, until that page starts, fails or
    /// turns into a download: the bar shows it at once, with the progress line. A slow server
    /// (a device of the local network) otherwise left the old address for many seconds, as if
    /// nothing had been asked.
    pub(crate) pending: HashMap<TabId, (Instant, Url)>,
    /// Error pages of the tabs and what led to them (`errors.rs`).
    pub(crate) errors: errors::Errors,
    /// HTTP authentication prompts waiting for the user, their engine answers, and the one shown.
    pub(crate) auth: http_auth::AuthPrompts,
    pub(crate) auth_replies: HashMap<u64, wry::HttpAuthResponder>,
    pub(crate) auth_shown: Option<u64>,
    /// The exact origin of each prompt (read from the engine), for the logins saved for it.
    pub(crate) auth_origins: HashMap<u64, String>,
    /// Cards of the sidebar that stay until answered (`bridges.rs`, `domain::signals`).
    pub(crate) signals: blackhole_domain::signals::Signals,
    /// The last tab received from another device: from whom, its address, when (start page).
    pub(crate) received: Option<(String, String, i64)>,
    #[cfg(target_os = "macos")]
    pub(crate) cards_rounded: Instant,
    pub(crate) media: Arc<Mutex<HashMap<u64, MediaProbe>>>,
    pub(crate) media_due: Instant,
    /// When each tab last said its media may have changed (see `PageEvent::Media`).
    pub(crate) media_hint: HashMap<u64, Instant>,
    /// When the engine process of a tab last died, to reload it once and not in a loop.
    pub(crate) crashed: HashMap<TabId, Instant>,
    pub(crate) media_seen: serde_json::Value,
    /// Last report with media per tab: a short gap in the probes keeps the card on screen.
    pub(crate) media_hold: media::Linger<MediaProbe>,
    /// The next address typed in the command bar opens in a black hole tab.
    pub(crate) burner_next: bool,
    /// Pilot split: the tab whose links open in the other pane (session only).
    pub(crate) pilot: Option<TabId>,
    /// The panes of the split view on their way into place (`pane_motion`).
    pub(crate) panes: pane_motion::PaneMotion,
    /// The two tabs of the split on screen last time (lower id first): a new pair slides in.
    pub(crate) split_seen: Option<(TabId, TabId)>,
    /// Splits opened and pilot links sent with motion, with the side the last link went to:
    /// the split bar plays its part when a number changes.
    pub(crate) split_opened: u32,
    pub(crate) pilot_sent: (u32, pane_motion::Side),
    /// Links cleaned on the way this session (the privacy report's count).
    pub(crate) links_cleaned: u32,
    /// The space or folder deleted last, for the toast's Undo.
    pub(crate) undo: Option<(Instant, Removed)>,
    /// What the Library or an Easel deleted last, for its toast's Undo (`trash.rs`).
    pub(crate) trashed: Option<(Instant, trash::Trashed)>,
    /// Library section asked for by the command bar ("Open History"), with a request number.
    pub(crate) library_jump: (u32, &'static str),
    /// Whether the Settings and Easel windows are maximized, last time their page was told.
    pub(crate) panels_maxed: (bool, bool),
    /// When the last frame of a motion was drawn (sidebar, panes: pacing).
    pub(crate) last_frame: Instant,
    /// A live capture area waiting for the user to choose its Easel.
    pub(crate) live_pending: Option<(TabId, String, (f64, f64, f64))>,
    /// The tab about to be opened from a page: its address and the `Referer` of its first load.
    pub(crate) referrer_next: Option<(Url, String)>,
    /// The picker was armed to watch the area (page menu), not to ask where it goes.
    pub(crate) pick_watches: bool,
    /// Google Calendar sync and the meeting card (see meet.rs).
    pub(crate) calendar: meet::Calendar,
    /// Paired devices and "Continue on…" (see handoff.rs).
    pub(crate) nearby: handoff::Nearby,
    /// Watched areas of pages (see watching.rs).
    pub(crate) watching: watching::Watching,
    /// Recall: the pages read, searchable by what they said (see recall.rs).
    pub(crate) recall: recall::Recall,
    /// Trackers learned while browsing (see trackers.rs).
    pub(crate) trackers: trackers::Learning,
    /// The command bar's ranking: candidates of the open palette, choices learned.
    pub(crate) choices: suggesting::Choices,
    /// How the address of the command running now was asked for (typed, quick access): the
    /// visit of the page it opens counts as such (`BrowserService::expect_visit`).
    pub(crate) next_visit: Option<blackhole_domain::VisitKind>,
    /// Highlights and notes on pages (see highlights.rs).
    pub(crate) marks: highlights::Marks,
    /// The tab comparator (see compare.rs).
    pub(crate) comparing: compare::Comparing,
    /// The last tidy-up, for its Undo; whether the suggestion was dismissed this session.
    pub(crate) tidied: Option<blackhole_domain::Tidied>,
    pub(crate) tidy_dismissed: bool,
    /// Tabs whose media card was closed, and when; they come back when the tab plays again.
    pub(crate) media_dismissed: HashMap<u64, Instant>,
    /// Tabs whose floating video the shell itself just brought back (their "left PiP" is no
    /// request from the user).
    pub(crate) pip_shell_exits: HashMap<u64, Instant>,
    pub(crate) svc: BrowserService<JsonWorkspaceRepository>,
    pub(crate) mailbox: Mailbox,
    pub(crate) sidebar: Sidebar,
    pub(crate) web_context: WebContext,
    pub(crate) blocked: BlockLog,
    /// Section requested for the settings window; `settings_seq` changes with each request.
    pub(crate) settings_tab: Option<String>,
    pub(crate) bookmarks: BookmarkStore,
    pub(crate) bookmark_list: Vec<import::Bookmark>,
    pub(crate) import_sources: Vec<import::Source>,
    pub(crate) import_msg: String,
    pub(crate) app_ext: Vec<blackhole_macos_webext::AppExtension>,
    pub(crate) bar_exts: serde_json::Value,
    pub(crate) bar_exts_at: Instant,
    pub(crate) settings_seq: u32,
    pub(crate) home_visible: bool,
    /// "What's new" shown on the start page until dismissed; empty when there is nothing to show.
    pub(crate) release_notes: Vec<serde_json::Value>,
    /// Command bar overlay; rebuilt whenever a view is added so it stays on top.
    pub(crate) splitbar: LazyView,
    pub(crate) palette: Option<WebView>,
    /// Context menu being shown by the overlay (position and items).
    pub(crate) menu: Option<serde_json::Value>,
    pub(crate) palette_ready: bool,
    pub(crate) palette_open: bool,
    pub(crate) palette_mode: String,
    /// Space or folder whose emoji the picker is editing.
    pub(crate) icon_target: Option<(String, u64)>,
    pub(crate) peek: Option<Peek>,
    /// Sites already told that their passkey request was refused (macOS, `domain::passkeys`).
    pub(crate) passkey_refusals: blackhole_domain::passkeys::Refusals,
    #[cfg(windows)]
    pub(crate) ext_popup: Option<ExtPopup>,
    /// Left edge and width of each extension's button in the command bar (`ExtAnchors`).
    #[cfg(windows)]
    pub(crate) ext_anchors: HashMap<String, (f64, f64)>,
    pub(crate) data: std::path::PathBuf,
    /// Where installed extensions are read from: the profile itself, or, in a private window
    /// that allows extensions, the main profile.
    pub(crate) ext_src: std::path::PathBuf,
    pub(crate) favicons: favicons::Favicons,
    /// The icon table is only sent to the sidebar / start page when it changed.
    pub(crate) icons_for_ui: std::cell::Cell<bool>,
    pub(crate) icons_for_home: std::cell::Cell<bool>,
    /// Page events only mark the UI stale; it is refreshed once per event-loop turn.
    pub(crate) ui_stale: bool,
    pub(crate) last_flush: Instant,
    /// Native WebExtension host (macOS 15.4+); `None` where unsupported.
    pub(crate) webext: Option<blackhole_macos_webext::ExtensionHost>,
    pub(crate) installing: std::collections::HashSet<String>,
    /// Private window: drops the welcome pages its freshly installed extensions open
    /// (`domain::welcome`); `ext_loaded` counts the loaded extensions to notice a new one.
    pub(crate) welcome: blackhole_domain::welcome::WelcomePages,
    /// The launch splash plays on the start page: only for the first window, never for a new
    /// window opened beside others (`is_new_window`).
    pub(crate) intro: bool,
    /// Welcome tour over the start page (`domain::onboarding`): at the first launch of the
    /// profile, or replayed from Settings. `None` once closed.
    pub(crate) onboarding: Option<blackhole_domain::onboarding::Mode>,
    /// The page shows the tour's dialog: the sidebar and the top bar are dimmed under it while
    /// the start page is in front (`sync_dim`). `dimmed` is what they show now.
    pub(crate) onboarding_open: bool,
    pub(crate) dimmed: std::cell::Cell<bool>,
    /// Imports finished while the tour was open: the page tells its own from the ones before.
    pub(crate) onboarding_imports: u32,
    /// The tour asked the system to make BlackHole the default browser: the choice is read
    /// again when the window gets the focus back (from the system's dialog or Settings app).
    pub(crate) default_asked: bool,
    pub(crate) ext_loaded: usize,
    /// Extensions offered: the signed market list kept on this device, else the build's own.
    pub(crate) market: market::Index,
    pub(crate) market_tx: std::sync::mpsc::Sender<Result<market::Index, String>>,
    pub(crate) market_rx: std::sync::mpsc::Receiver<Result<market::Index, String>>,
    pub(crate) next_market_check: Option<Instant>,
    /// A market download is running; `Some(true)` when "Check for updates" waits for it.
    pub(crate) market_fetching: Option<bool>,
    /// Why the last market download failed (the list in use stays).
    pub(crate) market_error: Option<String>,
    /// Extensions added from GitHub (unreviewed, pinned) and the add-from-GitHub steps.
    pub(crate) github: github_ext::Sideload,
    /// Outdated extensions found by the last "Check for updates"; `None` until it is pressed.
    pub(crate) catalog_checked: Option<usize>,
    pub(crate) update: UpdateState,
    pub(crate) update_tx: std::sync::mpsc::Sender<UpdateMsg>,
    pub(crate) update_rx: std::sync::mpsc::Receiver<UpdateMsg>,
    pub(crate) next_update_check: Option<Instant>,
    /// A release check is running (background or Settings > Updates).
    pub(crate) update_checking: bool,
    /// What the last finished check said: up to date, or why it failed (Settings > Updates).
    pub(crate) update_checked: Option<Result<(), String>>,
    pub(crate) quit: bool,
    pub(crate) maximized: bool,
    pub(crate) install_errors: HashMap<String, String>,
    pub(crate) install_tx: std::sync::mpsc::Sender<(String, Result<std::path::PathBuf, String>)>,
    pub(crate) install_rx: std::sync::mpsc::Receiver<(String, Result<std::path::PathBuf, String>)>,
    pub(crate) import_tx: std::sync::mpsc::Sender<ImportDone>,
    pub(crate) import_rx: std::sync::mpsc::Receiver<ImportDone>,
    pub(crate) webext_sig: String,
    /// The Settings, shown in the page card like a tab (`settings_shown`), with the start
    /// page's background; a shell page created when needed and released after `PANEL_KEEP`.
    pub(crate) settings_panel: LazyView,
    pub(crate) settings_shown: bool,
    pub(crate) settings_wanted: bool,
    pub(crate) easels: Easels,
    pub(crate) easel_store: EaselStore,
    pub(crate) easel_current: Option<u64>,
    /// Live views of the current easel's playing cards (children of the easel window, so they
    /// must be dropped before it: declared first).
    pub(crate) live: HashMap<u64, WebView>,
    pub(crate) live_slots: HashMap<u64, ipc::LiveSlot>,
    /// The live views are children of the main window (Easel shown as a tab or beside one),
    /// not of the Easel window: leaving that Easel must drop them, or they stay over the page.
    pub(crate) live_embedded: bool,
    /// Tab whose area picker is armed.
    pub(crate) live_pick: Option<TabId>,
    /// The Easel page shown in the content area, in place of a web page, while `easel_tab`.
    pub(crate) easel_panel: LazyView,
    pub(crate) easel_tab: bool,
    /// An Easel shown in the right pane beside a tab (a meeting's notes beside its Meet call),
    /// as `(tab, easel)`: on screen while that tab is the one shown and that easel the current.
    pub(crate) easel_beside: Option<(TabId, u64)>,
    /// The Library page, over the whole window below the bar, while `library`.
    pub(crate) library_panel: LazyView,
    pub(crate) library: bool,
    pub(crate) downloads: Vec<blackhole_infrastructure::store::DownloadRec>,
    pub(crate) dl_live: HashMap<u64, DownloadLive>,
    /// Tabs opened for a file alone (no page shown), by download URL: closed once it is done
    /// (closing the view earlier would cancel the download).
    pub(crate) dl_only_tabs: HashMap<String, TabId>,
    /// Tabs whose navigation just became a download: the engine ends that navigation as a
    /// failure, which is no error to show.
    pub(crate) dl_navs: HashMap<TabId, Instant>,
    /// Failed navigations waiting `ABORT_GRACE` (an error page unless a download, a new load
    /// or a page follows), with the engine's address when they failed (`errors::settle_failure`).
    pub(crate) aborted: HashMap<TabId, (Instant, Url, Option<String>)>,
    pub(crate) dl_polled: Instant,
    pub(crate) dl_size_tx: std::sync::mpsc::Sender<(u64, u64)>,
    pub(crate) dl_size_rx: std::sync::mpsc::Receiver<(u64, u64)>,
    pub(crate) archive: Vec<blackhole_infrastructure::store::ArchivedTab>,
    pub(crate) thumbs: std::cell::RefCell<HashMap<u64, Option<String>>>,
    pub(crate) easel: Option<EaselWindow>,
    pub(crate) easel_wanted: bool,
    /// Settings / Easel windows just closed: hidden at once, dropped on the next pass of the
    /// loop. Dropping a winit window while AppKit still treats it as the key window crashes on
    /// the next change of key window (macOS, `resignKeyWindow` on a freed delegate).
    pub(crate) closed_windows: Vec<Window>,
    pub(crate) capture_tx: std::sync::mpsc::Sender<Capture>,
    pub(crate) capture_rx: std::sync::mpsc::Receiver<Capture>,
    #[cfg(target_os = "macos")]
    pub(crate) _menu: muda::Menu,
    #[cfg(feature = "devtools")]
    pub(crate) dev: Option<devtools::Dev>,
    #[cfg(windows)]
    pub(crate) backdrop: Option<backdrop::Backdrop>,
    // Declared last: web views are children of the window and must be dropped first.
    pub(crate) window: Rc<Window>,
}

impl Running {
    /// The start page is shown at launch, via the Home button, and whenever no tab is active.
    pub(crate) fn home_shown(&self) -> bool {
        self.library
            || self.settings_shown
            || self.easel_tab
            || self.home_visible
            || self.svc.workspace().active_tab().is_none()
    }

    pub(crate) fn active_view(&self) -> &WebView {
        if self.settings_shown {
            return self.settings_panel.view.as_ref().unwrap_or(&self.home);
        }
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

    /// The tab shown beside the active one, if the split view is valid right now.
    pub(crate) fn split_tab(&self) -> Option<TabId> {
        if self.home_shown() || self.easel_side() {
            return None;
        }
        self.svc.workspace().split_tab()
    }

    pub(crate) fn view_of(&self, tab: TabId) -> Option<&WebView> {
        if tab == PEEK_TAB {
            return self.peek.as_ref().map(|p| &p.page);
        }
        self.tabs.get(&tab)
    }

    pub(crate) fn ui(&self, js: String) {
        let _ = self.chrome.evaluate_script(&js);
        if let Some(flyout) = &self.flyout {
            let _ = flyout.evaluate_script(&format!("window.bh&&(bh.role='flyout');{js}"));
        }
    }

    pub(crate) fn active_site(&self) -> Option<String> {
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

    pub(crate) fn with_active_tab(&self, f: impl FnOnce(&WebView)) {
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
}

//! The state of a window of the app (`Running`) and its small accessors.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use blackhole_application::BrowserService;
use blackhole_domain::{BlockLog, Easels, Removed, TabId};
use blackhole_infrastructure::JsonWorkspaceRepository;
use blackhole_infrastructure::store::EaselStore;
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
use crate::features::imports::Imports;
use crate::features::market::Market;
use crate::features::updates::Updates;
use crate::ipc::Mailbox;
use crate::shell::pane_motion;
use crate::shell::home::Onboarding;
use crate::shell::palette::Palette;
use crate::shell::panels::LazyView;
use crate::shell::peek::{Peek, PEEK_TAB};
use crate::shell::sidebar::Sidebar;
use crate::tabs::{errors, favicons};
use crate::tabs::auth::Auth;
use crate::tabs::downloads::Downloads;
use crate::tabs::media::MediaWatch;
use crate::tabs::pip::Pip;

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
    /// Videos the shell floated in Picture in Picture, and why.
    pub(crate) pip: Pip,
    /// What the tabs play: the probes' reports, the sidebar's media card, the tabs kept busy.
    pub(crate) media: MediaWatch,
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
    pub(crate) auth: Auth,
    /// Cards of the sidebar that stay until answered (`bridges.rs`, `domain::signals`).
    pub(crate) signals: blackhole_domain::signals::Signals,
    /// The last tab received from another device: from whom, its address, when (start page).
    pub(crate) received: Option<(String, String, i64)>,
    #[cfg(target_os = "macos")]
    pub(crate) cards_rounded: Instant,
    /// When the engine process of a tab last died, to reload it once and not in a loop.
    pub(crate) crashed: HashMap<TabId, Instant>,
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
    pub(crate) svc: BrowserService<JsonWorkspaceRepository>,
    pub(crate) mailbox: Mailbox,
    pub(crate) sidebar: Sidebar,
    pub(crate) web_context: WebContext,
    pub(crate) blocked: BlockLog,
    /// Section requested for the settings window; `settings_seq` changes with each request.
    pub(crate) settings_tab: Option<String>,
    /// Bookmarks kept by the profile, the browsers found to import from, and imports running.
    pub(crate) imports: Imports,
    pub(crate) app_ext: Vec<blackhole_macos_webext::AppExtension>,
    pub(crate) bar_exts: serde_json::Value,
    pub(crate) bar_exts_at: Instant,
    pub(crate) settings_seq: u32,
    pub(crate) home_visible: bool,
    /// "What's new" shown on the start page until dismissed; empty when there is nothing to show.
    pub(crate) release_notes: Vec<serde_json::Value>,
    /// The strip above a split view (each pane's title and close button).
    pub(crate) splitbar: LazyView,
    /// The command bar overlay and what it shows: its mode, a context menu, the emoji picker.
    pub(crate) palette: Palette,
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
    /// The welcome tour over the start page (`domain::onboarding`).
    pub(crate) onboarding: Onboarding,
    pub(crate) ext_loaded: usize,
    /// The extension market: the list in use, its background refresh, the last check.
    pub(crate) market: Market,
    /// Extensions added from GitHub (unreviewed, pinned) and the add-from-GitHub steps.
    pub(crate) github: github_ext::Sideload,
    /// Self-update of the app: where it stands, the background check and its answers.
    pub(crate) updates: Updates,
    pub(crate) quit: bool,
    pub(crate) maximized: bool,
    pub(crate) install_errors: HashMap<String, String>,
    pub(crate) install_tx: std::sync::mpsc::Sender<(String, Result<std::path::PathBuf, String>)>,
    pub(crate) install_rx: std::sync::mpsc::Receiver<(String, Result<std::path::PathBuf, String>)>,
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
    /// Downloads: their record, the cards of the running ones, and the tabs they came from.
    pub(crate) downloads: Downloads,
    /// Failed navigations waiting `ABORT_GRACE` (an error page unless a download, a new load
    /// or a page follows), with the engine's address when they failed (`errors::settle_failure`).
    pub(crate) aborted: HashMap<TabId, (Instant, Url, Option<String>)>,
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

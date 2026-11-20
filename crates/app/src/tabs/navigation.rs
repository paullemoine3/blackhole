//! Opening, loading, switching and closing tabs.

use std::time::{Duration, Instant};

use blackhole_application::NavigationOutcome;
use blackhole_domain::TabId;
use url::Url;

use crate::views;
use crate::i18n::t;
use crate::ipc::UiCommand;
use crate::messages::ABORT_GRACE;
use crate::shell::peek::PEEK_TAB;
use crate::state::Running;
use crate::tabs::lifecycle::MAX_BURNERS;

/// A new tab is covered by the loading screen until its page loads, at most this long.
const SPLASH_MAX: Duration = Duration::from_secs(5);

/// A page that never reports its end (error, cancelled) stops showing progress after this.
const LOADING_MAX: Duration = Duration::from_secs(30);

/// A new tab whose server never starts answering is reported unreachable after this long.
const CONNECT_MAX: Duration = Duration::from_secs(20);

/// How long a hidden sidebar shows itself when a link opens a tab in the background (middle
/// click): without it nothing on screen says the tab is there.
const LINK_REVEAL: Duration = Duration::from_millis(2500);

impl Running {
    /// Middle click / Ctrl+click on a link, or "Open link in new tab": the tab opens at the top
    /// of the list and starts loading, while the page shown stays on screen.
    pub(crate) fn open_in_background(&mut self, raw: &str) {
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

    fn show_unreachable(&mut self, tab: TabId, url: Url) {
        self.show_error(tab, url, blackhole_domain::error_page::ErrorPage::Unreachable);
    }

    /// Ends the loading screen of a tab, and the progress of a page that went quiet.
    pub(crate) fn service_loading(&mut self) -> bool {
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
            .filter(|(_, (since, _, _))| now.duration_since(*since) >= ABORT_GRACE)
            .map(|(id, _)| *id)
            .collect();
        for id in late {
            if let Some((_, url, engine_at)) = self.aborted.remove(&id) {
                self.settle_failure(id, url, engine_at);
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
                // No answer at all yet: asked once more (a connection that died silently)
                // before the page says the site cannot be reached.
                if self.retry_quietly(id, &url) {
                    self.awaiting.insert(id, (Instant::now(), url));
                } else {
                    self.show_unreachable(id, url);
                }
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

    /// A black hole tab on `input`: a tab with a session of its own (no cookie of the space,
    /// nothing kept), destroyed when it closes.
    pub(crate) fn open_burner(&mut self, input: &str) {
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
                self.settings_shown = false;
                self.sync();
            }
            None => self.toast(&t("blocked.unsupported_scheme")),
        }
    }

    pub(crate) fn open(&mut self, input: &str) {
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
                if let (Some(tab), Some(kind)) = (tab, self.next_visit) {
                    self.svc.expect_visit(tab, kind);
                }
                if let (Some(tab), "http", "https") = (tab, typed.scheme(), url.scheme()) {
                    self.on_upgraded(tab, typed);
                }
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.settings_shown = false;
                self.sync();
            }
        }
    }

    pub(crate) fn navigate(&mut self, input: &str) {
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
                if let Some(kind) = self.next_visit {
                    self.svc.expect_visit(id, kind);
                }
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

    pub(crate) fn close_active_tab(&mut self) {
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

    /// ⌘⇧T: brings back the tab closed last, in the space it was closed from.
    pub(crate) fn reopen_closed(&mut self) {
        match self.archive.len().checked_sub(1) {
            Some(index) => self.handle_easel(UiCommand::ReopenArchived { index }),
            None => self.toast(&t("toast.nothing_to_reopen")),
        }
    }

    /// Ctrl+Tab and ⌘⌥↑/↓ (Ctrl+Alt on Windows): show another tab of the space.
    pub(crate) fn switch_tab(&mut self, key: &str) {
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

    /// A new tab shows the start page behind the search bar, which opens straight away.
    pub(crate) fn new_tab(&mut self) {
        self.close_palette();
        self.close_peek();
        self.home_visible = true;
        self.easel_tab = false;
        self.library = false;
        self.settings_shown = false;
        self.sync();
        self.open_palette("new");
    }

    // ---- command bar --------------------------------------------------------------------

    /// Re-reads the address from the engine; page scripts cannot dictate what the bar shows.
    pub(crate) fn refresh_url(&mut self, tab: TabId) {
        let current = self.tabs.get(&tab).and_then(|v| v.url().ok());
        if let Some(url) = current.and_then(|raw| Url::parse(&raw).ok()) {
            self.page_url(tab, url);
        }
    }

    pub(crate) fn page_url(&mut self, tab: TabId, url: Url) {
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
}

impl Running {
    /// The loading line at the top of the page (`ui/progress.js`): `tint` when the engine says
    /// the page started, `done` at the end. Before the page starts (a typed address waiting for
    /// its server) the top bar draws the line (`waiting`).
    pub(crate) fn page_progress(&self, tab: TabId, step: &str) {
        let Some(view) = self.tabs.get(&tab) else {
            return;
        };
        let call = match step {
            "tint" => {
                let color = serde_json::to_string(&self.svc.workspace().active_space().color).unwrap_or_default();
                format!("{step}({color},{})", self.svc.settings().animations)
            }
            _ => "done()".to_owned(),
        };
        let _ = view.evaluate_script(&format!("window.__bhProgress&&__bhProgress.{call}"));
    }
}

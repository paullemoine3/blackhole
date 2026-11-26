//! Page views of the tabs: creation, spares, sleep, wake, crashes, and showing the right one.

use std::time::{Duration, Instant};

use blackhole_domain::{SpaceId, TabId};
use url::Url;

use crate::views;
use crate::i18n::t;
use crate::shell::windows::private_mode;
use crate::state::Running;
use crate::tabs::media::BUSY_GRACE;

/// A page whose engine process dies again this soon after a reload is not reloaded again.
const CRASH_LOOP: Duration = Duration::from_secs(30);

/// Spare page views kept ready, and how long the app must stay quiet before building them.
pub(crate) const SPARES_DELAY: Duration = Duration::from_millis(900);

/// Anchored tabs wake in the background this long after launch, once the shown page started.
pub(crate) const ANCHORS_DELAY: Duration = Duration::from_secs(3);

/// Black hole tabs open at once: each keeps its page alive (never put to sleep) until closed.
pub(crate) const MAX_BURNERS: usize = 4;

impl Running {
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
    pub(crate) fn opened_from(&mut self, opener: TabId, target: &Url) {
        self.referrer_next = self
            .view_of(opener)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok())
            .and_then(|page| blackhole_domain::popup::referrer(&page, target))
            .map(|referrer| (target.clone(), referrer));
    }

    pub(crate) fn ensure_view(&mut self, tab: &blackhole_domain::Tab) {
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
    pub(crate) fn store_of_space(&self, space: SpaceId) -> views::Store {
        if private_mode() {
            views::Store::Private
        } else if space == SpaceId(1) {
            views::Store::Main
        } else {
            views::Store::Space(space.0)
        }
    }

    /// The id of the space `tab` belongs to (the space shown for an unknown tab).
    pub(crate) fn space_of_tab(&self, tab: TabId) -> u64 {
        let ws = self.svc.workspace();
        ws.spaces()
            .iter()
            .find(|s| s.tabs.iter().any(|t| t.id == tab))
            .map_or(ws.active_space().id.0, |s| s.id.0)
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
    pub(crate) fn trim_spares(&mut self) {
        if !self.spares.is_empty() {
            self.spares.clear();
            self.raise_palette();
        }
    }

    /// Builds the spare views while nothing is going on. They all sit below the command bar,
    /// which is recreated once on top of them, so the next tabs open without any view creation.
    pub(crate) fn refill_spares(&mut self) {
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
        if self.palette.open || self.peek.is_some() || self.spares.iter().any(|(s, ..)| *s == store)
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

    /// Wakes the anchored tabs once, shortly after launch, behind the page shown: their mail or
    /// chat is live without opening them. One the user puts to sleep afterwards stays asleep
    /// until it is shown again.
    pub(crate) fn wake_anchored(&mut self) {
        if !self.anchors_due.is_some_and(|due| Instant::now() >= due) {
            return;
        }
        self.anchors_due = None;
        let ws = self.svc.workspace();
        let tabs: Vec<blackhole_domain::Tab> = ws
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .filter(|t| t.anchored && !self.tabs.contains_key(&t.id))
            .filter(|t| matches!(t.url.scheme(), "http" | "https"))
            .cloned()
            .collect();
        if tabs.is_empty() {
            return;
        }
        let shown: Vec<TabId> = ws.active_tab().map(|t| t.id).into_iter().chain(self.split_tab()).collect();
        for tab in tabs {
            self.ensure_view(&tab);
            if !shown.contains(&tab.id) {
                if let Some(view) = self.tabs.get(&tab.id) {
                    let _ = view.set_visible(false);
                    views::set_background(view, true);
                }
                // The loading cover is only for a tab on screen.
                self.splash.remove(&tab.id);
            }
            self.last_active.insert(tab.id, Instant::now());
        }
        self.ui_state();
    }

    /// Frees the engine view of tabs left in the background; they reload when selected again.
    pub(crate) fn discard_idle_tabs(&mut self) {
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
        let now_busy: Vec<TabId> = self.media.probes.lock().map_or_else(
            |_| Vec::new(),
            |probes| {
                self.tabs
                    .keys()
                    .filter(|id| {
                        probes
                            .get(&id.0)
                            .is_some_and(|p| p.keeps_page())
                    })
                    .copied()
                    .collect()
            },
        );
        for id in now_busy {
            self.media.busy_seen.insert(id, now);
        }
        // Releasing the page of a black hole tab would end its session: it stays alive until
        // the tab is closed (their number is capped, see `open_burner`). An anchored tab stays
        // alive by choice.
        let ws = self.svc.workspace();
        let anchored = ws.anchored_tabs();
        let holes: Vec<TabId> = self
            .tabs
            .keys()
            .filter(|id| ws.is_burner(**id) || anchored.contains(id))
            .copied()
            .collect();
        // Closing the page ends its downloads, paused ones included.
        for id in self.downloads.tabs_at_work() {
            self.media.busy_seen.insert(id, now);
        }
        for id in holes {
            self.media.busy_seen.insert(id, now);
        }
        self.media.busy_seen
            .retain(|id, seen| self.tabs.contains_key(id) && now.duration_since(*seen) < BUSY_GRACE);
        let busy: Vec<TabId> = self.media.busy_seen.keys().copied().collect();
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
        // Cap on live views, independent of the delay: each one is a renderer process. It holds
        // per space, so a space full of tabs never puts another one's to sleep (black hole tabs
        // count together). Tabs never shown sort first, then the least recently used.
        let spaces = self.svc.workspace().spaces();
        let live: Vec<(TabId, Option<blackhole_domain::SpaceId>, Option<Instant>)> = self
            .tabs
            .keys()
            .filter(|id| !idle.contains(id))
            .map(|id| {
                let space = spaces.iter().find(|s| s.tabs.iter().any(|t| t.id == *id)).map(|s| s.id);
                (*id, space, self.last_active.get(id).copied())
            })
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
        // Anchored tabs stay alive outside the cap: with three of them, a cap of four still keeps
        // four other tabs alive in the space (the counter of the sidebar still counts them all).
        let anchored = self.svc.workspace().anchored_tabs();
        idle.extend(blackhole_domain::overflow_per_group(
            &live,
            &visible,
            &anchored,
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

    pub(crate) fn sync(&mut self) {
        self.pilot_sync();
        self.drop_stray_live();
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
                let floating = self.pip.seen
                    && self
                        .pip.tab
                        .is_some_and(|t| t != id && self.tabs.contains_key(&t));
                if !floating || self.is_playing(id) {
                    self.pip.tab = Some(id);
                    self.pip.seen = false;
                }
            }
            if let Some(back) = active.and_then(|id| self.tabs.get(&id)) {
                let _ = back.evaluate_script(views::PIP_EXIT_JS);
                if let Some(id) = active {
                    self.pip.shell_exits.insert(id.0, Instant::now());
                }
                if self.pip.tab == active {
                    self.pip.tab = None;
                    self.pip.seen = false;
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
        if let Some(panel) = &self.settings_panel.view {
            let _ = panel.set_visible(self.settings_shown);
        }
        self.place_panels();
        let split = self.split_tab();
        let waiting = active.filter(|id| self.splash.contains_key(id));
        for (id, view) in &self.tabs {
            let shown = (Some(*id) == active && waiting.is_none()) || Some(*id) == split;
            let _ = view.set_visible(shown);
            // A call or media left for another tab keeps its memory: trimmed, its sound
            // crackled and its video froze (WebView2's "low" target).
            views::set_background(view, !shown && !self.keeps_running(*id));
        }
        let _ = self
            .home
            .set_visible((active.is_none() || waiting.is_some()) && !self.easel_tab && !self.library && !self.settings_shown);
        self.ui_wait();
        self.split_shown(active, split);
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

    pub(crate) fn apply_focus(&self) {
        let _ = if let (true, Some(palette)) = (self.palette.open, &self.palette.view) {
            palette.focus()
        } else if let Some(peek) = &self.peek {
            peek.page.focus()
        } else {
            self.active_view().focus()
        };
    }

    pub(crate) fn focus_content(&mut self) {
        self.close_palette();
        self.apply_focus();
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
        self.refused.clear(id.0);
        // Like Arc: a quick access or folder tab that wandered wakes up at its own address.
        self.svc.go_home(id);
        self.marks_push(id, false);
        self.last_active.remove(&id);
        if let Ok(mut probes) = self.media.probes.lock() {
            probes.remove(&id.0);
        }
        self.media.hold.forget(id.0);
        self.media.hints.remove(&id.0);
        self.crashed.remove(&id);
        if self.pip.tab == Some(id) {
            self.pip.tab = None;
            self.pip.seen = false;
        }
        if self.pip.on_blur == Some(id) {
            self.pip.on_blur = None;
        }
    }

    /// The engine process of a page died. A tab on screen is reloaded once (a second death
    /// within `CRASH_LOOP` leaves it alone, with a word to the user); a hidden one gives its
    /// view back and loads again when it is shown.
    pub(crate) fn on_crashed(&mut self, tab: TabId) {
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
            self.notify(crate::shell::notices::Notice::Problem, &t("toast.page_crashed"), serde_json::Value::Null);
            self.ui_bar();
        } else if let Some(view) = self.tabs.get(&tab) {
            let _ = view.reload();
        }
    }
}

//! State pushed to the sidebar, the top bar and the split bar.

use blackhole_domain::{SearchEngine, Settings, verbs};
use blackhole_domain::verbs::{Target, Verb};
use serde_json::json;
use url::Url;
use wry::WebView;

use crate::{i18n, views};
use crate::boot::main_title;
use crate::shell::windows::{private_mode, window_tasks};
use crate::state::Running;
use crate::tabs::errors;

#[cfg(target_os = "macos")]
use crate::boot::install_menu;

pub(crate) const SPACE_COLORS: [&str; 5] = ["#7c5cff", "#ff5c8a", "#2fd4a0", "#ffb020", "#3aa0ff"];

/// A site needs this many loose tabs to be suggested as a folder; the sidebar only suggests a
/// tidy-up from `TIDY_FROM` loose tabs.
pub(crate) const TIDY_MIN: usize = 3;

const TIDY_FROM: usize = 6;

impl Running {
    pub(crate) fn ui_state(&self) {
        self.sync_bar_notice();
        let ws = self.svc.workspace();
        let space = ws.active_space();
        let mut state = json!({
            "os": std::env::consts::OS,
            "private": private_mode(),
            "sidebarWidth": self.sidebar.full(),
            "sidebarOpen": self.sidebar.is_open(),
            "tileSize": self.svc.settings().quick_access_size,
            "engine": self.svc.settings().search_engine.id(),
            "animations": self.svc.settings().animations,
            "depth": self.svc.settings().depth,
            "tabSleep": self.svc.settings().tab_sleep_minutes,
            "tabSleepChoices": Settings::SLEEP_CHOICES,
            "engines": SearchEngine::ALL.iter().map(|e| json!({"id": e.id(), "label": e.label()})).collect::<Vec<_>>(),
            "spaces": ws.spaces().iter().map(|s| json!({"id": s.id.0, "name": s.name, "color": s.color, "icon": s.icon})).collect::<Vec<_>>(),
            "activeSpace": space.id.0,
            "collapsed": self.svc.settings().collapsed_sections,
            "color": space.color,
            "activeTab": if self.home_shown() { None } else { space.active_tab.map(|t| t.0) },
            "home": self.home_shown(),
            // What the tabs of the space shown blocked: each space has its own counter.
            "blocked": self.blocked.total_in(space.id.0),
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
                "anchored": t.anchored,
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
        // Apart: the macro above is at its recursion limit.
        for (tab, t) in state["tabs"].as_array_mut().into_iter().flatten().zip(&space.tabs) {
            tab["pulsar"] = json!(t.pulsar);
        }
        state["pulsars"] = self.pulsars_json();
        state["pulsarsOn"] = json!(self.pulsars_on());
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
        self.sync_dim();
    }

    /// Titles and visibility of the strip above the split panes.
    pub(crate) fn ui_splitbar(&self) {
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
            let state = json!({
                "left": pane(left), "right": right, "color": ws.active_space().color,
                "animations": self.svc.settings().animations,
                "depth": self.svc.settings().depth,
                "opened": self.split_opened,
                "sent": {"n": self.pilot_sent.0, "side": self.pilot_sent.1.id()},
            });
            let _ = splitbar.evaluate_script(&format!("window.bh&&bh.render({state})"));
        }
    }

    pub(crate) fn ui_bar(&self) {
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
            // A typed address waits for its server: the bar's line until the page starts.
            "serverWait": asked.is_some(),
            "zoom": zoom,
            "sitePermissions": self.site_has_permissions(),
            "canBack": nav(WebView::can_go_back),
            "canForward": nav(WebView::can_go_forward),
            "os": std::env::consts::OS,
            "maximized": self.window.is_maximized(),
            "private": private_mode(),
            "insecure": Url::parse(&url).is_ok_and(|u| errors::insecure(&u)),
            "url": url,
            "place": if self.settings_shown { "settings" } else { "" },
            "color": ws.active_space().color,
            "engine": self.svc.settings().search_engine.label(),
            "extensions": self.bar_exts,
            "animations": self.svc.settings().animations,
            "depth": self.svc.settings().depth,
            "side": self.bar_notice_state().0,
            "waiting": self.bar_notice_state().1,
        });
        self.bar_notice.set(self.bar_notice_state());
        let _ = self
            .bar
            .evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    /// The start page doubles as the loading screen of a new tab until its page is there.
    pub(crate) fn ui_wait(&self) {
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

    pub(crate) fn language_changed(&mut self) {
        i18n::apply(&self.svc.settings().language);
        window_tasks();
        self.close_palette();
        let mut views: Vec<&WebView> = vec![&self.chrome, &self.bar, &self.home];
        views.extend(self.flyout.as_ref());
        views.extend(self.hotzone.as_ref());
        views.extend(self.palette.view.as_ref());
        views.extend(self.peek.as_ref().map(|p| &p.scrim));
        views.extend(self.splitbar.view.as_ref());
        views.extend(self.library_panel.view.as_ref());
        views.extend(self.easel_panel.view.as_ref());
        views.extend(self.settings_panel.view.as_ref());
        views.extend(self.easel.as_ref().map(|w| &w.view));
        for view in views {
            views::send_language(view);
        }
        self.window.set_title(&main_title());
        if let Some(w) = &self.easel {
            w.window.set_title(&i18n::t("window.easel"));
        }
        #[cfg(target_os = "macos")]
        {
            self._menu = install_menu();
        }
        self.ui_state();
    }

    /// Every reload of the page shown (button, keys, menus, palette) turns the reload arrow of the
    /// top bar, not the button: it keeps turning while the page loads (`bh.reloading`).
    pub(crate) fn spin_reload(&self) {
        if !self.home_shown() {
            let _ = self.bar.evaluate_script("window.bh&&bh.reloading&&bh.reloading()");
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
            // The tabs of each group: the card lights them in the list while it is pointed at.
            "groups": groups.iter().map(|g| json!({
                "name": g.name,
                "count": g.tabs.len(),
                "tabs": g.tabs.iter().map(|t| t.0).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

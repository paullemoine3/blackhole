//! The start page and its welcome tour.

use std::time::Instant;

use blackhole_domain::{SearchEngine, suggest};
use blackhole_infrastructure::catalog;
use serde_json::json;

use crate::i18n;
use crate::ipc::UiCommand;
use crate::platform::default_browser_unavailable;
use crate::state::Running;

/// The welcome tour over the start page (`domain::onboarding`).
pub(crate) struct Onboarding {
    /// At the first launch of the profile, or replayed from Settings. `None` once closed.
    pub(crate) mode: Option<blackhole_domain::onboarding::Mode>,
    /// The page shows the tour's dialog: the sidebar and the top bar are dimmed under it while
    /// the start page is in front (`sync_dim`). `dimmed` is what they show now.
    pub(crate) open: bool,
    pub(crate) dimmed: std::cell::Cell<bool>,
    /// Imports finished while the tour was open: the page tells its own from the ones before.
    pub(crate) imports: u32,
    /// The tour asked the system to make BlackHole the default browser: the choice is read
    /// again when the window gets the focus back (from the system's dialog or Settings app).
    pub(crate) default_asked: bool,
}

impl Onboarding {
    pub(crate) fn new(mode: Option<blackhole_domain::onboarding::Mode>) -> Self {
        Self { mode, open: false, dimmed: std::cell::Cell::new(false), imports: 0, default_asked: false }
    }
}

impl Running {
    pub(crate) fn ui_home(&self) {
        // Each space has its own start page (0.8.33): sections, favorites and their order.
        let home = self.svc.home();
        let mut state = json!({
            "engine": self.svc.settings().search_engine.label(),
            "animations": self.svc.settings().animations,
            "intro": self.intro,
            "depth": self.svc.settings().depth,
            "favorites": self.svc.favorites().iter()
                .map(|f| json!({
                    "name": f.name, "url": f.url.as_str(),
                    // What its place is remembered by (`SetFavoriteOrder`).
                    "site": blackhole_domain::site_key(f.url.host_str().unwrap_or_default()),
                    // Only tabs of the current space can be unpinned from here.
                    "tab": f.tab.filter(|id| self.svc.workspace().active_space().tabs.iter().any(|t| t.id == *id)).map(|id| id.0),
                }))
                .collect::<Vec<_>>(),
            "recent": self.home_recent_json(),
            "releaseNotes": self.release_notes,
            "now": self.home_now_json(),
            "hidden": home.hidden,
            // In the user's order (dragged on the page).
            "sections": blackhole_domain::home_order(&home.order),
            "space": self.svc.workspace().active_space().name,
            "watches": self.home_watches_json(),
            "downloads": self.home_downloads_json(),
            "onboarding": self.onboarding_json(),
        });
        if self.icons_for_home.replace(false) {
            state["favicons"] = json!(self.favicons.all());
        }
        let _ = self
            .home
            .evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    /// What the welcome tour needs (`null` when it is closed). The replayed tour shows the
    /// features only; the first launch also offers its choices, from the shell's own state.
    fn onboarding_json(&self) -> serde_json::Value {
        use blackhole_domain::onboarding::{self, Mode};
        let Some(mode) = self.onboarding.mode else {
            return serde_json::Value::Null;
        };
        if mode == Mode::Tour {
            return json!({"mode": mode.id()});
        }
        let settings = self.svc.settings();
        let space = self.svc.workspace().active_space();
        // Only where extensions can run (not Linux, not without an extension host).
        let available: Vec<&str> = match &self.webext {
            Some(_) => self.market.index.for_this_system().map(|e| e.id.as_str()).collect(),
            None => Vec::new(),
        };
        let extensions: Vec<_> = onboarding::starters(available)
            .into_iter()
            .filter_map(|(id, on)| {
                let e = self.market.index.for_this_system().find(|e| e.id == id)?;
                Some(json!({
                    "id": id, "name": e.name, "description": e.description_in(i18n::lang()),
                    "category": e.category, "on": on,
                    "installed": catalog::is_installed(&self.data, id),
                    "installing": self.installing.contains(id),
                    "error": self.install_errors.get(id),
                }))
            })
            .collect();
        json!({
            "mode": mode.id(),
            "os": std::env::consts::OS,
            "sources": self.imports.sources.iter().map(|s| json!({"id": s.id, "browser": s.browser, "profile": s.profile})).collect::<Vec<_>>(),
            "imports": self.onboarding.imports,
            "importMsg": i18n::render(&self.imports.msg),
            "defaultBrowser": {
                "can": blackhole_macos_url::can_be_default_browser(),
                "is": blackhole_macos_url::is_default_browser(),
                "asked": self.onboarding.default_asked,
            },
            "extensions": extensions,
            "space": space.id.0,
            "color": space.color,
            "colors": onboarding::SPACE_COLORS,
            "engine": settings.search_engine.id(),
            "engines": SearchEngine::ALL.iter().map(|e| json!({"id": e.id(), "label": e.label()})).collect::<Vec<_>>(),
            "recall": settings.recall,
            "language": i18n::lang(),
        })
    }

    /// The start page itself is in front (not the Library, Settings or an Easel over it).
    pub(crate) fn start_page_front(&self) -> bool {
        self.home_shown() && !self.library && !self.settings_shown && !self.easel_tab
    }

    /// Dims the sidebar and the top bar under the tour's dialog (`bhDim` of theme.js) while it
    /// is open on the start page in front; a tab or panel brought forward lifts it.
    pub(crate) fn sync_dim(&self) {
        let on = self.onboarding.mode.is_some() && self.onboarding.open && self.start_page_front();
        if self.onboarding.dimmed.replace(on) == on {
            return;
        }
        let js = format!("window.bhDim&&bhDim({on})");
        let _ = self.chrome.evaluate_script(&js);
        if let Some(flyout) = &self.flyout {
            let _ = flyout.evaluate_script(&js);
        }
        let _ = self.bar.evaluate_script(&js);
    }

    /// The interface language changed: every open page of BlackHole gets the new catalog and
    /// re-renders, the shell's own texts (menus, titles, state) follow. Error pages already
    /// shown keep their language until the next error.
    /// The last pages of the start page, each once (domain::suggest::key), without sign-in steps:
    /// the ones seen in the space shown (and those of older files, which do not say where).
    fn home_recent_json(&self) -> Vec<serde_json::Value> {
        let mut seen = std::collections::HashSet::new();
        let space = self.svc.workspace().active_space().id.0;
        self.svc
            .recent_activity(400)
            .iter()
            .filter(|e| e.spaces.is_empty() || e.spaces.contains(&space))
            .filter(|e| !suggest::is_noise(&e.url) && seen.insert(suggest::key(&e.url)))
            .take(12)
            .map(|e| json!({"title": e.title, "url": e.url.as_str(), "at": e.visited_at}))
            .collect()
    }
}

impl Running {
    /// Commands of the welcome tour.
    pub(crate) fn handle_onboarding(&mut self, command: UiCommand) {
        match command {
            UiCommand::OnboardingOpen { on } => {
                self.onboarding.open = on;
                // The extensions it installs must not open their welcome pages over it.
                let welcome = on && self.onboarding.mode == Some(blackhole_domain::onboarding::Mode::Welcome);
                self.welcome.tour_open(welcome, Instant::now());
                self.sync_dim();
            }
            UiCommand::OnboardingDone => {
                self.onboarding.mode = None;
                self.onboarding.open = false;
                self.welcome.tour_open(false, Instant::now());
                self.onboarding.default_asked = false;
                self.sync_dim();
                self.ui_home();
            }
            UiCommand::ReplayTour => {
                self.onboarding.mode = Some(blackhole_domain::onboarding::Mode::Tour);
                self.handle_ui(UiCommand::Home);
            }
            UiCommand::SetDefaultBrowser => {
                self.onboarding.default_asked = blackhole_macos_url::set_default_browser();
                if !self.onboarding.default_asked {
                    self.toast(&default_browser_unavailable());
                }
                self.ui_home();
            }
            _ => {}
        }
    }
}

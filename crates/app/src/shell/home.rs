//! The start page and its welcome tour.

use std::time::Instant;

use blackhole_domain::{SearchEngine, suggest};
use blackhole_infrastructure::catalog;
use serde_json::json;

use crate::i18n;
use crate::ipc::UiCommand;
use crate::platform::default_browser_unavailable;
use crate::state::Running;

impl Running {
    pub(crate) fn ui_home(&self) {
        let mut state = json!({
            "engine": self.svc.settings().search_engine.label(),
            "animations": self.svc.settings().animations,
            "intro": self.intro,
            "depth": self.svc.settings().depth,
            "favorites": self.svc.favorites().iter()
                .map(|f| json!({
                    "name": f.name, "url": f.url.as_str(),
                    // Only tabs of the current space can be unpinned from here.
                    "tab": f.tab.filter(|id| self.svc.workspace().active_space().tabs.iter().any(|t| t.id == *id)).map(|id| id.0),
                }))
                .collect::<Vec<_>>(),
            "recent": self.home_recent_json(),
            "releaseNotes": self.release_notes,
            "now": self.home_now_json(),
            "hidden": self.svc.settings().hidden_home_sections,
            // In the user's order (dragged on the page).
            "sections": blackhole_domain::home_order(&self.svc.settings().home_order),
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
        let Some(mode) = self.onboarding else {
            return serde_json::Value::Null;
        };
        if mode == Mode::Tour {
            return json!({"mode": mode.id()});
        }
        let settings = self.svc.settings();
        let space = self.svc.workspace().active_space();
        // Only where extensions can run (not Linux, not without an extension host).
        let available: Vec<&str> = match &self.webext {
            Some(_) => self.market.for_this_system().map(|e| e.id.as_str()).collect(),
            None => Vec::new(),
        };
        let extensions: Vec<_> = onboarding::starters(available)
            .into_iter()
            .filter_map(|(id, on)| {
                let e = self.market.for_this_system().find(|e| e.id == id)?;
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
            "sources": self.import_sources.iter().map(|s| json!({"id": s.id, "browser": s.browser, "profile": s.profile})).collect::<Vec<_>>(),
            "imports": self.onboarding_imports,
            "importMsg": i18n::render(&self.import_msg),
            "defaultBrowser": {
                "can": blackhole_macos_url::can_be_default_browser(),
                "is": blackhole_macos_url::is_default_browser(),
                "asked": self.default_asked,
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
        let on = self.onboarding.is_some() && self.onboarding_open && self.start_page_front();
        if self.dimmed.replace(on) == on {
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
    /// The last pages of the start page, each once (domain::suggest::key), without sign-in steps.
    fn home_recent_json(&self) -> Vec<serde_json::Value> {
        let mut seen = std::collections::HashSet::new();
        self.svc
            .recent_activity(200)
            .iter()
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
                self.onboarding_open = on;
                // The extensions it installs must not open their welcome pages over it.
                let welcome = on && self.onboarding == Some(blackhole_domain::onboarding::Mode::Welcome);
                self.welcome.tour_open(welcome, Instant::now());
                self.sync_dim();
            }
            UiCommand::OnboardingDone => {
                self.onboarding = None;
                self.onboarding_open = false;
                self.welcome.tour_open(false, Instant::now());
                self.default_asked = false;
                self.sync_dim();
                self.ui_home();
            }
            UiCommand::ReplayTour => {
                self.onboarding = Some(blackhole_domain::onboarding::Mode::Tour);
                self.handle_ui(UiCommand::Home);
            }
            UiCommand::SetDefaultBrowser => {
                self.default_asked = blackhole_macos_url::set_default_browser();
                if !self.default_asked {
                    self.toast(&default_browser_unavailable());
                }
                self.ui_home();
            }
            _ => {}
        }
    }
}

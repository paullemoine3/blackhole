//! Pilot split: the links clicked in one pane of the split view open in the other pane. The
//! rules live in `domain::pilot`; the page only reports the click (`page_guard.js`), and the
//! shell decides where it goes.

use crate::i18n::t;
use crate::{Running, TabId};
use blackhole_application::NavigationOutcome;
use blackhole_domain::pilot;
use url::Url;
use wry::WebView;

/// Tells the page of the pilot pane to hand its link clicks to the shell (or to stop). A page
/// that sets the flag itself only gets its own links reported: the shell checks who the pilot is.
fn mark(view: &WebView, on: bool) {
    let _ = view.evaluate_script(if on { "window.__bhPilot=true" } else { "window.__bhPilot=false" });
}

impl Running {
    /// The pilot pane and the pane it drives, while both are on screen.
    pub(crate) fn pilot_panes(&self) -> Option<(TabId, TabId)> {
        if self.home_shown() || self.easel_side() {
            return None;
        }
        let active = self.svc.workspace().active_tab().map(|t| t.id);
        let other = pilot::target(self.pilot, active, self.split_tab())?;
        Some((self.pilot?, other))
    }

    /// The split bar's button, the command bar: `tab` becomes the pilot, or stops being it.
    pub(crate) fn toggle_pilot(&mut self, tab: Option<TabId>) {
        let active = self.svc.workspace().active_tab().map(|t| t.id);
        let Some(tab) = tab.or(active) else { return };
        if pilot::target(Some(tab), active, self.split_tab()).is_none() || self.easel_side() {
            self.toast(&t("toast.pilot_needs_split"));
            return;
        }
        let was = self.pilot.take();
        if let Some(view) = was.and_then(|id| self.tabs.get(&id)) {
            mark(view, false);
        }
        if was != Some(tab) {
            self.pilot = Some(tab);
            self.pilot_mark(tab);
            self.toast(&t("toast.pilot_on"));
        }
        self.ui_splitbar();
    }

    /// A new document of the pilot tab has no flag yet: it is set again.
    pub(crate) fn pilot_mark(&self, tab: TabId) {
        if self.pilot == Some(tab) {
            if let Some(view) = self.tabs.get(&tab) {
                mark(view, true);
            }
        }
    }

    /// The split ended, or the pilot left the screen: its page gets its links back.
    pub(crate) fn pilot_sync(&mut self) {
        let Some(tab) = self.pilot else { return };
        let active = self.svc.workspace().active_tab().map(|t| t.id);
        if pilot::target(Some(tab), active, self.split_tab()).is_none() {
            self.pilot = None;
            if let Some(view) = self.tabs.get(&tab) {
                mark(view, false);
            }
        }
    }

    /// A link clicked in a page that believes it is the pilot (`url` untrusted: checked like
    /// any navigation). It loads in the other pane; when this tab is not the pilot after all,
    /// in the tab itself, since the page gave the click up.
    pub(crate) fn pilot_link(&mut self, tab: TabId, raw: &str) {
        let Ok(link) = Url::parse(raw) else { return };
        if !matches!(link.scheme(), "http" | "https") {
            return;
        }
        let page = self
            .tabs
            .get(&tab)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok());
        let other = self
            .pilot_panes()
            .filter(|(pilot, _)| *pilot == tab)
            .map(|(_, other)| other)
            .filter(|_| page.as_ref().is_some_and(|page| pilot::leaves_page(page, &link)));
        let destination = other.unwrap_or(tab);
        if destination == tab {
            if let Some(view) = self.tabs.get(&tab) {
                mark(view, self.pilot == Some(tab));
            }
        }
        match self.svc.evaluate(&link) {
            NavigationOutcome::Go(url) => {
                if self.refuse_restricted_port(destination, &url) {
                    // Nothing to load: WebKit would refuse it without a word.
                } else if let Some(view) = self.tabs.get(&destination) {
                    let _ = view.load_url(url.as_str());
                }
            }
            NavigationOutcome::Blocked(why) => {
                self.toast(&t(&format!("blocked.{}", why.replace(' ', "_"))));
            }
        }
    }
}

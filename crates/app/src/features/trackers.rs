//! Trackers learned while browsing (rules in `domain::trackers`, file `trackers.json`). The pages
//! report their requests to other hosts (`ui/tracker_watch.js`, untrusted); the shell reads the
//! page's address on the engine, learns a domain seen tracking on several sites and hands the
//! learned and allowed hosts to the policy, which every view shares: the navigation and request
//! checks follow at once, the native lists (macOS rule lists, WebView2 filters) from the next
//! page view on.
//!
//! Never in a private window nor a black hole tab (the sites visited are not written down there).

use blackhole_domain::{is_third_party, PrivacyDecision, TabId};
use blackhole_domain::trackers::Trackers;
use blackhole_infrastructure::store::TrackerStore;
use serde_json::{json, Value};
use url::Url;

use crate::state::Running;
use crate::i18n::tf;
use crate::ipc::{ResourceHit, UiCommand};

pub struct Learning {
    store: TrackerStore,
    pub(crate) data: Trackers,
    private: bool,
    /// Something changed since the last write.
    dirty: bool,
}

impl Learning {
    pub fn new(data: &std::path::Path, private: bool) -> Self {
        let store = TrackerStore::new(data);
        // A private window learns nothing and writes nothing, but blocks what was learned.
        let trackers = store.load();
        Self { store, data: trackers, private, dirty: false }
    }

    /// Writes what changed (off the UI thread).
    pub fn flush(&mut self) {
        if std::mem::take(&mut self.dirty) && !self.private {
            self.store.save(&self.data);
        }
    }
}

impl Running {
    /// Gives the policy the learned trackers and the hosts the user allowed.
    pub(crate) fn trackers_to_policy(&self) {
        let t = &self.trackers.data;
        self.svc.policy().set_learned(t.learned().iter().cloned(), t.allowed().iter().cloned());
    }

    /// Windows: an extension (uBlock…) stops a request before WebView2 asks the Shields, so
    /// nothing was counted. The ones to a host of the Shields' lists are, like on macOS where
    /// WebKit tells of both; what only the extension refuses is not counted anywhere.
    fn count_stopped_before(&mut self, tab: TabId, list: &[ResourceHit]) {
        let Some(page) = self.tabs.get(&tab).and_then(|v| v.url().ok()).and_then(|raw| Url::parse(&raw).ok()) else {
            return;
        };
        let page = page.host_str().unwrap_or_default().to_owned();
        let policy = self.svc.policy();
        let stopped: Vec<String> = list
            .iter()
            .take(50)
            .filter_map(|hit| Url::parse(&hit.url).ok())
            .filter(|url| {
                let host = url.host_str().unwrap_or_default();
                let decision = crate::tabs::errors::with_exceptions(policy.evaluate(url), url);
                is_third_party(host, &page)
                    && self.refused.misses(tab.0, host)
                    && matches!(decision, PrivacyDecision::Block(_))
            })
            .filter_map(|url| url.host_str().map(str::to_owned))
            .collect();
        let (site, space) = (self.site_of(tab), self.space_of_tab(tab));
        for host in stopped {
            self.on_blocked("", &host, &site, space);
        }
    }

    /// A page reported requests to other hosts.
    pub(crate) fn tracker_requests(&mut self, tab: TabId, mut list: Vec<ResourceHit>) {
        if cfg!(windows) {
            self.count_stopped_before(tab, &list);
        }
        // Read from a tag: it says nothing of the request, which may never have left.
        list.retain(|hit| hit.kind != "tag");
        if self.trackers.private || !self.svc.settings().tracker_learning || self.svc.workspace().is_burner(tab) {
            return;
        }
        // The page's own address comes from the engine, never from the page.
        let Some(page) = self.tabs.get(&tab).and_then(|v| v.url().ok()).and_then(|raw| Url::parse(&raw).ok()) else {
            return;
        };
        let Some(page_host) = page.host_str().map(str::to_owned) else { return };
        // Everything only grows here: the sizes tell whether there is anything to write.
        let size = |t: &Trackers| t.watched().values().map(|s| s.len() + 1).sum::<usize>() + t.learned().len() + t.visited().len();
        let before = size(&self.trackers.data);
        self.trackers.data.visit(&page_host);
        let mut learned = Vec::new();
        for hit in list.into_iter().take(50) {
            let Ok(url) = Url::parse(&hit.url) else { continue };
            let Some(host) = url.host_str() else { continue };
            let policy = self.svc.policy();
            let listed = policy.is_listed(host) || policy.is_blocked_host(host);
            if let Some(domain) = self.trackers.data.observe(&url, &hit.kind, &page_host, listed) {
                learned.push(domain);
            }
        }
        if size(&self.trackers.data) != before {
            self.trackers.dirty = true;
        }
        if !learned.is_empty() {
            self.trackers_to_policy();
            crate::platform::trace(|| format!("trackers learned: {learned:?}"));
            self.ui_stale = true;
        }
    }

    /// Spare views carry the blocklists of their creation time: new ones are made.
    fn reset_spares(&mut self) {
        self.spares.clear();
        self.spares_due = Some(std::time::Instant::now() + crate::tabs::lifecycle::SPARES_DELAY);
    }

    /// Settings › Privacy: the learned trackers and the allowed hosts.
    pub(crate) fn trackers_json(&self) -> Value {
        let t = &self.trackers.data;
        json!({
            "learning": self.svc.settings().tracker_learning,
            "available": !self.trackers.private,
            "learned": t.learned(),
            "allowed": t.allowed(),
            "watched": t.watched().len(),
        })
    }

    pub(crate) fn handle_trackers(&mut self, command: UiCommand) {
        match command {
            UiCommand::SetTrackerLearning { on } => {
                self.svc.set_tracker_learning(on);
            }
            UiCommand::TrackerAllow { host } => {
                if let Some(host) = self.trackers.data.allow(&host) {
                    self.trackers.dirty = true;
                    self.trackers_to_policy();
                    self.reset_spares();
                    self.toast(&tf("toast.tracker_allowed", &[("host", &host)]));
                }
            }
            UiCommand::TrackerBlock { host } => {
                // A listed host is blocked by its list again; another one goes back to the
                // learned trackers.
                let listed = self.svc.policy().is_listed(&host);
                if self.trackers.data.block_again(&host, !listed) {
                    self.trackers.dirty = true;
                    self.trackers_to_policy();
                    self.reset_spares();
                }
            }
            UiCommand::TrackersForget => {
                self.trackers.data.forget();
                self.trackers.dirty = true;
                self.trackers_to_policy();
                self.reset_spares();
            }
            _ => return,
        }
        self.trackers.flush();
        self.ui_state();
        // The Library's Shields report shows the host allowed or blocked again.
        self.ui_library();
    }
}

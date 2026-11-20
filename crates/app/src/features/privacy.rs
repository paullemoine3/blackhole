//! The privacy report of a site and what the blockers stopped.

use blackhole_domain::TabId;
use serde_json::json;
use url::Url;

use crate::i18n::{t, tf, tn};
use crate::state::Running;
use crate::tabs::errors;

impl Running {
    /// The shield's report: for the site shown, what was blocked this session (trackers and
    /// ads apart, the hosts tried most) and a grade; on the start page, the session total.
    pub(crate) fn privacy_report(&mut self, at: (f64, f64)) {
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
                    // A host that breaks the page can be allowed from here (Settings › Privacy
                    // lists it and blocks it again).
                    items.push(json!({"head": t("menu.blocked_hosts_allow")}));
                    items.extend(hosts.iter().take(8).map(|(host, n)| {
                        json!({"icon": if is_ad(host) { "close" } else { "eye" }, "label": format!("{host} · {n}"),
                            "run": {"cmd": "tracker_allow", "host": host}})
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

    pub(crate) fn site_of(&self, tab: TabId) -> String {
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

    pub(crate) fn on_blocked(&mut self, why: &str, host: &str, site: &str, space: u64) {
        self.blocked.record_in(space, host, site);
        // `why` is the domain's reason ("tracker blocked"…), shown in the interface language.
        if !why.is_empty() {
            self.toast(&t(&format!("blocked.{}", why.replace(' ', "_"))));
        }
        self.ui_state();
    }
}

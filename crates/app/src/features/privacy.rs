//! The privacy report of a site and what the blockers stopped: the shield's menu, and the
//! Shields section of the Library (0.8.28).

use std::time::{Duration, Instant};

use blackhole_domain::shields_report::{companies, owner_of, time_saved_secs, Kind};
use blackhole_domain::TabId;
use serde_json::{json, Value};
use url::Url;

use crate::i18n::{t, tf, tn};
use crate::state::Running;
use crate::tabs::errors;

/// How often the open Library hears of new blocked requests.
const SHIELDS_EVERY: Duration = Duration::from_millis(400);

impl Running {
    /// The shield's report: for the site shown, what was blocked this session (trackers and
    /// ads apart, the hosts tried most) and a grade; on the start page, the session total.
    pub(crate) fn privacy_report(&mut self, at: (f64, f64)) {
        let mut items = Vec::new();
        match self.active_site() {
            Some(site) => {
                let hosts = self.blocked.for_site(&site);
                let is_ad = |host: &str| self.is_ad_host(host);
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
        items.push(json!({"icon": "shield", "label": t("menu.shields_report"), "run": {"cmd": "open_library", "section": "shields"}}));
        items.push(json!({"icon": "settings", "label": t("menu.privacy_settings"), "run": {"cmd": "open_settings", "tab": "privacy"}}));
        self.show_menu(at, json!(items));
    }

    /// Whether `host` counts with the ads rather than the trackers: the ad list says so, or
    /// its owner is an ad network (`shields_report::owner_of`, e.g. doubleclick.net, which the
    /// tracker list also has).
    fn is_ad_host(&self, host: &str) -> bool {
        if owner_of(host).is_some_and(|o| o.kind == Kind::Ads) {
            return true;
        }
        let policy = self.svc.policy();
        Url::parse(&format!("https://{host}/"))
            .is_ok_and(|u| policy.evaluate(&u) == blackhole_domain::PrivacyDecision::Block("ad blocked"))
    }

    /// The Library's Shields report: the sites of the space shown (and any other that had
    /// something blocked), each with its grade, its blocked hosts (who owns them, what they
    /// do, learned or allowed) and its companies; the session's totals; the Shields switched on.
    pub(crate) fn shields_json(&self) -> Value {
        let ws = self.svc.workspace();
        let policy = self.svc.policy();
        let learned = self.trackers.data.learned();
        // (site, every tab of it in https)
        let mut sites: Vec<(String, bool)> = Vec::new();
        for tab in &ws.active_space().tabs {
            if !matches!(tab.url.scheme(), "http" | "https") {
                continue;
            }
            let Some(host) = tab.url.host_str() else { continue };
            let https = tab.url.scheme() == "https";
            match sites.iter_mut().find(|(s, _)| s == host) {
                Some(entry) => entry.1 &= https,
                None => sites.push((host.to_owned(), https)),
            }
        }
        let open = sites.len();
        for (site, _) in self.blocked.sites() {
            if !sites.iter().any(|(s, _)| *s == site) {
                sites.push((site, true));
            }
        }
        let mut list: Vec<Value> = sites
            .iter()
            .enumerate()
            .map(|(i, (site, secure))| {
                let hosts = self.blocked.for_site(site);
                let (mut trackers, mut ads) = (0u32, 0u32);
                let rows: Vec<Value> = hosts
                    .iter()
                    .map(|(host, n)| {
                        let ad = self.is_ad_host(host);
                        if ad {
                            ads += n;
                        } else {
                            trackers += n;
                        }
                        let owner = owner_of(host);
                        let kind = match owner.map(|o| o.kind) {
                            Some(kind) => kind,
                            None if ad => Kind::Ads,
                            None => Kind::Tracker,
                        };
                        let base = blackhole_domain::trackers::base_domain(host);
                        json!({
                            "host": host, "n": n, "ad": ad, "kind": kind.as_str(),
                            "company": owner.map(|o| o.company),
                            "learned": learned.contains(host) || learned.contains(&base),
                            "allowed": policy.is_allowed(host),
                        })
                    })
                    .collect();
                json!({
                    "site": site, "open": i < open, "secure": secure,
                    "total": trackers + ads, "trackers": trackers, "ads": ads,
                    "grade": blackhole_domain::privacy_grade(trackers, ads).to_string(),
                    "hosts": rows, "companies": companies(&hosts),
                })
            })
            .collect();
        // The open sites first, the most blocked first among each.
        list.sort_by_key(|s| {
            (!s["open"].as_bool().unwrap_or(false), std::cmp::Reverse(s["total"].as_u64().unwrap_or(0)))
        });
        let (mut trackers, mut ads) = (0u32, 0u32);
        for entry in self.blocked.entries() {
            if self.is_ad_host(&entry.host) {
                ads += entry.count;
            } else {
                trackers += entry.count;
            }
        }
        let total = self.blocked.total();
        let shields = policy.shields();
        json!({
            "current": self.active_site(),
            "sites": list,
            "total": total,
            "trackers": trackers,
            "ads": ads,
            "links": self.links_cleaned,
            "saved": time_saved_secs(total),
            "blocking": policy.is_blocking(),
            "adBlocking": policy.is_blocking_ads(),
            "cookies": shields.cookie_banners,
            "fingerprinting": shields.fingerprinting,
            "cleanLinks": shields.clean_links,
        })
    }

    /// Sends the Shields report to the Library while it is open, at most every `SHIELDS_EVERY`:
    /// a page load blocks dozens of requests in a burst. Returns when to come back if a change
    /// is still waiting.
    pub(crate) fn service_shields(&mut self) -> Option<Duration> {
        if !self.shields_dirty {
            return None;
        }
        if !self.library {
            self.shields_dirty = false;
            return None;
        }
        let since = self.shields_sent.elapsed();
        if since < SHIELDS_EVERY {
            return Some(SHIELDS_EVERY - since);
        }
        self.shields_dirty = false;
        self.shields_sent = Instant::now();
        if let Some(panel) = &self.library_panel.view {
            let _ = panel.evaluate_script(&format!("window.bh&&bh.shields&&bh.shields({})", self.shields_json()));
        }
        None
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
        self.shields_dirty = true;
        // `why` is the domain's reason ("tracker blocked"…), shown in the interface language.
        if !why.is_empty() {
            self.toast(&t(&format!("blocked.{}", why.replace(' ', "_"))));
        }
        self.ui_state();
    }
}

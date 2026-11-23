//! Trackers learned while browsing, like Privacy Badger: a third-party domain whose requests
//! look like tracking (beacons, pixels, collect endpoints, visitor ids) on several unrelated
//! sites is blocked like a listed tracker. The user can allow any blocked host back (the
//! listed ones too) and block it again.
//!
//! Guards against blocking what a page needs: only tracking-shaped requests count, a domain is
//! learned after `SITES_TO_LEARN` distinct sites, never when the user visits it as a site of
//! its own, never for the shared infrastructure of `ESSENTIAL`.

use crate::privacy::is_third_party;
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

/// Distinct sites a domain must track the user on before it is blocked.
pub const SITES_TO_LEARN: usize = 3;
/// Domains under watch at most (pages are untrusted: they could report anything).
pub const MAX_WATCHED: usize = 2000;
/// Sites visited remembered at most (a domain visited as a site is never learned).
pub const MAX_VISITED: usize = 2000;
pub const MAX_LEARNED: usize = 1000;
pub const MAX_ALLOWED: usize = 500;

/// Shared infrastructure and services pages embed for what they show (videos, fonts, maps,
/// sign-in, captchas, CDNs): never learned, whatever their requests look like.
pub const ESSENTIAL: [&str; 34] = [
    "google.com", "gstatic.com", "googleapis.com", "googleusercontent.com", "youtube.com",
    "ytimg.com", "youtube-nocookie.com", "recaptcha.net", "cloudflare.com", "cloudflareinsights.com",
    "cloudfront.net", "akamaihd.net", "akamaized.net", "fastly.net", "jsdelivr.net", "unpkg.com",
    "jquery.com", "bootstrapcdn.com", "microsoft.com", "live.com", "office.com", "apple.com",
    "icloud.com", "github.com", "githubusercontent.com", "githubassets.com", "vimeo.com",
    "vimeocdn.com", "twimg.com", "x.com", "wikimedia.org", "mozilla.org", "stripe.com",
    "paypal.com",
];

/// Path segments of tracking endpoints (a segment, or its name before the extension).
const PATH_WORDS: [&str; 22] = [
    "collect", "track", "tracking", "tracker", "pixel", "beacon", "beacons", "analytics",
    "telemetry", "pageview", "impression", "impressions", "__utm", "tr", "ping", "hit",
    "metrics", "rum", "events", "event", "log_event", "fingerprint",
];

/// Query parameters that carry a visitor or a click id.
const ID_PARAMS: [&str; 16] = [
    "uid", "cid", "_ga", "gclid", "fbp", "fbclid", "visitor_id", "visitorid", "vid", "user_id",
    "userid", "client_id", "clientid", "device_id", "deviceid", "anonymous_id",
];

/// The registrable domain of `host` (`a.b.example.co.uk` → `example.co.uk`), approximated
/// without the public suffix list: two labels, three under a short second-level suffix.
pub fn base_domain(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return host;
    }
    let short_sld = labels[n - 1].len() == 2
        && matches!(labels[n - 2], "co" | "com" | "net" | "org" | "gov" | "ac" | "edu" | "ne" | "or" | "go" | "gouv");
    let keep = if short_sld { 3 } else { 2 };
    labels[n - keep..].join(".")
}

/// A host that is a name on the internet (not an address, not local).
fn is_public_name(host: &str) -> bool {
    host.contains('.')
        && !host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && !host.contains(':')
        && !host.ends_with(".local")
        && !host.ends_with(".lan")
        && !host.ends_with(".localhost")
        && !host.ends_with(".test")
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// Whether a request looks like tracking. `initiator` is how the page made it (the resource
/// timing's `initiatorType`: `beacon`, `img`, `script`, `fetch`…).
pub fn looks_like_tracking(url: &Url, initiator: &str) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    if matches!(initiator, "beacon" | "ping") {
        return true;
    }
    let word = |seg: &str| {
        let seg = seg.to_ascii_lowercase();
        let stem = seg.split('.').next().unwrap_or_default().to_owned();
        PATH_WORDS.contains(&seg.as_str()) || PATH_WORDS.contains(&stem.as_str())
    };
    let path_says = url.path_segments().is_some_and(|mut segs| segs.any(word));
    // A visitor id sent with an image or a request nobody shows: a pixel.
    let ids = url.query_pairs().any(|(k, _)| ID_PARAMS.contains(&k.to_ascii_lowercase().as_str()));
    let silent = matches!(initiator, "img" | "image" | "xmlhttprequest" | "fetch" | "other" | "");
    path_says && initiator != "link" && initiator != "css" || ids && silent
}

/// What was learned: domains under watch with the sites they tracked the user on, the learned
/// trackers, the hosts the user allowed and the sites the user visited.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trackers {
    watched: BTreeMap<String, BTreeSet<String>>,
    learned: BTreeSet<String>,
    allowed: BTreeSet<String>,
    visited: BTreeSet<String>,
}

impl Trackers {
    /// Rebuilds from saved lists, bounded and cleaned (the file may have been edited).
    pub fn restore(
        watched: BTreeMap<String, BTreeSet<String>>,
        learned: impl IntoIterator<Item = String>,
        allowed: impl IntoIterator<Item = String>,
        visited: impl IntoIterator<Item = String>,
    ) -> Self {
        let clean = |h: String| {
            let h = h.trim().trim_start_matches("www.").to_ascii_lowercase();
            is_public_name(&h).then_some(h)
        };
        Self {
            watched: watched
                .into_iter()
                .filter_map(|(d, sites)| {
                    let sites: BTreeSet<String> = sites.into_iter().filter_map(clean).take(SITES_TO_LEARN).collect();
                    Some((clean(d)?, sites))
                })
                .take(MAX_WATCHED)
                .collect(),
            learned: learned.into_iter().filter_map(clean).take(MAX_LEARNED).collect(),
            allowed: allowed.into_iter().filter_map(clean).take(MAX_ALLOWED).collect(),
            visited: visited.into_iter().filter_map(clean).take(MAX_VISITED).collect(),
        }
    }

    pub fn watched(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.watched
    }

    pub fn learned(&self) -> &BTreeSet<String> {
        &self.learned
    }

    pub fn allowed(&self) -> &BTreeSet<String> {
        &self.allowed
    }

    pub fn visited(&self) -> &BTreeSet<String> {
        &self.visited
    }

    /// A page of `page_host` is shown: its site is one the user visits, never learned.
    /// Returns whether anything changed.
    pub fn visit(&mut self, page_host: &str) -> bool {
        let site = base_domain(page_host);
        if !is_public_name(&site) || self.visited.contains(&site) {
            return false;
        }
        if self.visited.len() >= MAX_VISITED {
            return false;
        }
        self.watched.remove(&site);
        self.visited.insert(site)
    }

    /// A request of a page of `page_host` to `url`. `listed`: its host is already on a list
    /// (nothing to learn). Returns the domain learned by this request, if it was the last
    /// site needed.
    pub fn observe(&mut self, url: &Url, initiator: &str, page_host: &str, listed: bool) -> Option<String> {
        let host = url.host_str()?.to_ascii_lowercase();
        let (domain, site) = (base_domain(&host), base_domain(page_host));
        if listed
            || !is_public_name(&domain)
            || !is_public_name(&site)
            || domain == site
            || !is_third_party(&host, page_host)
            || ESSENTIAL.contains(&domain.as_str())
            || self.learned.contains(&domain)
            || self.allowed.contains(&domain)
            || self.visited.contains(&domain)
            || !looks_like_tracking(url, initiator)
        {
            return None;
        }
        if !self.watched.contains_key(&domain) && self.watched.len() >= MAX_WATCHED {
            return None;
        }
        let sites = self.watched.entry(domain.clone()).or_default();
        sites.insert(site);
        if sites.len() < SITES_TO_LEARN || self.learned.len() >= MAX_LEARNED {
            return None;
        }
        self.watched.remove(&domain);
        self.learned.insert(domain.clone());
        Some(domain)
    }

    /// The user allows `host` (a learned tracker or a listed one): never blocked again until
    /// blocked by hand. Returns the host as kept.
    pub fn allow(&mut self, host: &str) -> Option<String> {
        let host = host.trim().trim_start_matches("www.").to_ascii_lowercase();
        if !is_public_name(&host) || (self.allowed.len() >= MAX_ALLOWED && !self.allowed.contains(&host)) {
            return None;
        }
        self.learned.remove(&host);
        self.watched.remove(&host);
        self.allowed.insert(host.clone());
        Some(host)
    }

    /// Takes `host` off the allowed list, or the allowed domain above it (`www.` dropped, as
    /// `allow` keeps it): the Shields report names the host that was refused, not the entry.
    /// `learned_again`: it was a learned tracker (not a listed one), blocked again at once.
    /// Returns whether it was allowed.
    pub fn block_again(&mut self, host: &str, learned_again: bool) -> bool {
        let wanted = host.trim().trim_start_matches("www.").to_ascii_lowercase();
        let mut candidate = wanted.as_str();
        let host = loop {
            if self.allowed.contains(candidate) {
                break candidate.to_owned();
            }
            match candidate.split_once('.') {
                Some((_, rest)) if rest.contains('.') => candidate = rest,
                _ => return false,
            }
        };
        self.allowed.remove(&host);
        if learned_again && self.learned.len() < MAX_LEARNED {
            self.learned.insert(host);
        }
        true
    }

    /// Forgets what was learned (the allowed hosts stay).
    pub fn forget(&mut self) {
        self.watched.clear();
        self.learned.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn base_domains_keep_the_registrable_part() {
        assert_eq!(base_domain("a.b.example.com"), "example.com");
        assert_eq!(base_domain("www.example.co.uk"), "example.co.uk");
        assert_eq!(base_domain("example.fr"), "example.fr");
        assert_eq!(base_domain("stats.gouv.fr"), "stats.gouv.fr");
    }

    #[test]
    fn tracking_requests_are_told_from_ordinary_ones() {
        assert!(looks_like_tracking(&u("https://x.test/anything"), "beacon"));
        assert!(looks_like_tracking(&u("https://px.ads.test/collect?v=1"), "img"));
        assert!(looks_like_tracking(&u("https://px.ads.test/p/pixel.gif"), "img"));
        assert!(looks_like_tracking(&u("https://px.ads.test/i.gif?uid=42"), "img"));
        assert!(!looks_like_tracking(&u("https://cdn.test/app.js"), "script"));
        assert!(!looks_like_tracking(&u("https://cdn.test/img/logo.png"), "img"));
        assert!(!looks_like_tracking(&u("https://fonts.test/css?family=x&uid=1"), "css"));
        assert!(!looks_like_tracking(&u("https://cdn.test/track.css"), "link"), "a stylesheet");
        assert!(!looks_like_tracking(&u("ftp://x.test/collect"), "img"));
    }

    #[test]
    fn a_domain_is_learned_after_three_unrelated_sites() {
        let mut t = Trackers::default();
        let pixel = u("https://px.spy.example/collect?cid=1");
        assert_eq!(t.observe(&pixel, "img", "news.one.test.com", false), None);
        assert_eq!(t.observe(&pixel, "img", "www.one.test.com", false), None, "same site twice");
        assert_eq!(t.observe(&pixel, "img", "shop.two.com", false), None);
        assert_eq!(t.watched()["spy.example"].len(), 2);
        assert_eq!(t.observe(&pixel, "img", "blog.three.org", false).as_deref(), Some("spy.example"));
        assert!(t.learned().contains("spy.example") && !t.watched().contains_key("spy.example"));
        assert_eq!(t.observe(&pixel, "img", "four.net", false), None, "already learned");
    }

    #[test]
    fn what_a_page_needs_is_never_learned() {
        let mut t = Trackers::default();
        let sites = ["a.com", "b.com", "c.com", "d.com"];
        let learn = |t: &mut Trackers, url: &str, listed: bool| {
            sites.iter().filter_map(|s| t.observe(&u(url), "img", s, listed)).count()
        };
        assert_eq!(learn(&mut t, "https://www.youtube.com/api/stats/collect", false), 0, "essential");
        assert_eq!(learn(&mut t, "https://t.listed.example/collect", true), 0, "already listed");
        assert_eq!(learn(&mut t, "https://cdn.example/logo.png", false), 0, "not tracking");
        assert_eq!(t.observe(&u("https://cdn.a.com/collect"), "img", "www.a.com", false), None, "first party");
        assert_eq!(t.observe(&u("https://10.0.0.1/collect"), "img", "a.com", false), None);
        // A site the user visits is never learned, and stops being watched.
        for s in ["a.com", "b.com"] {
            assert_eq!(t.observe(&u("https://mail.portal.example/collect"), "img", s, false), None);
        }
        assert!(t.visit("www.portal.example"));
        assert!(!t.watched().contains_key("portal.example"));
        assert_eq!(t.observe(&u("https://x.portal.example/collect"), "img", "e.com", false), None);
    }

    #[test]
    fn allowed_hosts_stay_allowed_until_blocked_again() {
        let mut t = Trackers::default();
        let pixel = u("https://px.spy.example/pixel");
        for s in ["a.com", "b.com", "c.com"] {
            t.observe(&pixel, "img", s, false);
        }
        assert_eq!(t.allow("spy.example").as_deref(), Some("spy.example"));
        assert!(t.learned().is_empty() && t.allowed().contains("spy.example"));
        for s in ["d.com", "e.com", "f.com"] {
            assert_eq!(t.observe(&pixel, "img", s, false), None);
        }
        assert!(t.allow("not a host").is_none());
        // A listed host allowed then blocked again is not learned (its list blocks it).
        t.allow("listed.example");
        assert!(t.block_again("listed.example", false));
        assert!(!t.learned().contains("listed.example"));
        assert!(t.block_again("spy.example", true));
        assert!(t.learned().contains("spy.example"));
        assert!(!t.block_again("spy.example", true), "no longer allowed");
        // From the Shields report: the host refused, under the entry allowed, `www.` or not.
        t.allow("www.metrics.example");
        assert!(t.block_again("www.metrics.example", false));
        t.allow("ads.example");
        assert!(t.block_again("px.eu.ads.example", false));
        assert!(t.allowed().is_empty());
        assert!(!t.block_again("example", false));
        t.forget();
        assert!(t.learned().is_empty() && t.watched().is_empty());
    }

    #[test]
    fn restored_lists_are_cleaned_and_bounded() {
        let mut watched = BTreeMap::new();
        watched.insert("Spy.Example".to_owned(), ["a.com", "b.com", "c.com", "d.com"].map(String::from).into());
        watched.insert("bad host".to_owned(), BTreeSet::new());
        let t = Trackers::restore(watched, ["www.x.example".to_owned(), "127.0.0.1".to_owned()], [], []);
        assert_eq!(t.watched()["spy.example"].len(), SITES_TO_LEARN);
        assert_eq!(t.watched().len(), 1);
        assert_eq!(t.learned().iter().collect::<Vec<_>>(), ["x.example"]);
    }
}

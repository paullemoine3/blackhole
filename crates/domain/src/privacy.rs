use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use url::Url;

/// What the page itself is asked to clean up (Brave-style "shields"), on top of the blocked
/// requests: applied by a script in every page opened (or reloaded) afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shields {
    /// Hide ad slots and the space they leave (cosmetic filtering).
    pub hide_ads: bool,
    /// Skip the ads of video players (YouTube).
    pub video_ads: bool,
    /// Hide cookie consent banners (and give the page its scrolling back).
    pub cookie_banners: bool,
}

impl Default for Shields {
    fn default() -> Self {
        Self {
            hide_ads: true,
            video_ads: true,
            cookie_banners: true,
        }
    }
}

impl Shields {
    fn bits(self) -> u8 {
        u8::from(self.hide_ads) | (u8::from(self.video_ads) << 1) | (u8::from(self.cookie_banners) << 2)
    }

    fn from_bits(bits: u8) -> Self {
        Self {
            hide_ads: bits & 1 != 0,
            video_ads: bits & 2 != 0,
            cookie_banners: bits & 4 != 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrivacyDecision {
    Allow,
    /// Navigate to this rewritten URL instead (https upgrade, tracking params stripped).
    Rewrite(Url),
    Block(&'static str),
}

/// Decides whether and how a navigation may proceed.
#[derive(Debug, Clone)]
pub struct PrivacyPolicy {
    /// Trackers and analytics.
    blocked_domains: HashSet<String>,
    /// Ad servers, blocked on their own switch.
    ad_domains: HashSet<String>,
    /// Shared by every clone, so switching it applies to views that already exist.
    blocking: Arc<AtomicBool>,
    ads: Arc<AtomicBool>,
    shields: Arc<AtomicU8>,
}

impl Default for PrivacyPolicy {
    fn default() -> Self {
        Self::new([])
    }
}

const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "dclid", "msclkid", "yclid", "mc_eid", "mc_cid", "igshid", "_hsenc",
    "_hsmi", "ref_src", "wbraid", "gbraid",
];

fn is_tracking_param(key: &str) -> bool {
    key.starts_with("utm_") || TRACKING_PARAMS.contains(&key)
}

impl PrivacyPolicy {
    pub fn new(blocked_domains: impl IntoIterator<Item = String>) -> Self {
        Self::with_ads(blocked_domains, [])
    }

    /// Trackers and ad servers, each list on its own switch (both on at first).
    pub fn with_ads(
        trackers: impl IntoIterator<Item = String>,
        ads: impl IntoIterator<Item = String>,
    ) -> Self {
        let lower = |list: &mut dyn Iterator<Item = String>| list.map(|d| d.to_lowercase()).collect();
        Self {
            blocked_domains: lower(&mut trackers.into_iter()),
            ad_domains: lower(&mut ads.into_iter()),
            blocking: Arc::new(AtomicBool::new(true)),
            ads: Arc::new(AtomicBool::new(true)),
            shields: Arc::new(AtomicU8::new(Shields::default().bits())),
        }
    }

    /// Turns the tracker blocklist on or off (the https upgrade and parameter clean-up stay).
    pub fn set_blocking(&self, on: bool) {
        self.blocking.store(on, Ordering::Relaxed);
    }

    pub fn is_blocking(&self) -> bool {
        self.blocking.load(Ordering::Relaxed)
    }

    /// Turns the ad server blocklist on or off.
    pub fn set_ad_blocking(&self, on: bool) {
        self.ads.store(on, Ordering::Relaxed);
    }

    pub fn is_blocking_ads(&self) -> bool {
        self.ads.load(Ordering::Relaxed)
    }

    pub fn set_shields(&self, shields: Shields) {
        self.shields.store(shields.bits(), Ordering::Relaxed);
    }

    pub fn shields(&self) -> Shields {
        Shields::from_bits(self.shields.load(Ordering::Relaxed))
    }

    /// The raw blocklist, for engines that must enforce it inside the page: the lists that are
    /// on (empty when both are off).
    pub fn blocked_domains(&self) -> impl Iterator<Item = &str> {
        let (trackers, ads) = (self.is_blocking(), self.is_blocking_ads());
        self.blocked_domains
            .iter()
            .filter(move |_| trackers)
            .chain(self.ad_domains.iter().filter(move |_| ads))
            .map(String::as_str)
    }

    /// Why `host` (or a domain above it) is blocked, if it is.
    fn block_reason(&self, host: &str) -> Option<&'static str> {
        let (trackers, ads) = (self.is_blocking(), self.is_blocking_ads());
        if !trackers && !ads {
            return None;
        }
        let host = host.to_lowercase();
        let mut candidate = host.as_str();
        loop {
            if trackers && self.blocked_domains.contains(candidate) {
                return Some("tracker blocked");
            }
            if ads && self.ad_domains.contains(candidate) {
                return Some("ad blocked");
            }
            match candidate.split_once('.') {
                Some((_, rest)) if rest.contains('.') => candidate = rest,
                _ => return None,
            }
        }
    }

    pub fn is_blocked_host(&self, host: &str) -> bool {
        self.block_reason(host).is_some()
    }

    pub fn evaluate(&self, url: &Url) -> PrivacyDecision {
        // Pages of installed extensions (e.g. a password manager's unlock popout) are local.
        if crate::omnibox::is_extension_page(url) {
            return PrivacyDecision::Allow;
        }
        match url.scheme() {
            "https" | "http" | "about" | "data" | "blob" => {}
            _ => return PrivacyDecision::Block("unsupported scheme"),
        }
        if let Some(why) = url.host_str().and_then(|host| self.block_reason(host)) {
            return PrivacyDecision::Block(why);
        }

        let mut rewritten = url.clone();
        let mut changed = false;

        if url.scheme() == "http" && !is_local(url) {
            // Port is reset: the http port rarely serves https.
            if rewritten.set_scheme("https").is_ok() && rewritten.set_port(None).is_ok() {
                changed = true;
            }
        }

        if url.query_pairs().any(|(k, _)| is_tracking_param(&k)) {
            let kept: Vec<(String, String)> = url
                .query_pairs()
                .filter(|(k, _)| !is_tracking_param(k))
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            if kept.is_empty() {
                rewritten.set_query(None);
            } else {
                rewritten.query_pairs_mut().clear().extend_pairs(kept);
            }
            changed = true;
        }

        if changed {
            PrivacyDecision::Rewrite(rewritten)
        } else {
            PrivacyDecision::Allow
        }
    }
}

/// Hosts that cannot serve a public https certificate: loopback, private/link-local IPs,
/// single-label names and `.local`/`.lan` names. They stay on plain http.
pub(crate) fn is_local(url: &Url) -> bool {
    use url::Host;
    match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Some(Host::Ipv6(ip)) => {
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
        Some(Host::Domain(d)) => {
            let d = d.trim_end_matches('.');
            !d.contains('.')
                || d.ends_with(".local")
                || d.ends_with(".lan")
                || d.ends_with(".localhost")
        }
        None => false,
    }
}

/// One blocked host, with how often it was hit and on which sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedHost {
    pub host: String,
    pub count: u32,
    /// Most recent page (host) that tried to reach it.
    pub site: String,
}

/// What was blocked during this session. Bounded, because pages are untrusted and can report
/// arbitrarily many hosts.
#[derive(Debug, Clone, Default)]
pub struct BlockLog {
    entries: Vec<BlockedHost>,
    total: u32,
    /// Per site, how often each blocked host was tried (the privacy report of a site).
    by_site: Vec<(String, Vec<(String, u32)>)>,
}

/// A site's privacy grade from what it tried this session: A (nothing) to E (a lot). A tracker
/// weighs twice an ad.
pub fn privacy_grade(trackers: u32, ads: u32) -> char {
    match trackers * 2 + ads {
        0 => 'A',
        1..=4 => 'B',
        5..=12 => 'C',
        13..=30 => 'D',
        _ => 'E',
    }
}

impl BlockLog {
    const MAX_HOSTS: usize = 200;
    const MAX_SITES: usize = 100;
    const MAX_PER_SITE: usize = 100;

    /// The hosts `site` tried to reach and were blocked, most frequent first.
    pub fn for_site(&self, site: &str) -> Vec<(String, u32)> {
        let site = site.to_lowercase();
        let mut out = self
            .by_site
            .iter()
            .find(|(s, _)| *s == site)
            .map(|(_, hosts)| hosts.clone())
            .unwrap_or_default();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    fn record_for_site(&mut self, site: &str, host: &str) {
        if site.is_empty() {
            return;
        }
        let at = match self.by_site.iter().position(|(s, _)| s == site) {
            Some(at) => at,
            None if self.by_site.len() < Self::MAX_SITES => {
                self.by_site.push((site.to_owned(), Vec::new()));
                self.by_site.len() - 1
            }
            None => return,
        };
        let hosts = &mut self.by_site[at].1;
        if let Some(entry) = hosts.iter_mut().find(|(h, _)| h == host) {
            entry.1 += 1;
        } else if hosts.len() < Self::MAX_PER_SITE {
            hosts.push((host.to_owned(), 1));
        }
    }

    /// Counts a blocked request. `host` comes from a page, so anything that is not a plain
    /// hostname is filed under "unknown".
    pub fn record(&mut self, host: &str, site: &str) {
        let valid = |h: &str| {
            !h.is_empty()
                && h.len() <= 253
                && h.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        };
        let host = host.to_lowercase();
        let host = if valid(&host) {
            host
        } else {
            "unknown".to_owned()
        };
        let site = if valid(&site.to_lowercase()) {
            site.to_lowercase()
        } else {
            String::new()
        };
        self.total += 1;
        self.record_for_site(&site, &host);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.host == host) {
            entry.count += 1;
            entry.site = site;
        } else if self.entries.len() < Self::MAX_HOSTS {
            self.entries.push(BlockedHost {
                host,
                count: 1,
                site,
            });
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    /// Most frequently blocked hosts first.
    pub fn entries(&self) -> Vec<&BlockedHost> {
        let mut out: Vec<_> = self.entries.iter().collect();
        out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.host.cmp(&b.host)));
        out
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> PrivacyPolicy {
        PrivacyPolicy::new(["doubleclick.net".to_owned()])
    }

    #[test]
    fn trackers_and_ads_are_blocked_on_their_own_switch() {
        let p = PrivacyPolicy::with_ads(["tracker.test".to_owned()], ["Ads.Test".to_owned()]);
        let ad = Url::parse("https://cdn.ads.test/banner.js").unwrap();
        let tracker = Url::parse("https://x.tracker.test/t").unwrap();
        assert_eq!(p.evaluate(&ad), PrivacyDecision::Block("ad blocked"));
        assert_eq!(p.evaluate(&tracker), PrivacyDecision::Block("tracker blocked"));
        assert_eq!(p.blocked_domains().count(), 2);
        // A clone made before the switch follows it (views that already exist).
        let clone = p.clone();
        p.set_ad_blocking(false);
        assert_eq!(clone.evaluate(&ad), PrivacyDecision::Allow);
        assert!(clone.is_blocked_host("tracker.test"));
        assert_eq!(clone.blocked_domains().collect::<Vec<_>>(), ["tracker.test"]);
        p.set_blocking(false);
        assert_eq!(clone.blocked_domains().count(), 0);
    }

    #[test]
    fn the_report_of_a_site_counts_only_what_this_site_tried() {
        let mut log = BlockLog::default();
        log.record("tracker.test", "news.test");
        log.record("tracker.test", "news.test");
        log.record("ads.test", "news.test");
        log.record("tracker.test", "shop.test");
        log.record("x.test", "not a site!");
        assert_eq!(log.for_site("News.Test"), [("tracker.test".to_owned(), 2), ("ads.test".to_owned(), 1)]);
        assert_eq!(log.for_site("shop.test"), [("tracker.test".to_owned(), 1)]);
        assert!(log.for_site("other.test").is_empty());
        assert_eq!(log.total(), 5);
    }

    #[test]
    fn the_privacy_grade_goes_from_a_to_e() {
        assert_eq!(privacy_grade(0, 0), 'A');
        assert_eq!(privacy_grade(1, 2), 'B');
        assert_eq!(privacy_grade(3, 4), 'C');
        assert_eq!(privacy_grade(10, 5), 'D');
        assert_eq!(privacy_grade(20, 0), 'E');
    }

    #[test]
    fn shields_are_on_by_default_and_shared_by_clones() {
        let p = policy();
        assert_eq!(p.shields(), Shields::default());
        let clone = p.clone();
        let some = Shields {
            hide_ads: false,
            video_ads: true,
            cookie_banners: false,
        };
        p.set_shields(some);
        assert_eq!(clone.shields(), some);
    }

    #[test]
    fn the_blocker_can_be_switched_off_for_every_clone() {
        let policy = policy();
        let clone = policy.clone();
        let url = Url::parse("https://ads.doubleclick.net/x").unwrap();
        assert!(matches!(clone.evaluate(&url), PrivacyDecision::Block(_)));
        policy.set_blocking(false);
        assert!(!clone
            .evaluate(&url)
            .eq(&PrivacyDecision::Block("tracker blocked")));
        assert_eq!(clone.blocked_domains().count(), 0);
        let http = Url::parse("http://example.com/?utm_x=1").unwrap();
        assert!(matches!(clone.evaluate(&http), PrivacyDecision::Rewrite(_)));
    }

    #[test]
    fn upgrades_http_and_strips_trackers() {
        let url = Url::parse("http://example.com/a?utm_source=x&id=3&fbclid=z").unwrap();
        match policy().evaluate(&url) {
            PrivacyDecision::Rewrite(u) => assert_eq!(u.as_str(), "https://example.com/a?id=3"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn blocks_subdomains_of_blocked_domains() {
        let url = Url::parse("https://ad.doubleclick.net/x").unwrap();
        assert!(matches!(policy().evaluate(&url), PrivacyDecision::Block(_)));
    }

    #[test]
    fn a_listed_suffix_never_blocks_a_whole_country() {
        // Only domains with a dot are compared: a bare `fr` (or `com`) never matches.
        let p = PrivacyPolicy::with_ads(["fr".to_owned()], ["com".to_owned()]);
        for url in ["https://nas.example.fr/web/main.js", "https://example.com/"] {
            let url = Url::parse(url).unwrap();
            assert_eq!(p.evaluate(&url), PrivacyDecision::Allow, "{url}");
        }
        // A listed domain blocks itself and below, never its parent nor a look-alike.
        let p = PrivacyPolicy::with_ads(Vec::new(), ["ads.example.fr".to_owned()]);
        assert!(p.is_blocked_host("x.ads.example.fr"));
        assert!(!p.is_blocked_host("example.fr"));
        assert!(!p.is_blocked_host("nas.example.fr"));
        assert!(!p.is_blocked_host("myads.example.fr"));
    }

    #[test]
    fn localhost_stays_http() {
        let url = Url::parse("http://localhost:3000/").unwrap();
        assert_eq!(policy().evaluate(&url), PrivacyDecision::Allow);
    }

    #[test]
    fn lan_hosts_stay_http_with_their_port() {
        for raw in [
            "http://192.168.1.90:7878/",
            "http://10.0.0.2:8080/x",
            "http://nas:5000/",
            "http://printer.local/",
        ] {
            let url = Url::parse(raw).unwrap();
            assert_eq!(policy().evaluate(&url), PrivacyDecision::Allow, "{raw}");
        }
        let public = Url::parse("http://8.8.8.8:80/").unwrap();
        assert!(matches!(
            policy().evaluate(&public),
            PrivacyDecision::Rewrite(_)
        ));
    }

    #[test]
    fn rejects_dangerous_schemes() {
        let url = Url::parse("file:///etc/passwd").unwrap();
        assert!(matches!(policy().evaluate(&url), PrivacyDecision::Block(_)));
    }

    #[test]
    fn block_log_groups_by_host_and_rejects_garbage() {
        let mut log = BlockLog::default();
        log.record("Ads.Example.com", "news.test");
        log.record("ads.example.com", "blog.test");
        log.record("cdn.test", "blog.test");
        log.record("<script>", "blog.test");
        assert_eq!(log.total(), 4);
        let e = log.entries();
        assert_eq!(
            (e[0].host.as_str(), e[0].count, e[0].site.as_str()),
            ("ads.example.com", 2, "blog.test")
        );
        assert!(e.iter().any(|x| x.host == "unknown"));
        log.clear();
        assert_eq!(log.total(), 0);
    }
}

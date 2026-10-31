use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub enum PrivacyDecision {
    Allow,
    /// Navigate to this rewritten URL instead (https upgrade, tracking params stripped).
    Rewrite(Url),
    Block(&'static str),
}

/// Decides whether and how a navigation may proceed.
#[derive(Debug, Clone, Default)]
pub struct PrivacyPolicy {
    blocked_domains: HashSet<String>,
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
        Self {
            blocked_domains: blocked_domains
                .into_iter()
                .map(|d| d.to_lowercase())
                .collect(),
        }
    }

    /// The raw blocklist, for engines that must enforce it inside the page.
    pub fn blocked_domains(&self) -> impl Iterator<Item = &str> {
        self.blocked_domains.iter().map(String::as_str)
    }

    pub fn is_blocked_host(&self, host: &str) -> bool {
        let host = host.to_lowercase();
        let mut candidate = host.as_str();
        loop {
            if self.blocked_domains.contains(candidate) {
                return true;
            }
            match candidate.split_once('.') {
                Some((_, rest)) if rest.contains('.') => candidate = rest,
                _ => return false,
            }
        }
    }

    pub fn evaluate(&self, url: &Url) -> PrivacyDecision {
        match url.scheme() {
            "https" | "http" | "about" | "data" | "blob" => {}
            _ => return PrivacyDecision::Block("unsupported scheme"),
        }
        if let Some(host) = url.host_str() {
            if self.is_blocked_host(host) {
                return PrivacyDecision::Block("tracker blocked");
            }
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

fn is_local(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("[::1]")
    )
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
}

impl BlockLog {
    const MAX_HOSTS: usize = 200;

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
    fn localhost_stays_http() {
        let url = Url::parse("http://localhost:3000/").unwrap();
        assert_eq!(policy().evaluate(&url), PrivacyDecision::Allow);
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

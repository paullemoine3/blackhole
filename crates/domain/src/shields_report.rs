//! The Shields report of the Library (0.8.28): who is behind a blocked host and what it does,
//! the companies a site sends its visitors to, and the time the blocked requests did not cost.
//! Pure rules; the counts come from `BlockLog`.

use crate::trackers::base_domain;

/// What a blocked host does, as far as the table below knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Audience measurement, analytics, session recording.
    Analytics,
    /// Ad networks and exchanges.
    Ads,
    /// Social networks' pixels and widgets.
    Social,
    /// Anything else (a listed or learned tracker the table does not name).
    Tracker,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Analytics => "analytics",
            Kind::Ads => "ads",
            Kind::Social => "social",
            Kind::Tracker => "tracker",
        }
    }
}

/// The company behind a host and what the host does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    pub company: &'static str,
    pub kind: Kind,
}

/// Registrable domains of the most common trackers, with their owner. A host matches its domain
/// or any subdomain of it. Kept short: an unknown host is grouped under its own base domain.
const OWNERS: &[(&str, &str, Kind)] = &[
    ("google-analytics.com", "Google", Kind::Analytics),
    ("googletagmanager.com", "Google", Kind::Analytics),
    ("googletagservices.com", "Google", Kind::Ads),
    ("doubleclick.net", "Google", Kind::Ads),
    ("googlesyndication.com", "Google", Kind::Ads),
    ("googleadservices.com", "Google", Kind::Ads),
    ("2mdn.net", "Google", Kind::Ads),
    ("facebook.net", "Meta", Kind::Social),
    ("facebook.com", "Meta", Kind::Social),
    ("instagram.com", "Meta", Kind::Social),
    ("scorecardresearch.com", "Comscore", Kind::Analytics),
    ("criteo.com", "Criteo", Kind::Ads),
    ("criteo.net", "Criteo", Kind::Ads),
    ("amazon-adsystem.com", "Amazon", Kind::Ads),
    ("adnxs.com", "Microsoft", Kind::Ads),
    ("bat.bing.com", "Microsoft", Kind::Ads),
    ("clarity.ms", "Microsoft", Kind::Analytics),
    ("linkedin.com", "LinkedIn", Kind::Social),
    ("licdn.com", "LinkedIn", Kind::Social),
    ("ads-twitter.com", "X", Kind::Ads),
    ("twitter.com", "X", Kind::Social),
    ("tiktok.com", "TikTok", Kind::Social),
    ("snapchat.com", "Snap", Kind::Ads),
    ("pinterest.com", "Pinterest", Kind::Social),
    ("taboola.com", "Taboola", Kind::Ads),
    ("outbrain.com", "Outbrain", Kind::Ads),
    ("teads.tv", "Teads", Kind::Ads),
    ("smartadserver.com", "Equativ", Kind::Ads),
    ("rubiconproject.com", "Magnite", Kind::Ads),
    ("pubmatic.com", "PubMatic", Kind::Ads),
    ("openx.net", "OpenX", Kind::Ads),
    ("quantserve.com", "Quantcast", Kind::Ads),
    ("hotjar.com", "Hotjar", Kind::Analytics),
    ("mixpanel.com", "Mixpanel", Kind::Analytics),
    ("segment.io", "Segment", Kind::Analytics),
    ("chartbeat.com", "Chartbeat", Kind::Analytics),
    ("chartbeat.net", "Chartbeat", Kind::Analytics),
    ("xiti.com", "Piano", Kind::Analytics),
    ("ati-host.net", "Piano", Kind::Analytics),
    ("omtrdc.net", "Adobe", Kind::Analytics),
    ("demdex.net", "Adobe", Kind::Ads),
    ("adobedtm.com", "Adobe", Kind::Analytics),
    ("nr-data.net", "New Relic", Kind::Analytics),
    ("hs-analytics.net", "HubSpot", Kind::Analytics),
    ("yandex.ru", "Yandex", Kind::Analytics),
    ("mc.yandex.com", "Yandex", Kind::Analytics),
];

/// The owner of `host`, if the table knows it.
pub fn owner_of(host: &str) -> Option<Owner> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    OWNERS
        .iter()
        .find(|(domain, _, _)| {
            host == *domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
        })
        .map(|(_, company, kind)| Owner { company, kind: *kind })
}

/// The companies behind `hosts` (blocked host, times), most requests first; an unknown host
/// counts for its base domain.
pub fn companies(hosts: &[(String, u32)]) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for (host, n) in hosts {
        let name = owner_of(host).map_or_else(|| base_domain(host), |o| o.company.to_owned());
        match out.iter_mut().find(|(c, _)| *c == name) {
            Some(entry) => entry.1 += n,
            None => out.push((name, *n)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// What a blocked request is reckoned to cost when it goes through: the connection, the
/// download and the script it would have run. An estimate, shown as such ("≈").
pub const MS_PER_REQUEST: u32 = 50;

/// Seconds the blocked requests did not cost, rounded down.
pub fn time_saved_secs(blocked: u32) -> u32 {
    blocked.saturating_mul(MS_PER_REQUEST) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_belongs_to_its_domain_and_subdomains_only() {
        assert_eq!(owner_of("www.google-analytics.com").map(|o| o.company), Some("Google"));
        assert_eq!(owner_of("doubleclick.net").map(|o| o.kind), Some(Kind::Ads));
        assert_eq!(owner_of("connect.facebook.net").map(|o| o.kind), Some(Kind::Social));
        assert_eq!(owner_of("SB.ScorecardResearch.com.").map(|o| o.company), Some("Comscore"));
        // Ends with the name but is another domain.
        assert_eq!(owner_of("notdoubleclick.net"), None);
        assert_eq!(owner_of("bing.com"), None, "only bing's ad host is listed");
        assert_eq!(owner_of("tracker.example"), None);
    }

    #[test]
    fn companies_add_up_their_hosts_and_unknown_ones_keep_their_domain() {
        let hosts = vec![
            ("www.google-analytics.com".to_owned(), 9),
            ("stats.g.doubleclick.net".to_owned(), 7),
            ("connect.facebook.net".to_owned(), 5),
            ("a.px.spy.example".to_owned(), 3),
            ("b.px.spy.example".to_owned(), 2),
        ];
        assert_eq!(
            companies(&hosts),
            vec![
                ("Google".to_owned(), 16),
                ("Meta".to_owned(), 5),
                ("spy.example".to_owned(), 5),
            ]
        );
        assert!(companies(&[]).is_empty());
    }

    #[test]
    fn the_time_saved_is_an_estimate_per_request() {
        assert_eq!(time_saved_secs(0), 0);
        assert_eq!(time_saved_secs(19), 0);
        assert_eq!(time_saved_secs(412), 20);
        assert_eq!(time_saved_secs(u32::MAX), u32::MAX / 1000);
    }
}

//! Extension sync between paired devices (0.8.25), beside the tab and history syncs: what one
//! device has installed is offered to the other. Additions only: removing an extension on one
//! device never removes it elsewhere. Pure rules; the wire format is in
//! `blackhole_infrastructure::ext_sync`.

use std::collections::HashSet;

/// Where an offered extension comes from, which says how it is installed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Reviewed and signed (`market/market.json`): installed without a question.
    Market,
    /// Added from a GitHub repository (`owner`, `repo`): its latest release for this system,
    /// installed on the person's word.
    Github { owner: String, repo: String },
    /// Added from a link (an https address): installed on the person's word.
    Link { url: String },
}

/// An extension installed on the other device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub id: String,
    pub name: String,
    pub source: Source,
}

/// What to do with what a device offers.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Market extensions to install now (ids).
    pub install: Vec<String>,
    /// Unreviewed extensions to propose (GitHub, link).
    pub propose: Vec<Offer>,
}

/// Compares `offers` with what is here: `installed` (ids), `in_market` (a market entry with a
/// package for this system), `declined` (proposals already answered in this session). An
/// extension being installed counts as installed.
pub fn plan(offers: &[Offer], installed: &HashSet<String>, in_market: impl Fn(&str) -> bool, declined: &HashSet<String>) -> Plan {
    let mut out = Plan::default();
    let mut seen = HashSet::new();
    for offer in offers {
        if installed.contains(&offer.id) || declined.contains(&offer.id) || !seen.insert(offer.id.clone()) {
            continue;
        }
        match offer.source {
            Source::Market if in_market(&offer.id) => out.install.push(offer.id.clone()),
            Source::Market => {}
            Source::Github { .. } | Source::Link { .. } => out.propose.push(offer.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(id: &str, source: Source) -> Offer {
        Offer { id: id.into(), name: id.into(), source }
    }

    #[test]
    fn installs_the_market_ones_and_proposes_the_others() {
        let offers = [
            offer("dark-reader", Source::Market),
            offer("bitwarden", Source::Market),
            offer("not-for-this-system", Source::Market),
            offer("gh-someone-tab-tamer", Source::Github { owner: "someone".into(), repo: "tab-tamer".into() }),
            offer("url-example-com-1234abcd", Source::Link { url: "https://example.com/a.crx".into() }),
            offer("gh-other-thing", Source::Github { owner: "other".into(), repo: "thing".into() }),
            offer("dark-reader", Source::Market),
        ];
        let installed: HashSet<String> = ["bitwarden".to_owned()].into();
        let declined: HashSet<String> = ["gh-other-thing".to_owned()].into();
        let p = plan(&offers, &installed, |id| id != "not-for-this-system", &declined);
        assert_eq!(p.install, ["dark-reader"]);
        let proposed: Vec<&str> = p.propose.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(proposed, ["gh-someone-tab-tamer", "url-example-com-1234abcd"]);
    }

    #[test]
    fn nothing_when_everything_is_here() {
        let offers = [offer("dark-reader", Source::Market)];
        let installed: HashSet<String> = ["dark-reader".to_owned()].into();
        assert_eq!(plan(&offers, &installed, |_| true, &HashSet::new()), Plan::default());
    }
}

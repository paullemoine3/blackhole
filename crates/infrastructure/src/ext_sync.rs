//! Extension sync between paired devices (0.8.25): the wire format, on the transport of the tab
//! and history syncs (`handoff`, `Request::Snapshot`, encrypted for the paired device only). A
//! device asks (`extensions_pull`, a few bytes); the other answers with what it has installed
//! (`extensions`): market ids, GitHub repositories, links. A device older than 0.8.25 ignores
//! both (`"v": 2`, an unknown `kind`). Everything received is checked and bounded here; what to
//! do with it is `blackhole_domain::ext_sync::plan`.

use blackhole_domain::ext_sync::{Offer, Source};
use blackhole_domain::market as rules;
use serde::{Deserialize, Serialize};

const WIRE_VERSION: u32 = 2;
/// Extensions in one answer at most.
const MAX_ITEMS: usize = 200;
const MAX_NAME: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ItemDto {
    id: String,
    #[serde(default)]
    name: String,
    /// `market`, `github` or `link`.
    source: String,
    /// `owner/repo` for `github`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    repo: String,
    /// The package address for `link`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    url: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum WireDto {
    #[serde(rename = "extensions_pull")]
    Pull { v: u32 },
    #[serde(rename = "extensions")]
    List {
        v: u32,
        #[serde(default)]
        list: Vec<ItemDto>,
    },
}

/// An extension message from a paired device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// It asks what is installed here.
    Pull,
    /// What it has installed.
    List(Vec<Offer>),
}

pub fn pull() -> String {
    serde_json::to_string(&WireDto::Pull { v: WIRE_VERSION }).unwrap_or_default()
}

/// The answer to a pull: what is installed here.
pub fn list(offers: &[Offer]) -> String {
    let list = offers
        .iter()
        .take(MAX_ITEMS)
        .map(|o| {
            let (source, repo, url) = match &o.source {
                Source::Market => ("market", String::new(), String::new()),
                Source::Github { owner, repo } => ("github", format!("{owner}/{repo}"), String::new()),
                Source::Link { url } => ("link", String::new(), url.clone()),
            };
            ItemDto { id: o.id.clone(), name: o.name.chars().take(MAX_NAME).collect(), source: source.into(), repo, url }
        })
        .collect();
    serde_json::to_string(&WireDto::List { v: WIRE_VERSION, list }).unwrap_or_default()
}

/// Reads an extension message; `None` for anything else (a ledger, a history message). An item
/// whose id does not match its source (a GitHub or link id is derived from it) is dropped.
pub fn parse(json: &str) -> Option<Incoming> {
    match serde_json::from_str::<WireDto>(json).ok()? {
        WireDto::Pull { v } => (v == WIRE_VERSION).then_some(Incoming::Pull),
        WireDto::List { v, list } => {
            if v != WIRE_VERSION {
                return None;
            }
            let offers = list
                .into_iter()
                .take(MAX_ITEMS)
                .filter(|i| rules::valid_id(&i.id))
                .filter_map(|i| {
                    let source = match i.source.as_str() {
                        "market" if !i.id.starts_with("gh-") && !i.id.starts_with("url-") => Source::Market,
                        "github" => {
                            let (owner, repo) = rules::github_repo(&i.repo)?;
                            (rules::sideload_id(&owner, &repo) == i.id).then_some(())?;
                            Source::Github { owner, repo }
                        }
                        "link" => {
                            let url = rules::direct_package_url(&i.url)?;
                            (rules::direct_id(&url) == i.id).then_some(())?;
                            Source::Link { url: url.to_string() }
                        }
                        _ => return None,
                    };
                    let name: String = i.name.trim().chars().take(MAX_NAME).collect();
                    Some(Offer { name: if name.is_empty() { i.id.clone() } else { name }, id: i.id, source })
                })
                .collect();
            Some(Incoming::List(offers))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_travels_and_comes_back_checked() {
        let link = url::Url::parse("https://example.com/night-owl.crx").unwrap();
        let offers = vec![
            Offer { id: "dark-reader".into(), name: "Dark Reader".into(), source: Source::Market },
            Offer {
                id: rules::sideload_id("someone", "tab-tamer"),
                name: "Tab Tamer".into(),
                source: Source::Github { owner: "someone".into(), repo: "tab-tamer".into() },
            },
            Offer { id: rules::direct_id(&link), name: "Night Owl".into(), source: Source::Link { url: link.to_string() } },
        ];
        assert_eq!(parse(&list(&offers)), Some(Incoming::List(offers)));
        assert_eq!(parse(&pull()), Some(Incoming::Pull));
    }

    #[test]
    fn what_does_not_match_its_source_is_dropped() {
        let json = r#"{"kind":"extensions","v":2,"list":[
            {"id":"gh-evil-x","source":"github","repo":"someone/tab-tamer"},
            {"id":"url-a-00000000","source":"link","url":"http://example.com/a.crx"},
            {"id":"gh-someone-tab-tamer","source":"market"},
            {"id":"../etc","source":"market"},
            {"id":"ok","source":"market","name":"   "},
            {"id":"x","source":"other"}
        ]}"#;
        match parse(json) {
            Some(Incoming::List(list)) => {
                assert_eq!(list.len(), 1);
                assert_eq!((list[0].id.as_str(), list[0].name.as_str()), ("ok", "ok"));
            }
            other => panic!("{other:?}"),
        }
        // Another version, a history message, a ledger: not for this module.
        assert_eq!(parse(r#"{"kind":"extensions_pull","v":3}"#), None);
        assert_eq!(parse(r#"{"kind":"history_pull","v":2,"since":0}"#), None);
        assert_eq!(parse(r#"{"tabs":[]}"#), None);
    }
}

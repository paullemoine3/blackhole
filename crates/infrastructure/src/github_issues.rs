//! The GitHub source of the Pulsars: pull requests and issues found with the user's own token.
//! The token is kept encrypted (same key as the password vault), only ever sent to the API it was
//! given for (never after a redirection), and never in a backup nor a page.

use std::path::Path;
use std::time::Duration;

use blackhole_domain::i18n::message;
use blackhole_domain::pulsar::{self, Entry, GithubQuery};
use serde::{Deserialize, Serialize};
use url::Url;
use zeroize::Zeroize;

use crate::store::Vault;
use crate::updater::read_limited;

pub const API: &str = "https://api.github.com";
const FILE: &str = "github.bin";
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// The account connected.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Account {
    pub token: String,
    pub login: String,
}

impl Drop for Account {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

pub struct AccountStore(Vault);

impl AccountStore {
    pub fn new(data: &Path) -> Self {
        Self(Vault::named(data, FILE))
    }

    #[cfg(test)]
    fn with_key(data: &Path) -> Self {
        Self(Vault::named_with_key(data, FILE, [7; 32]))
    }

    pub fn load(&self) -> Option<Account> {
        // No file: the keychain is not even asked.
        if !self.0.exists() {
            return None;
        }
        self.0.read_json::<Account>().ok().filter(|a| !a.token.is_empty())
    }

    pub fn save(&self, account: &Account) -> Result<(), String> {
        self.0.write_json(account)
    }

    pub fn remove(&self) -> Result<(), String> {
        self.0.remove()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Failed {
    /// The token is wrong, expired or revoked.
    Refused,
    Other(String),
}

impl Failed {
    pub fn text(&self) -> String {
        match self {
            Self::Refused => message("msg.github_refused", &[]),
            Self::Other(why) => why.clone(),
        }
    }
}

/// A token as GitHub writes them: one word, so it cannot carry a header of its own.
pub fn clean_token(raw: &str) -> Option<&str> {
    let token = raw.trim();
    let ok = (20..=300).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    ok.then_some(token)
}

fn get(base: &str, path: &str, query: &[(&str, &str)], token: &str) -> Result<Vec<u8>, Failed> {
    let other = || Failed::Other(message("msg.pulsar_unreachable", &[("host", "GitHub")]));
    let token = clean_token(token).ok_or(Failed::Refused)?;
    let agent = ureq::AgentBuilder::new()
        .user_agent(concat!("BlackHole/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .redirects(0)
        .build();
    let mut req = agent
        .get(&format!("{base}{path}"))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .set("Authorization", &format!("Bearer {token}"));
    for (k, v) in query {
        req = req.query(k, v);
    }
    match req.call() {
        Ok(resp) if resp.status() == 200 => read_limited(resp, MAX_BYTES).map_err(|_| other()),
        Err(ureq::Error::Status(401, _)) => Err(Failed::Refused),
        Err(ureq::Error::Status(403 | 429, _)) => Err(Failed::Other(message("msg.github_limited", &[]))),
        // A repository the token cannot see.
        Err(ureq::Error::Status(422, _)) => Err(Failed::Other(message("msg.github_repo", &[]))),
        _ => Err(other()),
    }
}

/// Blocking: the login a token belongs to.
pub fn login(base: &str, token: &str) -> Result<String, Failed> {
    #[derive(Deserialize)]
    struct User {
        login: String,
    }
    let body = get(base, "/user", &[], token)?;
    serde_json::from_slice::<User>(&body)
        .map(|u| pulsar::clean(&u.login, 60))
        .map_err(|_| Failed::Other(message("msg.pulsar_unreachable", &[("host", "GitHub")])))
}

/// Blocking: what a query finds, most recently updated first.
pub fn search(base: &str, token: &str, query: &GithubQuery) -> Result<Vec<Entry>, Failed> {
    let q = query.search();
    let body = get(
        base,
        "/search/issues",
        &[("q", &q), ("sort", "updated"), ("order", "desc"), ("per_page", "50")],
        token,
    )?;
    parse(&body).ok_or_else(|| Failed::Other(message("msg.pulsar_unreachable", &[("host", "GitHub")])))
}

fn parse(body: &[u8]) -> Option<Vec<Entry>> {
    #[derive(Deserialize)]
    struct Found {
        items: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        html_url: String,
        title: String,
        number: u64,
        #[serde(default)]
        updated_at: String,
        #[serde(default)]
        repository_url: String,
    }
    let found: Found = serde_json::from_slice(body).ok()?;
    Some(
        found
            .items
            .into_iter()
            .filter_map(|i| {
                let at = chrono::DateTime::parse_from_rfc3339(&i.updated_at).map_or(0, |t| t.timestamp());
                let repo = i.repository_url.rsplit('/').next().unwrap_or_default();
                let mut entry = pulsar::entry(&i.html_url, &i.title, Url::parse(&i.html_url).ok()?, at)?;
                entry.note = pulsar::clean(&format!("{repo} #{}", i.number), 40);
                Some(entry)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_of_a_search_becomes_lines() {
        let body = br#"{"total_count":2,"items":[
            {"html_url":"https://github.com/o/wry/pull/12","title":"Fix <b>it</b>","number":12,"updated_at":"2026-07-01T08:00:00Z","repository_url":"https://api.github.com/repos/o/wry"},
            {"html_url":"javascript:alert(1)","title":"x","number":1}]}"#;
        let list = parse(body).unwrap();
        assert_eq!(list.len(), 1);
        let e = &list[0];
        assert_eq!((e.title.as_str(), e.note.as_str(), e.at), ("Fix <b>it</b>", "wry #12", 1_782_892_800));
        assert!(parse(b"{}").is_none());
    }

    #[test]
    fn only_a_single_word_is_a_token() {
        assert!(clean_token(" ghp_0123456789abcdefABCDEF ").is_some());
        for bad in ["", "short", "ghp_0123456789abcdef\r\nX-Evil: 1", "two words of a certain length"] {
            assert!(clean_token(bad).is_none(), "{bad}");
        }
        assert_eq!(login("http://127.0.0.1:1", "bad token"), Err(Failed::Refused), "never sent");
    }

    #[test]
    fn the_account_is_sealed_and_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::with_key(dir.path());
        assert!(store.load().is_none());
        store.save(&Account { token: "ghp_secret".into(), login: "me".into() }).unwrap();
        assert_eq!(store.load().unwrap().login, "me");
        let raw = std::fs::read(dir.path().join(FILE)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"));
        store.remove().unwrap();
        assert!(store.load().is_none());
    }
}

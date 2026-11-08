//! HTTP authentication (Basic, Digest) asked by a server: what the prompt says, which tab it
//! belongs to and when it is shown. Credentials never go through here: only the prompt.

use crate::TabId;

/// Longest realm shown: it comes from the server.
const REALM_MAX: usize = 80;

/// The realm of a challenge, ready to show. `raw` is either the realm itself (WebKit) or a
/// `WWW-Authenticate` value such as `Basic realm="Transmission", charset="UTF-8"` (WebView2).
pub fn realm_of(raw: &str) -> String {
    let realm = find_realm(raw).unwrap_or_else(|| {
        // A bare scheme (`Basic`) carries no realm; anything else is already the realm.
        let scheme_only = raw.trim().eq_ignore_ascii_case("basic")
            || raw.trim().eq_ignore_ascii_case("digest");
        if scheme_only || has_scheme(raw) {
            String::new()
        } else {
            raw.to_owned()
        }
    });
    let clean: String = realm
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() > REALM_MAX {
        let cut: String = clean.chars().take(REALM_MAX - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        clean
    }
}

fn has_scheme(raw: &str) -> bool {
    let lower = raw.trim_start().to_ascii_lowercase();
    lower.starts_with("basic ") || lower.starts_with("digest ")
}

/// The `realm` parameter of a `WWW-Authenticate` value (quoted or not, any case).
fn find_realm(header: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("realm") {
        let start = from + at;
        from = start + 5;
        // A whole parameter name: preceded by a separator, followed by `=`.
        let before = lower[..start].chars().next_back();
        if before.is_some_and(|c| !(c == ' ' || c == ',' || c == '\t')) {
            continue;
        }
        let rest = header[from..].trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        if let Some(quoted) = value.strip_prefix('"') {
            let mut out = String::new();
            let mut chars = quoted.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => out.extend(chars.next()),
                    '"' => return Some(out),
                    c => out.push(c),
                }
            }
            return Some(out);
        }
        let token = value.split([',', ' ', '\t']).next().unwrap_or_default();
        return Some(token.to_owned());
    }
    None
}

/// A prompt waiting for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub tab: TabId,
    pub host: String,
    pub realm: String,
    /// Whether the password travels encrypted (https).
    pub secure: bool,
    /// The credentials given just before for the same site were refused.
    pub retry: bool,
}

/// Challenges waiting for an answer, in arrival order. A page can raise several at once for the
/// same realm (page and sub-resources): one answer serves them all.
#[derive(Debug, Default)]
pub struct AuthPrompts {
    pending: Vec<(u64, Challenge)>,
    /// Sites (tab, host, realm) given credentials and not loaded since: a new challenge for one
    /// of them means the credentials were refused.
    tried: Vec<(TabId, String, String)>,
    next_id: u64,
}

impl AuthPrompts {
    /// Records a challenge; `retry` says the engine already knows the credentials were refused.
    pub fn push(&mut self, tab: TabId, host: &str, realm: &str, secure: bool, retry: bool) -> u64 {
        self.next_id += 1;
        let host = host.to_ascii_lowercase();
        let retry = retry || self.tried.iter().any(|t| t.0 == tab && t.1 == host && t.2 == realm);
        self.pending.push((
            self.next_id,
            Challenge {
                tab,
                host,
                realm: realm.to_owned(),
                secure,
                retry,
            },
        ));
        self.next_id
    }

    pub fn get(&self, id: u64) -> Option<&Challenge> {
        self.pending.iter().find(|(i, _)| *i == id).map(|(_, c)| c)
    }

    /// The oldest challenge of a tab on screen (a background tab waits until it is shown).
    pub fn next_for(&self, shown: &[TabId]) -> Option<(u64, &Challenge)> {
        self.pending
            .iter()
            .find(|(_, c)| shown.contains(&c.tab))
            .map(|(id, c)| (*id, c))
    }

    /// Answers a challenge: returns it with every other one of the same tab, host and realm,
    /// which get the same answer.
    pub fn answer(&mut self, id: u64, with_credentials: bool) -> Vec<u64> {
        let Some(key) = self.get(id).cloned() else {
            return Vec::new();
        };
        let mut ids = Vec::new();
        self.pending.retain(|(i, c)| {
            let same = c.tab == key.tab && c.host == key.host && c.realm == key.realm;
            if same {
                ids.push(*i);
            }
            !same
        });
        let entry = (key.tab, key.host, key.realm);
        self.tried.retain(|t| *t != entry);
        if with_credentials {
            self.tried.push(entry);
        }
        ids
    }

    /// The tab finished loading a page: credentials given before were accepted.
    pub fn loaded(&mut self, tab: TabId) {
        self.tried.retain(|t| t.0 != tab);
    }

    /// The tab is gone: its challenges are dropped (and must be cancelled).
    pub fn drop_tab(&mut self, tab: TabId) -> Vec<u64> {
        self.tried.retain(|t| t.0 != tab);
        let mut ids = Vec::new();
        self.pending.retain(|(i, c)| {
            if c.tab == tab {
                ids.push(*i);
            }
            c.tab != tab
        });
        ids
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realm_from_header_or_plain() {
        assert_eq!(realm_of(r#"Basic realm="Transmission""#), "Transmission");
        assert_eq!(realm_of(r#"Basic realm="NAS", charset="UTF-8""#), "NAS");
        assert_eq!(realm_of(r#"Digest qop="auth", REALM="Box \"2\"", nonce="x""#), "Box \"2\"");
        assert_eq!(realm_of("Basic realm=intranet"), "intranet");
        assert_eq!(realm_of("Transmission"), "Transmission");
        assert_eq!(realm_of("Basic"), "");
        assert_eq!(realm_of(r#"Basic charset="UTF-8""#), "");
        // `xrealm` is not `realm`.
        assert_eq!(realm_of(r#"Basic xrealm="a", realm="b""#), "b");
    }

    #[test]
    fn realm_is_cleaned_and_bounded() {
        assert_eq!(realm_of("Basic realm=\"a\u{7}\nb\""), "a b");
        let long = format!("Basic realm=\"{}\"", "x".repeat(300));
        let shown = realm_of(&long);
        assert_eq!(shown.chars().count(), REALM_MAX);
        assert!(shown.ends_with('…'));
    }

    #[test]
    fn background_tabs_wait_their_turn() {
        let mut p = AuthPrompts::default();
        let a = p.push(TabId(1), "nas.example", "NAS", true, false);
        let b = p.push(TabId(2), "other.example", "X", true, false);
        assert_eq!(p.next_for(&[TabId(2)]).map(|(id, _)| id), Some(b));
        assert_eq!(p.next_for(&[TabId(1), TabId(2)]).map(|(id, _)| id), Some(a));
        assert!(p.next_for(&[TabId(3)]).is_none());
    }

    #[test]
    fn one_answer_serves_the_same_realm() {
        let mut p = AuthPrompts::default();
        let a = p.push(TabId(1), "nas.example", "NAS", true, false);
        let b = p.push(TabId(1), "NAS.example", "NAS", true, false);
        let other = p.push(TabId(1), "nas.example", "Admin", true, false);
        assert_eq!(p.answer(a, true), vec![a, b]);
        assert!(p.get(other).is_some());
        assert!(p.answer(a, true).is_empty());
    }

    #[test]
    fn refused_credentials_ask_again_as_a_retry() {
        let mut p = AuthPrompts::default();
        let a = p.push(TabId(1), "nas.example", "NAS", false, false);
        assert!(!p.get(a).unwrap().retry);
        p.answer(a, true);
        let again = p.push(TabId(1), "nas.example", "NAS", false, false);
        assert!(p.get(again).unwrap().retry);
        // Cancelling forgets the attempt.
        p.answer(again, false);
        let fresh = p.push(TabId(1), "nas.example", "NAS", false, false);
        assert!(!p.get(fresh).unwrap().retry);
        // A page loaded with the credentials: they were right.
        p.answer(fresh, true);
        p.loaded(TabId(1));
        let later = p.push(TabId(1), "nas.example", "NAS", false, false);
        assert!(!p.get(later).unwrap().retry);
        // The engine may know better (macOS counts failures itself).
        let known = p.push(TabId(2), "nas.example", "NAS", true, true);
        assert!(p.get(known).unwrap().retry);
    }

    #[test]
    fn closing_a_tab_drops_its_prompts() {
        let mut p = AuthPrompts::default();
        let a = p.push(TabId(1), "nas.example", "NAS", true, false);
        let b = p.push(TabId(2), "nas.example", "NAS", true, false);
        assert_eq!(p.drop_tab(TabId(1)), vec![a]);
        assert!(p.get(b).is_some());
        assert_eq!(p.drop_tab(TabId(2)), vec![b]);
        assert!(p.is_empty());
    }
}

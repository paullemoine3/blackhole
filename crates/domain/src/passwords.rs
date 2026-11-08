//! The password wallet: saved logins, the rules for offering them to a page, saving after a
//! form submission, and merging two copies (another device, later). No I/O, no randomness:
//! ids, clocks and random bytes come from the caller.
//!
//! Every entry has a stable `id`, a modification time and the device that made the change;
//! a removal leaves a tombstone (secrets wiped) so a merge does not bring the login back.

use url::Url;
use zeroize::Zeroize;

/// Tombstones older than this are dropped: every device is expected to have synced by then.
pub const TOMBSTONE_TTL_MS: i64 = 90 * 24 * 3600 * 1000;

/// What an entry signs into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A login form on a page of `origin`.
    Web,
    /// HTTP authentication (Basic/Digest) of `origin` for this realm.
    HttpAuth { realm: String },
}

/// One saved login. The password and the notes are wiped from memory when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// `scheme://host[:port]`, see [`origin_of`].
    pub origin: String,
    pub username: String,
    pub password: String,
    pub notes: String,
    pub kind: Kind,
    /// Unix milliseconds.
    pub created: i64,
    /// Unix milliseconds of the last change (edit or removal).
    pub modified: i64,
    /// The device that made the last change: breaks ties between equal times.
    pub device: String,
    /// A removal kept for merging; its secrets are empty.
    pub deleted: bool,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.password.zeroize();
        self.notes.zeroize();
    }
}

/// Never prints the secrets (logs, panics).
impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("origin", &self.origin)
            .field("modified", &self.modified)
            .field("deleted", &self.deleted)
            .finish_non_exhaustive()
    }
}

impl Entry {
    fn newer_than(&self, other: &Entry) -> bool {
        (self.modified, &self.device) > (other.modified, &other.device)
    }

    fn wipe(&mut self) {
        self.password.zeroize();
        self.notes.zeroize();
        self.password.clear();
        self.notes.clear();
        self.username.clear();
    }
}

fn local_host(host: &str) -> bool {
    host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host.ends_with(".localhost")
}

/// The origin a page may receive saved logins for: `https://host[:port]`, or plain http on the
/// local machine only (a login sent in clear on the network is never filled).
#[must_use]
pub fn origin_of(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    match url.scheme() {
        "https" => {}
        "http" if local_host(&host) => {}
        _ => return None,
    }
    Some(url.origin().ascii_serialization())
}

/// The origin of an address kept in an import (any http(s) address, even one never filled).
#[must_use]
pub fn origin_of_saved(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

/// The host shown to the user for an origin (`example.com:8443`).
#[must_use]
pub fn display_host(origin: &str) -> String {
    Url::parse(origin)
        .ok()
        .and_then(|u| {
            let host = u.host_str()?.to_owned();
            Some(match u.port() {
                Some(p) => format!("{host}:{p}"),
                None => host,
            })
        })
        .unwrap_or_else(|| origin.to_owned())
}

/// What to propose after a login form was submitted on `origin`.
#[derive(Debug, PartialEq, Eq)]
pub enum Offer {
    /// A new login.
    Save,
    /// The same username with another password: update entry `id`.
    Update { id: String },
    /// Already saved as is, or the user said never for this site.
    Nothing,
}

/// The wallet content: entries (tombstones included) and the sites never to offer saving for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Passwords {
    pub entries: Vec<Entry>,
    /// Origins the user said "Never" for.
    pub never: Vec<String>,
}

/// The fields of a new or edited entry.
pub struct Draft<'a> {
    pub origin: &'a str,
    pub username: &'a str,
    pub password: &'a str,
    pub notes: &'a str,
    pub kind: Kind,
}

impl Passwords {
    /// Live (not removed) entries, sorted by site then username.
    #[must_use]
    pub fn live(&self) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = self.entries.iter().filter(|e| !e.deleted).collect();
        out.sort_by(|a, b| {
            display_host(&a.origin)
                .cmp(&display_host(&b.origin))
                .then_with(|| a.username.cmp(&b.username))
        });
        out
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id && !e.deleted)
    }

    /// Login-form entries for exactly `origin` (scheme, host and port).
    #[must_use]
    pub fn for_origin(&self, origin: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| {
                !e.deleted && e.kind == Kind::Web && e.origin == origin && !e.password.is_empty()
            })
            .collect()
    }

    /// Entries for an HTTP authentication challenge of `origin`: the realm's first, then
    /// the site's login-form entries.
    #[must_use]
    pub fn http_auth_candidates(&self, origin: &str, realm: &str) -> Vec<&Entry> {
        let mut out: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| !e.deleted && e.origin == origin && !e.password.is_empty())
            .collect();
        out.sort_by_key(|e| match &e.kind {
            Kind::HttpAuth { realm: r } if r == realm => 0,
            Kind::HttpAuth { .. } => 2,
            Kind::Web => 1,
        });
        out
    }

    /// What to propose for the credentials just submitted on `origin`.
    #[must_use]
    pub fn offer(&self, origin: &str, username: &str, password: &str) -> Offer {
        if password.is_empty() || self.never.iter().any(|n| n == origin) {
            return Offer::Nothing;
        }
        match self
            .entries
            .iter()
            .find(|e| !e.deleted && e.kind == Kind::Web && e.origin == origin && e.username == username)
        {
            Some(e) if e.password == password => Offer::Nothing,
            Some(e) => Offer::Update { id: e.id.clone() },
            None => Offer::Save,
        }
    }

    /// Adds an entry with the fresh `id`.
    pub fn add(&mut self, id: String, draft: &Draft<'_>, now: i64, device: &str) {
        self.entries.push(Entry {
            id,
            origin: draft.origin.to_owned(),
            username: draft.username.to_owned(),
            password: draft.password.to_owned(),
            notes: draft.notes.to_owned(),
            kind: draft.kind.clone(),
            created: now,
            modified: now,
            device: device.to_owned(),
            deleted: false,
        });
    }

    /// Replaces the fields of entry `id`; false when it does not exist (or was removed).
    pub fn edit(&mut self, id: &str, draft: &Draft<'_>, now: i64, device: &str) -> bool {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id && !e.deleted) else {
            return false;
        };
        e.origin = draft.origin.to_owned();
        e.username = draft.username.to_owned();
        e.password.zeroize();
        e.password = draft.password.to_owned();
        e.notes.zeroize();
        e.notes = draft.notes.to_owned();
        e.kind = draft.kind.clone();
        e.modified = now.max(e.modified + 1);
        e.device = device.to_owned();
        true
    }

    /// Only the password of entry `id` changes (save after submission).
    pub fn set_password(&mut self, id: &str, password: &str, now: i64, device: &str) -> bool {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id && !e.deleted) else {
            return false;
        };
        e.password.zeroize();
        e.password = password.to_owned();
        e.modified = now.max(e.modified + 1);
        e.device = device.to_owned();
        true
    }

    /// Turns entry `id` into a tombstone and returns the removed copy (for Undo).
    pub fn remove(&mut self, id: &str, now: i64, device: &str) -> Option<Entry> {
        let e = self.entries.iter_mut().find(|e| e.id == id && !e.deleted)?;
        let copy = e.clone();
        e.wipe();
        e.deleted = true;
        e.modified = now.max(e.modified + 1);
        e.device = device.to_owned();
        Some(copy)
    }

    /// Undo of [`Passwords::remove`]: the entry comes back as a newer change.
    pub fn restore(&mut self, mut entry: Entry, now: i64, device: &str) {
        entry.modified = now.max(entry.modified + 1);
        entry.device = device.to_owned();
        entry.deleted = false;
        match self.entries.iter_mut().find(|e| e.id == entry.id) {
            Some(slot) => *slot = entry,
            None => self.entries.push(entry),
        }
    }

    pub fn never_for(&mut self, origin: &str) {
        if !self.never.iter().any(|n| n == origin) {
            self.never.push(origin.to_owned());
        }
    }

    /// Imported logins (`(address, username, password)`): the ones not already saved for the
    /// same origin and username become entries, ids taken from `ids`. Returns how many.
    pub fn import(
        &mut self,
        logins: &[(&str, &str, &str)],
        mut ids: impl FnMut() -> String,
        now: i64,
        device: &str,
    ) -> usize {
        let mut added = 0;
        for &(address, username, password) in logins {
            let Some(origin) = origin_of_saved(address) else {
                continue;
            };
            let known = self.entries.iter().any(|e| {
                !e.deleted && e.kind == Kind::Web && e.origin == origin && e.username == username
            });
            if known || password.is_empty() {
                continue;
            }
            let draft = Draft {
                origin: &origin,
                username,
                password,
                notes: "",
                kind: Kind::Web,
            };
            self.add(ids(), &draft, now, device);
            added += 1;
        }
        added
    }

    /// Merges another copy (another device): for each id the latest change wins, removals
    /// included. Returns true when something changed here.
    pub fn merge(&mut self, remote: Vec<Entry>) -> bool {
        let mut changed = false;
        for r in remote {
            match self.entries.iter_mut().find(|e| e.id == r.id) {
                Some(local) if r.newer_than(local) => {
                    *local = r;
                    changed = true;
                }
                Some(_) => {}
                None => {
                    self.entries.push(r);
                    changed = true;
                }
            }
        }
        changed
    }

    /// Drops the tombstones older than [`TOMBSTONE_TTL_MS`].
    pub fn purge_tombstones(&mut self, now: i64) {
        self.entries
            .retain(|e| !e.deleted || now - e.modified < TOMBSTONE_TTL_MS);
    }
}

/// Character classes of a generated password.
#[derive(Clone, Copy, Debug)]
pub struct Recipe {
    pub length: usize,
    pub digits: bool,
    pub symbols: bool,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            length: 20,
            digits: true,
            symbols: true,
        }
    }
}

const LOWER: &str = "abcdefghijkmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ";
const DIGITS: &str = "23456789";
const SYMBOLS: &str = "!#$%&*+-=?@_~";

/// A random password from `random` (uniform bytes): no look-alike characters, at least one of
/// each chosen class, unbiased (bytes beyond the last whole multiple are skipped).
pub fn generate(recipe: Recipe, mut random: impl FnMut() -> u8) -> String {
    let length = recipe.length.clamp(8, 128);
    let mut classes = vec![LOWER, UPPER];
    if recipe.digits {
        classes.push(DIGITS);
    }
    if recipe.symbols {
        classes.push(SYMBOLS);
    }
    let alphabet: Vec<char> = classes.concat().chars().collect();
    let limit = 256 - 256 % alphabet.len();
    loop {
        let mut out = String::with_capacity(length);
        while out.chars().count() < length {
            let b = usize::from(random());
            if b < limit {
                out.push(alphabet[b % alphabet.len()]);
            }
        }
        if classes.iter().all(|c| out.chars().any(|ch| c.contains(ch))) {
            return out;
        }
        out.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn draft<'a>(origin: &'a str, username: &'a str, password: &'a str) -> Draft<'a> {
        Draft {
            origin,
            username,
            password,
            notes: "",
            kind: Kind::Web,
        }
    }

    fn wallet() -> Passwords {
        let mut p = Passwords::default();
        p.add("a".into(), &draft("https://example.test", "alice", "pw-test-1"), 10, "dev1");
        p
    }

    #[test]
    fn origin_is_exact_and_https_only() {
        assert_eq!(
            origin_of(&url("https://Example.test/login?x=1")).as_deref(),
            Some("https://example.test")
        );
        assert_eq!(
            origin_of(&url("https://example.test:8443/")).as_deref(),
            Some("https://example.test:8443")
        );
        assert_eq!(origin_of(&url("http://example.test/")), None);
        assert_eq!(
            origin_of(&url("http://localhost:3000/a")).as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(origin_of(&url("file:///c:/x.html")), None);
        assert_eq!(display_host("https://example.test:8443"), "example.test:8443");
    }

    #[test]
    fn matching_needs_the_exact_origin() {
        let p = wallet();
        assert_eq!(p.for_origin("https://example.test").len(), 1);
        assert!(p.for_origin("https://login.example.test").is_empty());
        assert!(p.for_origin("http://example.test").is_empty());
        assert!(p.for_origin("https://example.test:444").is_empty());
        assert!(p.for_origin("https://example.test.evil.test").is_empty());
    }

    #[test]
    fn offer_after_submission() {
        let mut p = wallet();
        assert_eq!(p.offer("https://example.test", "alice", "pw-test-1"), Offer::Nothing);
        assert_eq!(
            p.offer("https://example.test", "alice", "pw-test-2"),
            Offer::Update { id: "a".into() }
        );
        assert_eq!(p.offer("https://example.test", "bob", "pw-test-3"), Offer::Save);
        assert_eq!(p.offer("https://example.test", "bob", ""), Offer::Nothing);
        p.never_for("https://other.test");
        assert_eq!(p.offer("https://other.test", "bob", "x"), Offer::Nothing);
    }

    #[test]
    fn remove_leaves_a_wiped_tombstone_and_undo_restores() {
        let mut p = wallet();
        let removed = p.remove("a", 20, "dev1").unwrap();
        let t = &p.entries[0];
        assert!(t.deleted && t.password.is_empty() && t.username.is_empty());
        assert!(p.live().is_empty() && p.get("a").is_none());
        p.restore(removed, 30, "dev1");
        assert_eq!(p.get("a").unwrap().password, "pw-test-1");
        assert_eq!(p.entries.len(), 1);
    }

    #[test]
    fn merge_keeps_the_latest_change_per_id() {
        let mut here = wallet();
        let mut there = here.clone();
        // Edited there later, removed here earlier: the edit wins.
        here.remove("a", 20, "dev1");
        there.edit("a", &draft("https://example.test", "alice", "pw-new"), 25, "dev2");
        there.add("b".into(), &draft("https://b.test", "u", "p"), 26, "dev2");
        assert!(here.merge(there.entries.clone()));
        assert_eq!(here.get("a").unwrap().password, "pw-new");
        assert!(here.get("b").is_some());
        // Merging the same copy again changes nothing (idempotent).
        assert!(!here.merge(there.entries.clone()));
        // A later removal there wins over the edit here.
        there.remove("b", 40, "dev2");
        here.merge(there.entries.clone());
        assert!(here.get("b").is_none());
        // Equal times: the device id decides, the same way on both sides.
        let mut x = wallet();
        let mut y = wallet();
        x.set_password("a", "from-x", 50, "dev1");
        y.set_password("a", "from-y", 50, "dev2");
        let (xs, ys) = (x.entries.clone(), y.entries.clone());
        x.merge(ys);
        y.merge(xs);
        assert_eq!(x.get("a").unwrap().password, "from-y");
        assert_eq!(y.get("a").unwrap().password, "from-y");
    }

    #[test]
    fn tombstones_expire() {
        let mut p = wallet();
        p.remove("a", 1000, "dev1");
        p.purge_tombstones(1000 + TOMBSTONE_TTL_MS - 1);
        assert_eq!(p.entries.len(), 1);
        p.purge_tombstones(1000 + TOMBSTONE_TTL_MS);
        assert!(p.entries.is_empty());
    }

    #[test]
    fn import_skips_known_and_invalid() {
        let mut p = wallet();
        let mut n = 0;
        let added = p.import(
            &[
                ("https://example.test/login", "alice", "pw-test-1"),
                ("https://example.test/", "bob", "pw-test-2"),
                ("android://abc@com.app/", "x", "y"),
                ("https://c.test", "c", ""),
            ],
            || {
                n += 1;
                format!("id{n}")
            },
            5,
            "dev1",
        );
        assert_eq!(added, 1);
        assert_eq!(p.for_origin("https://example.test").len(), 2);
    }

    #[test]
    fn http_auth_prefers_the_realm() {
        let mut p = wallet();
        let auth = Draft {
            kind: Kind::HttpAuth { realm: "admin".into() },
            ..draft("https://example.test", "root", "pw-test-r")
        };
        p.add("r".into(), &auth, 11, "dev1");
        let c = p.http_auth_candidates("https://example.test", "admin");
        assert_eq!(c[0].id, "r");
        assert_eq!(c.len(), 2);
        // An HTTP auth entry is not offered in login forms.
        assert_eq!(p.for_origin("https://example.test").len(), 1);
    }

    #[test]
    fn generated_passwords_follow_the_recipe() {
        let mut state = 7u32;
        let mut rng = || {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) as u8
        };
        let pw = generate(Recipe::default(), &mut rng);
        assert_eq!(pw.chars().count(), 20);
        assert!(pw.chars().any(|c| DIGITS.contains(c)));
        assert!(pw.chars().any(|c| SYMBOLS.contains(c)));
        let plain = generate(Recipe { length: 4, digits: false, symbols: false }, &mut rng);
        assert_eq!(plain.len(), 8);
        assert!(plain.chars().all(char::is_alphabetic));
        assert!(!pw.contains('l') && !pw.contains('O') && !pw.contains('0'));
    }

    #[test]
    fn debug_hides_secrets() {
        let p = wallet();
        let shown = format!("{p:?}");
        assert!(!shown.contains("pw-test-1") && !shown.contains("alice"));
    }
}

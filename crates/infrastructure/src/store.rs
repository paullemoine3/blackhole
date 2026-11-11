//! Local storage of imported bookmarks (plain JSON) and logins (encrypted vault).
//!
//! The vault is a ChaCha20-Poly1305 file; its random 256-bit key lives in the OS credential store
//! (macOS Keychain / Windows Credential Manager), never next to the data.

use crate::import::{Bookmark, Login};
use blackhole_domain::{Easel, EaselItem, EaselItemKind, Easels};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

fn create_parent(path: &std::path::Path) -> Result<(), String> {
    path.parent()
        .map_or(Ok(()), fs::create_dir_all)
        .map_err(|e| e.to_string())
}

pub struct BookmarkStore {
    path: PathBuf,
}

impl BookmarkStore {
    pub fn new(data: &std::path::Path) -> Self {
        Self {
            path: data.join("bookmarks.json"),
        }
    }

    pub fn load(&self) -> Vec<Bookmark> {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Adds the bookmarks whose URL is not known yet and returns how many were new.
    pub fn merge(&self, incoming: Vec<Bookmark>) -> Result<usize, String> {
        let mut all = self.load();
        let before = all.len();
        for b in incoming {
            if !all.iter().any(|o| o.url == b.url && o.space == b.space) {
                all.push(b);
            }
        }
        let tmp = self.path.with_extension("tmp");
        create_parent(&self.path)?;
        fs::write(&tmp, serde_json::to_vec(&all).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        Ok(all.len() - before)
    }
}

#[derive(Serialize, Deserialize)]
struct ItemDto {
    id: u64,
    kind: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    selector: String,
    #[serde(default)]
    color: String,
}

#[derive(Serialize, Deserialize)]
struct EaselDto {
    id: u64,
    name: String,
    #[serde(default)]
    folder: Option<u64>,
    #[serde(default)]
    space: Option<u64>,
    #[serde(default)]
    items: Vec<ItemDto>,
}

/// Easels as one JSON file (`easels.json`), written atomically.
pub struct EaselStore {
    path: PathBuf,
}

impl EaselStore {
    pub fn new(data: &std::path::Path) -> Self {
        Self {
            path: data.join("easels.json"),
        }
    }

    pub fn load(&self) -> Easels {
        let dtos: Vec<EaselDto> = fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Easels::from_stored(
            dtos.into_iter()
                .map(|e| Easel {
                    id: e.id,
                    name: e.name,
                    folder: e.folder,
                    space: e.space,
                    items: e
                        .items
                        .into_iter()
                        .filter_map(|i| {
                            Some(EaselItem {
                                id: i.id,
                                kind: EaselItemKind::from_id(&i.kind)?,
                                x: i.x,
                                y: i.y,
                                w: i.w,
                                h: i.h,
                                text: i.text,
                                title: i.title,
                                url: i.url,
                                selector: i.selector,
                                color: i.color,
                            })
                        })
                        .collect(),
                })
                .collect(),
        )
    }

    pub fn save(&self, easels: &Easels) -> Result<(), String> {
        let dtos: Vec<EaselDto> = easels
            .all()
            .iter()
            .map(|e| EaselDto {
                id: e.id,
                name: e.name.clone(),
                folder: e.folder,
                space: e.space,
                items: e
                    .items
                    .iter()
                    .map(|i| ItemDto {
                        id: i.id,
                        kind: i.kind.id().to_owned(),
                        x: i.x,
                        y: i.y,
                        w: i.w,
                        h: i.h,
                        text: i.text.clone(),
                        title: i.title.clone(),
                        url: i.url.clone(),
                        selector: i.selector.clone(),
                        color: i.color.clone(),
                    })
                    .collect(),
            })
            .collect();
        let tmp = self.path.with_extension("tmp");
        create_parent(&self.path)?;
        fs::write(&tmp, serde_json::to_vec(&dtos).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

pub struct Vault {
    path: PathBuf,
    key: Option<[u8; 32]>,
}

impl Vault {
    /// Vault whose key is kept in the OS credential store.
    pub fn new(data: &std::path::Path) -> Self {
        Self {
            path: data.join("vault.bin"),
            key: None,
        }
    }

    #[cfg(test)]
    fn with_key(data: &std::path::Path, key: [u8; 32]) -> Self {
        Self::named_with_key(data, "vault.bin", key)
    }

    /// A vault file sealed with a key the platform keeps (Android: the Android Keystore).
    pub fn named_with_key(data: &std::path::Path, file: &str, key: [u8; 32]) -> Self {
        Self {
            path: data.join(file),
            key: Some(key),
        }
    }

    fn key(&self) -> Result<[u8; 32], String> {
        match self.key {
            Some(k) => Ok(k),
            None => Self::shared_key(),
        }
    }

    /// The vault's key, kept by the system's credential store (made on first use). For stores
    /// that seal many files with it: read once, then `named_with_key`.
    pub fn shared_key() -> Result<[u8; 32], String> {
        let entry = keyring::Entry::new("BlackHole", "vault-key").map_err(|e| e.to_string())?;
        match entry.get_secret() {
            Ok(s) => s
                .try_into()
                .map_err(|_| "clé du coffre invalide".to_string()),
            Err(keyring::Error::NoEntry) => {
                let k: [u8; 32] = ChaCha20Poly1305::generate_key(&mut OsRng).into();
                entry.set_secret(&k).map_err(|e| e.to_string())?;
                Ok(k)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Another file sealed with the same key (e.g. the secret calendar addresses).
    pub fn named(data: &std::path::Path, file: &str) -> Self {
        Self {
            path: data.join(file),
            key: None,
        }
    }

    /// The decrypted JSON content; the default value while the file does not exist.
    pub fn read_json<T: serde::de::DeserializeOwned + Default>(&self) -> Result<T, String> {
        let Ok(raw) = fs::read(&self.path) else {
            return Ok(T::default());
        };
        if raw.len() < 12 {
            return Err("coffre corrompu".into());
        }
        let key = self.key()?;
        let plain = ChaCha20Poly1305::new(Key::from_slice(&key))
            .decrypt(Nonce::from_slice(&raw[..12]), &raw[12..])
            .map_err(|_| "coffre illisible (clé ou fichier modifié)".to_string())?;
        serde_json::from_slice(&plain).map_err(|e| e.to_string())
    }

    /// Encrypts `value` as JSON (fresh nonce) and replaces the file atomically.
    pub fn write_json<T: serde::Serialize>(&self, value: &T) -> Result<(), String> {
        let key = self.key()?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut blob = nonce.to_vec();
        let plain = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        blob.extend(
            cipher
                .encrypt(&nonce, plain.as_ref())
                .map_err(|e| e.to_string())?,
        );
        let tmp = self.path.with_extension("tmp");
        create_parent(&self.path)?;
        fs::write(&tmp, blob).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn load(&self) -> Result<Vec<Login>, String> {
        self.read_json()
    }

    /// Adds logins not already stored (same site + username) and returns how many were new.
    pub fn merge(&self, incoming: Vec<Login>) -> Result<usize, String> {
        let mut all = self.load()?;
        let before = all.len();
        for l in incoming {
            if !all
                .iter()
                .any(|o| o.url == l.url && o.username == l.username)
            {
                all.push(l);
            }
        }
        self.write_json(&all)?;
        Ok(all.len() - before)
    }

    pub fn count(&self) -> usize {
        self.load().map_or(0, |l| l.len())
    }

    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Deletes the file (its logins moved to the password wallet, see `wallet::open_at_start`).
    pub fn remove(&self) -> Result<(), String> {
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    /// Logins saved for the site `host` (see [`same_site`]), for autofill.
    pub fn for_host(&self, host: &str) -> Result<Vec<Login>, String> {
        Ok(self
            .load()?
            .into_iter()
            .filter(|l| !l.password.is_empty() && same_site(&l.url, host))
            .collect())
    }
}

/// True when a saved login URL belongs to `host`: same host ignoring `www.`, or one is a
/// subdomain of the other (login.example.com <-> example.com). Never matches a bare TLD.
pub fn same_site(saved_url: &str, host: &str) -> bool {
    let Some(saved) = url::Url::parse(saved_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
    else {
        return false;
    };
    let norm = |h: &str| h.strip_prefix("www.").unwrap_or(h).to_owned();
    let (a, b) = (norm(&saved), norm(&host.to_ascii_lowercase()));
    let sub = |long: &str, short: &str| long.ends_with(&format!(".{short}")) && short.contains('.');
    a == b || sub(&a, &b) || sub(&b, &a)
}

/// A file saved from a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DownloadRec {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub url: String,
    /// Unix seconds.
    pub at: u64,
    pub done: bool,
    pub ok: bool,
}

/// A tab that was closed, kept so it can be found and reopened later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArchivedTab {
    pub title: String,
    pub url: String,
    pub space: u64,
    /// Unix seconds.
    pub at: u64,
}

const LIBRARY_CAP: usize = 500;

fn load_list<T: for<'a> Deserialize<'a>>(path: &std::path::Path) -> Vec<T> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_list<T: Serialize>(path: &std::path::Path, list: &[T]) -> Result<(), String> {
    let start = list.len().saturating_sub(LIBRARY_CAP);
    let tmp = path.with_extension("tmp");
    create_parent(path)?;
    fs::write(
        &tmp,
        serde_json::to_vec(&list[start..]).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Watched areas of pages (`watches.json`).
pub struct WatchStore {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct WatchDto {
    id: u64,
    url: String,
    selector: String,
    title: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    previous: String,
    #[serde(default)]
    checked_at: i64,
    #[serde(default)]
    changed_at: i64,
    #[serde(default)]
    unread: bool,
}

impl WatchStore {
    pub fn new(data: &std::path::Path) -> Self {
        Self {
            path: data.join("watches.json"),
        }
    }

    pub fn load(&self) -> blackhole_domain::watch::Watches {
        let dtos: Vec<WatchDto> = load_list(&self.path);
        blackhole_domain::watch::Watches::restore(
            dtos.into_iter()
                .filter(|d| d.url.starts_with("https://") || d.url.starts_with("http://"))
                .map(|d| blackhole_domain::watch::Watch {
                    id: d.id,
                    url: d.url,
                    selector: d.selector,
                    title: d.title,
                    value: d.value,
                    previous: d.previous,
                    checked_at: d.checked_at,
                    changed_at: d.changed_at,
                    unread: d.unread,
                })
                .collect(),
        )
    }

    pub fn save(&self, watches: &blackhole_domain::watch::Watches) -> Result<(), String> {
        let dtos: Vec<WatchDto> = watches
            .all()
            .iter()
            .map(|w| WatchDto {
                id: w.id,
                url: w.url.clone(),
                selector: w.selector.clone(),
                title: w.title.clone(),
                value: w.value.clone(),
                previous: w.previous.clone(),
                checked_at: w.checked_at,
                changed_at: w.changed_at,
                unread: w.unread,
            })
            .collect();
        save_list(&self.path, &dtos)
    }
}

/// Highlights and notes on pages (`highlights.json`), written on the background thread.
pub struct HighlightStore {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct MarkDto {
    id: u64,
    quote: String,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    suffix: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    created: i64,
}

#[derive(Serialize, Deserialize)]
struct MarkedPageDto {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    marks: Vec<MarkDto>,
}

impl HighlightStore {
    pub fn new(data: &std::path::Path) -> Self {
        Self {
            path: data.join("highlights.json"),
        }
    }

    pub fn load(&self) -> blackhole_domain::highlights::Highlights {
        use blackhole_domain::highlights::{Highlight, Highlights, Page};
        crate::flush_writes();
        let dtos: Vec<MarkedPageDto> = load_list(&self.path);
        Highlights::restore(
            dtos.into_iter()
                .map(|p| Page {
                    url: p.url,
                    title: p.title,
                    marks: p
                        .marks
                        .into_iter()
                        .map(|m| Highlight {
                            id: m.id,
                            quote: m.quote,
                            prefix: m.prefix,
                            suffix: m.suffix,
                            note: m.note,
                            created: m.created,
                        })
                        .collect(),
                })
                .collect(),
        )
    }

    /// Queues the list to be written (atomically, off the UI thread).
    pub fn save(&self, highlights: &blackhole_domain::highlights::Highlights) {
        let dtos: Vec<MarkedPageDto> = highlights
            .pages()
            .iter()
            .map(|p| MarkedPageDto {
                url: p.url.clone(),
                title: p.title.clone(),
                marks: p
                    .marks
                    .iter()
                    .map(|m| MarkDto {
                        id: m.id,
                        quote: m.quote.clone(),
                        prefix: m.prefix.clone(),
                        suffix: m.suffix.clone(),
                        note: m.note.clone(),
                        created: m.created,
                    })
                    .collect(),
            })
            .collect();
        if let Ok(json) = serde_json::to_string(&dtos) {
            crate::write_later(self.path.clone(), json);
        }
    }
}

/// Download history (`downloads.json`), newest last.
pub struct DownloadStore {
    path: PathBuf,
}

impl DownloadStore {
    pub fn new(data: &std::path::Path) -> Self {
        Self {
            path: data.join("downloads.json"),
        }
    }

    pub fn load(&self) -> Vec<DownloadRec> {
        load_list(&self.path)
    }

    pub fn save(&self, list: &[DownloadRec]) -> Result<(), String> {
        save_list(&self.path, list)
    }
}

/// Closed tabs (`archive.json`), newest last.
pub struct ArchiveStore {
    path: PathBuf,
}

impl ArchiveStore {
    pub fn new(data: &std::path::Path) -> Self {
        Self {
            path: data.join("archive.json"),
        }
    }

    pub fn load(&self) -> Vec<ArchivedTab> {
        load_list(&self.path)
    }

    pub fn save(&self, list: &[ArchivedTab]) -> Result<(), String> {
        save_list(&self.path, list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watches_survive_a_restart_and_odd_entries_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let store = WatchStore::new(dir.path());
        let mut w = blackhole_domain::watch::Watches::default();
        let id = w.add("https://shop.test/p", "#price", "Headphones", 0).unwrap();
        w.record(id, Some("249 €"), 10);
        w.record(id, Some("199 €"), 20);
        store.save(&w).unwrap();
        assert_eq!(store.load(), w);
        fs::write(
            dir.path().join("watches.json"),
            r#"[{"id":1,"url":"javascript:alert(1)","selector":"a","title":"x"}]"#,
        )
        .unwrap();
        assert!(store.load().all().is_empty());
    }

    #[test]
    fn highlights_survive_a_restart_and_odd_entries_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let store = HighlightStore::new(dir.path());
        assert!(store.load().is_empty());
        let mut h = blackhole_domain::highlights::Highlights::default();
        let page = url::Url::parse("https://a.test/doc").unwrap();
        let id = h.add(&page, "Doc", "black holes evaporate", "that", "slowly", 7).unwrap();
        h.set_note(id, "Hawking, 1974");
        store.save(&h);
        assert_eq!(store.load(), h);
        fs::write(
            dir.path().join("highlights.json"),
            r#"[{"url":"javascript:alert(1)","marks":[{"id":1,"quote":"x"}]},{"url":"https://b.test/","marks":[{"id":2,"quote":"kept"}]}]"#,
        )
        .unwrap();
        let back = store.load();
        assert_eq!(back.pages().len(), 1);
        assert_eq!(back.pages()[0].marks[0].quote, "kept");
    }

    fn bm(url: &str) -> Bookmark {
        Bookmark {
            title: "t".into(),
            url: url.into(),
            folder: String::new(),
            space: None,
        }
    }

    #[test]
    fn library_lists_roundtrip_and_keep_only_the_newest() {
        let t = tempfile::tempdir().unwrap();
        let store = ArchiveStore::new(t.path());
        let tab = |n: usize| ArchivedTab {
            title: n.to_string(),
            url: format!("https://a.test/{n}"),
            space: 1,
            at: n as u64,
        };
        let all: Vec<_> = (0..LIBRARY_CAP + 3).map(tab).collect();
        store.save(&all).unwrap();
        let back = store.load();
        assert_eq!(back.len(), LIBRARY_CAP);
        assert_eq!(back[0].title, "3");
        assert!(DownloadStore::new(&t.path().join("none")).load().is_empty());
    }

    #[test]
    fn bookmarks_are_deduped_per_space() {
        let t = tempfile::tempdir().unwrap();
        let store = BookmarkStore::new(t.path());
        let mut other = bm("https://a.com/");
        other.space = Some(2);
        assert_eq!(store.merge(vec![bm("https://a.com/")]).unwrap(), 1);
        assert_eq!(store.merge(vec![other, bm("https://a.com/")]).unwrap(), 1);
        assert_eq!(store.load().len(), 2);
    }

    #[test]
    fn autofill_only_returns_logins_of_the_site() {
        let t = tempfile::tempdir().unwrap();
        let v = Vault::with_key(t.path(), [7; 32]);
        let login = |url: &str, user: &str| Login {
            url: url.into(),
            username: user.into(),
            password: "pw".into(),
        };
        v.merge(vec![
            login("https://a.com/", "x"),
            login("https://b.com/", "y"),
        ])
        .unwrap();
        let found = v.for_host("www.a.com").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].username, "x");
    }

    #[test]
    fn easels_roundtrip_on_disk_and_survive_a_corrupt_file() {
        let t = tempfile::tempdir().unwrap();
        let s = EaselStore::new(&t.path().join("not-yet-created"));
        assert!(s.load().all().is_empty());
        let mut easels = Easels::default();
        let id = easels.create("Ideas").unwrap();
        easels
            .capture(id, "https://a.com/", "A", "snippet")
            .unwrap();
        easels.set_folder(id, Some(5));
        easels.add_note(id, Some((10.0, 20.0)));
        easels
            .capture_live(id, "https://a.com/", "A", "main h1", (300.0, 100.0))
            .unwrap();
        s.save(&easels).unwrap();
        assert_eq!(s.load(), easels);
        fs::write(t.path().join("not-yet-created/easels.json"), "{oops").unwrap();
        assert!(s.load().all().is_empty());
    }

    #[test]
    fn site_matching_is_strict() {
        assert!(same_site("https://www.example.com/login", "example.com"));
        assert!(same_site("https://example.com", "login.example.com"));
        assert!(!same_site("https://example.com", "evilexample.com"));
        assert!(!same_site("https://example.com", "example.com.evil.io"));
        assert!(!same_site("https://com", "example.com"));
    }

    #[test]
    fn bookmarks_are_deduplicated_by_url() {
        let t = tempfile::tempdir().unwrap();
        let s = BookmarkStore::new(&t.path().join("not-yet-created"));
        assert_eq!(
            s.merge(vec![bm("https://a.com/"), bm("https://a.com/")])
                .unwrap(),
            1
        );
        assert_eq!(
            s.merge(vec![bm("https://a.com/"), bm("https://b.com/")])
                .unwrap(),
            1
        );
        assert_eq!(s.load().len(), 2);
    }

    #[test]
    fn vault_is_encrypted_at_rest_and_roundtrips() {
        let t = tempfile::tempdir().unwrap();
        let v = Vault::with_key(t.path(), [7; 32]);
        let l = Login {
            url: "https://a.com/".into(),
            username: "me".into(),
            password: "hunter2-secret".into(),
        };
        assert_eq!(v.merge(vec![l.clone(), l.clone()]).unwrap(), 1);
        let raw = fs::read(t.path().join("vault.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("hunter2"));
        assert_eq!(v.load().unwrap(), vec![l]);
        assert!(Vault::with_key(t.path(), [8; 32]).load().is_err());
    }
}

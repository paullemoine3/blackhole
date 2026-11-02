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
            if !all.iter().any(|o| o.url == b.url) {
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
    color: String,
}

#[derive(Serialize, Deserialize)]
struct EaselDto {
    id: u64,
    name: String,
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
        Self {
            path: data.join("vault.bin"),
            key: Some(key),
        }
    }

    fn key(&self) -> Result<[u8; 32], String> {
        if let Some(k) = self.key {
            return Ok(k);
        }
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

    pub fn load(&self) -> Result<Vec<Login>, String> {
        let Ok(raw) = fs::read(&self.path) else {
            return Ok(Vec::new());
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
        let key = self.key()?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut blob = nonce.to_vec();
        blob.extend(
            cipher
                .encrypt(
                    &nonce,
                    serde_json::to_vec(&all)
                        .map_err(|e| e.to_string())?
                        .as_ref(),
                )
                .map_err(|e| e.to_string())?,
        );
        let tmp = self.path.with_extension("tmp");
        create_parent(&self.path)?;
        fs::write(&tmp, blob).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        Ok(all.len() - before)
    }

    pub fn count(&self) -> usize {
        self.load().map_or(0, |l| l.len())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(url: &str) -> Bookmark {
        Bookmark {
            title: "t".into(),
            url: url.into(),
            folder: String::new(),
        }
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
        easels.add_note(id, Some((10.0, 20.0)));
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

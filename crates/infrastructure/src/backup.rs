//! What leaves the profile when the user asks for it: bookmarks (HTML) and passwords (CSV) any
//! browser reads, and a whole backup sealed with a password (Argon2id, XChaCha20-Poly1305).
//!
//! A backup is a first line of JSON (how the key is derived) and the sealed content. Restoring
//! checks it, then only prepares the files (`stage`): they take their place at the next launch,
//! before anything is read (`apply_staged`). What they replace is kept in `before-restore`.

use std::collections::BTreeMap;
use std::path::Path;

use blackhole_application::WorkspaceRepository;
use blackhole_domain::i18n::message;
use blackhole_domain::passwords::Entry;
use blackhole_domain::{FolderId, Space, Workspace};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::import::Login;
use crate::wallet::{hex, kdf, random, unhex, Kdf, KDF_COST};
use crate::{write_atomic_now, JsonWorkspaceRepository};

pub const EXTENSION: &str = "bhbackup";
/// Shortest password of a backup: the file may end up anywhere.
pub const MIN_PASSWORD: usize = 8;
/// What a backup holds. Never: cookies, Recall (sealed to this computer), paired devices.
const FILES: [&str; 12] = [
    "workspace.json",
    "settings.json",
    "history.json",
    "history-erased.json",
    "easels.json",
    "highlights.json",
    "watches.json",
    "archive.json",
    "bookmarks.json",
    "trackers.json",
    "suggest.json",
    "pulsars.json",
];
/// Sync's own memory of the tabs: kept, it would read the restored workspace as tabs the user
/// closed and close them on the paired devices.
const SYNC_MEMORY: &str = "sync.json";
const STAGE: &str = "restore";
const READY: &str = "ready";
const BEFORE: &str = "before-restore";
/// Largest file read back.
const MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Header {
    blackhole_backup: u32,
    kdf: Kdf,
    nonce: String,
}

#[derive(Serialize, Deserialize)]
struct LoginDto {
    url: String,
    username: String,
    password: String,
}

impl Drop for LoginDto {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
struct Body {
    files: BTreeMap<String, String>,
    #[serde(default)]
    logins: Vec<LoginDto>,
}

/// An opened backup.
pub struct Backup {
    body: Body,
}

impl Backup {
    /// The passwords it holds, as an import takes them.
    pub fn logins(&self) -> Vec<Login> {
        self.body
            .logins
            .iter()
            .map(|l| Login { url: l.url.clone(), username: l.username.clone(), password: l.password.clone() })
            .collect()
    }
}

/// Seals the profile's files and `logins` with `password`.
pub fn seal(data: &Path, logins: &[Entry], password: &str) -> Result<Vec<u8>, String> {
    if password.chars().count() < MIN_PASSWORD {
        return Err(message("msg.backup_short", &[]));
    }
    crate::flush_writes();
    let mut files = BTreeMap::new();
    for name in FILES {
        if let Ok(text) = std::fs::read_to_string(data.join(name)) {
            files.insert(name.to_owned(), text);
        }
    }
    let logins = logins
        .iter()
        .filter(|e| !e.deleted)
        .map(|e| LoginDto { url: e.origin.clone(), username: e.username.clone(), password: e.password.clone() })
        .collect();
    let plain = Zeroizing::new(serde_json::to_vec(&Body { files, logins }).map_err(|e| e.to_string())?);
    let (m, t, p) = KDF_COST;
    let header = Header { blackhole_backup: 1, kdf: Kdf { m, t, p, salt: hex(&random::<16>()) }, nonce: hex(&random::<24>()) };
    let key = kdf(password, &header.kdf)?;
    let mut out = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
    let sealed = XChaCha20Poly1305::new(key.as_ref().into())
        .encrypt(XNonce::from_slice(&unhex(&header.nonce)?), Payload { msg: &plain, aad: &out })
        .map_err(|e| e.to_string())?;
    out.push(b'\n');
    out.extend(sealed);
    Ok(out)
}

/// Reads a backup file back.
pub fn read(path: &Path, password: &str) -> Result<Backup, String> {
    let unreadable = || message("settings.import_unreadable", &[]);
    if std::fs::metadata(path).map_err(|_| unreadable())?.len() > MAX_BYTES {
        return Err(unreadable());
    }
    open(&std::fs::read(path).map_err(|_| unreadable())?, password)
}

/// Opens a sealed backup.
pub fn open(bytes: &[u8], password: &str) -> Result<Backup, String> {
    let not_one = || message("msg.backup_not_one", &[]);
    let at = bytes.iter().take(4096).position(|b| *b == b'\n').ok_or_else(not_one)?;
    let (head, sealed) = (&bytes[..at], &bytes[at + 1..]);
    let header: Header = serde_json::from_slice(head).map_err(|_| not_one())?;
    let nonce = unhex(&header.nonce).ok().filter(|n| n.len() == 24).ok_or_else(not_one)?;
    // The cost comes from the file: never one that would freeze the computer.
    let k = &header.kdf;
    if header.blackhole_backup != 1 || k.m > 256 * 1024 || k.t > 10 || k.p > 8 {
        return Err(not_one());
    }
    let key = kdf(password, k)?;
    let plain = Zeroizing::new(
        XChaCha20Poly1305::new(key.as_ref().into())
            .decrypt(XNonce::from_slice(&nonce), Payload { msg: sealed, aad: head })
            .map_err(|_| message("msg.backup_wrong", &[]))?,
    );
    let mut body: Body = serde_json::from_slice(&plain).map_err(|_| not_one())?;
    body.files.retain(|name, _| FILES.contains(&name.as_str()));
    let sound = |text: &String| serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok();
    if !body.files.contains_key("workspace.json") || !body.files.values().all(sound) {
        return Err(not_one());
    }
    Ok(Backup { body })
}

/// Prepares the files of a backup for the next launch. Nothing of the profile changes yet.
pub fn stage(data: &Path, backup: &Backup) -> Result<(), String> {
    let dir = data.join(STAGE);
    let _ = std::fs::remove_dir_all(&dir);
    for (name, text) in &backup.body.files {
        write_atomic_now(&dir.join(name), text).map_err(|e| e.0)?;
    }
    // What the app could not load would be a profile lost at launch.
    let repo = JsonWorkspaceRepository::new(dir.clone());
    let loads = matches!(repo.load(), Ok(Some(_))) && repo.load_settings().is_ok() && repo.load_history().is_ok();
    if !loads {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(message("msg.backup_not_one", &[]));
    }
    std::fs::write(dir.join(READY), "1").map_err(|e| e.to_string())
}

/// At launch, before anything is read: a staged backup takes its place. Interrupted, it goes on
/// at the next launch (the mark is removed last).
pub fn apply_staged(data: &Path) -> bool {
    let dir = data.join(STAGE);
    if !dir.join(READY).is_file() {
        return false;
    }
    let before = data.join(BEFORE);
    if std::fs::create_dir_all(&before).is_err() {
        return false;
    }
    for name in FILES.iter().chain([&SYNC_MEMORY]) {
        let (staged, current) = (dir.join(name), data.join(name));
        let replaces = staged.is_file() || *name == SYNC_MEMORY;
        if !replaces {
            continue;
        }
        if current.is_file() && std::fs::rename(&current, before.join(name)).is_err() {
            return false;
        }
        if staged.is_file() && std::fs::rename(&staged, &current).is_err() {
            return false;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    true
}

/// Logins as a CSV file other browsers and password managers import (Chrome's columns).
pub fn logins_csv(entries: &[Entry]) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut out = csv::Writer::from_writer(Vec::new());
    out.write_record(["name", "url", "username", "password", "note"]).map_err(|e| e.to_string())?;
    for e in entries.iter().filter(|e| !e.deleted) {
        let name = blackhole_domain::passwords::display_host(&e.origin);
        out.write_record([name.as_str(), &e.origin, &e.username, &e.password, &e.notes]).map_err(|e| e.to_string())?;
    }
    out.into_inner().map(Zeroizing::new).map_err(|e| e.to_string())
}

/// The tabs of every space as a bookmark file: a folder per space, quick access, the folders,
/// then the open tabs. Black hole tabs never leave.
pub fn bookmarks_html(ws: &Workspace) -> String {
    let mut out = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n<TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
    );
    for space in ws.spaces() {
        out.push_str(&format!("<DT><H3>{}</H3>\n<DL><p>\n", escape(&space.name)));
        let mut links = |out: &mut String, keep: &dyn Fn(&blackhole_domain::Tab) -> bool| {
            for tab in space.tabs.iter().filter(|t| keep(t) && !ws.is_burner(t.id)) {
                // A kept tab is where it lives, not where it wandered.
                let (url, title) = tab.home.as_ref().map_or((&tab.url, &tab.title), |h| (&h.url, &h.title));
                if matches!(url.scheme(), "http" | "https") {
                    let title = if title.is_empty() { url.as_str() } else { title };
                    out.push_str(&format!("<DT><A HREF=\"{}\">{}</A>\n", escape(url.as_str()), escape(title)));
                }
            }
        };
        links(&mut out, &|t| t.pinned);
        folders(&mut out, space, None, &mut links);
        links(&mut out, &|t| !t.pinned && t.folder.is_none());
        out.push_str("</DL><p>\n");
    }
    out.push_str("</DL><p>\n");
    out
}

type Links<'a> = dyn FnMut(&mut String, &dyn Fn(&blackhole_domain::Tab) -> bool) + 'a;

fn folders(out: &mut String, space: &Space, parent: Option<FolderId>, links: &mut Links<'_>) {
    for f in space.folders.iter().filter(|f| f.parent == parent) {
        out.push_str(&format!("<DT><H3>{}</H3>\n<DL><p>\n", escape(&f.name)));
        links(out, &|t| !t.pinned && t.folder == Some(f.id));
        folders(out, space, Some(f.id), links);
        out.push_str("</DL><p>\n");
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Writes an exported file; on Unix, readable by the user alone.
pub fn write_export(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;
    use blackhole_domain::passwords::Kind;
    use url::Url;

    const PASSWORD: &str = "a long password";

    fn entry(origin: &str, password: &str) -> Entry {
        Entry {
            id: "1".into(),
            origin: origin.into(),
            username: "me, \"you\"".into(),
            password: password.into(),
            notes: String::new(),
            kind: Kind::Web,
            created: 0,
            modified: 0,
            device: String::new(),
            deleted: false,
        }
    }

    fn profile(space: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonWorkspaceRepository::new(dir.path().into());
        let mut ws = Workspace::default();
        let id = ws.active_space().id;
        ws.rename_space(id, space);
        repo.save(&ws).unwrap();
        repo.save_settings(&blackhole_domain::Settings::default()).unwrap();
        crate::flush_writes();
        dir
    }

    fn name_of(dir: &Path) -> String {
        let ws = JsonWorkspaceRepository::new(dir.into()).load().unwrap().unwrap();
        ws.active_space().name.clone()
    }

    #[test]
    fn a_backup_gives_the_profile_back_at_the_next_launch_only() {
        let (from, to) = (profile("Work"), profile("Other"));
        std::fs::write(to.path().join("sync.json"), "{}").unwrap();
        std::fs::write(from.path().join("devices.bin"), "secret").unwrap();
        let bytes = seal(from.path(), &[entry("https://a.test", "pw-42")], PASSWORD).unwrap();
        assert!(!bytes.windows(5).any(|w| w == b"pw-42" || w == b"Work\""), "nothing readable");
        let backup = open(&bytes, PASSWORD).unwrap();
        assert_eq!(backup.logins()[0].password, "pw-42");
        assert!(!apply_staged(to.path()), "nothing staged");
        stage(to.path(), &backup).unwrap();
        assert_eq!(name_of(to.path()), "Other");
        assert!(apply_staged(to.path()));
        assert_eq!(name_of(to.path()), "Work");
        assert_eq!(name_of(&to.path().join(BEFORE)), "Other", "the profile replaced is kept");
        assert!(!to.path().join("sync.json").exists(), "sync starts again instead of closing tabs elsewhere");
        assert!(!to.path().join("devices.bin").exists());
        assert!(!apply_staged(to.path()), "once");
    }

    #[test]
    fn a_wrong_password_a_changed_file_and_hostile_files_are_refused() {
        let dir = profile("Work");
        let bytes = seal(dir.path(), &[], PASSWORD).unwrap();
        assert!(open(&bytes, "another password").is_err());
        let mut changed = bytes.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(open(&changed, PASSWORD).is_err());
        assert!(open(b"{}\nx", PASSWORD).is_err());
        assert!(open(b"not a backup", PASSWORD).is_err());
        let heavy = br#"{"blackhole_backup":1,"kdf":{"m":4000000000,"t":1,"p":1,"salt":"00112233445566778899aabbccddeeff"},"nonce":"000000000000000000000000000000000000000000000000"}
x"#;
        assert!(open(heavy, PASSWORD).is_err());
        assert!(seal(dir.path(), &[], "short").is_err());
        // A workspace the app cannot load is never staged.
        let files = [("workspace.json".to_owned(), "[]".to_owned())].into();
        let bad = Backup { body: Body { files, logins: Vec::new() } };
        assert!(stage(dir.path(), &bad).is_err());
        assert!(!apply_staged(dir.path()));
    }

    #[test]
    fn exports_are_read_back_by_the_importer() {
        let mut gone = entry("https://gone.test", "");
        gone.deleted = true;
        let csv = logins_csv(&[entry("https://a.test", "p,\"w"), gone]).unwrap();
        let back = import::parse_logins_csv(std::str::from_utf8(&csv).unwrap()).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].username.as_str(), back[0].password.as_str()), ("me, \"you\"", "p,\"w"));

        let mut ws = Workspace::default();
        let space = ws.active_space().id;
        ws.rename_space(space, "A & B");
        ws.open_tab(Url::parse("https://open.test/?a=1&b=2").unwrap());
        ws.open_burner(Url::parse("https://secret.test/").unwrap());
        let html = bookmarks_html(&ws);
        assert!(!html.contains("secret.test"));
        let list = import::parse_html(&html);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].url.as_str(), list[0].folder.as_str()), ("https://open.test/?a=1&b=2", "A & B"));
    }
}

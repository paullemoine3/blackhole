//! Registry of the open BlackHole windows (one process each) and the inbox used to hand a tab
//! from one window to another, private or not.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A window that has not refreshed its entry for this long is considered gone.
const STALE_MS: u128 = 5_000;

/// Where a window sits on the screen (logical pixels) and which process owns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub pid: u32,
    pub private: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub updated: u128,
}

impl Peer {
    #[must_use]
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// A tab travelling to another window. Only its address moves; the page reloads there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transfer {
    pub url: String,
    pub title: String,
}

#[must_use]
pub fn dir() -> PathBuf {
    std::env::temp_dir().join("BlackHole-windows")
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Writes (or refreshes) the entry of this window.
pub fn publish(dir: &Path, peer: &Peer) {
    let mut peer = peer.clone();
    peer.updated = now_ms();
    if std::fs::create_dir_all(dir.join("inbox")).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(&peer) {
        let tmp = dir.join(format!("{}.tmp", peer.pid));
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, dir.join(format!("{}.json", peer.pid)));
        }
    }
}

/// Removes this window's entry (and any transfer nobody collected).
pub fn withdraw(dir: &Path, pid: u32) {
    let _ = std::fs::remove_file(dir.join(format!("{pid}.json")));
    for path in inbox_files(dir, pid) {
        let _ = std::fs::remove_file(path);
    }
}

/// The live windows other than `except`.
#[must_use]
pub fn peers(dir: &Path, except: u32) -> Vec<Peer> {
    let now = now_ms();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<Peer>(&s).ok())
        .filter(|p| p.pid != except && now.saturating_sub(p.updated) < STALE_MS)
        .collect()
}

/// The window under a screen point, if any.
#[must_use]
pub fn at(peers: &[Peer], x: f64, y: f64) -> Option<&Peer> {
    peers.iter().find(|p| p.contains(x, y))
}

/// Drops a tab into the inbox of window `pid`.
pub fn send(dir: &Path, pid: u32, transfer: &Transfer) -> bool {
    let inbox = dir.join("inbox");
    if std::fs::create_dir_all(&inbox).is_err() {
        return false;
    }
    let Ok(json) = serde_json::to_string(transfer) else {
        return false;
    };
    let name = format!("{pid}-{}", now_ms());
    let tmp = inbox.join(format!("{name}.tmp"));
    std::fs::write(&tmp, json).is_ok()
        && std::fs::rename(tmp, inbox.join(format!("{name}.json"))).is_ok()
}

fn inbox_files(dir: &Path, pid: u32) -> Vec<PathBuf> {
    let prefix = format!("{pid}-");
    let Ok(entries) = std::fs::read_dir(dir.join("inbox")) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "json")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    files.sort();
    files
}

/// Takes every transfer waiting for window `pid`.
#[must_use]
pub fn receive(dir: &Path, pid: u32) -> Vec<Transfer> {
    inbox_files(dir, pid)
        .into_iter()
        .filter_map(|path| {
            let parsed = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
            let _ = std::fs::remove_file(path);
            parsed
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(pid: u32, private: bool, x: f64) -> Peer {
        Peer {
            pid,
            private,
            x,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            updated: 0,
        }
    }

    #[test]
    fn lists_other_live_windows_and_finds_the_one_under_a_point() {
        let tmp = tempfile::tempdir().unwrap();
        publish(tmp.path(), &peer(1, false, 0.0));
        publish(tmp.path(), &peer(2, true, 200.0));
        let others = peers(tmp.path(), 1);
        assert_eq!(others.len(), 1);
        assert!(others[0].private);
        assert_eq!(at(&others, 250.0, 50.0).map(|p| p.pid), Some(2));
        assert!(at(&others, 50.0, 50.0).is_none());
    }

    #[test]
    fn forgets_stale_and_withdrawn_windows() {
        let tmp = tempfile::tempdir().unwrap();
        publish(tmp.path(), &peer(2, false, 0.0));
        let old = Peer {
            updated: 1,
            ..peer(3, false, 0.0)
        };
        std::fs::write(
            tmp.path().join("3.json"),
            serde_json::to_string(&old).unwrap(),
        )
        .unwrap();
        assert_eq!(peers(tmp.path(), 1).len(), 1);
        withdraw(tmp.path(), 2);
        assert!(peers(tmp.path(), 1).is_empty());
    }

    #[test]
    fn delivers_a_transfer_only_to_its_target_once() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Transfer {
            url: "https://example.com/".into(),
            title: "Example".into(),
        };
        assert!(send(tmp.path(), 7, &t));
        assert!(receive(tmp.path(), 8).is_empty());
        assert_eq!(receive(tmp.path(), 7), vec![t]);
        assert!(receive(tmp.path(), 7).is_empty());
    }
}

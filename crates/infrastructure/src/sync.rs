//! Tab synchronization between paired devices: the ledger on disk (`sync.json`), its wire
//! format and the decision taken when the ledger of another device arrives. The merge rules
//! are `blackhole_domain::sync`; the transport is `handoff` (`Request::Snapshot`).
//!
//! Used the same way by the desktop app and by Android:
//! 1. `observe` the workspace regularly (it tells whether anything synced changed);
//! 2. send `snapshot` to a device coming online, and to `auto` devices after a change;
//! 3. on a received ledger, `receive` says what to do ([`Decision`]); `accept` applies it.

use blackhole_application::{BrowserService, WorkspaceRepository};
use blackhole_domain::sync::{syncable, Item, Ledger, Merge, SpaceItem};
use blackhole_domain::TabId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

const FILE: &str = "sync.json";
const WIRE_VERSION: u32 = 1;
const MAX_SPACES: usize = 64;
const MAX_TEXT: usize = 300;
const MAX_URL: usize = 8 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ItemDto {
    space: String,
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    pinned: bool,
    modified: i64,
    device: String,
    #[serde(default)]
    deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpaceDto {
    name: String,
    color: String,
    modified: i64,
    device: String,
}

/// `sync.json`: this device's sync identity and ledger.
#[derive(Debug, Default, Serialize, Deserialize)]
struct FileDto {
    me: String,
    #[serde(default)]
    items: Vec<ItemDto>,
    #[serde(default)]
    spaces: Vec<SpaceDto>,
}

/// What travels to a paired device.
#[derive(Debug, Serialize, Deserialize)]
struct WireDto {
    v: u32,
    /// An answer to a ledger the user agreed to merge, or asked for ("Sync now").
    #[serde(default)]
    reply: bool,
    /// The sender wants this device's ledger back whatever happens ("Sync now").
    #[serde(default)]
    pull: bool,
    #[serde(default)]
    items: Vec<ItemDto>,
    #[serde(default)]
    spaces: Vec<SpaceDto>,
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_TEXT).collect()
}

fn to_dto(ledger: &Ledger) -> (Vec<ItemDto>, Vec<SpaceDto>) {
    let items = ledger
        .items
        .iter()
        .map(|i| ItemDto {
            space: i.space.clone(),
            url: i.url.to_string(),
            title: i.title.clone(),
            pinned: i.pinned,
            modified: i.modified,
            device: i.device.clone(),
            deleted: i.deleted,
        })
        .collect();
    let spaces = ledger
        .spaces
        .iter()
        .map(|s| SpaceDto {
            name: s.name.clone(),
            color: s.color.clone(),
            modified: s.modified,
            device: s.device.clone(),
        })
        .collect();
    (items, spaces)
}

fn valid_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Rebuilds a ledger from untrusted data: bad entries are dropped, texts bounded.
fn from_dto(items: Vec<ItemDto>, spaces: Vec<SpaceDto>) -> Ledger {
    let items = items
        .into_iter()
        .take(blackhole_domain::sync::MAX_ITEMS)
        .filter(|i| i.url.len() <= MAX_URL && !i.space.trim().is_empty())
        .filter_map(|i| {
            let url = Url::parse(&i.url).ok().filter(syncable)?;
            Some(Item {
                space: bounded(i.space.trim()),
                url,
                title: bounded(&i.title),
                pinned: i.pinned,
                modified: i.modified,
                device: bounded(&i.device),
                deleted: i.deleted,
            })
        })
        .collect();
    let spaces = spaces
        .into_iter()
        .take(MAX_SPACES)
        .filter(|s| !s.name.trim().is_empty() && valid_color(&s.color))
        .map(|s| SpaceItem {
            name: bounded(s.name.trim()),
            color: s.color.to_ascii_lowercase(),
            modified: s.modified,
            device: bounded(&s.device),
        })
        .collect();
    Ledger { items, spaces }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// What to do with a ledger received from a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing to change here, and nothing to send.
    Nothing,
    /// Nothing changes here, but the other device misses changes: send it this ledger.
    SendBack,
    /// Ask the user first (`ask` mode): tabs to open, to close, other changes.
    Ask { opens: usize, closes: usize, changes: usize },
    /// Apply now (`auto` mode, or an answer the user asked for on either side).
    Apply,
}

/// A ledger received, parsed.
#[derive(Debug, Clone)]
pub struct Received {
    pub ledger: Ledger,
    pub reply: bool,
    pub pull: bool,
}

/// The decision for `merge` in `mode` (`ask`, `auto`, `off`).
pub fn decide(mode: &str, received: &Received, merge: &Merge) -> Decision {
    if mode == "off" {
        return Decision::Nothing;
    }
    if merge.plan.is_empty() {
        return if merge.remote_behind || received.pull {
            Decision::SendBack
        } else {
            Decision::Nothing
        };
    }
    if mode == "auto" || received.reply {
        return Decision::Apply;
    }
    let (opens, closes, changes) = merge.counts();
    Decision::Ask { opens, closes, changes }
}

pub struct SyncEngine {
    path: PathBuf,
    me: String,
    ledger: Ledger,
}

impl SyncEngine {
    /// The ledger of the profile in `data` (a new identity on the first run).
    pub fn open(data: &Path) -> Self {
        let path = data.join(FILE);
        crate::flush_writes();
        let file: FileDto = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let me = if file.me.len() == 32 && file.me.chars().all(|c| c.is_ascii_hexdigit()) {
            file.me
        } else {
            let mut bytes = [0u8; 16];
            use chacha20poly1305::aead::{rand_core::RngCore, OsRng};
            OsRng.fill_bytes(&mut bytes);
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        };
        Self {
            path,
            me,
            ledger: from_dto(file.items, file.spaces),
        }
    }

    fn save(&self) {
        let (items, spaces) = to_dto(&self.ledger);
        let file = FileDto {
            me: self.me.clone(),
            items,
            spaces,
        };
        if let Ok(raw) = serde_json::to_string(&file) {
            crate::write_later(self.path.clone(), raw);
        }
    }

    /// Records the workspace's synced state; true when something synced changed.
    pub fn observe<R: WorkspaceRepository>(&mut self, svc: &BrowserService<R>) -> bool {
        let changed = self
            .ledger
            .observe(&svc.workspace().sync_view(), &self.me, now_ms());
        if changed {
            self.save();
        }
        changed
    }

    /// This device's ledger, for a paired device.
    pub fn snapshot(&self, reply: bool, pull: bool) -> String {
        let (items, spaces) = to_dto(&self.ledger);
        serde_json::to_string(&WireDto {
            v: WIRE_VERSION,
            reply,
            pull,
            items,
            spaces,
        })
        .unwrap_or_default()
    }

    /// Reads a ledger received from a device; `None` when it is not one.
    pub fn parse(json: &str) -> Option<Received> {
        let wire: WireDto = serde_json::from_str(json).ok()?;
        (wire.v == WIRE_VERSION).then(|| Received {
            ledger: from_dto(wire.items, wire.spaces),
            reply: wire.reply,
            pull: wire.pull,
        })
    }

    /// What merging `received` would do here (the workspace is observed first).
    pub fn preview<R: WorkspaceRepository>(&mut self, svc: &BrowserService<R>, received: &Received) -> Merge {
        self.observe(svc);
        self.ledger.merge(&received.ledger)
    }

    /// Merges `received` into the workspace. Returns the tabs removed (their views must go)
    /// and whether the other device misses changes made here.
    pub fn accept<R: WorkspaceRepository>(
        &mut self,
        svc: &mut BrowserService<R>,
        received: &Received,
    ) -> (Vec<TabId>, bool) {
        let merge = self.preview(svc, received);
        let removed = svc.apply_sync(&merge.plan);
        self.ledger = merge.ledger;
        self.save();
        (removed, merge.remote_behind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackhole_application::StorageError;
    use blackhole_domain::{History, PrivacyPolicy, Settings, Workspace};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<Option<Workspace>>>);

    impl WorkspaceRepository for Memory {
        fn load(&self) -> Result<Option<Workspace>, StorageError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, w: &Workspace) -> Result<(), StorageError> {
            *self.0.lock().unwrap() = Some(w.clone());
            Ok(())
        }
        fn load_settings(&self) -> Result<Option<Settings>, StorageError> {
            Ok(None)
        }
        fn save_settings(&self, _: &Settings) -> Result<(), StorageError> {
            Ok(())
        }
        fn load_history(&self) -> Result<Option<History>, StorageError> {
            Ok(None)
        }
        fn save_history(&self, _: &History) -> Result<(), StorageError> {
            Ok(())
        }
    }

    fn device(dir: &Path) -> (BrowserService<Memory>, SyncEngine) {
        (
            BrowserService::start(Memory::default(), PrivacyPolicy::default()),
            SyncEngine::open(dir),
        )
    }

    fn urls<R: WorkspaceRepository>(svc: &BrowserService<R>) -> Vec<String> {
        let mut out: Vec<String> = svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter().map(|t| format!("{}:{}{}", s.name, t.url, if t.pinned { "*" } else { "" })))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn two_devices_converge_after_asking_once() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (mut phone, mut pe) = device(da.path());
        let (mut pc, mut ce) = device(db.path());
        phone.open_tab("https://p.example/");
        let c = pc.open_tab("https://c.example/").unwrap();
        pc.toggle_pin(c);
        pe.observe(&phone);
        ce.observe(&pc);

        // The pc comes online: the phone receives its ledger and, in `ask` mode, asks.
        let from_pc = SyncEngine::parse(&ce.snapshot(false, false)).unwrap();
        let merge = pe.preview(&phone, &from_pc);
        assert_eq!(decide("ask", &from_pc, &merge), Decision::Ask { opens: 1, closes: 0, changes: 0 });
        assert_eq!(decide("off", &from_pc, &merge), Decision::Nothing);
        assert_eq!(decide("auto", &from_pc, &merge), Decision::Apply);

        // The user accepts: the phone applies, then answers since the pc misses its tab.
        let (removed, behind) = pe.accept(&mut phone, &from_pc);
        assert!(removed.is_empty() && behind);
        let answer = SyncEngine::parse(&pe.snapshot(true, false)).unwrap();
        let merge = ce.preview(&pc, &answer);
        assert_eq!(decide("ask", &answer, &merge), Decision::Apply, "an answer needs no second question");
        ce.accept(&mut pc, &answer);
        assert_eq!(urls(&phone), urls(&pc));
        assert!(urls(&pc).contains(&"Personal:https://c.example/*".to_owned()));

        // Settled: nothing more on either side.
        let again = SyncEngine::parse(&ce.snapshot(false, false)).unwrap();
        let merge = pe.preview(&phone, &again);
        assert_eq!(decide("ask", &again, &merge), Decision::Nothing);
    }

    #[test]
    fn a_closed_tab_closes_on_the_other_device_and_its_view_goes() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (mut a, mut ae) = device(da.path());
        let (mut b, mut be) = device(db.path());
        let t = a.open_tab("https://gone.example/").unwrap();
        ae.observe(&a);
        be.accept(&mut b, &SyncEngine::parse(&ae.snapshot(false, false)).unwrap());
        let on_b = b.workspace().active_space().tabs[0].id;
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.close_tab(t);
        ae.observe(&a);
        let (removed, _) = be.accept(&mut b, &SyncEngine::parse(&ae.snapshot(false, false)).unwrap());
        assert_eq!(removed, vec![on_b]);
        assert!(urls(&b).is_empty());
    }

    #[test]
    fn the_ledger_and_identity_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, mut e) = device(dir.path());
        let mut svc = svc;
        svc.open_tab("https://kept.example/");
        assert!(e.observe(&svc));
        let me = e.me.clone();
        crate::flush_writes();
        let again = SyncEngine::open(dir.path());
        assert_eq!(again.me, me);
        assert_eq!(again.ledger.items.len(), 1);
    }

    #[test]
    fn untrusted_ledgers_are_cleaned() {
        let json = r##"{"v":1,"items":[
            {"space":"Home","url":"file:///etc/passwd","modified":1,"device":"x"},
            {"space":"","url":"https://a.example/","modified":1,"device":"x"},
            {"space":"Home","url":"https://ok.example/","modified":1,"device":"x"}],
            "spaces":[{"name":"Home","color":"red","modified":1,"device":"x"},
                      {"name":"Work","color":"#2DD4BF","modified":1,"device":"x"}]}"##;
        let r = SyncEngine::parse(json).unwrap();
        assert_eq!(r.ledger.items.len(), 1);
        assert_eq!(r.ledger.spaces.len(), 1);
        assert_eq!(r.ledger.spaces[0].color, "#2dd4bf");
        assert!(SyncEngine::parse(r#"{"v":2}"#).is_none());
        assert!(SyncEngine::parse("not json").is_none());
    }

    #[test]
    fn sync_now_gets_an_answer_even_when_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, mut e) = device(dir.path());
        let pull = SyncEngine::parse(&e.snapshot(false, true)).unwrap();
        let merge = e.preview(&svc, &pull);
        assert_eq!(decide("ask", &pull, &merge), Decision::SendBack);
    }
}

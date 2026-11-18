//! History sync between paired devices (0.8.18), apart from the tab sync: one switch
//! (`Settings::history_sync`), no question, done in the background. The transport is the tab
//! sync's (`handoff`, `Request::Snapshot`), encrypted for the paired device only; the messages
//! carry `"v": 2` so that a device older than 0.8.18 ignores them.
//!
//! Incremental, to stay light: a device asks another for what changed after a mark of that
//! device's clock (`history_pull`, a few bytes); the answer (`history`) is the changes after it,
//! oldest first, `BATCH` pages at most, with the mark reached (`until`) and whether more wait.
//! Each device keeps the mark reached per paired device (`history-sync.json`). Nothing changed:
//! the answer is empty. A page carries what ranks it in the command bar (dated and typed
//! visits, title, spaces by name); pages forgotten and a clearing travel too, and the learned
//! choices of the command bar. Merge rules: `domain::history::History::merge`,
//! `domain::suggest::Adaptive::merge`.

use blackhole_application::{BrowserService, WorkspaceRepository};
use blackhole_domain::history::{RECENT_VISITS, SPACES_KEPT};
use blackhole_domain::suggest::{Adaptive, Learned};
use blackhole_domain::sync::syncable;
use blackhole_domain::{HistoryEntry, SharedHistory, Visit, VisitKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use url::Url;

/// Pages in one answer at most (about 80 KB on the wire).
pub const BATCH: usize = 200;
/// Learned choices in one answer at most.
const CHOICES: usize = 200;
/// Pages accepted in one answer: a batch may exceed `BATCH` to end on a whole second, never
/// a whole history (1 000 pages).
const RECEIVED_MAX: usize = 1_000;
const WIRE_VERSION: u32 = 2;
const MAX_URL: usize = 2 * 1024;
const MAX_TITLE: usize = 160;
const MAX_SPACE: usize = 64;
/// Choices made this long before the mark still travel (the devices' clocks may differ).
const CLOCK_SLACK: u64 = 24 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PageDto {
    url: String,
    #[serde(default)]
    title: String,
    at: u64,
    #[serde(default)]
    visits: u32,
    #[serde(default)]
    recent: Vec<(u64, u8)>,
    #[serde(default)]
    spaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChoiceDto {
    input: String,
    key: String,
    uses: f64,
    at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum WireDto {
    #[serde(rename = "history_pull")]
    Pull { v: u32, since: u64 },
    #[serde(rename = "history")]
    Batch {
        v: u32,
        until: u64,
        #[serde(default)]
        more: bool,
        #[serde(default)]
        pages: Vec<PageDto>,
        #[serde(default)]
        forgotten: Vec<(String, u64)>,
        #[serde(default)]
        cleared_at: u64,
        #[serde(default)]
        choices: Vec<ChoiceDto>,
    },
}

/// A history message received from a paired device, checked and bounded.
#[derive(Debug, Clone)]
pub enum Incoming {
    /// It asks for what changed here after `since` (this device's clock).
    Pull { since: u64 },
    /// Its changes after the mark asked.
    Batch(Batch),
}

#[derive(Debug, Clone)]
pub struct Batch {
    /// Pages with the names of their spaces (matched by name on arrival).
    pages: Vec<(HistoryEntry, Vec<String>)>,
    forgotten: Vec<(String, u64)>,
    cleared_at: u64,
    pub until: u64,
    pub more: bool,
    choices: Vec<Learned>,
}

/// The request for what `device` changed after `since` (its clock).
pub fn pull(since: u64) -> String {
    serde_json::to_string(&WireDto::Pull { v: WIRE_VERSION, since }).unwrap_or_default()
}

/// The answer to a pull: what changed here after `since`, with the learned `choices`.
pub fn batch<R: WorkspaceRepository>(svc: &BrowserService<R>, choices: &Adaptive, since: u64) -> String {
    let changes = svc.history_changes_since(since, BATCH);
    let name_of = |id: u64| svc.workspace().spaces().iter().find(|s| s.id.0 == id).map(|s| s.name.clone());
    let pages = changes
        .entries
        .iter()
        .filter(|e| syncable(&e.url) && e.url.as_str().len() <= MAX_URL)
        .map(|e| PageDto {
            url: e.url.to_string(),
            title: e.title.chars().take(MAX_TITLE).collect(),
            at: e.visited_at,
            visits: e.visits,
            recent: e.recent.iter().map(|v| (v.at, v.kind.id())).collect(),
            spaces: e.spaces.iter().filter_map(|id| name_of(*id)).collect(),
        })
        .collect();
    let choices = choices
        .shared(CHOICES)
        .into_iter()
        .filter(|c| c.at + CLOCK_SLACK > since)
        .map(|c| ChoiceDto { input: c.input, key: c.key, uses: c.uses, at: c.at })
        .collect();
    serde_json::to_string(&WireDto::Batch {
        v: WIRE_VERSION,
        until: changes.until,
        more: changes.more,
        pages,
        forgotten: changes.forgotten,
        cleared_at: changes.cleared_at,
        choices,
    })
    .unwrap_or_default()
}

fn bounded(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

/// Reads a history message; `None` for anything else (a tab ledger, another version).
pub fn parse(json: &str) -> Option<Incoming> {
    match serde_json::from_str::<WireDto>(json).ok()? {
        WireDto::Pull { v, since } => (v == WIRE_VERSION).then_some(Incoming::Pull { since }),
        WireDto::Batch { v, until, more, pages, forgotten, cleared_at, choices } => {
            if v != WIRE_VERSION {
                return None;
            }
            let pages = pages
                .into_iter()
                .take(RECEIVED_MAX)
                .filter(|p| p.url.len() <= MAX_URL)
                .filter_map(|p| {
                    let url = Url::parse(&p.url).ok().filter(syncable)?;
                    let entry = HistoryEntry {
                        title: bounded(&p.title, MAX_TITLE),
                        visited_at: p.at,
                        visits: p.visits.clamp(1, 1_000_000),
                        recent: p.recent.iter().take(RECENT_VISITS).map(|&(at, k)| Visit { at, kind: VisitKind::from_id(k) }).collect(),
                        spaces: Vec::new(),
                        changed: 0,
                        url,
                    };
                    Some((entry, p.spaces.iter().take(SPACES_KEPT).map(|s| bounded(s, MAX_SPACE)).collect()))
                })
                .collect();
            let forgotten = forgotten
                .into_iter()
                .take(500)
                .filter(|(k, _)| k.len() <= MAX_URL && Url::parse(k).is_ok_and(|u| syncable(&u)))
                .collect();
            let choices = choices
                .into_iter()
                .take(CHOICES)
                .filter(|c| c.input.len() <= 200 && c.key.len() <= MAX_URL)
                .map(|c| Learned { input: c.input, key: c.key, uses: c.uses, at: c.at })
                .collect();
            Some(Incoming::Batch(Batch { pages, forgotten, cleared_at, until, more, choices }))
        }
    }
}

/// Merges a batch here: its spaces matched by name to this device's (unknown ones dropped).
/// Returns whether the history and the choices changed.
pub fn merge<R: WorkspaceRepository>(svc: &mut BrowserService<R>, choices: &mut Adaptive, batch: &Batch) -> (bool, bool) {
    let id_of = |name: &str| {
        svc.workspace()
            .spaces()
            .iter()
            .find(|s| s.name.trim().eq_ignore_ascii_case(name))
            .map(|s| s.id.0)
    };
    let shared = SharedHistory {
        entries: batch
            .pages
            .iter()
            .map(|(e, names)| {
                let mut e = e.clone();
                e.spaces = names.iter().filter_map(|n| id_of(n)).collect();
                e
            })
            .collect(),
        forgotten: batch.forgotten.clone(),
        cleared_at: batch.cleared_at,
        until: batch.until,
        more: batch.more,
    };
    (svc.merge_history(&shared), choices.merge(&batch.choices))
}

/// The mark reached in each paired device's history (`history-sync.json`): the next pull asks
/// from it, so only what changed since travels.
pub struct Marks {
    path: PathBuf,
    marks: BTreeMap<String, u64>,
}

impl Marks {
    pub fn open(data: &Path) -> Self {
        let path = data.join("history-sync.json");
        crate::flush_writes();
        let marks = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self { path, marks }
    }

    pub fn get(&self, device: &str) -> u64 {
        self.marks.get(device).copied().unwrap_or(0)
    }

    pub fn set(&mut self, device: &str, until: u64) {
        if self.marks.insert(device.to_owned(), until) != Some(until) {
            if let Ok(raw) = serde_json::to_string(&self.marks) {
                crate::write_later(self.path.clone(), raw);
            }
        }
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

    fn svc() -> BrowserService<Memory> {
        BrowserService::start(Memory::default(), PrivacyPolicy::default())
    }

    /// `to` pulls everything `from` has after `mark`, batch after batch; returns the new mark.
    fn sync(from: &BrowserService<Memory>, from_choices: &Adaptive, to: &mut BrowserService<Memory>, to_choices: &mut Adaptive, mut mark: u64) -> (u64, usize) {
        let mut rounds = 0;
        loop {
            rounds += 1;
            let Some(Incoming::Pull { since }) = parse(&pull(mark)) else { panic!("pull") };
            let Some(Incoming::Batch(b)) = parse(&batch(from, from_choices, since)) else { panic!("batch") };
            merge(to, to_choices, &b);
            mark = b.until;
            if !b.more {
                return (mark, rounds);
            }
        }
    }

    #[test]
    fn the_history_and_its_ranking_travel_and_only_changes_go_again() {
        let mut a = svc();
        let mut b = svc();
        let tab = a.open_tab("https://mail.example/").unwrap();
        a.expect_visit(tab, VisitKind::Typed);
        a.page_changed(tab, Some(Url::parse("https://mail.example/").unwrap()), Some("Mail".into()));
        let other = a.open_tab("https://secret.example/").unwrap();
        a.page_changed(other, Some(Url::parse("https://secret.example/").unwrap()), None);
        a.forget_history(&Url::parse("https://secret.example/").unwrap());
        let mut chosen = Adaptive::default();
        chosen.record("ma", "mail.example", 2_000_000_000);
        let mut b_choices = Adaptive::default();

        let (mark, _) = sync(&a, &chosen, &mut b, &mut b_choices, 0);
        assert!(mark > 0);
        let mail = b.recent_activity(10).iter().find(|e| e.url.host_str() == Some("mail.example")).cloned().unwrap();
        assert_eq!(mail.title, "Mail");
        assert_eq!(mail.recent[0].kind, VisitKind::Typed, "how the page was reached travels");
        assert_eq!(mail.spaces, vec![b.workspace().active_space().id.0], "spaces are matched by name");
        assert!(b.recent_activity(10).iter().all(|e| e.url.host_str() != Some("secret.example")));
        assert!(b_choices.boost("ma", "mail.example", 2_000_000_000) > 0.0);

        // Nothing changed since: the answer carries nothing.
        let Some(Incoming::Batch(empty)) = parse(&batch(&a, &Adaptive::default(), mark)) else { panic!() };
        assert!(empty.pages.is_empty() && empty.forgotten.is_empty() && !empty.more);
        assert_eq!(empty.until, mark);
    }

    #[test]
    fn a_long_history_comes_in_several_batches() {
        let mut a = svc();
        let mut b = svc();
        let tab = a.open_tab("https://p.example/").unwrap();
        for i in 0..(BATCH * 2 + 10) {
            a.page_changed(tab, Some(Url::parse(&format!("https://p.example/{i}")).unwrap()), None);
        }
        let (_, rounds) = sync(&a, &Adaptive::default(), &mut b, &mut Adaptive::default(), 0);
        assert_eq!(rounds, 1, "visits within one second share one mark: they come in one batch");
        assert_eq!(b.recent_activity(usize::MAX).len(), a.recent_activity(usize::MAX).len());
    }

    #[test]
    fn untrusted_messages_are_cleaned_and_ledgers_are_not_history() {
        let json = r##"{"kind":"history","v":2,"until":9,"pages":[
            {"url":"file:///etc/passwd","at":5},
            {"url":"https://ok.example/","title":"ok","at":5,"visits":0,"recent":[[5,1],[4,9]],"spaces":["Nope"]}],
            "forgotten":[["javascript:alert(1)",3],["https://gone.example/",3]],
            "choices":[{"input":"o","key":"ok.example","uses":-1,"at":1},{"input":"o","key":"ok.example","uses":2,"at":1}]}"##;
        let Some(Incoming::Batch(b)) = parse(json) else { panic!() };
        assert_eq!(b.pages.len(), 1);
        assert_eq!(b.pages[0].0.visits, 1);
        assert_eq!(b.forgotten.len(), 1);
        let mut a = Adaptive::default();
        assert!(a.merge(&b.choices));
        assert_eq!(a.entries().len(), 1, "a choice with no use is dropped");
        assert!(parse(r#"{"v":1,"items":[],"spaces":[]}"#).is_none(), "a tab ledger");
        assert!(parse(r#"{"kind":"history_pull","v":3,"since":1}"#).is_none(), "another version");
    }

    #[test]
    fn marks_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = Marks::open(dir.path());
        assert_eq!(m.get("pc"), 0);
        m.set("pc", 42);
        crate::flush_writes();
        assert_eq!(Marks::open(dir.path()).get("pc"), 42);
    }
}

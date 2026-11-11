//! The store of Recall (see `domain::recall`): the pages read, kept encrypted on this computer
//! and searched on a thread of their own, so the interface never waits for the disk.
//!
//! Pages are filed in `SHARDS` small files (`recall/NN.bin`, ChaCha20-Poly1305 like the vault,
//! with the vault's key from the system's credential store). Remembering a page rewrites one
//! file; a search reads them one after the other. Nothing is kept in memory between searches
//! beyond a short while (`CACHE_FOR`).

use crate::store::Vault;
use blackhole_domain::recall::{self, Hit, Page};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// The decrypted pages stay in memory this long after the last search (typing a query reads
/// the files once, not at each key).
const CACHE_FOR: Duration = Duration::from_secs(30);

pub enum Request {
    /// Remembers a page (it replaces what was known of its address).
    Index(Page),
    /// Looks for the pages matching `query`; answered by `Event::Results` with the same `id`.
    Search { id: u64, query: String },
    /// Forgets the page at this address (fragment ignored).
    Forget(String),
    /// Forgets every page of a site (a host and the hosts under it).
    ForgetSite(String),
    /// Forgets everything.
    Clear,
    /// Asks how many pages are remembered (`Event::Count`).
    Count,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Results { id: u64, hits: Vec<Hit> },
    Count(usize),
}

/// The channel to the store's thread. Dropping it stops the thread.
pub struct Handle {
    tx: Sender<Request>,
    rx: Receiver<Event>,
}

impl Handle {
    pub fn send(&self, request: Request) {
        let _ = self.tx.send(request);
    }

    /// The next answer, if one is ready.
    pub fn poll(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    /// Waits for the next answer (tests, and nothing on the UI thread).
    pub fn wait(&self, timeout: Duration) -> Option<Event> {
        self.rx.recv_timeout(timeout).ok()
    }
}

#[derive(Serialize, Deserialize)]
struct PageDto {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    at: i64,
}

struct Store {
    dir: PathBuf,
    key: [u8; 32],
    /// Every shard, decrypted, while searches follow one another.
    cache: Option<Vec<Vec<Page>>>,
}

impl Store {
    fn vault(&self, shard: u8) -> Vault {
        Vault::named_with_key(&self.dir, &format!("{shard:02}.bin"), self.key)
    }

    /// The pages of a shard; an unreadable file (another key, a damaged file) counts as empty
    /// and is replaced by the next page filed there.
    fn read(&self, shard: u8) -> Vec<Page> {
        self.vault(shard)
            .read_json::<Vec<PageDto>>()
            .unwrap_or_default()
            .into_iter()
            .map(|p| Page { url: p.url, title: p.title, text: p.text, at: p.at })
            .collect()
    }

    fn write(&self, shard: u8, pages: &[Page]) {
        if pages.is_empty() {
            let _ = self.vault(shard).remove();
            return;
        }
        let dtos: Vec<PageDto> = pages
            .iter()
            .map(|p| PageDto { url: p.url.clone(), title: p.title.clone(), text: p.text.clone(), at: p.at })
            .collect();
        let _ = self.vault(shard).write_json(&dtos);
    }

    fn all(&mut self) -> &Vec<Vec<Page>> {
        if self.cache.is_none() {
            self.cache = Some((0..recall::SHARDS).map(|s| self.read(s)).collect());
        }
        self.cache.as_ref().expect("just filled")
    }

    /// Applies `change` to a shard's pages and writes them back when it says they changed.
    fn update(&mut self, shard: u8, change: impl FnOnce(&mut Vec<Page>) -> bool) {
        let mut pages = match &self.cache {
            Some(cache) => cache[usize::from(shard)].clone(),
            None => self.read(shard),
        };
        if change(&mut pages) {
            self.write(shard, &pages);
            if let Some(cache) = &mut self.cache {
                cache[usize::from(shard)] = pages;
            }
        }
    }

    fn handle(&mut self, request: Request, events: &Sender<Event>) {
        match request {
            Request::Index(page) => {
                let shard = recall::shard_of(&page.url);
                self.update(shard, |pages| {
                    recall::upsert(pages, page);
                    true
                });
            }
            Request::Search { id, query } => {
                let terms = recall::terms(&query);
                let mut hits: Vec<Hit> = self
                    .all()
                    .iter()
                    .flatten()
                    .filter_map(|page| recall::hit(page, &terms))
                    .collect();
                recall::rank(&mut hits);
                let _ = events.send(Event::Results { id, hits });
            }
            Request::Forget(url) => {
                let key = url::Url::parse(&url).map_or(url, |u| recall::page_key(&u));
                self.update(recall::shard_of(&key), |pages| {
                    let before = pages.len();
                    pages.retain(|p| p.url != key);
                    pages.len() != before
                });
            }
            Request::ForgetSite(site) => {
                let list = std::collections::BTreeSet::from([site]);
                let of_site = |p: &Page| {
                    url::Url::parse(&p.url)
                        .ok()
                        .and_then(|u| u.host_str().map(|h| recall::excluded(h, &list)))
                        .unwrap_or(false)
                };
                for shard in 0..recall::SHARDS {
                    self.update(shard, |pages| {
                        let before = pages.len();
                        pages.retain(|p| !of_site(p));
                        pages.len() != before
                    });
                }
            }
            Request::Clear => {
                self.cache = None;
                let _ = std::fs::remove_dir_all(&self.dir);
            }
            Request::Count => {
                let count = match &self.cache {
                    Some(cache) => cache.iter().map(Vec::len).sum(),
                    None => (0..recall::SHARDS).map(|s| self.read(s).len()).sum(),
                };
                let _ = events.send(Event::Count(count));
            }
        }
    }
}

/// Starts the store on `<data>/recall`, sealed with `key`.
pub fn spawn_with_key(data: &Path, key: [u8; 32]) -> Handle {
    start(data, move || Some(key))
}

/// Starts the store with the vault's key from the system's credential store, read on the
/// store's own thread. Without a key (no credential store) nothing is remembered and every
/// search comes back empty.
pub fn spawn(data: &Path) -> Handle {
    start(data, || Vault::shared_key().ok())
}

fn start(data: &Path, key: impl FnOnce() -> Option<[u8; 32]> + Send + 'static) -> Handle {
    let (tx, requests) = channel::<Request>();
    let (events, rx) = channel::<Event>();
    let dir = data.join("recall");
    let _ = std::thread::Builder::new().name("bh-recall".into()).spawn(move || {
        let Some(key) = key() else {
            for request in requests {
                let _ = match request {
                    Request::Search { id, .. } => events.send(Event::Results { id, hits: Vec::new() }),
                    Request::Count => events.send(Event::Count(0)),
                    _ => Ok(()),
                };
            }
            return;
        };
        let mut store = Store { dir, key, cache: None };
        let mut queue: VecDeque<Request> = VecDeque::new();
        let mut last_search = Instant::now();
        loop {
            if queue.is_empty() {
                // Idle: the decrypted pages leave the memory after a while.
                let next = if store.cache.is_some() {
                    match requests.recv_timeout(CACHE_FOR.saturating_sub(last_search.elapsed())) {
                        Ok(request) => Some(request),
                        Err(RecvTimeoutError::Timeout) => {
                            store.cache = None;
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => None,
                    }
                } else {
                    requests.recv().ok()
                };
                match next {
                    Some(request) => queue.push_back(request),
                    None => return,
                }
            }
            queue.extend(requests.try_iter());
            let Some(request) = queue.pop_front() else { continue };
            // A newer query is already waiting: this one is no longer read.
            let stale = matches!(request, Request::Search { .. })
                && queue.iter().any(|r| matches!(r, Request::Search { .. }));
            if stale {
                continue;
            }
            if matches!(request, Request::Search { .. }) {
                last_search = Instant::now();
            }
            store.handle(request, &events);
        }
    });
    Handle { tx, rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    fn page(url: &str, title: &str, text: &str, at: i64) -> Page {
        Page { url: url.into(), title: title.into(), text: text.into(), at }
    }

    fn search(handle: &Handle, id: u64, query: &str) -> Vec<String> {
        handle.send(Request::Search { id, query: query.into() });
        loop {
            match handle.wait(WAIT).expect("an answer") {
                Event::Results { id: got, hits } if got == id => {
                    return hits.into_iter().map(|h| h.url).collect();
                }
                _ => {}
            }
        }
    }

    #[test]
    fn pages_are_found_again_after_a_restart_and_never_stored_in_clear() {
        let dir = tempfile::tempdir().unwrap();
        let handle = spawn_with_key(dir.path(), [3; 32]);
        handle.send(Request::Index(page("https://a.test/hawking", "Hawking radiation", "Black holes evaporate slowly.", 1)));
        handle.send(Request::Index(page("https://b.test/tea", "Tea", "Oolong is partly oxidised.", 2)));
        assert_eq!(search(&handle, 1, "evaporate"), ["https://a.test/hawking"]);
        assert_eq!(search(&handle, 2, "oolong tea"), ["https://b.test/tea"]);
        assert!(search(&handle, 3, "nothing like it").is_empty());
        handle.send(Request::Count);
        assert_eq!(handle.wait(WAIT), Some(Event::Count(2)));
        drop(handle);

        let files: Vec<_> = std::fs::read_dir(dir.path().join("recall")).unwrap().flatten().collect();
        assert!(!files.is_empty());
        for file in files {
            let raw = String::from_utf8_lossy(&std::fs::read(file.path()).unwrap()).into_owned();
            assert!(!raw.contains("evaporate") && !raw.contains("a.test"), "stored in clear");
        }

        let again = spawn_with_key(dir.path(), [3; 32]);
        assert_eq!(search(&again, 1, "hawking"), ["https://a.test/hawking"]);
        // Another key reads nothing (and breaks nothing).
        let other = spawn_with_key(dir.path(), [4; 32]);
        assert!(search(&other, 1, "hawking").is_empty());
    }

    #[test]
    fn without_a_key_nothing_is_kept_and_searches_come_back_empty() {
        let dir = tempfile::tempdir().unwrap();
        let handle = start(dir.path(), || None);
        handle.send(Request::Index(page("https://a.test/", "A", "some text", 1)));
        assert!(search(&handle, 1, "text").is_empty());
        handle.send(Request::Count);
        assert_eq!(handle.wait(WAIT), Some(Event::Count(0)));
        assert!(!dir.path().join("recall").exists());
    }

    #[test]
    fn a_page_is_replaced_forgotten_alone_with_its_site_or_with_everything() {
        let dir = tempfile::tempdir().unwrap();
        let handle = spawn_with_key(dir.path(), [5; 32]);
        handle.send(Request::Index(page("https://a.test/1", "One", "first version of the text", 1)));
        handle.send(Request::Index(page("https://a.test/1", "One", "second version of the text", 2)));
        handle.send(Request::Index(page("https://sub.a.test/2", "Two", "another text", 3)));
        handle.send(Request::Index(page("https://b.test/3", "Three", "a third text", 4)));
        assert!(search(&handle, 1, "first").is_empty());
        assert_eq!(search(&handle, 2, "second"), ["https://a.test/1"]);
        handle.send(Request::Forget("https://a.test/1#anchor".into()));
        assert!(search(&handle, 3, "second").is_empty());
        assert_eq!(search(&handle, 4, "text").len(), 2);
        handle.send(Request::ForgetSite("a.test".into()));
        assert_eq!(search(&handle, 5, "text"), ["https://b.test/3"]);
        handle.send(Request::Clear);
        assert!(search(&handle, 6, "text").is_empty());
        handle.send(Request::Count);
        assert_eq!(handle.wait(WAIT), Some(Event::Count(0)));
        assert!(!dir.path().join("recall").exists());
    }
}

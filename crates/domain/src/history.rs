use std::collections::HashMap;

use url::Url;

const MAX_ENTRIES: usize = 1000;
/// Two loads of the same page this close together (a reload, a redirect that comes back, the
/// title arriving after the address) are one visit.
const SAME_VISIT_SECS: u64 = 30;

/// How a page was reached, which weighs its visit in the command bar's ranking
/// (`suggest::frecency`): an address typed or a result chosen counts most, a step of a
/// redirect not at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisitKind {
    /// A link followed, or a page opened by another page.
    #[default]
    Link,
    /// An address typed, or a result chosen in the command bar or on the start page.
    Typed,
    /// Quick access, a favorite or a bookmark.
    Bookmark,
    /// A page left at once without the user asking: a step of a redirect.
    Redirect,
}

impl VisitKind {
    /// Stored form (`history.json`).
    pub fn id(self) -> u8 {
        match self {
            Self::Link => 0,
            Self::Typed => 1,
            Self::Bookmark => 2,
            Self::Redirect => 3,
        }
    }

    pub fn from_id(id: u8) -> Self {
        match id {
            1 => Self::Typed,
            2 => Self::Bookmark,
            3 => Self::Redirect,
            _ => Self::Link,
        }
    }

    /// Which of two kinds one visit keeps when it is seen twice (a reload): the stronger.
    fn strength(self) -> u8 {
        match self {
            Self::Redirect => 0,
            Self::Link => 1,
            Self::Bookmark => 2,
            Self::Typed => 3,
        }
    }
}

/// One visit of a page: when, and how it was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visit {
    pub at: u64,
    pub kind: VisitKind,
}

/// Visits of a page kept with their date and kind (the newest), for the ranking's recency.
pub const RECENT_VISITS: usize = 10;
/// Spaces a page was seen in, kept per page (the newest).
pub const SPACES_KEPT: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub url: Url,
    pub title: String,
    /// Seconds since the Unix epoch.
    pub visited_at: u64,
    /// How many times the page was visited (at least 1); ranks the command bar's suggestions.
    pub visits: u32,
    /// The last visits (`RECENT_VISITS` at most, newest first); empty in files written before
    /// 0.8.18 (the ranking then reads `visited_at` and `visits`).
    pub recent: Vec<Visit>,
    /// The spaces the page was seen in (ids, `SPACES_KEPT` at most, newest first).
    pub spaces: Vec<u64>,
    /// When the entry last changed on this device (seconds of its clock: a visit, a merge):
    /// what a paired device asks for (`History::changes_since`).
    pub changed: u64,
}

/// Pages forgotten (Library > History) are remembered this long, so a paired device does not
/// bring them back (seconds).
pub const FORGOTTEN_FOR: u64 = 30 * 86_400;
/// Forgotten pages remembered at most.
const FORGOTTEN_MAX: usize = 500;

/// Browsing history, most recent first, one entry per URL. It also remembers what the user
/// erased (`forgotten`, `cleared_at`): a paired device that still has those pages must not
/// bring them back (`merge`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    entries: Vec<HistoryEntry>,
    /// Pages forgotten one by one: (address without fragment, when, in seconds).
    forgotten: Vec<(String, u64)>,
    /// When the whole history was last cleared (seconds; 0: never).
    cleared_at: u64,
}

/// A batch of history changes between paired devices (`History::changes_since`), as
/// `History::merge` takes it: entries changed after a mark of the sender (their spaces already
/// given the receiver's ids), pages forgotten and the clearing made after it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shared {
    pub entries: Vec<HistoryEntry>,
    pub forgotten: Vec<(String, u64)>,
    pub cleared_at: u64,
    /// The sender's mark reached by this batch: the next request starts from it.
    pub until: u64,
    /// More changes wait after `until` (the batch was full): ask again at once.
    pub more: bool,
}

/// The address an entry is known by between devices: no fragment.
pub fn sync_key(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.into()
}

impl History {
    pub fn restore(mut entries: Vec<HistoryEntry>) -> Self {
        entries.sort_by_key(|e| std::cmp::Reverse(e.visited_at));
        entries.truncate(MAX_ENTRIES);
        Self { entries, forgotten: Vec::new(), cleared_at: 0 }
    }

    /// Restores what was erased (from `history.json`).
    pub fn with_erased(mut self, mut forgotten: Vec<(String, u64)>, cleared_at: u64) -> Self {
        forgotten.truncate(FORGOTTEN_MAX);
        self.forgotten = forgotten;
        self.cleared_at = cleared_at;
        self
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn forgotten(&self) -> &[(String, u64)] {
        &self.forgotten
    }

    pub fn cleared_at(&self) -> u64 {
        self.cleared_at
    }

    /// Forgets one page and remembers it was forgotten (for paired devices); returns whether
    /// it was there.
    pub fn forget(&mut self, url: &Url, now: u64) -> bool {
        let key = sync_key(url);
        self.forgotten.retain(|(k, at)| *k != key && now.saturating_sub(*at) < FORGOTTEN_FOR);
        self.forgotten.insert(0, (key.clone(), now));
        self.forgotten.truncate(FORGOTTEN_MAX);
        let before = self.entries.len();
        self.entries.retain(|e| sync_key(&e.url) != key);
        self.entries.len() != before
    }

    /// Clears everything and remembers when (for paired devices).
    pub fn clear_at(&mut self, now: u64) {
        self.entries.clear();
        self.forgotten.clear();
        self.cleared_at = self.cleared_at.max(now);
    }

    /// Merges a batch of changes from a paired device; `now` marks what changes here (so it
    /// goes on to other devices). Idempotent and the same in both directions: visits are united
    /// (the last `RECENT_VISITS`, each once), the count is the larger one, the title is the
    /// latest visit's (a page's own title over a bare host), spaces are united; what either
    /// device erased stays erased, a later visit brings the page back. Returns whether anything
    /// changed here.
    pub fn merge(&mut self, other: &Shared, now: u64) -> bool {
        let mut changed = false;
        if other.cleared_at > self.cleared_at {
            self.cleared_at = other.cleared_at;
            changed = true;
        }
        for (key, at) in &other.forgotten {
            match self.forgotten.iter_mut().find(|(k, _)| k == key) {
                Some((_, mine)) if *at > *mine => {
                    *mine = *at;
                    changed = true;
                }
                Some(_) => {}
                None => {
                    self.forgotten.push((key.clone(), *at));
                    changed = true;
                }
            }
        }
        self.forgotten.sort_by_key(|(_, at)| std::cmp::Reverse(*at));
        self.forgotten.truncate(FORGOTTEN_MAX);
        let forgotten: HashMap<&str, u64> = self.forgotten.iter().map(|(k, at)| (k.as_str(), *at)).collect();
        let cleared = self.cleared_at;
        let erased = |key: &str, at: u64| (cleared > 0 && at <= cleared) || forgotten.get(key).is_some_and(|when| *when >= at);
        let mut index: HashMap<String, usize> =
            self.entries.iter().enumerate().map(|(i, e)| (sync_key(&e.url), i)).collect();
        for theirs in &other.entries {
            if !matches!(theirs.url.scheme(), "http" | "https") {
                continue;
            }
            let key = sync_key(&theirs.url);
            if erased(&key, theirs.visited_at) {
                continue;
            }
            match index.get(&key) {
                Some(&i) => {
                    let mine = &mut self.entries[i];
                    let before = mine.clone();
                    merge_entry(mine, theirs);
                    if *mine != before {
                        mine.changed = now;
                        changed = true;
                    }
                }
                None => {
                    let mut e = theirs.clone();
                    e.url.set_fragment(None);
                    e.recent.truncate(RECENT_VISITS);
                    e.spaces.truncate(SPACES_KEPT);
                    e.visits = e.visits.max(1).max(e.recent.len() as u32);
                    e.changed = now;
                    index.insert(key, self.entries.len());
                    self.entries.push(e);
                    changed = true;
                }
            }
        }
        let before = self.entries.len();
        self.entries.retain(|e| !erased(&sync_key(&e.url), e.visited_at));
        changed |= self.entries.len() != before;
        if changed {
            self.entries.sort_by_key(|e| std::cmp::Reverse(e.visited_at));
            self.entries.truncate(MAX_ENTRIES);
        }
        changed
    }

    /// What changed here after the mark `since` (seconds of this device's clock), oldest change
    /// first, `max` entries at most: the batch a paired device asked for. `until` is the mark to
    /// ask from next time.
    pub fn changes_since(&self, since: u64, max: usize) -> Shared {
        let mut changed: Vec<&HistoryEntry> = self.entries.iter().filter(|e| e.changed > since).collect();
        changed.sort_by_key(|e| e.changed);
        // Marks are seconds: a batch never stops inside one, or the rest of it would be lost
        // (the next request asks for what changed after it). It may exceed `max` for that.
        let more = changed.len() > max;
        if more {
            let last = changed[max.max(1) - 1].changed;
            let keep = changed.iter().take_while(|e| e.changed <= last).count();
            changed.truncate(keep);
        }
        let more = more && self.entries.iter().any(|e| e.changed > changed.last().map_or(since, |e| e.changed));
        let forgotten: Vec<(String, u64)> = self.forgotten.iter().filter(|(_, at)| *at > since).cloned().collect();
        let cleared_at = if self.cleared_at > since { self.cleared_at } else { 0 };
        let until = if more {
            changed.last().map_or(since, |e| e.changed)
        } else {
            changed
                .iter()
                .map(|e| e.changed)
                .chain(forgotten.iter().map(|(_, at)| *at))
                .chain(std::iter::once(cleared_at))
                .fold(since, u64::max)
        };
        Shared { entries: changed.into_iter().cloned().collect(), forgotten, cleared_at, until, more }
    }

    /// Records a visit reached by a link, in no particular space (see `visit_as`).
    pub fn visit(&mut self, url: &Url, now: u64) {
        self.visit_as(url, now, VisitKind::Link, None);
    }

    /// Records a visit. Only http(s) pages are remembered; a revisit moves the entry to the top.
    /// The `#fragment` is dropped: in-page anchors are the same page. A load within
    /// `SAME_VISIT_SECS` of the last is the same visit (it keeps the stronger kind).
    pub fn visit_as(&mut self, url: &Url, now: u64, kind: VisitKind, space: Option<u64>) {
        if !matches!(url.scheme(), "http" | "https") {
            return;
        }
        let mut url = url.clone();
        url.set_fragment(None);
        let url = &url;
        let mut entry = match self.entries.iter().position(|e| &e.url == url) {
            Some(i) => self.entries.remove(i),
            None => HistoryEntry {
                url: url.clone(),
                title: url.host_str().unwrap_or_default().to_owned(),
                visited_at: now,
                visits: 0,
                recent: Vec::new(),
                spaces: Vec::new(),
                changed: now,
            },
        };
        let again = entry.visits == 0 || now.saturating_sub(entry.visited_at) >= SAME_VISIT_SECS;
        if again {
            entry.visits = entry.visits.saturating_add(1);
            entry.recent.insert(0, Visit { at: now, kind });
            entry.recent.truncate(RECENT_VISITS);
        } else if let Some(last) = entry.recent.first_mut() {
            if kind.strength() > last.kind.strength() {
                last.kind = kind;
            }
        } else {
            entry.recent.push(Visit { at: now, kind });
        }
        entry.visits = entry.visits.max(1);
        entry.visited_at = now;
        entry.changed = now;
        if let Some(space) = space {
            entry.spaces.retain(|s| *s != space);
            entry.spaces.insert(0, space);
            entry.spaces.truncate(SPACES_KEPT);
        }
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);
    }

    /// The last visit of `url` was a step the tab left at once (a redirect made by the page):
    /// it no longer counts for the ranking. Returns how it had been reached.
    pub fn bounced(&mut self, url: &Url) -> Option<VisitKind> {
        let mut url = url.clone();
        url.set_fragment(None);
        let last = self.entries.iter_mut().find(|e| e.url == url)?.recent.first_mut()?;
        Some(std::mem::replace(&mut last.kind, VisitKind::Redirect))
    }

    /// Names the entry of `url` with `title` while it is still named after its host (no title
    /// of its own yet).
    pub fn name_if_bare(&mut self, url: &Url, title: &str) {
        let mut url = url.clone();
        url.set_fragment(None);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.url == url) {
            if entry.title == url.host_str().unwrap_or_default() && !title.trim().is_empty() {
                entry.title = title.to_owned();
            }
        }
    }

    pub fn set_title(&mut self, url: &Url, title: &str) {
        if title.trim().is_empty() {
            return;
        }
        let mut url = url.clone();
        url.set_fragment(None);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.url == url) {
            entry.title = title.to_owned();
        }
    }

    pub fn recent(&self, n: usize) -> &[HistoryEntry] {
        &self.entries[..n.min(self.entries.len())]
    }

    /// Forgets one page; returns whether it was there.
    pub fn remove(&mut self, url: &Url) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| &e.url != url);
        self.entries.len() != before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// One page known on both devices: see `History::merge`.
fn merge_entry(mine: &mut HistoryEntry, theirs: &HistoryEntry) {
    let bare = |e: &HistoryEntry| e.title.trim().is_empty() || e.title == e.url.host_str().unwrap_or_default();
    if theirs.visited_at > mine.visited_at {
        if !bare(theirs) || bare(mine) {
            mine.title = theirs.title.clone();
        }
        if mine.url.scheme() == "http" && theirs.url.scheme() == "https" {
            mine.url = theirs.url.clone();
            mine.url.set_fragment(None);
        }
    } else if bare(mine) && !bare(theirs) {
        mine.title = theirs.title.clone();
    }
    mine.visited_at = mine.visited_at.max(theirs.visited_at);
    for v in &theirs.recent {
        if !mine.recent.contains(v) {
            mine.recent.push(*v);
        }
    }
    mine.recent.sort_by_key(|v| std::cmp::Reverse((v.at, v.kind.id())));
    mine.recent.truncate(RECENT_VISITS);
    mine.visits = mine.visits.max(theirs.visits).max(mine.recent.len() as u32).max(1);
    for s in &theirs.spaces {
        if !mine.spaces.contains(s) {
            mine.spaces.push(*s);
        }
    }
    mine.spaces.truncate(SPACES_KEPT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn revisit_moves_to_top_and_keeps_title() {
        let mut h = History::default();
        h.visit(&u("https://a.test/"), 1);
        h.set_title(&u("https://a.test/"), "A");
        h.visit(&u("https://b.test/"), 2);
        h.visit(&u("https://a.test/"), 3);
        assert_eq!(h.entries().len(), 2);
        assert_eq!(h.entries()[0].title, "A");
        assert_eq!(h.entries()[0].visited_at, 3);
    }

    #[test]
    fn anchors_of_a_page_share_one_entry_and_entries_can_be_removed() {
        let mut h = History::default();
        h.visit(&u("https://a.test/doc#intro"), 1);
        h.visit(&u("https://a.test/doc#usage"), 2);
        h.set_title(&u("https://a.test/doc#usage"), "Doc");
        assert_eq!(h.entries().len(), 1);
        assert_eq!(h.entries()[0].title, "Doc");
        assert!(h.remove(&u("https://a.test/doc")));
        assert!(!h.remove(&u("https://a.test/doc")));
        assert!(h.entries().is_empty());
    }

    #[test]
    fn revisits_are_counted_but_a_reload_is_the_same_visit() {
        let mut h = History::default();
        h.visit(&u("https://a.test/"), 100);
        h.visit(&u("https://a.test/"), 110);
        assert_eq!(h.entries()[0].visits, 1);
        h.visit(&u("https://a.test/"), 200);
        h.visit(&u("https://a.test/"), 1000);
        assert_eq!(h.entries()[0].visits, 3);
    }

    #[test]
    fn visits_keep_their_kind_date_and_space() {
        let mut h = History::default();
        h.visit_as(&u("https://a.test/"), 100, VisitKind::Link, Some(1));
        h.visit_as(&u("https://a.test/"), 110, VisitKind::Typed, Some(2));
        let e = &h.entries()[0];
        assert_eq!(e.recent.len(), 1, "a reload is the same visit");
        assert_eq!(e.recent[0].kind, VisitKind::Typed, "it keeps the stronger kind");
        assert_eq!(e.spaces, vec![2, 1]);
        for i in 0..20 {
            h.visit(&u("https://a.test/"), 1000 + i * 100);
        }
        assert_eq!(h.entries()[0].recent.len(), RECENT_VISITS);
        assert_eq!(h.entries()[0].recent[0].at, 2900, "newest first");
        assert_eq!(h.bounced(&u("https://a.test/#x")), Some(VisitKind::Link));
        assert_eq!(h.entries()[0].recent[0].kind, VisitKind::Redirect);
        assert!(h.bounced(&u("https://b.test/")).is_none());
        for k in [VisitKind::Link, VisitKind::Typed, VisitKind::Bookmark, VisitKind::Redirect] {
            assert_eq!(VisitKind::from_id(k.id()), k);
        }
    }

    /// Everything `from` changed after `since`, as one batch (the tests send no more).
    fn batch(from: &History, since: u64) -> Shared {
        from.changes_since(since, 10_000)
    }

    #[test]
    fn two_devices_merge_their_visits_and_agree() {
        let mut a = History::default();
        let mut b = History::default();
        a.visit_as(&u("https://mail.test/"), 100, VisitKind::Typed, Some(1));
        a.visit_as(&u("https://mail.test/"), 1000, VisitKind::Typed, Some(1));
        a.set_title(&u("https://mail.test/"), "Mail");
        b.visit_as(&u("https://mail.test/#inbox"), 500, VisitKind::Link, Some(7));
        b.visit_as(&u("https://news.test/"), 600, VisitKind::Link, None);
        assert!(a.merge(&batch(&b, 0), 2000));
        let mail = a.entries().iter().find(|e| e.url.host_str() == Some("mail.test")).unwrap();
        assert_eq!(mail.recent.len(), 3, "visits of both devices, each once");
        assert_eq!(mail.visits, 3);
        assert_eq!(mail.title, "Mail", "a bare host never replaces a title");
        assert_eq!(mail.spaces, vec![1, 7]);
        assert_eq!(mail.changed, 2000, "a merged change goes on to other devices");
        assert!(!a.merge(&batch(&b, 0), 3000), "merging twice changes nothing");
        assert!(b.merge(&batch(&a, 0), 2000));
        assert!(!a.merge(&batch(&b, 0), 4000) && !b.merge(&batch(&a, 0), 4000), "they agree");
        assert_eq!(a.entries().len(), b.entries().len());
    }

    #[test]
    fn batches_carry_only_what_changed_after_the_mark_oldest_first() {
        let mut h = History::default();
        for i in 0..5u64 {
            h.visit(&u(&format!("https://p{i}.test/")), 100 + i);
        }
        let first = h.changes_since(0, 3);
        assert!(first.more);
        assert_eq!(first.until, 102);
        assert_eq!(first.entries.iter().map(|e| e.visited_at).collect::<Vec<_>>(), [100, 101, 102]);
        let rest = h.changes_since(first.until, 3);
        assert!(!rest.more);
        assert_eq!(rest.entries.len(), 2);
        assert_eq!(rest.until, 104);
        let none = h.changes_since(rest.until, 3);
        assert!(none.entries.is_empty() && none.forgotten.is_empty() && none.cleared_at == 0);
        assert_eq!(none.until, 104, "nothing new: the mark stays");
        h.forget(&u("https://p0.test/"), 200);
        let gone = h.changes_since(104, 3);
        assert_eq!(gone.forgotten, vec![("https://p0.test/".to_owned(), 200)]);
        assert_eq!(gone.until, 200);
    }

    #[test]
    fn what_one_device_erased_never_comes_back() {
        let mut a = History::default();
        let mut b = History::default();
        a.visit(&u("https://secret.test/"), 100);
        a.visit(&u("https://kept.test/"), 100);
        b.merge(&batch(&a, 0), 150);
        a.forget(&u("https://secret.test/#x"), 200);
        b.merge(&batch(&a, 150), 250);
        assert!(b.entries().iter().all(|e| e.url.host_str() != Some("secret.test")));
        // b sending its older copy back does not revive it on a.
        a.merge(&batch(&b, 0), 260);
        assert!(a.entries().iter().all(|e| e.url.host_str() != Some("secret.test")));
        // Visited again after it was forgotten: it is history again.
        b.visit(&u("https://secret.test/"), 300);
        a.merge(&batch(&b, 0), 310);
        assert!(a.entries().iter().any(|e| e.url.host_str() == Some("secret.test")));
        // Cleared on one device: everything before goes on both.
        b.clear_at(400);
        a.visit(&u("https://after.test/"), 500);
        a.merge(&batch(&b, 0), 510);
        let left: Vec<_> = a.entries().iter().map(|e| e.url.host_str().unwrap().to_owned()).collect();
        assert_eq!(left, ["after.test"]);
    }

    #[test]
    fn ignores_non_web_urls() {
        let mut h = History::default();
        h.visit(&u("data:text/html,hi"), 1);
        assert!(h.entries().is_empty());
    }
}

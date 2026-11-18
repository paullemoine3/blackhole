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
}

/// Browsing history, most recent first, one entry per URL.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    entries: Vec<HistoryEntry>,
}

impl History {
    pub fn restore(mut entries: Vec<HistoryEntry>) -> Self {
        entries.sort_by_key(|e| std::cmp::Reverse(e.visited_at));
        entries.truncate(MAX_ENTRIES);
        Self { entries }
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
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

    #[test]
    fn ignores_non_web_urls() {
        let mut h = History::default();
        h.visit(&u("data:text/html,hi"), 1);
        assert!(h.entries().is_empty());
    }
}

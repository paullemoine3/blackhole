//! Pulsars: folders of the sidebar that fill themselves from a site's feed (RSS, Atom) or from
//! GitHub. Their lines are addresses, not tabs: nothing runs until one is opened. A line of a feed
//! leaves once read; one of GitHub stays until it is dealt with there (merged, closed, reviewed).

use url::Url;

/// Pulsars at most, every space together.
pub const MAX: usize = 30;
/// Entries remembered per Pulsar (read or not, so that a read one does not come back).
pub const KEEP: usize = 80;
/// Entries unread when a Pulsar starts: the rest of the feed is old news.
pub const FIRST: usize = 5;
/// Each failure in a row doubles the delay between two readings, up to this (seconds).
pub const EVERY_MAX: i64 = 8 * 3600;
/// Choices of the Settings: minutes between two readings, lines shown under a Pulsar.
pub const EVERY_CHOICES: [u32; 4] = [15, 30, 60, 180];
pub const DEFAULT_EVERY: u32 = 30;
pub const LINES_CHOICES: [u32; 4] = [5, 8, 12, 20];
pub const DEFAULT_LINES: u32 = 8;
const NAME_MAX: usize = 60;
const TITLE_MAX: usize = 200;

/// An entry of a feed, as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What the feed knows it by (its id, or its address).
    pub key: String,
    pub title: String,
    pub url: Url,
    /// Seconds since the epoch; 0 when the feed does not say.
    pub at: i64,
    /// Where it comes from in a word (`wry #12`), empty for a feed.
    pub note: String,
}

/// What a Pulsar of GitHub gathers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubKind {
    /// Pull requests waiting for the user's review.
    Reviews,
    /// The user's open pull requests.
    Mine,
    /// Issues and pull requests assigned to the user.
    Assigned,
    /// Open ones that mention the user.
    Mentions,
}

impl GithubKind {
    pub const ALL: [Self; 4] = [Self::Reviews, Self::Mine, Self::Assigned, Self::Mentions];

    /// Stored form, and the last part of its text key (`pulsar.github_<id>`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Reviews => "reviews",
            Self::Mine => "mine",
            Self::Assigned => "assigned",
            Self::Mentions => "mentions",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

/// A search on GitHub, in every repository or in one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubQuery {
    pub kind: GithubKind,
    /// `owner/name`, or empty.
    pub repo: String,
}

impl GithubQuery {
    /// `repo` as typed: `owner/name`, its address, or nothing. Only the characters GitHub allows:
    /// it goes into a search.
    pub fn new(kind: GithubKind, repo: &str) -> Option<Self> {
        let raw = repo.trim().trim_end_matches('/');
        let raw = raw.trim_start_matches("https://").trim_start_matches("github.com/").trim_end_matches(".git");
        if raw.is_empty() {
            return Some(Self { kind, repo: String::new() });
        }
        let (owner, name) = raw.split_once('/')?;
        let ok = |s: &str| {
            !s.is_empty()
                && s.len() <= 100
                && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        (ok(owner) && ok(name)).then(|| Self { kind, repo: format!("{owner}/{name}") })
    }

    /// The search, in GitHub's words.
    pub fn search(&self) -> String {
        let what = match self.kind {
            GithubKind::Reviews => "is:pr review-requested:@me",
            GithubKind::Mine => "is:pr author:@me",
            GithubKind::Assigned => "assignee:@me",
            GithubKind::Mentions => "mentions:@me",
        };
        let mut q = format!("is:open archived:false {what}");
        if !self.repo.is_empty() {
            q.push_str(" repo:");
            q.push_str(&self.repo);
        }
        q
    }

    /// The same on the site, which also tells two Pulsars apart.
    pub fn page(&self) -> Url {
        let mut url = Url::parse("https://github.com/issues").expect("static url");
        url.query_pairs_mut().append_pair("q", &self.search());
        url
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub entry: Entry,
    pub read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pulsar {
    pub id: u64,
    pub space: u64,
    pub name: String,
    /// The feed, or for GitHub the page of the same search.
    pub feed: Url,
    pub github: Option<GithubQuery>,
    /// With the folders rather than the open tabs.
    pub pinned: bool,
    pub collapsed: bool,
    pub lines: Vec<Line>,
    /// When the feed was last asked, and the failures in a row.
    pub checked_at: i64,
    pub failures: u32,
    /// What the server gave to ask only for what is new.
    pub etag: String,
    pub modified: String,
}

impl Pulsar {
    pub fn unread(&self) -> usize {
        self.lines.iter().filter(|l| !l.read).count()
    }

    /// The lines of its folder, newest first: the unread ones; GitHub's stay once seen.
    pub fn shown(&self, max: usize) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|l| self.github.is_some() || !l.read).take(max)
    }

    /// Lines there would be without `max`.
    pub fn total(&self) -> usize {
        if self.github.is_some() { self.lines.len() } else { self.unread() }
    }

    pub fn due(&self, now: i64, every_minutes: u32) -> bool {
        let every = ((i64::from(every_minutes.max(1)) * 60) << self.failures.min(10)).min(EVERY_MAX);
        // A clock set back does not stop it.
        now < self.checked_at || now - self.checked_at >= every
    }
}

/// One line of text from a feed: no control character, spaces gathered, cut.
pub fn clean(text: &str, max: usize) -> String {
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// An entry worth keeping: a web address, and a title (its address when it has none).
pub fn entry(key: &str, title: &str, url: Url, at: i64) -> Option<Entry> {
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    let title = clean(title, TITLE_MAX);
    let title = if title.is_empty() { clean(url.as_str(), TITLE_MAX) } else { title };
    let key = if key.trim().is_empty() { url.as_str() } else { key.trim() };
    Some(Entry { key: key.chars().take(500).collect(), title, url, at, note: String::new() })
}

/// The feed a site is known to have without asking it: the releases of a GitHub repository.
pub fn known_feed(url: &Url) -> Option<Url> {
    if !matches!(url.host_str(), Some("github.com" | "www.github.com")) {
        return None;
    }
    let parts: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let [owner, repo] = parts[..] else { return None };
    Url::parse(&format!("https://github.com/{owner}/{repo}/releases.atom")).ok()
}

/// The Pulsars of every space, in the order they were made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pulsars {
    list: Vec<Pulsar>,
}

impl Pulsars {
    pub fn restore(mut list: Vec<Pulsar>) -> Self {
        list.truncate(MAX);
        let mut seen = std::collections::BTreeSet::new();
        list.retain(|p| seen.insert(p.id) && matches!(p.feed.scheme(), "http" | "https"));
        for p in &mut list {
            p.lines.truncate(KEEP);
        }
        Self { list }
    }

    pub fn all(&self) -> &[Pulsar] {
        &self.list
    }

    pub fn get(&self, id: u64) -> Option<&Pulsar> {
        self.list.iter().find(|p| p.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Pulsar> {
        self.list.iter_mut().find(|p| p.id == id)
    }

    pub fn in_space(&self, space: u64) -> impl Iterator<Item = &Pulsar> {
        self.list.iter().filter(move |p| p.space == space)
    }

    /// A new Pulsar in `space`, or the one already there for that feed. `None`: too many.
    pub fn add(&mut self, space: u64, name: &str, feed: Url) -> Option<u64> {
        self.make(space, name, feed, None)
    }

    /// The same for a search on GitHub.
    pub fn add_github(&mut self, space: u64, name: &str, query: GithubQuery) -> Option<u64> {
        self.make(space, name, query.page(), Some(query))
    }

    fn make(&mut self, space: u64, name: &str, feed: Url, github: Option<GithubQuery>) -> Option<u64> {
        if let Some(p) = self.list.iter().find(|p| p.space == space && p.feed == feed) {
            return Some(p.id);
        }
        if self.list.len() >= MAX {
            return None;
        }
        let id = self.list.iter().map(|p| p.id).max().unwrap_or(0) + 1;
        let name = clean(name, NAME_MAX);
        let name = if name.is_empty() {
            feed.host_str().unwrap_or_default().trim_start_matches("www.").to_owned()
        } else {
            name
        };
        self.list.push(Pulsar {
            id,
            space,
            name,
            feed,
            github,
            pinned: false,
            collapsed: false,
            lines: Vec::new(),
            checked_at: 0,
            failures: 0,
            etag: String::new(),
            modified: String::new(),
        });
        Some(id)
    }

    /// What a feed holds now. Read entries stay read; a first reading leaves `FIRST` of them
    /// unread. Returns how many are new.
    pub fn merge(&mut self, id: u64, mut entries: Vec<Entry>, now: i64) -> usize {
        let Some(p) = self.get_mut(id) else { return 0 };
        let github = p.github.is_some();
        let first = p.lines.is_empty() && p.checked_at == 0 && !github;
        p.checked_at = now;
        p.failures = 0;
        // Newest first; without dates, the order of the feed.
        entries.sort_by_key(|e| std::cmp::Reverse(e.at));
        let mut seen = std::collections::HashSet::new();
        entries.retain(|e| seen.insert(e.key.clone()));
        entries.truncate(KEEP);
        let mut new = 0;
        let mut lines: Vec<Line> = Vec::with_capacity(KEEP);
        for entry in entries {
            let known = p.lines.iter().find(|l| l.entry.key == entry.key);
            // Something happened there since it was seen.
            let known = known.map(|l| l.read && !(github && entry.at > l.entry.at));
            let read = known.unwrap_or(first && new >= FIRST);
            if known.is_none() && !read {
                new += 1;
            }
            lines.push(Line { entry, read });
        }
        // What the feed no longer lists is remembered while there is room.
        // GitHub's answer is the whole truth: what is not there is dealt with.
        let gone = p.lines.drain(..).filter(|l| !github && !seen.contains(&l.entry.key));
        lines.extend(gone);
        lines.truncate(KEEP);
        p.lines = lines;
        new
    }

    /// The server has nothing new.
    pub fn unchanged(&mut self, id: u64, now: i64) {
        if let Some(p) = self.get_mut(id) {
            p.checked_at = now;
            p.failures = 0;
        }
    }

    pub fn failed(&mut self, id: u64, now: i64) {
        if let Some(p) = self.get_mut(id) {
            p.checked_at = now;
            p.failures = p.failures.saturating_add(1);
        }
    }

    pub fn set_marks(&mut self, id: u64, etag: String, modified: String) {
        if let Some(p) = self.get_mut(id) {
            p.etag = etag;
            p.modified = modified;
        }
    }

    /// A line is opened: it leaves. Returns its address.
    pub fn open(&mut self, id: u64, key: &str) -> Option<Url> {
        let line = self.get_mut(id)?.lines.iter_mut().find(|l| l.entry.key == key)?;
        line.read = true;
        Some(line.entry.url.clone())
    }

    pub fn read_all(&mut self, id: u64) {
        if let Some(p) = self.get_mut(id) {
            p.lines.iter_mut().for_each(|l| l.read = true);
        }
    }

    pub fn toggle(&mut self, id: u64) {
        if let Some(p) = self.get_mut(id) {
            p.collapsed = !p.collapsed;
        }
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) {
        if let Some(p) = self.get_mut(id) {
            p.pinned = pinned;
        }
    }

    /// To another space; a Pulsar there for the same thing stays, this one does not move.
    pub fn move_to(&mut self, id: u64, space: u64) -> bool {
        let Some(feed) = self.get(id).map(|p| p.feed.clone()) else { return false };
        if self.list.iter().any(|p| p.space == space && p.feed == feed) {
            return false;
        }
        if let Some(p) = self.get_mut(id) {
            p.space = space;
        }
        true
    }

    pub fn rename(&mut self, id: u64, name: &str) {
        let name = clean(name, NAME_MAX);
        if let (Some(p), false) = (self.get_mut(id), name.is_empty()) {
            p.name = name;
        }
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.list.len();
        self.list.retain(|p| p.id != id);
        self.list.len() != before
    }

    /// Forgets those of a space: the id of a deleted one may be given again.
    pub fn forget_space(&mut self, space: u64) -> bool {
        let before = self.list.len();
        self.list.retain(|p| p.space != space);
        self.list.len() != before
    }

    /// The Pulsar that has waited longest, when it is time to ask (GitHub's only with an account).
    pub fn next_due(&self, now: i64, every_minutes: u32, github: bool) -> Option<&Pulsar> {
        self.list.iter().filter(|p| p.due(now, every_minutes) && (github || p.github.is_none())).min_by_key(|p| p.checked_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn entries(range: std::ops::Range<i64>) -> Vec<Entry> {
        range.map(|i| entry(&format!("k{i}"), &format!("T{i}"), url(&format!("https://a.test/{i}")), i).unwrap()).collect()
    }

    fn one() -> (Pulsars, u64) {
        let mut p = Pulsars::default();
        let id = p.add(1, "  A \n blog ", url("https://a.test/feed")).unwrap();
        (p, id)
    }

    #[test]
    fn a_pulsar_starts_with_the_newest_entries_and_gains_the_new_ones() {
        let (mut p, id) = one();
        assert_eq!(p.get(id).unwrap().name, "A blog");
        assert_eq!(p.merge(id, entries(1..20), 100), FIRST);
        let titles: Vec<_> = p.get(id).unwrap().shown(8).map(|l| l.entry.title.clone()).collect();
        assert_eq!(titles, ["T19", "T18", "T17", "T16", "T15"]);
        assert_eq!(p.merge(id, entries(1..22), 200), 2);
        assert_eq!(p.get(id).unwrap().unread(), 7);
        assert_eq!(p.merge(id, entries(1..22), 300), 0, "nothing twice");
    }

    #[test]
    fn a_line_leaves_once_opened_and_does_not_come_back() {
        let (mut p, id) = one();
        p.merge(id, entries(1..4), 100);
        assert_eq!(p.open(id, "k3").unwrap().as_str(), "https://a.test/3");
        p.merge(id, entries(1..4), 200);
        assert_eq!(p.get(id).unwrap().unread(), 2);
        // Out of the feed for a while, then back: still read.
        p.merge(id, entries(1..3), 300);
        p.merge(id, entries(1..4), 400);
        assert!(p.get(id).unwrap().shown(8).all(|l| l.entry.key != "k3"));
        p.read_all(id);
        assert_eq!(p.get(id).unwrap().unread(), 0);
        assert!(p.open(id, "nope").is_none());
    }

    #[test]
    fn a_feed_is_asked_every_half_hour_and_less_often_when_it_fails() {
        const EVERY: i64 = 30 * 60;
        let (mut p, id) = one();
        let due = |p: &Pulsars, now: i64| p.next_due(now, 30, true).is_some();
        assert!(due(&p, 1_000_000), "never asked");
        p.merge(id, Vec::new(), 1_000_000);
        assert!(!due(&p, 1_000_000 + EVERY - 1));
        assert!(due(&p, 1_000_000 + EVERY));
        assert!(p.next_due(1_000_000 + EVERY, 60, true).is_none(), "the user's own delay");
        p.failed(id, 2_000_000);
        assert!(!p.get(id).unwrap().due(2_000_000 + EVERY, 30));
        assert!(p.get(id).unwrap().due(2_000_000 + 2 * EVERY, 30));
        for _ in 0..40 {
            p.failed(id, 2_000_000);
        }
        assert!(p.get(id).unwrap().due(2_000_000 + EVERY_MAX, 30));
        assert!(p.get(id).unwrap().due(5, 30), "a clock set back does not stop it");
        p.unchanged(id, 3_000_000);
        assert!(p.get(id).unwrap().due(3_000_000 + EVERY, 30));
    }

    #[test]
    fn pulsars_are_one_per_feed_and_space_and_capped() {
        let (mut p, id) = one();
        assert_eq!(p.add(1, "again", url("https://a.test/feed")), Some(id));
        assert_ne!(p.add(2, "", url("https://www.a.test/feed")), Some(id));
        assert_eq!(p.all()[1].name, "a.test");
        assert!(p.forget_space(2));
        for i in 1..MAX {
            assert!(p.add(1, "", url(&format!("https://a.test/{i}"))).is_some());
        }
        assert_eq!(p.add(1, "", url("https://a.test/more")), None);
        p.set_pinned(id, true);
        p.rename(id, "   ");
        assert!(p.get(id).unwrap().pinned);
        assert_eq!(p.get(id).unwrap().name, "A blog", "a blank name changes nothing");
        assert!(p.remove(id) && !p.remove(id));
    }

    #[test]
    fn only_web_addresses_and_plain_titles_are_kept() {
        assert!(entry("k", "x", url("javascript:alert(1)"), 0).is_none());
        assert!(entry("k", "x", url("file:///etc/passwd"), 0).is_none());
        let e = entry(" ", " two\n lines\u{7} ", url("https://a.test/"), 0).unwrap();
        assert_eq!((e.key.as_str(), e.title.as_str()), ("https://a.test/", "two lines"));
        assert_eq!(entry("k", "", url("https://a.test/"), 0).unwrap().title, "https://a.test/");
    }

    #[test]
    fn a_github_repository_is_followed_by_its_releases() {
        let of = |s: &str| known_feed(&url(s)).map(|u| u.to_string());
        assert_eq!(of("https://github.com/o/r/").as_deref(), Some("https://github.com/o/r/releases.atom"));
        assert_eq!(of("https://github.com/o"), None);
        assert_eq!(of("https://github.com/o/r/commits.atom"), None);
        assert_eq!(of("https://example.test/o/r"), None);
    }

    #[test]
    fn a_line_of_github_stays_until_it_is_dealt_with_there() {
        let mut p = Pulsars::default();
        let query = GithubQuery::new(GithubKind::Reviews, "").unwrap();
        let id = p.add_github(1, "Reviews", query.clone()).unwrap();
        assert_eq!(p.add_github(1, "again", query), Some(id));
        assert_eq!(p.merge(id, entries(1..9), 100), 8, "all of it: nothing there is old news");
        p.open(id, "k8");
        let g = p.get(id).unwrap();
        assert_eq!((g.unread(), g.total(), g.shown(20).count()), (7, 8, 8), "seen, and still there");
        // It moved since: to be seen again. Another is merged: gone.
        let mut next = entries(2..9);
        next.iter_mut().filter(|e| e.key == "k8").for_each(|e| e.at = 50);
        p.merge(id, next, 200);
        let g = p.get(id).unwrap();
        assert_eq!((g.unread(), g.total()), (7, 7));
        assert_eq!(g.lines[0].entry.key, "k8");
        assert!(p.next_due(1_000_000, 30, false).is_none(), "no account: never asked");
        assert!(p.next_due(1_000_000, 30, true).is_some());
    }

    #[test]
    fn a_github_search_holds_only_what_it_should() {
        let q = |repo: &str| GithubQuery::new(GithubKind::Mine, repo);
        assert_eq!(q("").unwrap().search(), "is:open archived:false is:pr author:@me");
        assert_eq!(q("https://github.com/o/r.git/").unwrap().repo, "o/r");
        assert!(q("o/r").unwrap().search().ends_with(" repo:o/r"));
        for bad in ["o", "o/r/x", "o/r is:closed", "o/r\"", "/r", "o/"] {
            assert!(q(bad).is_none(), "{bad}");
        }
        for k in GithubKind::ALL {
            assert_eq!(GithubKind::from_id(k.id()), Some(k));
        }
        assert_ne!(q("").unwrap().page(), q("o/r").unwrap().page());
    }

    #[test]
    fn a_pulsar_moves_to_a_space_that_does_not_have_it() {
        let (mut p, id) = one();
        let twin = p.add(2, "", url("https://a.test/feed")).unwrap();
        assert!(!p.move_to(id, 2));
        assert!(p.remove(twin) && p.move_to(id, 2));
        assert_eq!(p.in_space(2).count(), 1);
    }
}

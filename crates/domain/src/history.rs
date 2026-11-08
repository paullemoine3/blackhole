use url::Url;

const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub url: Url,
    pub title: String,
    /// Seconds since the Unix epoch.
    pub visited_at: u64,
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

    /// Records a visit. Only http(s) pages are remembered; a revisit moves the entry to the top.
    /// The `#fragment` is dropped: in-page anchors are the same page.
    pub fn visit(&mut self, url: &Url, now: u64) {
        if !matches!(url.scheme(), "http" | "https") {
            return;
        }
        let mut url = url.clone();
        url.set_fragment(None);
        let url = &url;
        let title = match self.entries.iter().position(|e| &e.url == url) {
            Some(i) => self.entries.remove(i).title,
            None => url.host_str().unwrap_or_default().to_owned(),
        };
        self.entries.insert(
            0,
            HistoryEntry {
                url: url.clone(),
                title,
                visited_at: now,
            },
        );
        self.entries.truncate(MAX_ENTRIES);
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
    fn ignores_non_web_urls() {
        let mut h = History::default();
        h.visit(&u("data:text/html,hi"), 1);
        assert!(h.entries().is_empty());
    }
}

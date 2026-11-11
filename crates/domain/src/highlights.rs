//! Highlights and notes on pages: a passage the user marked is kept with a little of the text
//! around it, so it can be found again when the page is opened later (even if the page moved
//! things around), and may carry a note. Pure rules; everything stays on this computer.
//!
//! What comes from a page (the quote and its context) is untrusted: bounded and cleaned here.

use url::Url;

/// Pages that may carry highlights, and highlights on one page.
pub const MAX_PAGES: usize = 500;
pub const MAX_PER_PAGE: usize = 100;
/// Longest passage, context on each side and note, in characters.
pub const QUOTE_MAX: usize = 1000;
pub const CONTEXT_MAX: usize = 48;
pub const NOTE_MAX: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Highlight {
    pub id: u64,
    /// The marked text, white space collapsed.
    pub quote: String,
    /// The text just before and just after it: tells two identical passages apart.
    pub prefix: String,
    pub suffix: String,
    pub note: String,
    /// Unix seconds.
    pub created: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The page's address without its fragment.
    pub url: String,
    pub title: String,
    pub marks: Vec<Highlight>,
}

/// Highlights by page, the page marked last first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Highlights {
    pages: Vec<Page>,
    next_id: u64,
}

/// One line of clean text: control and invisible characters dropped, white space collapsed,
/// at most `max` characters.
pub fn clean(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut gap = false;
    let mut count = 0;
    for c in text.chars() {
        if c.is_whitespace() {
            gap = !out.is_empty();
            continue;
        }
        let invisible = matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}');
        if c.is_control() || invisible {
            continue;
        }
        let needed = 1 + usize::from(gap);
        if count + needed > max {
            break;
        }
        if gap {
            out.push(' ');
            gap = false;
        }
        out.push(c);
        count += needed;
    }
    out
}

/// A note keeps its line breaks: only control characters go, and it is bounded.
pub fn clean_note(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .take(NOTE_MAX)
        .collect();
    kept.trim().to_owned()
}

/// The key highlights are filed under: a web address without its fragment.
pub fn page_key(url: &Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let mut url = url.clone();
    url.set_fragment(None);
    Some(url.into())
}

impl Highlights {
    /// Rebuilds the list from stored data, dropping what breaks its rules.
    pub fn restore(pages: Vec<Page>) -> Self {
        let mut out = Self::default();
        for mut page in pages {
            let Some(key) = Url::parse(&page.url).ok().and_then(|u| page_key(&u)) else {
                continue;
            };
            if out.pages.len() >= MAX_PAGES || out.pages.iter().any(|p| p.url == key) {
                continue;
            }
            page.url = key;
            page.title = clean(&page.title, 200);
            page.marks.retain(|m| !m.quote.is_empty() && m.id != 0);
            page.marks.truncate(MAX_PER_PAGE);
            if page.marks.is_empty() {
                continue;
            }
            out.next_id = out.next_id.max(page.marks.iter().map(|m| m.id).max().unwrap_or(0));
            out.pages.push(page);
        }
        // Ids are unique across pages: a duplicate (edited file) is renumbered.
        let mut seen = std::collections::HashSet::new();
        for mark in out.pages.iter_mut().flat_map(|p| p.marks.iter_mut()) {
            if !seen.insert(mark.id) {
                out.next_id += 1;
                mark.id = out.next_id;
                seen.insert(mark.id);
            }
        }
        out
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn count(&self) -> usize {
        self.pages.iter().map(|p| p.marks.len()).sum()
    }

    /// The highlights of the page at `url` (fragment ignored).
    pub fn for_page(&self, url: &Url) -> &[Highlight] {
        page_key(url)
            .and_then(|key| self.pages.iter().find(|p| p.url == key))
            .map_or(&[], |p| p.marks.as_slice())
    }

    /// The page and highlight of an id.
    pub fn get(&self, id: u64) -> Option<(&Page, &Highlight)> {
        self.pages
            .iter()
            .find_map(|p| p.marks.iter().find(|m| m.id == id).map(|m| (p, m)))
    }

    /// Marks a passage of the page at `url`. `None` when there is nothing to mark, when the
    /// same passage is already marked there, or when a limit is reached.
    pub fn add(
        &mut self,
        url: &Url,
        title: &str,
        quote: &str,
        prefix: &str,
        suffix: &str,
        now: i64,
    ) -> Option<u64> {
        let key = page_key(url)?;
        let quote = clean(quote, QUOTE_MAX);
        if quote.is_empty() {
            return None;
        }
        // The context is kept from the side that touches the passage.
        let prefix = clean(prefix, usize::MAX);
        let skip = prefix.chars().count().saturating_sub(CONTEXT_MAX);
        let prefix: String = prefix.chars().skip(skip).collect();
        let suffix = clean(suffix, CONTEXT_MAX);
        let at = match self.pages.iter().position(|p| p.url == key) {
            Some(at) => at,
            None if self.pages.len() < MAX_PAGES => {
                self.pages.push(Page { url: key, title: String::new(), marks: Vec::new() });
                self.pages.len() - 1
            }
            None => return None,
        };
        let page = &mut self.pages[at];
        let same = |m: &Highlight| m.quote == quote && m.prefix == prefix && m.suffix == suffix;
        if page.marks.len() >= MAX_PER_PAGE || page.marks.iter().any(same) {
            if page.marks.is_empty() {
                self.pages.remove(at);
            }
            return None;
        }
        self.next_id += 1;
        let title = clean(title, 200);
        if !title.is_empty() {
            page.title = title;
        }
        page.marks.push(Highlight {
            id: self.next_id,
            quote,
            prefix,
            suffix,
            note: String::new(),
            created: now,
        });
        // The page marked last comes first.
        let page = self.pages.remove(at);
        self.pages.insert(0, page);
        Some(self.next_id)
    }

    /// Removes a highlight (and its page once it has none left).
    pub fn remove(&mut self, id: u64) -> bool {
        let Some(at) = self.pages.iter().position(|p| p.marks.iter().any(|m| m.id == id)) else {
            return false;
        };
        self.pages[at].marks.retain(|m| m.id != id);
        if self.pages[at].marks.is_empty() {
            self.pages.remove(at);
        }
        true
    }

    /// Writes (or, when blank, erases) the note of a highlight.
    pub fn set_note(&mut self, id: u64, note: &str) -> bool {
        let mark = self
            .pages
            .iter_mut()
            .flat_map(|p| p.marks.iter_mut())
            .find(|m| m.id == id);
        match mark {
            Some(mark) => {
                mark.note = clean_note(note);
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(raw: &str) -> Url {
        Url::parse(raw).unwrap()
    }

    #[test]
    fn a_passage_is_cleaned_bounded_and_filed_under_its_page() {
        let mut h = Highlights::default();
        let id = h
            .add(&u("https://a.test/doc#part2"), " Doc\n", "black \n\t holes\u{200B}  evaporate", "x".repeat(80).as_str(), " slowly. And so on, for a very long time indeed, far beyond the context.", 10)
            .unwrap();
        let marks = h.for_page(&u("https://a.test/doc"));
        assert_eq!(marks.len(), 1);
        assert_eq!((marks[0].id, marks[0].quote.as_str()), (id, "black holes evaporate"));
        assert_eq!(marks[0].prefix.chars().count(), CONTEXT_MAX);
        assert!(marks[0].suffix.starts_with("slowly.") && marks[0].suffix.chars().count() <= CONTEXT_MAX);
        assert_eq!(h.pages()[0].title, "Doc");
        // The same passage twice, an empty one, a page that is not on the web: nothing.
        assert_eq!(h.add(&u("https://a.test/doc"), "", "black holes evaporate", &"x".repeat(80), " slowly. And so on, for a very long time indeed, far beyond the context.", 11), None);
        assert_eq!(h.add(&u("https://a.test/doc"), "", " \n ", "", "", 11), None);
        assert_eq!(h.add(&u("file:///etc/passwd"), "", "root", "", "", 11), None);
        assert_eq!(h.count(), 1);
        let long = "é".repeat(QUOTE_MAX + 50);
        let id = h.add(&u("https://a.test/doc"), "", &long, "", "", 12).unwrap();
        assert_eq!(h.get(id).unwrap().1.quote.chars().count(), QUOTE_MAX);
    }

    #[test]
    fn the_same_words_elsewhere_on_the_page_are_another_highlight() {
        let mut h = Highlights::default();
        let page = u("https://a.test/");
        let a = h.add(&page, "", "the end", "This is", "of part one", 1).unwrap();
        let b = h.add(&page, "", "the end", "Here comes", "of it all", 2).unwrap();
        assert_ne!(a, b);
        assert_eq!(h.for_page(&page).len(), 2);
    }

    #[test]
    fn notes_removal_and_order() {
        let mut h = Highlights::default();
        let a = h.add(&u("https://a.test/"), "A", "one", "", "", 1).unwrap();
        let b = h.add(&u("https://b.test/"), "B", "two", "", "", 2).unwrap();
        assert_eq!(h.pages()[0].url, "https://b.test/");
        h.add(&u("https://a.test/"), "", "three", "", "", 3).unwrap();
        assert_eq!(h.pages()[0].url, "https://a.test/", "the page marked last comes first");
        assert_eq!(h.pages()[0].title, "A", "an empty title keeps the one known");
        assert!(h.set_note(a, "  line one\nline\u{7} two  "));
        assert_eq!(h.get(a).unwrap().1.note, "line one\nline two");
        assert!(h.set_note(a, &"n".repeat(NOTE_MAX + 9)));
        assert_eq!(h.get(a).unwrap().1.note.len(), NOTE_MAX);
        assert!(!h.set_note(999, "x"));
        assert!(h.remove(b) && !h.remove(b));
        assert_eq!(h.pages().len(), 1, "a page without highlights is forgotten");
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn limits_hold_and_stored_data_is_checked() {
        let mut h = Highlights::default();
        let page = u("https://a.test/");
        for i in 0..MAX_PER_PAGE + 5 {
            h.add(&page, "", &format!("passage {i}"), "", "", 0);
        }
        assert_eq!(h.for_page(&page).len(), MAX_PER_PAGE);
        let mark = |id, quote: &str| Highlight {
            id,
            quote: quote.to_owned(),
            prefix: String::new(),
            suffix: String::new(),
            note: String::new(),
            created: 0,
        };
        let restored = Highlights::restore(vec![
            Page { url: "https://a.test/x#frag".into(), title: "A".into(), marks: vec![mark(4, "kept"), mark(0, "no id"), mark(5, "")] },
            Page { url: "javascript:alert(1)".into(), title: String::new(), marks: vec![mark(6, "bad")] },
            Page { url: "https://b.test/".into(), title: String::new(), marks: vec![mark(4, "same id")] },
            Page { url: "https://c.test/".into(), title: String::new(), marks: vec![] },
        ]);
        assert_eq!(restored.pages().len(), 2);
        assert_eq!(restored.pages()[0].url, "https://a.test/x");
        let ids: Vec<u64> = restored.pages().iter().flat_map(|p| p.marks.iter().map(|m| m.id)).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        let mut restored = restored;
        let fresh = restored.add(&u("https://d.test/"), "", "new", "", "", 0).unwrap();
        assert!(!ids.contains(&fresh));
    }
}

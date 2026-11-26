//! Watched areas of pages: an element of a page (a price, a stock, an order status) read again
//! from time to time, with a notice when its text changes. Times are Unix seconds.

/// At most this many watches, and this much text kept per watch.
const MAX_WATCHES: usize = 50;
const MAX_TEXT: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    pub id: u64,
    pub url: String,
    /// CSS selector of the element (from the area picker).
    pub selector: String,
    pub title: String,
    /// Text of the element at the last reading, and before its last change.
    pub value: String,
    pub previous: String,
    pub checked_at: i64,
    pub changed_at: i64,
    /// Changed since the user last looked.
    pub unread: bool,
    /// The last reading did not find the element (page changed, signed out…): `value` is older.
    pub missing: bool,
    /// A reading was asked for ("Check now", 0.8.33): it goes first, `checked_at` keeps the time
    /// of the last one meanwhile. Not kept across launches.
    pub wanted: bool,
}

/// What a reading found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// The first reading: the reference, nothing to tell.
    First,
    Same,
    Changed { before: String, after: String },
    /// The element was not found (page changed, not signed in…): nothing is recorded.
    Missing,
}

/// The text of an element as compared: white space collapsed, bounded.
pub fn normalize(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().take(MAX_TEXT).collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watches {
    list: Vec<Watch>,
}

impl Watches {
    pub fn restore(list: Vec<Watch>) -> Self {
        Self { list }
    }

    pub fn all(&self) -> &[Watch] {
        &self.list
    }

    /// Starts watching `selector` of `url`; `None` when the list is full. The first reading
    /// sets the reference value.
    pub fn add(&mut self, url: &str, selector: &str, title: &str, now: i64) -> Option<u64> {
        if self.list.len() >= MAX_WATCHES || selector.trim().is_empty() {
            return None;
        }
        let id = self.list.iter().map(|w| w.id).max().unwrap_or(0) + 1;
        self.list.push(Watch {
            id,
            url: url.to_owned(),
            selector: selector.to_owned(),
            title: normalize(title),
            value: String::new(),
            previous: String::new(),
            checked_at: now - 1_000_000_000,
            changed_at: 0,
            unread: false,
            missing: false,
            wanted: false,
        });
        Some(id)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.list.len();
        self.list.retain(|w| w.id != id);
        before != self.list.len()
    }

    /// Removes a watch and returns it with its place, for an undo (`put_back`).
    pub fn take(&mut self, id: u64) -> Option<(usize, Watch)> {
        let at = self.list.iter().position(|w| w.id == id)?;
        Some((at, self.list.remove(at)))
    }

    /// Puts a removed watch back; refused if its id is in use again or the list is full.
    pub fn put_back(&mut self, at: usize, watch: Watch) -> bool {
        if self.list.len() >= MAX_WATCHES || self.get(watch.id).is_some() {
            return false;
        }
        self.list.insert(at.min(self.list.len()), watch);
        true
    }

    pub fn get(&self, id: u64) -> Option<&Watch> {
        self.list.iter().find(|w| w.id == id)
    }

    /// The watch to read next: one asked for, else the one read longest ago once `every`
    /// seconds have passed.
    pub fn due(&self, now: i64, every: i64) -> Option<&Watch> {
        self.list
            .iter()
            .filter(|w| w.wanted || w.checked_at + every <= now)
            .min_by_key(|w| (!w.wanted, w.checked_at))
    }

    /// Asks for a reading at the next opportunity; the time of the last one stays known.
    pub fn check_soon(&mut self, id: u64) {
        if let Some(w) = self.list.iter_mut().find(|w| w.id == id) {
            w.wanted = true;
        }
    }

    /// Asks for a reading of every watch (the start page's refresh of the section).
    pub fn check_all_soon(&mut self) {
        for w in &mut self.list {
            w.wanted = true;
        }
    }

    /// Records a reading of the element's text (`None`: not found).
    pub fn record(&mut self, id: u64, text: Option<&str>, now: i64) -> Reading {
        let Some(w) = self.list.iter_mut().find(|w| w.id == id) else {
            return Reading::Missing;
        };
        w.checked_at = now;
        w.wanted = false;
        let Some(text) = text.map(normalize).filter(|t| !t.is_empty()) else {
            w.missing = true;
            return Reading::Missing;
        };
        w.missing = false;
        if w.value.is_empty() {
            w.value = text;
            return Reading::First;
        }
        if w.value == text {
            return Reading::Same;
        }
        let before = std::mem::replace(&mut w.value, text.clone());
        w.previous = before.clone();
        w.changed_at = now;
        w.unread = true;
        Reading::Changed {
            before,
            after: text,
        }
    }

    /// The watch of `selector` on `url`, if that area is watched already (a live card of the
    /// Easel and the area picker watch the same thing).
    pub fn find(&self, url: &str, selector: &str) -> Option<&Watch> {
        self.list.iter().find(|w| w.url == url && w.selector == selector)
    }

    /// The areas that changed since the user last looked, the latest change first.
    pub fn changed(&self, limit: usize) -> Vec<&Watch> {
        let mut out: Vec<&Watch> = self.list.iter().filter(|w| w.unread).collect();
        out.sort_by_key(|w| std::cmp::Reverse(w.changed_at));
        out.truncate(limit);
        out
    }

    pub fn mark_read(&mut self) {
        for w in &mut self.list {
            w.unread = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taken_watch_comes_back_in_its_place() {
        let mut w = Watches::default();
        let a = w.add("https://a.test", "#p", "A", 0).unwrap();
        w.add("https://b.test", "#p", "B", 0).unwrap();
        let before = w.clone();
        let (at, watch) = w.take(a).unwrap();
        assert_eq!(w.all().len(), 1);
        assert!(w.put_back(at, watch.clone()));
        assert_eq!(w, before);
        assert!(!w.put_back(at, watch), "already there");
    }

    #[test]
    fn the_first_reading_is_the_reference_and_a_change_is_told_once() {
        let mut w = Watches::default();
        let id = w.add("https://shop.test/p", "#price", "Headphones", 100).unwrap();
        assert_eq!(w.due(100, 1800).map(|x| x.id), Some(id), "read at once");
        assert_eq!(w.record(id, Some(" 249 €\n"), 100), Reading::First);
        assert_eq!(w.due(200, 1800), None);
        assert_eq!(w.record(id, Some("249   €"), 2000), Reading::Same, "white space does not count");
        assert_eq!(
            w.record(id, Some("199 €"), 4000),
            Reading::Changed { before: "249 €".into(), after: "199 €".into() }
        );
        let watch = w.get(id).unwrap();
        assert!(watch.unread && watch.previous == "249 €" && watch.changed_at == 4000);
        w.mark_read();
        assert!(!w.get(id).unwrap().unread);
    }

    #[test]
    fn a_missing_element_records_nothing_and_the_list_is_bounded() {
        let mut w = Watches::default();
        let id = w.add("https://a.test/", "#x", "A", 0).unwrap();
        w.record(id, Some("ok"), 10);
        assert_eq!(w.record(id, None, 20), Reading::Missing);
        assert_eq!(w.record(id, Some("   "), 30), Reading::Missing);
        assert_eq!(w.get(id).unwrap().value, "ok");
        assert!(w.add("https://a.test/", " ", "blank selector", 0).is_none());
        for _ in 0..60 {
            w.add("https://a.test/", "#x", "A", 0);
        }
        assert_eq!(w.all().len(), MAX_WATCHES);
        assert!(w.remove(id));
        w.record(2, Some("read"), 100);
        assert!(w.due(200, 1800).is_some_and(|x| x.id != 2), "the ones never read come first");
        w.check_soon(2);
        assert_eq!(w.get(2).unwrap().checked_at, 100, "the last reading's time stays known");
        assert_eq!(w.due(150, 1800).map(|x| x.id), Some(2), "asked for: first, before its time");
    }

    #[test]
    fn a_reading_asked_for_says_when_it_was_done_and_whether_it_found_the_area() {
        let mut w = Watches::default();
        let a = w.add("https://a.test/", "#x", "A", 0).unwrap();
        let b = w.add("https://b.test/", "#y", "B", 0).unwrap();
        w.record(a, Some("1"), 10);
        w.record(b, Some("1"), 10);
        assert!(w.due(20, 1800).is_none());
        w.check_all_soon();
        assert!(w.all().iter().all(|x| x.wanted));
        assert_eq!(w.record(a, None, 30), Reading::Missing);
        let got = w.get(a).unwrap();
        assert!(got.missing && !got.wanted && got.checked_at == 30 && got.value == "1");
        assert_eq!(w.due(31, 1800).map(|x| x.id), Some(b), "the other one still asked for");
        w.record(a, Some("1"), 40);
        assert!(!w.get(a).unwrap().missing, "found again");
    }

    #[test]
    fn a_watch_is_found_by_its_area_and_changes_come_latest_first() {
        let mut w = Watches::default();
        let a = w.add("https://a.test/", "#price", "A", 0).unwrap();
        let b = w.add("https://b.test/", "#stock", "B", 0).unwrap();
        assert_eq!(w.find("https://a.test/", "#price").map(|x| x.id), Some(a));
        assert!(w.find("https://a.test/", "#stock").is_none());
        for id in [a, b] {
            w.record(id, Some("1"), 10);
        }
        assert!(w.changed(5).is_empty());
        w.record(a, Some("2"), 20);
        w.record(b, Some("2"), 30);
        let ids: Vec<u64> = w.changed(5).iter().map(|x| x.id).collect();
        assert_eq!(ids, [b, a]);
        assert_eq!(w.changed(1).len(), 1);
        w.mark_read();
        assert!(w.changed(5).is_empty());
    }
}

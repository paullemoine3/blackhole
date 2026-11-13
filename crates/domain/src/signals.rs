//! Signals that last: what happened while the user may have been away (a watched area that
//! changed, a text received from another device, notes proposed for a meeting). A toast still
//! tells it at once; the signal stays as a card in the sidebar until it is answered, dismissed,
//! or too old to matter. Times are Unix seconds; nothing here is written to disk.

/// At most this many cards at once (the oldest leave).
pub const MAX_SIGNALS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A watched area changed: `key` names the watch.
    AreaChanged,
    /// A text came through the Wormhole: `key` names the kept text.
    TextReceived,
    /// Notes are proposed for a meeting that started: `key` names the meeting.
    Notes,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::AreaChanged => "area",
            Kind::TextReceived => "text",
            Kind::Notes => "notes",
        }
    }

    /// How long a card of this kind is worth keeping.
    fn keep(self) -> i64 {
        match self {
            // A meeting's notes matter while it is on.
            Kind::Notes => 2 * 3600,
            Kind::AreaChanged | Kind::TextReceived => 24 * 3600,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub id: u64,
    pub kind: Kind,
    /// What it is about, for the shell (a watch id, a text id, a meeting): one card per key.
    pub key: String,
    pub title: String,
    pub detail: String,
    pub at: i64,
}

/// The cards, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Signals {
    list: Vec<Signal>,
    next: u64,
}

impl Signals {
    pub fn all(&self) -> &[Signal] {
        &self.list
    }

    pub fn get(&self, id: u64) -> Option<&Signal> {
        self.list.iter().find(|s| s.id == id)
    }

    /// Adds a card, or brings the card of the same `key` back on top with the new text.
    pub fn push(&mut self, kind: Kind, key: &str, title: &str, detail: &str, now: i64) -> u64 {
        self.list.retain(|s| !(s.kind == kind && s.key == key));
        self.next += 1;
        let id = self.next;
        self.list.insert(
            0,
            Signal {
                id,
                kind,
                key: key.to_owned(),
                title: crate::selection::excerpt(title, 80),
                detail: crate::selection::excerpt(detail, 160),
                at: now,
            },
        );
        self.list.truncate(MAX_SIGNALS);
        id
    }

    /// Removes a card (answered or dismissed) and gives it back.
    pub fn take(&mut self, id: u64) -> Option<Signal> {
        let at = self.list.iter().position(|s| s.id == id)?;
        Some(self.list.remove(at))
    }

    /// Removes the card of `key`, if any (its subject was dealt with elsewhere).
    pub fn forget(&mut self, kind: Kind, key: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|s| !(s.kind == kind && s.key == key));
        before != self.list.len()
    }

    /// Removes every card of a kind (the user looked at all of them elsewhere).
    pub fn forget_kind(&mut self, kind: Kind) -> bool {
        let before = self.list.len();
        self.list.retain(|s| s.kind != kind);
        before != self.list.len()
    }

    /// Drops the cards too old to matter; `true` when one left.
    pub fn expire(&mut self, now: i64) -> bool {
        let before = self.list.len();
        self.list.retain(|s| now - s.at < s.kind.keep());
        before != self.list.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_per_subject_newest_first() {
        let mut s = Signals::default();
        s.push(Kind::AreaChanged, "1", "Price", "10 → 9", 100);
        s.push(Kind::TextReceived, "1", "Phone", "hello", 110);
        let again = s.push(Kind::AreaChanged, "1", "Price", "9 → 8", 120);
        assert_eq!(s.all().len(), 2);
        assert_eq!(s.all()[0].id, again);
        assert_eq!(s.all()[0].detail, "9 → 8");
    }

    #[test]
    fn the_oldest_leave_past_the_limit() {
        let mut s = Signals::default();
        for i in 0..6 {
            s.push(Kind::TextReceived, &i.to_string(), "Phone", "hi", i);
        }
        assert_eq!(s.all().len(), MAX_SIGNALS);
        assert_eq!(s.all()[0].key, "5");
        assert!(s.all().iter().all(|c| c.key != "0"));
    }

    #[test]
    fn taken_forgotten_and_expired_cards_leave() {
        let mut s = Signals::default();
        let a = s.push(Kind::AreaChanged, "7", "Stock", "out → in", 0);
        s.push(Kind::Notes, "meet", "Standup", "", 0);
        s.push(Kind::TextReceived, "2", "Mac", "link", 0);
        assert_eq!(s.take(a).map(|c| c.key), Some("7".to_owned()));
        assert!(s.take(a).is_none());
        assert!(s.forget(Kind::TextReceived, "2"));
        s.push(Kind::AreaChanged, "8", "Stock", "in → out", 0);
        assert!(s.forget_kind(Kind::AreaChanged));
        assert!(!s.forget_kind(Kind::AreaChanged));
        assert!(!s.expire(3600));
        assert!(s.expire(2 * 3600));
        assert!(s.all().is_empty());
    }

    #[test]
    fn long_texts_are_cut() {
        let mut s = Signals::default();
        s.push(Kind::TextReceived, "1", &"t".repeat(200), &"d".repeat(400), 0);
        assert!(s.all()[0].title.chars().count() <= 80);
        assert!(s.all()[0].detail.chars().count() <= 160);
    }
}

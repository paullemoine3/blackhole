//! Synchronization of tabs between paired devices (quick access, open tabs, spaces).
//!
//! Each device keeps a [`Ledger`]: one entry per tab, known by its space (by name) and its
//! address, with the time of its last change and the device that made it. Closing a tab leaves
//! a tombstone, so the closing travels too; tombstones go after [`TOMBSTONE_DAYS`]. Merging two
//! ledgers keeps, entry by entry, the latest change (the device id breaks ties), and tells what
//! to do locally ([`Plan`]) and whether the other device is behind.
//!
//! Not synced: private tabs, tabs inside folders, titles (they follow the address), spaces
//! deleted on one device (a space is only ever created or recoloured elsewhere).

use url::Url;

/// Tombstones of closed tabs are kept this long, then forgotten.
pub const TOMBSTONE_DAYS: i64 = 30;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// At most this many entries travel (the rest, the oldest tombstones first, stay local).
pub const MAX_ITEMS: usize = 2_000;

/// A tab as the ledger knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The space's name (spaces are matched by name, ignoring case).
    pub space: String,
    pub url: Url,
    pub title: String,
    pub pinned: bool,
    /// Milliseconds since the Unix epoch of the last change (open, close, pin, unpin).
    pub modified: i64,
    pub device: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceItem {
    pub name: String,
    pub color: String,
    pub modified: i64,
    pub device: String,
}

/// What a device has synced, closed tabs included.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    pub items: Vec<Item>,
    pub spaces: Vec<SpaceItem>,
}

/// The synced part of a workspace, as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSpace {
    pub name: String,
    pub color: String,
    /// Address, title, pinned (quick access).
    pub tabs: Vec<(Url, String, bool)>,
}

/// What the local workspace must do to match a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    AddSpace { name: String, color: String },
    SetColor { space: String, color: String },
    Open { space: String, url: Url, title: String, pinned: bool },
    Close { space: String, url: Url },
    SetPinned { space: String, url: Url, pinned: bool },
}

pub type Plan = Vec<Action>;

/// The result of merging the other device's ledger into this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    pub ledger: Ledger,
    pub plan: Plan,
    /// The other device misses changes made here: it should get this ledger back.
    pub remote_behind: bool,
}

impl Merge {
    /// Tabs opened, closed and changed here, for the proposal ("3 tabs to open, 1 to close").
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut out = (0, 0, 0);
        for action in &self.plan {
            match action {
                Action::Open { .. } => out.0 += 1,
                Action::Close { .. } => out.1 += 1,
                _ => out.2 += 1,
            }
        }
        out
    }
}

fn space_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// The address an entry is known by: the fragment does not make another tab.
fn url_key(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.into()
}

fn same(a: &Item, space: &str, url: &Url) -> bool {
    space_key(&a.space) == space_key(space) && url_key(&a.url) == url_key(url)
}

/// Whether `a` is a later change than `b` (the device id breaks ties).
fn later(a_at: i64, a_dev: &str, b_at: i64, b_dev: &str) -> bool {
    (a_at, a_dev) > (b_at, b_dev)
}

/// Only web pages travel.
pub fn syncable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
}

impl Ledger {
    /// Records what changed in the workspace since the last call; returns whether anything did.
    /// A title change alone is kept without counting as a change (it would sync constantly).
    pub fn observe(&mut self, live: &[LiveSpace], device: &str, now: i64) -> bool {
        let mut changed = false;
        let mut present: Vec<(String, String)> = Vec::new();
        for space in live {
            match self.spaces.iter_mut().find(|s| space_key(&s.name) == space_key(&space.name)) {
                Some(s) if s.color != space.color => {
                    s.color = space.color.clone();
                    s.modified = now;
                    s.device = device.to_owned();
                    changed = true;
                }
                Some(_) => {}
                None => {
                    self.spaces.push(SpaceItem {
                        name: space.name.trim().to_owned(),
                        color: space.color.clone(),
                        modified: now,
                        device: device.to_owned(),
                    });
                    changed = true;
                }
            }
            for (url, title, pinned) in space.tabs.iter().filter(|(u, _, _)| syncable(u)) {
                let key = (space_key(&space.name), url_key(url));
                if present.contains(&key) {
                    continue;
                }
                present.push(key);
                match self.items.iter_mut().find(|i| same(i, &space.name, url)) {
                    Some(item) if !item.deleted && item.pinned == *pinned => {
                        item.title.clone_from(title);
                    }
                    Some(item) => {
                        item.title.clone_from(title);
                        item.pinned = *pinned;
                        item.deleted = false;
                        item.modified = now;
                        item.device = device.to_owned();
                        changed = true;
                    }
                    None => {
                        self.items.push(Item {
                            space: space.name.trim().to_owned(),
                            url: url.clone(),
                            title: title.clone(),
                            pinned: *pinned,
                            modified: now,
                            device: device.to_owned(),
                            deleted: false,
                        });
                        changed = true;
                    }
                }
            }
        }
        for item in self.items.iter_mut().filter(|i| !i.deleted) {
            if !present.contains(&(space_key(&item.space), url_key(&item.url))) {
                item.deleted = true;
                item.modified = now;
                item.device = device.to_owned();
                changed = true;
            }
        }
        self.purge(now);
        changed
    }

    /// Forgets old tombstones, and the oldest ones beyond [`MAX_ITEMS`].
    fn purge(&mut self, now: i64) {
        self.items
            .retain(|i| !i.deleted || now - i.modified < TOMBSTONE_DAYS * DAY_MS);
        if self.items.len() > MAX_ITEMS {
            self.items
                .sort_by_key(|i| (!i.deleted, i.modified));
            let extra = self.items.len() - MAX_ITEMS;
            self.items.drain(..extra);
        }
    }

    /// Merges `remote` into this ledger (this one must have just observed the workspace).
    pub fn merge(&self, remote: &Ledger) -> Merge {
        let mut ledger = self.clone();
        let mut plan = Vec::new();
        let mut remote_behind = false;

        for theirs in &remote.spaces {
            match ledger.spaces.iter_mut().find(|s| space_key(&s.name) == space_key(&theirs.name)) {
                Some(mine) if later(theirs.modified, &theirs.device, mine.modified, &mine.device) => {
                    if mine.color != theirs.color {
                        plan.push(Action::SetColor {
                            space: mine.name.clone(),
                            color: theirs.color.clone(),
                        });
                    }
                    *mine = SpaceItem {
                        name: mine.name.clone(),
                        ..theirs.clone()
                    };
                }
                Some(mine) => remote_behind |= mine != theirs && mine.color != theirs.color,
                None => {
                    plan.push(Action::AddSpace {
                        name: theirs.name.trim().to_owned(),
                        color: theirs.color.clone(),
                    });
                    ledger.spaces.push(theirs.clone());
                }
            }
        }
        remote_behind |= ledger
            .spaces
            .iter()
            .any(|s| !remote.spaces.iter().any(|t| space_key(&t.name) == space_key(&s.name)));

        for theirs in remote.items.iter().filter(|i| syncable(&i.url)) {
            match ledger.items.iter_mut().find(|i| same(i, &theirs.space, &theirs.url)) {
                Some(mine) if later(theirs.modified, &theirs.device, mine.modified, &mine.device) => {
                    match (mine.deleted, theirs.deleted) {
                        (true, false) => plan.push(Action::Open {
                            space: theirs.space.clone(),
                            url: theirs.url.clone(),
                            title: theirs.title.clone(),
                            pinned: theirs.pinned,
                        }),
                        (false, true) => plan.push(Action::Close {
                            space: mine.space.clone(),
                            url: mine.url.clone(),
                        }),
                        (false, false) if mine.pinned != theirs.pinned => {
                            plan.push(Action::SetPinned {
                                space: mine.space.clone(),
                                url: mine.url.clone(),
                                pinned: theirs.pinned,
                            });
                        }
                        _ => {}
                    }
                    *mine = Item {
                        space: mine.space.clone(),
                        url: mine.url.clone(),
                        title: if theirs.title.is_empty() { mine.title.clone() } else { theirs.title.clone() },
                        ..theirs.clone()
                    };
                }
                Some(mine) => {
                    let differs = mine.deleted != theirs.deleted
                        || mine.pinned != theirs.pinned
                        || mine.modified != theirs.modified;
                    remote_behind |= differs;
                }
                None => {
                    if !theirs.deleted {
                        plan.push(Action::Open {
                            space: theirs.space.clone(),
                            url: theirs.url.clone(),
                            title: theirs.title.clone(),
                            pinned: theirs.pinned,
                        });
                    }
                    ledger.items.push(theirs.clone());
                }
            }
        }
        remote_behind |= ledger
            .items
            .iter()
            .any(|i| !remote.items.iter().any(|t| same(t, &i.space, &i.url)));

        // Spaces first, so that tabs find theirs.
        plan.sort_by_key(|a| !matches!(a, Action::AddSpace { .. } | Action::SetColor { .. }));
        Merge {
            ledger,
            plan,
            remote_behind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn live(spaces: &[(&str, &[(&str, bool)])]) -> Vec<LiveSpace> {
        spaces
            .iter()
            .map(|(name, tabs)| LiveSpace {
                name: (*name).into(),
                color: "#7c5cff".into(),
                tabs: tabs.iter().map(|(url, pinned)| (u(url), String::new(), *pinned)).collect(),
            })
            .collect()
    }

    #[test]
    fn observing_records_opens_closes_and_pins_but_not_titles() {
        let mut l = Ledger::default();
        assert!(l.observe(&live(&[("Home", &[("https://a.example/", false)])]), "pc", 1));
        assert!(!l.observe(&live(&[("Home", &[("https://a.example/#top", false)])]), "pc", 2));
        let mut titled = live(&[("Home", &[("https://a.example/", false)])]);
        titled[0].tabs[0].1 = "A".into();
        assert!(!l.observe(&titled, "pc", 3));
        assert_eq!(l.items[0].title, "A");
        assert!(l.observe(&live(&[("Home", &[("https://a.example/", true)])]), "pc", 4));
        assert!(l.observe(&live(&[("Home", &[])]), "pc", 5));
        assert!(l.items[0].deleted && l.items[0].modified == 5);
        // Non-web pages never enter.
        assert!(!l.observe(&live(&[("Home", &[("about:blank", false)])]), "pc", 6));
    }

    #[test]
    fn a_first_merge_unites_both_devices() {
        let mut phone = Ledger::default();
        phone.observe(&live(&[("Home", &[("https://p.example/", false)])]), "phone", 10);
        let mut pc = Ledger::default();
        pc.observe(&live(&[("Home", &[("https://c.example/", true)]), ("Work", &[("https://w.example/", false)])]), "pc", 20);
        let m = phone.merge(&pc);
        assert!(m.remote_behind, "the pc lacks the phone's tab");
        assert_eq!(m.plan[0], Action::AddSpace { name: "Work".into(), color: "#7c5cff".into() });
        assert!(m.plan.contains(&Action::Open { space: "Home".into(), url: u("https://c.example/"), title: String::new(), pinned: true }));
        assert_eq!(m.counts(), (2, 0, 1));
        // Back on the pc, after the phone applied: only the phone's tab is new there.
        let back = pc.merge(&m.ledger);
        assert_eq!(back.plan, vec![Action::Open { space: "Home".into(), url: u("https://p.example/"), title: String::new(), pinned: false }]);
        assert!(!back.remote_behind);
        // And once both agree, nothing more travels.
        let settled = back.ledger.merge(&m.ledger);
        assert!(settled.plan.is_empty() && !settled.remote_behind);
    }

    #[test]
    fn a_closing_travels_and_the_latest_change_wins() {
        let tab = [("Home", &[("https://a.example/", false)][..])];
        let mut a = Ledger::default();
        a.observe(&live(&tab), "a", 10);
        let mut b = a.clone();
        b.observe(&live(&tab), "b", 11);
        a.observe(&live(&[("Home", &[])]), "a", 20);
        let m = b.merge(&a);
        assert_eq!(m.plan, vec![Action::Close { space: "Home".into(), url: u("https://a.example/") }]);
        // An older reopening loses to the closing; a newer one wins.
        let mut old = a.clone();
        old.items[0] = Item { deleted: false, modified: 15, device: "c".into(), ..old.items[0].clone() };
        assert!(a.merge(&old).plan.is_empty());
        let mut new = old.clone();
        new.items[0].modified = 30;
        assert!(matches!(a.merge(&new).plan[..], [Action::Open { .. }]));
    }

    #[test]
    fn pinning_and_colours_follow_the_latest_change() {
        let mut a = Ledger::default();
        a.observe(&live(&[("Home", &[("https://a.example/", false)])]), "a", 10);
        let mut b = a.clone();
        let mut pinned = live(&[("Home", &[("https://a.example/", true)])]);
        pinned[0].color = "#2dd4bf".into();
        b.observe(&pinned, "b", 20);
        let m = a.merge(&b);
        assert!(m.plan.contains(&Action::SetPinned { space: "Home".into(), url: u("https://a.example/"), pinned: true }));
        assert!(m.plan.contains(&Action::SetColor { space: "Home".into(), color: "#2dd4bf".into() }));
        assert!(matches!(m.plan[0], Action::SetColor { .. }), "spaces come first");
    }

    #[test]
    fn spaces_match_by_name_whatever_the_case() {
        let mut a = Ledger::default();
        a.observe(&live(&[("Work", &[("https://w.example/", false)])]), "a", 10);
        let mut b = Ledger::default();
        b.observe(&live(&[(" work ", &[("https://w.example/", false)])]), "b", 10);
        let m = a.merge(&b);
        assert!(m.plan.is_empty());
    }

    #[test]
    fn old_tombstones_are_forgotten() {
        let mut l = Ledger::default();
        l.observe(&live(&[("Home", &[("https://a.example/", false)])]), "a", 0);
        l.observe(&live(&[("Home", &[])]), "a", 1);
        assert_eq!(l.items.len(), 1);
        l.observe(&live(&[("Home", &[])]), "a", TOMBSTONE_DAYS * DAY_MS + 2);
        assert!(l.items.is_empty());
    }
}

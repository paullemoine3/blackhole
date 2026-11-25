//! What a site may use (camera, microphone, location, notifications, clipboard): what the user
//! decided for each origin, and which requests are asked, refused or granted without a question.

use std::collections::BTreeMap;

use url::Url;

use crate::TabId;

/// Something a page must ask the user for. Anything else stays the engine's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    Camera,
    Microphone,
    Location,
    Notifications,
    Clipboard,
}

impl Permission {
    pub const ALL: [Self; 5] =
        [Self::Camera, Self::Microphone, Self::Location, Self::Notifications, Self::Clipboard];

    /// Stored form, and the last part of its text key (`permission.camera`…).
    pub fn id(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::Location => "location",
            Self::Notifications => "notifications",
            Self::Clipboard => "clipboard",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }
}

/// Sites whose answers are kept at most: past this count a new site is asked again each time.
pub const MAX_SITES: usize = 500;
/// A question left unanswered (Esc, a click outside) this many times is refused for the session.
pub const DISMISS_MAX: u8 = 3;

/// The origin a decision is kept for: `https://host[:port]` or plain http (a device of the local
/// network, a development server; engines already refuse most permissions to other http pages).
pub fn origin_of(url: &Url) -> Option<String> {
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .then(|| url.origin().ascii_serialization())
}

/// The user's answers, by origin: `true` allowed, `false` blocked, absent asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SitePermissions {
    sites: BTreeMap<String, BTreeMap<Permission, bool>>,
}

impl SitePermissions {
    /// Restores stored answers: anything that is not an origin or a known permission is dropped.
    pub fn restore<'a>(stored: impl IntoIterator<Item = (&'a str, &'a str, bool)>) -> Self {
        let mut out = Self::default();
        for (origin, id, allowed) in stored {
            let known = Url::parse(origin).ok().and_then(|u| origin_of(&u)).is_some_and(|o| o == origin);
            if let (true, Some(p)) = (known, Permission::from_id(id)) {
                out.set(origin, p, Some(allowed));
            }
        }
        out
    }

    pub fn get(&self, origin: &str, permission: Permission) -> Option<bool> {
        self.sites.get(origin)?.get(&permission).copied()
    }

    /// Remembers an answer, or forgets it (`None`: the site asks again). Returns whether
    /// anything changed.
    pub fn set(&mut self, origin: &str, permission: Permission, allowed: Option<bool>) -> bool {
        let Some(allowed) = allowed else {
            let Some(site) = self.sites.get_mut(origin) else { return false };
            let gone = site.remove(&permission).is_some();
            if site.is_empty() {
                self.sites.remove(origin);
            }
            return gone;
        };
        if !self.sites.contains_key(origin) && self.sites.len() >= MAX_SITES {
            return false;
        }
        self.sites.entry(origin.to_owned()).or_default().insert(permission, allowed) != Some(allowed)
    }

    pub fn forget_site(&mut self, origin: &str) -> bool {
        self.sites.remove(origin).is_some()
    }

    pub fn clear(&mut self) {
        self.sites.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    pub fn of(&self, origin: &str) -> Vec<(Permission, bool)> {
        self.sites.get(origin).map(|s| s.iter().map(|(p, a)| (*p, *a)).collect()).unwrap_or_default()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, Permission, bool)> {
        self.sites.iter().flat_map(|(o, s)| s.iter().map(move |(p, a)| (o.as_str(), *p, *a)))
    }

    /// What a private window keeps of the profile: what was blocked stays blocked, nothing that
    /// was allowed comes along.
    #[must_use]
    pub fn for_private(&self) -> Self {
        let mut out = self.clone();
        for site in out.sites.values_mut() {
            site.retain(|_, allowed| !*allowed);
        }
        out.sites.retain(|_, site| !site.is_empty());
        out
    }
}

/// A request of a page, waiting for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub tab: TabId,
    /// The origin of the page shown: what the user sees and what the answer is kept for.
    pub origin: String,
    /// The origin of the frame that asks, when it is not the page's own: such a request is
    /// always asked and its "allow" never kept.
    pub frame: Option<String>,
    pub wanted: Vec<Permission>,
}

/// What to do with a request as it comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
    Ask,
}

/// Requests waiting for an answer, in arrival order, and the questions the user keeps dismissing.
#[derive(Debug, Default)]
pub struct Asks {
    pending: Vec<(u64, Ask)>,
    dismissed: BTreeMap<(String, Permission), u8>,
    next_id: u64,
}

impl Asks {
    /// A blocked permission refuses the whole request; it is granted at once only when each is
    /// allowed for the page itself.
    pub fn verdict(&self, ask: &Ask, stored: &SitePermissions) -> Verdict {
        let worn = |p: &Permission| {
            self.dismissed.get(&(ask.origin.clone(), *p)).is_some_and(|n| *n >= DISMISS_MAX)
        };
        if ask.wanted.is_empty()
            || ask.wanted.iter().any(|p| stored.get(&ask.origin, *p) == Some(false) || worn(p))
        {
            Verdict::Deny
        } else if ask.frame.is_none() && ask.wanted.iter().all(|p| stored.get(&ask.origin, *p) == Some(true)) {
            Verdict::Allow
        } else {
            Verdict::Ask
        }
    }

    pub fn push(&mut self, ask: Ask) -> u64 {
        self.next_id += 1;
        self.pending.push((self.next_id, ask));
        self.next_id
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// The oldest question of a tab on screen, with every other one of the same page and frame
    /// (an engine asks for the camera and the microphone apart): one prompt answers them all.
    pub fn next_for(&self, shown: &[TabId]) -> Option<(Vec<u64>, Ask)> {
        let (_, first) = self.pending.iter().find(|(_, a)| shown.contains(&a.tab))?;
        let mut ask = first.clone();
        let mut ids = Vec::new();
        for (id, a) in &self.pending {
            if a.tab == first.tab && a.origin == first.origin && a.frame == first.frame {
                ids.push(*id);
                ask.wanted.extend(&a.wanted);
            }
        }
        ask.wanted.sort_unstable();
        ask.wanted.dedup();
        Some((ids, ask))
    }

    /// Removes answered questions.
    pub fn take(&mut self, ids: &[u64]) {
        self.pending.retain(|(i, _)| !ids.contains(i));
    }

    /// The tab closed or left the page: its questions are gone (their ids).
    pub fn drop_tab(&mut self, tab: TabId) -> Vec<u64> {
        let gone = self.pending.iter().filter(|(_, a)| a.tab == tab).map(|(i, _)| *i).collect();
        self.pending.retain(|(_, a)| a.tab != tab);
        gone
    }

    pub fn tabs(&self) -> Vec<TabId> {
        self.pending.iter().map(|(_, a)| a.tab).collect()
    }

    /// The prompt was left without an answer.
    pub fn dismiss(&mut self, ask: &Ask) {
        for p in &ask.wanted {
            let n = self.dismissed.entry((ask.origin.clone(), *p)).or_default();
            *n = n.saturating_add(1);
        }
    }

    /// The user changed their mind in the Settings: the site may ask again.
    pub fn forgive(&mut self, origin: &str) {
        self.dismissed.retain(|(o, _), _| o != origin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEET: &str = "https://meet.example";

    fn ask(tab: u64, wanted: &[Permission]) -> Ask {
        Ask { tab: TabId(tab), origin: MEET.into(), frame: None, wanted: wanted.to_vec() }
    }

    #[test]
    fn a_stored_answer_settles_the_request_of_the_page_itself() {
        let (asks, mut s) = (Asks::default(), SitePermissions::default());
        let both = ask(1, &[Permission::Camera, Permission::Microphone]);
        assert_eq!(asks.verdict(&both, &s), Verdict::Ask);
        s.set(MEET, Permission::Camera, Some(true));
        assert_eq!(asks.verdict(&both, &s), Verdict::Ask, "the microphone is still unknown");
        s.set(MEET, Permission::Microphone, Some(true));
        assert_eq!(asks.verdict(&both, &s), Verdict::Allow);
        s.set(MEET, Permission::Camera, Some(false));
        assert_eq!(asks.verdict(&both, &s), Verdict::Deny);
        assert_eq!(asks.verdict(&ask(1, &[]), &s), Verdict::Deny);
    }

    #[test]
    fn a_frame_of_another_site_never_uses_what_the_page_was_allowed() {
        let (asks, mut s) = (Asks::default(), SitePermissions::default());
        s.set(MEET, Permission::Camera, Some(true));
        let mut framed = ask(1, &[Permission::Camera]);
        framed.frame = Some("https://ads.example".into());
        assert_eq!(asks.verdict(&framed, &s), Verdict::Ask);
        s.set(MEET, Permission::Camera, Some(false));
        assert_eq!(asks.verdict(&framed, &s), Verdict::Deny);
    }

    #[test]
    fn one_prompt_gathers_the_requests_of_a_page_on_screen() {
        let mut asks = Asks::default();
        asks.push(ask(2, &[Permission::Location]));
        let a = asks.push(ask(1, &[Permission::Microphone]));
        let b = asks.push(ask(1, &[Permission::Camera]));
        let mut framed = ask(1, &[Permission::Location]);
        framed.frame = Some("https://map.example".into());
        asks.push(framed);
        let (ids, shown) = asks.next_for(&[TabId(1)]).unwrap();
        assert_eq!(ids, [a, b]);
        assert_eq!(shown.wanted, [Permission::Camera, Permission::Microphone]);
        asks.take(&ids);
        assert!(asks.next_for(&[TabId(1)]).unwrap().1.frame.is_some());
        assert_eq!(asks.drop_tab(TabId(1)).len(), 1);
        assert!(asks.next_for(&[TabId(1)]).is_none());
        assert!(!asks.is_empty(), "the hidden tab still waits");
    }

    #[test]
    fn a_question_dismissed_three_times_is_refused_until_forgiven() {
        let (mut asks, s) = (Asks::default(), SitePermissions::default());
        let q = ask(1, &[Permission::Notifications]);
        for _ in 0..DISMISS_MAX {
            assert_eq!(asks.verdict(&q, &s), Verdict::Ask);
            asks.dismiss(&q);
        }
        assert_eq!(asks.verdict(&q, &s), Verdict::Deny);
        assert_eq!(asks.verdict(&ask(1, &[Permission::Camera]), &s), Verdict::Ask);
        asks.forgive(MEET);
        assert_eq!(asks.verdict(&q, &s), Verdict::Ask);
    }

    #[test]
    fn stored_answers_are_checked_capped_and_blocked_only_in_private() {
        let mut s = SitePermissions::restore([
            (MEET, "camera", true),
            (MEET, "location", false),
            (MEET, "usb", true),
            ("meet.example", "camera", true),
            ("https://meet.example/path", "camera", true),
            ("file:///x", "camera", true),
        ]);
        assert_eq!(s.iter().count(), 2);
        let private = s.for_private();
        assert_eq!(private.of(MEET), [(Permission::Location, false)]);
        assert!(!s.set(MEET, Permission::Camera, Some(true)), "nothing new");
        assert!(s.set(MEET, Permission::Camera, None));
        assert!(s.forget_site(MEET));
        for i in 0..MAX_SITES {
            assert!(s.set(&format!("https://s{i}.test"), Permission::Camera, Some(true)));
        }
        assert!(!s.set(MEET, Permission::Camera, Some(true)));
    }
}

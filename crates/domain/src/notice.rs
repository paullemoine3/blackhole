//! Notices: the short messages the shell shows about what just happened (a download, a watched
//! area, something kept in an Easel…). Every kind is declared here, with the icon, the tone and
//! the time it stays; the shell has one way to emit them and the pages one component to draw
//! them, in the sidebar when it is on screen, in the top bar's address field when it is hidden.

/// The colour a notice takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The colour of the space.
    Plain,
    Good,
    Bad,
}

impl Tone {
    pub fn id(self) -> &'static str {
        match self {
            Tone::Plain => "plain",
            Tone::Good => "good",
            Tone::Bad => "bad",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Anything said in passing.
    Info,
    /// Something could not be done.
    Problem,
    /// A download started.
    Download,
    /// Something finished well (a download, a save).
    Done,
    /// A watched area: added, or changed.
    Watch,
    /// Something went into an Easel.
    Easel,
    /// Another device: paired, a tab or a text sent or received, a sync.
    Device,
    /// A password to save, saved or removed.
    Password,
    /// Something was removed and can come back.
    Undo,
    /// A meeting to join, notes to take.
    Meeting,
}

/// A notice without a button stays this long, in milliseconds.
pub const STAY_MS: u64 = 2200;
/// One that asks something stays longer.
pub const STAY_ASKING_MS: u64 = 8000;
/// A problem stays at least this long: it says why something did not happen.
pub const STAY_PROBLEM_MS: u64 = 6000;
/// Time to read one character, and the longest any notice stays (its line of time then pauses
/// under the pointer or the keyboard focus anyway).
pub const READ_MS_PER_CHAR: u64 = 60;
pub const STAY_MAX_MS: u64 = 15_000;

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Info,
        Kind::Problem,
        Kind::Download,
        Kind::Done,
        Kind::Watch,
        Kind::Easel,
        Kind::Device,
        Kind::Password,
        Kind::Undo,
        Kind::Meeting,
    ];

    /// A name of the shared icon table (`ui/icons.js`).
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Info => "info",
            Kind::Problem => "alert",
            Kind::Download => "download",
            Kind::Done => "check",
            Kind::Watch => "eye",
            Kind::Easel => "easel",
            Kind::Device => "devices",
            Kind::Password => "key",
            Kind::Undo => "history",
            Kind::Meeting => "calendar",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Kind::Problem => Tone::Bad,
            Kind::Done => Tone::Good,
            _ => Tone::Plain,
        }
    }

    /// How long a notice of `chars` characters stays: long enough to be read, longer when it
    /// has buttons to answer, and a problem long enough to tell why.
    pub fn stay_ms(self, asking: bool, chars: usize) -> u64 {
        let base = match (asking, self.tone()) {
            (true, _) => STAY_ASKING_MS,
            (false, Tone::Bad) => STAY_PROBLEM_MS,
            (false, _) => STAY_MS,
        };
        let reading = (chars as u64).saturating_mul(READ_MS_PER_CHAR);
        base.max(reading).min(STAY_MAX_MS.max(base))
    }
}

/// Where a notice is drawn: the top bar takes it only while no sidebar is on screen (neither
/// pinned open nor floating over the page), so it never shows twice.
pub fn in_top_bar(sidebar_open: bool, flyout_shown: bool) -> bool {
    !sidebar_open && !flyout_shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_own_icon() {
        let mut icons: Vec<_> = Kind::ALL.iter().map(|k| k.icon()).collect();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), Kind::ALL.len());
    }

    #[test]
    fn only_problems_are_bad_and_only_done_is_good() {
        for kind in Kind::ALL {
            assert_eq!(kind.tone() == Tone::Bad, kind == Kind::Problem);
            assert_eq!(kind.tone() == Tone::Good, kind == Kind::Done);
        }
    }

    #[test]
    fn a_notice_that_asks_stays_longer() {
        for kind in Kind::ALL {
            assert!(kind.stay_ms(true, 10) > kind.stay_ms(false, 10));
        }
    }

    #[test]
    fn a_long_notice_stays_long_enough_to_be_read_and_a_problem_longer() {
        assert_eq!(Kind::Info.stay_ms(false, 10), STAY_MS);
        let long = "Close a black hole tab first: each one keeps its page alive".chars().count();
        assert!(Kind::Info.stay_ms(false, long) >= long as u64 * READ_MS_PER_CHAR);
        assert!(Kind::Problem.stay_ms(false, 5) >= 6000);
        assert_eq!(Kind::Info.stay_ms(false, 10_000), STAY_MAX_MS, "bounded");
    }

    #[test]
    fn the_top_bar_takes_it_only_without_a_sidebar_on_screen() {
        assert!(in_top_bar(false, false));
        assert!(!in_top_bar(true, false));
        assert!(!in_top_bar(false, true), "the floating sidebar shows it already");
    }
}

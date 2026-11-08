//! Keeps the sidebar's media card steady. Players often report "nothing" for a moment (an ad
//! starts, Twitch or YouTube rebuilds its player, a page navigates inside itself): without a
//! grace period the card disappears and slides back in a probe later, which reads as flicker.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a tab keeps its card after its page last reported media.
pub const LINGER: Duration = Duration::from_secs(5);

pub struct Linger<T: Copy> {
    kept: HashMap<u64, (Instant, T)>,
    hold: Duration,
}

impl<T: Copy> Linger<T> {
    pub fn new(hold: Duration) -> Self {
        Self { kept: HashMap::new(), hold }
    }

    /// Records what a tab reports at `at`: `Some` when it has media to show.
    pub fn note(&mut self, id: u64, at: Instant, shown: Option<T>) {
        if let Some(value) = shown {
            self.kept.insert(id, (at, value));
        }
    }

    /// What to show for a tab at `at`: its last report with media, while recent enough.
    pub fn shown(&self, id: u64, at: Instant) -> Option<T> {
        self.kept
            .get(&id)
            .filter(|(seen, _)| at.saturating_duration_since(*seen) <= self.hold)
            .map(|(_, value)| *value)
    }

    /// Forgets a tab at once (closed, or its card dismissed).
    pub fn forget(&mut self, id: u64) {
        self.kept.remove(&id);
    }

    /// Drops the tabs that are gone and the reports past their grace period.
    pub fn prune(&mut self, at: Instant, alive: impl Fn(u64) -> bool) {
        let hold = self.hold;
        self.kept
            .retain(|id, (seen, _)| alive(*id) && at.saturating_duration_since(*seen) <= hold);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_gap_in_the_reports_keeps_the_card() {
        let t0 = Instant::now();
        let mut hold = Linger::new(Duration::from_secs(5));
        hold.note(1, t0, Some('▶'));
        hold.note(1, t0 + Duration::from_secs(2), None);
        assert_eq!(hold.shown(1, t0 + Duration::from_secs(2)), Some('▶'));
    }

    #[test]
    fn the_card_goes_once_the_media_has_been_gone_long_enough() {
        let t0 = Instant::now();
        let mut hold = Linger::new(Duration::from_secs(5));
        hold.note(1, t0, Some('▶'));
        assert_eq!(hold.shown(1, t0 + Duration::from_secs(6)), None);
        hold.prune(t0 + Duration::from_secs(6), |_| true);
        assert_eq!(hold.shown(1, t0), None);
    }

    #[test]
    fn the_newest_report_wins_and_closed_tabs_are_dropped() {
        let t0 = Instant::now();
        let mut hold = Linger::new(Duration::from_secs(5));
        hold.note(1, t0, Some('▶'));
        hold.note(1, t0 + Duration::from_secs(1), Some('⏸'));
        assert_eq!(hold.shown(1, t0 + Duration::from_secs(1)), Some('⏸'));
        hold.note(2, t0, Some('▶'));
        hold.prune(t0, |id| id == 1);
        assert_eq!(hold.shown(2, t0), None);
        hold.forget(1);
        assert_eq!(hold.shown(1, t0), None);
    }
}

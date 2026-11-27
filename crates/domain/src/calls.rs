//! Calls in several tabs at once. A call left open in a tab (a meeting room joined in the
//! morning, a call that came back on its own after the engine restarted its media process) keeps
//! its microphone, its echo cancelling and its videos running beside the new one: twice the
//! work for the computer, and the colleagues of the first call still hear you (0.8.34). When a
//! tab joins a call while another one is still in a call, the shell says so once, with a button
//! that leaves the other one.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A call that stops answering for a moment (its connection being set up again, a probe that
/// came back late) is still the same call: it only counts as left after this long.
pub const LEFT_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct Calls {
    /// Tabs in a call: when the call began, and when it was last seen.
    ongoing: HashMap<u64, (Instant, Instant)>,
}

impl Calls {
    /// What the probes say at `at`: the tabs in a call now. Gives the tab that just joined a call
    /// and the tab already in a call beside it (the one in a call the longest), once per call.
    pub fn update(&mut self, in_call: &[u64], at: Instant) -> Option<(u64, u64)> {
        self.ongoing
            .retain(|id, (_, seen)| in_call.contains(id) || at.saturating_duration_since(*seen) < LEFT_AFTER);
        let mut joined = None;
        for id in in_call {
            match self.ongoing.get_mut(id) {
                Some((_, seen)) => *seen = at,
                None => {
                    self.ongoing.insert(*id, (at, at));
                    joined.get_or_insert(*id);
                }
            }
        }
        let joined = joined?;
        let other = self
            .ongoing
            .iter()
            .filter(|(id, (began, _))| **id != joined && *began < at)
            .min_by_key(|(id, (began, _))| (*began, **id))
            .map(|(id, _)| *id)?;
        Some((joined, other))
    }

    /// The tabs closed (or whose page went away): their calls are over at once.
    pub fn retain(&mut self, alive: impl Fn(u64) -> bool) {
        self.ongoing.retain(|id, _| alive(*id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_call_beside_an_open_one_is_told_once() {
        let t0 = Instant::now();
        let mut calls = Calls::default();
        assert_eq!(calls.update(&[1], t0), None, "one call: nothing to say");
        assert_eq!(calls.update(&[1], t0 + Duration::from_secs(60)), None);
        let t1 = t0 + Duration::from_secs(120);
        assert_eq!(calls.update(&[1, 2], t1), Some((2, 1)), "the new call, and the one already open");
        assert_eq!(calls.update(&[1, 2], t1 + Duration::from_secs(2)), None, "told once");
    }

    #[test]
    fn a_call_that_drops_for_a_moment_is_still_the_same_call() {
        let t0 = Instant::now();
        let mut calls = Calls::default();
        calls.update(&[1, 2], t0);
        // Tab 1 reconnects: absent for a few probes, then back.
        assert_eq!(calls.update(&[2], t0 + Duration::from_secs(5)), None);
        assert_eq!(calls.update(&[1, 2], t0 + Duration::from_secs(10)), None, "not a new call");
        // Gone for longer than `LEFT_AFTER`: coming back is joining a call again.
        calls.update(&[2], t0 + Duration::from_secs(11));
        let back = t0 + Duration::from_secs(11) + LEFT_AFTER + Duration::from_secs(1);
        calls.update(&[2], back);
        assert_eq!(calls.update(&[1, 2], back + Duration::from_secs(1)), Some((1, 2)));
    }

    #[test]
    fn calls_that_begin_together_or_alone_say_nothing() {
        let t0 = Instant::now();
        let mut calls = Calls::default();
        assert_eq!(calls.update(&[1, 2], t0), None, "both found at once (launch): no order to tell");
        let mut alone = Calls::default();
        alone.update(&[1], t0);
        assert_eq!(alone.update(&[], t0 + LEFT_AFTER + Duration::from_secs(1)), None);
        assert_eq!(alone.update(&[2], t0 + LEFT_AFTER + Duration::from_secs(2)), None, "the first one was left");
    }

    #[test]
    fn the_other_call_is_the_oldest_one() {
        let t0 = Instant::now();
        let mut calls = Calls::default();
        calls.update(&[5], t0);
        calls.update(&[5, 3], t0 + Duration::from_secs(1));
        assert_eq!(calls.update(&[5, 3, 9], t0 + Duration::from_secs(2)), Some((9, 5)));
    }

    #[test]
    fn a_closed_tab_ends_its_call() {
        let t0 = Instant::now();
        let mut calls = Calls::default();
        calls.update(&[1], t0);
        calls.retain(|id| id != 1);
        assert_eq!(calls.update(&[2], t0 + Duration::from_secs(1)), None);
    }
}

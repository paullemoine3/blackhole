//! Motion of the panes of a split view (pure logic, logical pixels). A pane only slides, at its
//! final size: a web view resized on every frame lays its page out again and stutters (the same
//! rule as the sidebar). Two moves: the right pane comes in from the right when a split shows
//! up, and the pane a pilot link opens in rises into place.

use std::time::{Duration, Instant};

/// A split shows up: its right pane slides in.
const OPEN: Duration = Duration::from_millis(340);
/// A pilot link opens in the other pane: that pane rises.
const RISE: Duration = Duration::from_millis(380);
const RISE_BY: f64 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn id(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Move {
    /// The right pane, from that far to the right.
    Open(f64),
    /// One pane, from `RISE_BY` below.
    Rise(Side),
}

/// Where a pane is drawn, relative to its place.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shift {
    pub x: f64,
    pub y: f64,
}

#[derive(Default)]
pub struct PaneMotion {
    run: Option<(Move, Option<Instant>)>,
    /// What is left of the way, from 1 (just started) to 0 (in place).
    left: f64,
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(4)
}

impl PaneMotion {
    /// A split shows up: its right pane starts `distance` to the right of its place.
    pub fn open(&mut self, distance: f64) {
        if distance.is_finite() && distance > 0.0 {
            self.start(Move::Open(distance));
        }
    }

    /// A pilot link opens in the pane on `side`. The opening of the split wins: its pane is
    /// already on its way.
    pub fn rise(&mut self, side: Side) {
        if !matches!(self.run, Some((Move::Open(_), _))) {
            self.start(Move::Rise(side));
        }
    }

    fn start(&mut self, how: Move) {
        self.run = Some((how, None));
        self.left = 1.0;
    }

    /// The split is gone, or motion is switched off: panes are in place at once.
    pub fn stop(&mut self) {
        self.run = None;
        self.left = 0.0;
    }

    pub fn moving(&self) -> bool {
        self.run.is_some()
    }

    /// Advances to `now`; returns true while a pane is still on its way.
    pub fn tick_at(&mut self, now: Instant) -> bool {
        let Some((how, started)) = &mut self.run else {
            return false;
        };
        let length = match how {
            Move::Open(_) => OPEN,
            Move::Rise(_) => RISE,
        };
        let start = *started.get_or_insert(now);
        let t = now.saturating_duration_since(start).as_secs_f64() / length.as_secs_f64();
        if t >= 1.0 {
            self.stop();
            return false;
        }
        self.left = 1.0 - ease_out(t);
        true
    }

    pub fn tick(&mut self) -> bool {
        self.tick_at(Instant::now())
    }

    /// Where the pane on `side` is drawn right now.
    pub fn shift(&self, side: Side) -> Shift {
        match self.run {
            Some((Move::Open(distance), _)) if side == Side::Right => Shift { x: distance * self.left, y: 0.0 },
            Some((Move::Rise(which), _)) if side == which => Shift { x: 0.0, y: RISE_BY * self.left },
            _ => Shift::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_right_pane_of_a_new_split_slides_in_from_the_right() {
        let mut m = PaneMotion::default();
        assert!(!m.moving());
        m.open(500.0);
        let t0 = Instant::now();
        assert!(m.tick_at(t0));
        assert_eq!(m.shift(Side::Right), Shift { x: 500.0, y: 0.0 });
        assert_eq!(m.shift(Side::Left), Shift::default());
        // Half way through the time, most of the way is done (ease out), and it only gets closer.
        assert!(m.tick_at(t0 + OPEN / 2));
        let mid = m.shift(Side::Right).x;
        assert!(mid > 0.0 && mid < 100.0, "{mid}");
        assert!(m.tick_at(t0 + OPEN * 3 / 4));
        assert!(m.shift(Side::Right).x < mid);
        assert!(!m.tick_at(t0 + OPEN));
        assert!(!m.moving());
        assert_eq!(m.shift(Side::Right), Shift::default());
    }

    #[test]
    fn the_pane_a_pilot_link_opens_in_rises_into_place() {
        for side in [Side::Left, Side::Right] {
            let other = if side == Side::Left { Side::Right } else { Side::Left };
            let mut m = PaneMotion::default();
            m.rise(side);
            let t0 = Instant::now();
            assert!(m.tick_at(t0));
            assert_eq!(m.shift(side), Shift { x: 0.0, y: RISE_BY });
            assert_eq!(m.shift(other), Shift::default());
            assert!(m.tick_at(t0 + RISE / 2));
            assert!(m.shift(side).y > 0.0 && m.shift(side).y < RISE_BY / 2.0);
            assert!(!m.tick_at(t0 + RISE));
            assert_eq!(m.shift(side), Shift::default());
        }
    }

    #[test]
    fn a_link_does_not_interrupt_the_opening_and_stop_puts_panes_in_place() {
        let mut m = PaneMotion::default();
        m.open(400.0);
        m.rise(Side::Right);
        m.tick_at(Instant::now());
        assert_eq!(m.shift(Side::Right), Shift { x: 400.0, y: 0.0 });
        m.stop();
        assert!(!m.moving());
        assert_eq!(m.shift(Side::Right), Shift::default());
        // A second link restarts the rise from the bottom.
        m.rise(Side::Left);
        let t0 = Instant::now();
        m.tick_at(t0);
        m.tick_at(t0 + RISE / 2);
        m.rise(Side::Left);
        m.tick_at(t0 + RISE / 2);
        assert_eq!(m.shift(Side::Left).y, RISE_BY);
    }

    #[test]
    fn a_distance_that_makes_no_sense_starts_nothing() {
        let mut m = PaneMotion::default();
        for bad in [0.0, -3.0, f64::NAN, f64::INFINITY] {
            m.open(bad);
            assert!(!m.moving());
        }
    }
}

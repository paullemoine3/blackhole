//! Sidebar expand/collapse behaviour (pure logic, logical pixels).
//! The width follows a time-based ease-out curve, so the motion is smooth whatever the frame rate.

use std::time::{Duration, Instant};

pub const RAIL: f64 = 64.0;
pub const FULL: f64 = 280.0;
const DURATION: Duration = Duration::from_millis(240);

pub struct Sidebar {
    pub pinned: bool,
    width: f64,
    target: f64,
    from: f64,
    started: Option<Instant>,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self::new(false)
    }
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(4)
}

impl Sidebar {
    pub fn new(is_static: bool) -> Self {
        let width = if is_static { FULL } else { RAIL };
        Self {
            pinned: is_static,
            width,
            target: width,
            from: width,
            started: None,
        }
    }

    pub fn width(&self) -> f64 {
        self.width
    }

    pub fn expanded(&self) -> bool {
        self.target >= FULL
    }

    /// Width reserved for the page: the page always resizes with the sidebar.
    pub fn content_offset(&self) -> f64 {
        self.width
    }

    fn retarget(&mut self, target: f64) {
        if target != self.target {
            self.target = target;
            self.from = self.width;
            self.started = None;
        }
    }

    pub fn cursor_moved(&mut self, x: f64) {
        if self.pinned {
            return;
        }
        if x <= RAIL {
            self.retarget(FULL);
        } else if x > self.width + 8.0 {
            self.retarget(RAIL);
        }
    }

    pub fn cursor_left(&mut self) {
        if !self.pinned {
            self.retarget(RAIL);
        }
    }

    /// Static mode keeps the sidebar always expanded; otherwise it follows the cursor.
    pub fn set_static(&mut self, is_static: bool) {
        self.pinned = is_static;
        self.retarget(if is_static { FULL } else { RAIL });
    }

    /// True while the width is still travelling towards its target.
    pub fn moving(&self) -> bool {
        self.width != self.target
    }

    pub fn tick(&mut self) -> bool {
        self.tick_at(Instant::now())
    }

    /// Advances the animation to `now`; returns true when the width changed.
    pub fn tick_at(&mut self, now: Instant) -> bool {
        if !self.moving() {
            return false;
        }
        let start = *self.started.get_or_insert(now);
        let t =
            (now.saturating_duration_since(start).as_secs_f64() / DURATION.as_secs_f64()).min(1.0);
        self.width = if t >= 1.0 {
            self.target
        } else {
            self.from + (self.target - self.from) * ease_out(t)
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(s: &mut Sidebar) {
        let t0 = Instant::now();
        s.tick_at(t0);
        s.tick_at(t0 + DURATION);
    }

    #[test]
    fn hovering_the_rail_expands_and_leaving_collapses() {
        let mut s = Sidebar::default();
        s.cursor_moved(10.0);
        settle(&mut s);
        assert_eq!(s.width(), FULL);
        s.cursor_moved(FULL + 100.0);
        settle(&mut s);
        assert_eq!(s.width(), RAIL);
    }

    #[test]
    fn the_width_travels_smoothly_instead_of_jumping() {
        let mut s = Sidebar::default();
        s.cursor_moved(10.0);
        let t0 = Instant::now();
        let mut last = RAIL;
        let mut frames = 0;
        for ms in (0..=240).step_by(16) {
            s.tick_at(t0 + Duration::from_millis(ms));
            assert!(s.width() >= last, "width never goes backwards");
            assert!(
                s.width() - last < (FULL - RAIL) * 0.4,
                "no frame covers most of the distance"
            );
            last = s.width();
            frames += 1;
        }
        assert_eq!(last, FULL);
        assert!(frames > 10);
    }

    #[test]
    fn reversing_mid_flight_starts_from_the_current_width() {
        let mut s = Sidebar::default();
        s.cursor_moved(10.0);
        let t0 = Instant::now();
        s.tick_at(t0);
        s.tick_at(t0 + Duration::from_millis(80));
        let mid = s.width();
        assert!(mid > RAIL && mid < FULL);
        s.cursor_left();
        s.tick_at(t0 + Duration::from_millis(90));
        assert!((s.width() - mid).abs() < 1.0);
    }

    #[test]
    fn pinned_sidebar_pushes_content_and_ignores_hover() {
        let mut s = Sidebar::default();
        s.set_static(true);
        settle(&mut s);
        s.cursor_moved(900.0);
        settle(&mut s);
        assert_eq!(s.content_offset(), FULL);
        s.set_static(false);
        settle(&mut s);
        assert_eq!(s.content_offset(), RAIL);
    }

    #[test]
    fn hovering_resizes_the_content() {
        let mut s = Sidebar::default();
        s.cursor_moved(10.0);
        settle(&mut s);
        assert_eq!(s.content_offset(), FULL);
    }
}

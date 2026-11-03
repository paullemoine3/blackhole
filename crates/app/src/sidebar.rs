//! Sidebar show/hide behaviour (pure logic, logical pixels).
//! An open sidebar pushes the page; a hidden one can float over it as a flyout while the cursor
//! rests on the window's left edge. Both widths follow a time-based ease-out curve, so the motion
//! is smooth whatever the frame rate.

use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(240);

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(4)
}

/// One animated width.
struct Anim {
    value: f64,
    target: f64,
    from: f64,
    started: Option<Instant>,
}

impl Anim {
    fn new(value: f64) -> Self {
        Self {
            value,
            target: value,
            from: value,
            started: None,
        }
    }

    fn go(&mut self, target: f64) {
        self.target = target;
        self.from = self.value;
        self.started = None;
    }

    fn jump(&mut self, value: f64) {
        self.value = value;
        self.go(value);
    }

    fn moving(&self) -> bool {
        self.value != self.target
    }

    fn tick_at(&mut self, now: Instant) -> bool {
        if !self.moving() {
            return false;
        }
        let start = *self.started.get_or_insert(now);
        let t =
            (now.saturating_duration_since(start).as_secs_f64() / DURATION.as_secs_f64()).min(1.0);
        self.value = if t >= 1.0 {
            self.target
        } else {
            self.from + (self.target - self.from) * ease_out(t)
        };
        true
    }
}

pub struct Sidebar {
    full: f64,
    open: bool,
    layout: Anim,
    flyout: Anim,
}

impl Sidebar {
    /// A sidebar that starts open at `full` pixels wide.
    #[cfg(test)]
    pub fn new(full: f64) -> Self {
        Self::starting(full, true)
    }

    /// A sidebar restored as pinned open, or hidden (shown over the page on hover).
    pub fn starting(full: f64, open: bool) -> Self {
        Self {
            full,
            open,
            layout: Anim::new(if open { full } else { 0.0 }),
            flyout: Anim::new(0.0),
        }
    }

    /// Width of the sidebar when open.
    pub fn full(&self) -> f64 {
        self.full
    }

    /// An open sidebar animates to the new width; a hidden one just remembers it.
    pub fn set_full(&mut self, full: f64) {
        self.full = full;
        if self.open {
            self.layout.go(full);
        } else {
            let shown = self.flyout.target > 0.0;
            self.flyout.go(if shown { full } else { 0.0 });
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Width reserved beside the page.
    pub fn width(&self) -> f64 {
        self.layout.value
    }

    /// Visible width of the floating copy shown over the page while the sidebar is hidden.
    pub fn flyout_width(&self) -> f64 {
        self.flyout.value
    }

    /// True while the flyout should be on screen (sliding in, shown or sliding out).
    pub fn flyout_shown(&self) -> bool {
        self.flyout.target > 0.0 || self.flyout.value > 0.0
    }

    /// Opens a hidden sidebar, hides an open one.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            // Pinning a flyout continues from its current width, so the sidebar doesn't blink.
            if self.flyout.value > 0.0 {
                self.layout.jump(self.flyout.value);
            }
            self.flyout.jump(0.0);
            self.layout.go(self.full);
        } else {
            self.layout.go(0.0);
        }
    }

    /// Slides the flyout in or out; it only exists while the sidebar is hidden.
    pub fn peek(&mut self, on: bool) {
        if self.open {
            return;
        }
        let target = if on { self.full } else { 0.0 };
        if self.flyout.target != target {
            self.flyout.go(target);
        }
    }

    /// True while a width is still travelling towards its target.
    pub fn moving(&self) -> bool {
        self.layout.moving() || self.flyout.moving()
    }

    pub fn tick(&mut self) -> bool {
        self.tick_at(Instant::now())
    }

    /// Advances the animations to `now`; returns true when a width changed.
    pub fn tick_at(&mut self, now: Instant) -> bool {
        let layout = self.layout.tick_at(now);
        let flyout = self.flyout.tick_at(now);
        layout || flyout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: f64 = 240.0;

    fn settle(s: &mut Sidebar) {
        let t0 = Instant::now();
        s.tick_at(t0);
        s.tick_at(t0 + DURATION);
    }

    #[test]
    fn a_hidden_sidebar_starts_without_reserved_width() {
        let mut s = Sidebar::starting(FULL, false);
        assert!(!s.is_open());
        assert!(s.width().abs() < f64::EPSILON);
        s.toggle();
        assert!(s.is_open());
    }

    #[test]
    fn starts_fully_open_and_toggles_between_hidden_and_full() {
        let mut s = Sidebar::new(FULL);
        assert_eq!(s.width(), FULL);
        s.toggle();
        settle(&mut s);
        assert_eq!(s.width(), 0.0);
        s.toggle();
        settle(&mut s);
        assert_eq!(s.width(), FULL);
    }

    #[test]
    fn a_new_width_applies_to_the_open_sidebar_only() {
        let mut s = Sidebar::new(FULL);
        s.set_full(200.0);
        settle(&mut s);
        assert_eq!(s.width(), 200.0);
        s.toggle();
        s.set_full(280.0);
        settle(&mut s);
        assert_eq!(s.width(), 0.0);
        s.toggle();
        settle(&mut s);
        assert_eq!(s.width(), 280.0);
    }

    #[test]
    fn the_width_travels_smoothly_instead_of_jumping() {
        let mut s = Sidebar::new(FULL);
        s.toggle();
        let t0 = Instant::now();
        let mut last = FULL;
        let mut frames = 0;
        for ms in (0..=240).step_by(16) {
            s.tick_at(t0 + Duration::from_millis(ms));
            assert!(s.width() <= last, "width never grows while hiding");
            assert!(
                last - s.width() < FULL * 0.4,
                "no frame covers most of the distance"
            );
            last = s.width();
            frames += 1;
        }
        assert_eq!(last, 0.0);
        assert!(frames > 10);
    }

    #[test]
    fn reversing_mid_flight_starts_from_the_current_width() {
        let mut s = Sidebar::new(FULL);
        s.toggle();
        let t0 = Instant::now();
        s.tick_at(t0);
        s.tick_at(t0 + Duration::from_millis(80));
        let mid = s.width();
        assert!(mid > 0.0 && mid < FULL);
        s.toggle();
        s.tick_at(t0 + Duration::from_millis(90));
        assert!((s.width() - mid).abs() < 1.0);
    }

    #[test]
    fn the_flyout_floats_over_the_page_without_pushing_it() {
        let mut s = Sidebar::new(FULL);
        s.toggle();
        settle(&mut s);
        s.peek(true);
        settle(&mut s);
        assert_eq!(s.flyout_width(), FULL);
        assert_eq!(s.width(), 0.0);
        s.peek(false);
        settle(&mut s);
        assert_eq!(s.flyout_width(), 0.0);
    }

    #[test]
    fn the_flyout_never_shows_on_an_open_sidebar_and_vanishes_when_pinning() {
        let mut s = Sidebar::new(FULL);
        s.peek(true);
        settle(&mut s);
        assert_eq!(s.flyout_width(), 0.0);
        s.toggle();
        settle(&mut s);
        s.peek(true);
        settle(&mut s);
        s.toggle();
        assert_eq!(s.flyout_width(), 0.0);
    }
}

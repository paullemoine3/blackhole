//! Welcome pages extensions open for their first install, in a private window.
//!
//! A private window starts on an empty engine profile (on Windows a throwaway WebView2 folder), so
//! every extension it loads is installed afresh and runs its first-install code: Dark Reader opens
//! its help, Ghostery its onboarding, Bitwarden its start page. Those requests come with no tab
//! behind them, right after the extensions load; a page the person opens has its tab as opener.
//! While a private window starts, such pages are dropped; nothing else is.
//!
//! The welcome tour of the first launch (`onboarding`) installs extensions too: their welcome
//! pages would open as tabs over it, so they are dropped the same way (`tour_installs`).

use std::time::{Duration, Instant};

/// How long after the last extension finished loading a page asked for with no opening tab is
/// taken for a welcome page.
pub const WELCOME_WINDOW: Duration = Duration::from_secs(15);

#[derive(Debug, Default)]
pub struct WelcomePages {
    until: Option<Instant>,
}

impl WelcomePages {
    /// Nothing is ever dropped (a normal window: the welcome page of a real install is wanted).
    pub fn off() -> Self {
        Self::default()
    }

    /// A private window with extensions: drops welcome pages from `now` on.
    pub fn private(now: Instant) -> Self {
        Self { until: Some(now + WELCOME_WINDOW) }
    }

    /// The welcome tour installs extensions: from `now` on, their welcome pages are dropped like
    /// in a private window (each load starts the time over).
    pub fn tour_installs(&mut self, now: Instant) {
        let end = now + WELCOME_WINDOW;
        self.until = Some(self.until.map_or(end, |u| u.max(end)));
    }

    /// An extension finished loading: its first-install code runs now, so the window starts over.
    /// No effect on a window that drops nothing.
    pub fn extension_loaded(&mut self, now: Instant) {
        if self.until.is_some() {
            self.until = Some(now + WELCOME_WINDOW);
        }
    }

    /// Whether a page asked for at `now` is a welcome page to drop. `opener` tells whether a tab
    /// the person sees asked for it (a link, a script of the page).
    pub fn drops(&self, opener: bool, now: Instant) -> bool {
        !opener && self.until.is_some_and(|until| now < until)
    }

    /// Whether some page may still be dropped (the shell keeps watching extension loads).
    pub fn watching(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_window_keeps_every_page() {
        let now = Instant::now();
        let mut pages = WelcomePages::off();
        pages.extension_loaded(now);
        assert!(!pages.drops(false, now));
        assert!(!pages.watching(now));
    }

    #[test]
    fn a_private_window_drops_pages_without_opener_while_extensions_start() {
        let now = Instant::now();
        let pages = WelcomePages::private(now);
        assert!(pages.drops(false, now + Duration::from_secs(3)));
        // A link or a script of a page the person sees always opens.
        assert!(!pages.drops(true, now + Duration::from_secs(3)));
        assert!(!pages.drops(false, now + WELCOME_WINDOW));
        assert!(!pages.watching(now + WELCOME_WINDOW));
    }

    #[test]
    fn each_extension_load_starts_the_window_over() {
        let now = Instant::now();
        let mut pages = WelcomePages::private(now);
        let late = now + Duration::from_secs(12);
        pages.extension_loaded(late);
        assert!(pages.drops(false, now + WELCOME_WINDOW + Duration::from_secs(5)));
        assert!(!pages.drops(false, late + WELCOME_WINDOW));
    }

    #[test]
    fn the_welcome_tour_drops_the_pages_of_the_extensions_it_installs() {
        let now = Instant::now();
        let mut pages = WelcomePages::off();
        pages.tour_installs(now);
        assert!(pages.drops(false, now + Duration::from_secs(3)));
        assert!(!pages.drops(true, now + Duration::from_secs(3)));
        pages.extension_loaded(now + Duration::from_secs(10));
        assert!(pages.drops(false, now + Duration::from_secs(20)));
        assert!(!pages.watching(now + Duration::from_secs(10) + WELCOME_WINDOW));
    }
}

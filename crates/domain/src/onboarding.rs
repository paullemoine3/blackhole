//! The welcome tour (0.8.23): offered once, at the very first launch of a profile, over the start
//! page. It shows the main features, then offers to import bookmarks, to make BlackHole the
//! default browser, a few extensions of the signed market and the look of the first space.
//! Settings > General replays the features alone.

/// What the tour shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// First launch: the features, then the choices (import, default browser, extensions, style).
    Welcome,
    /// Replayed from Settings: the features only.
    Tour,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Tour => "tour",
        }
    }
}

/// Whether this launch offers the welcome tour: only the first launch of a profile (no settings
/// written yet), in a normal window that is the only one open. A private window or a second
/// window never shows it; quitting during the tour does not bring it back.
pub fn offered(first_launch: bool, private: bool, new_window: bool) -> bool {
    first_launch && !private && !new_window
}

/// Extensions of the signed market the tour proposes, in its order, and whether each starts
/// ticked. The Shields already block ads and trackers, so the ad blocker starts unticked; a
/// password manager is only useful to those who already use it.
pub const STARTERS: &[(&str, bool)] = &[
    ("dark-reader", true),
    ("sponsorblock", true),
    ("bitwarden", false),
    ("ublock-origin-lite", false),
];

/// The starters this system can install (`available`: ids of the market entries with a package
/// for it), in the tour's order. An id the list lacks (an older list, no package for this
/// system) is left out; no extension host at all leaves nothing, and the tour skips the step.
pub fn starters<'a>(available: impl IntoIterator<Item = &'a str>) -> Vec<(&'static str, bool)> {
    let available: Vec<&str> = available.into_iter().collect();
    STARTERS
        .iter()
        .copied()
        .filter(|(id, _)| available.contains(id))
        .collect()
}

/// Colors the tour offers for the first space: the design system's eight space colors.
pub const SPACE_COLORS: [&str; 8] = [
    "#7c5cff", "#ff5c8a", "#2fd4a0", "#ffb020", "#3aa0ff", "#e5534b", "#14b8c4", "#8b8a99",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_launch_of_a_normal_window_offers_the_tour() {
        assert!(offered(true, false, false));
        assert!(!offered(false, false, false));
        assert!(!offered(true, true, false));
        assert!(!offered(true, false, true));
    }

    #[test]
    fn starters_keep_the_tour_order_and_only_what_this_system_has() {
        let market = ["ublock-origin-lite", "ghostery", "dark-reader", "bitwarden"];
        assert_eq!(
            starters(market),
            vec![
                ("dark-reader", true),
                ("bitwarden", false),
                ("ublock-origin-lite", false)
            ]
        );
        assert!(starters([]).is_empty());
    }

    #[test]
    fn the_ad_blocker_starts_unticked_since_the_shields_block_ads() {
        assert!(STARTERS.contains(&("ublock-origin-lite", false)));
        assert!(STARTERS.iter().any(|(_, on)| *on));
    }

    #[test]
    fn space_colors_are_hex_colors() {
        for c in SPACE_COLORS {
            assert!(c.len() == 7 && c.starts_with('#'));
            assert!(c[1..].chars().all(|d| d.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn modes_have_ids_for_the_page() {
        assert_eq!(Mode::Welcome.id(), "welcome");
        assert_eq!(Mode::Tour.id(), "tour");
    }
}

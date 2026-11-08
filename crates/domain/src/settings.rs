use crate::{SearchEngine, Shields, SpaceId};
use std::collections::{BTreeMap, BTreeSet};

/// User preferences. New settings are added here with a sensible private default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub search_engine: SearchEngine,
    /// Width of the open sidebar in logical pixels (one of `SIDEBAR_WIDTH_CHOICES`).
    pub sidebar_width: u32,
    /// Plays the black-hole animation when a new tab opens.
    pub animations: bool,
    /// GPU rendering and decoding. Takes effect at the next launch; only the Windows engine
    /// (WebView2) can switch it, WebKit always uses the GPU.
    pub hardware_acceleration: bool,
    /// Minutes a background tab may sit unused before its engine view is released.
    pub tab_sleep_minutes: u32,
    /// Tabs allowed to keep a live engine view at once (one of `LIVE_TABS_CHOICES`); the least
    /// recently used are released.
    pub max_live_tabs: u32,
    /// Blank hidden views kept ready so the next tab opens instantly (one of `SPARE_CHOICES`);
    /// each costs a renderer's worth of memory.
    pub spare_views: u32,
    /// Per-site CSS injected into matching pages ("Boosts"), keyed by host without `www.`.
    pub boosts: BTreeMap<String, String>,
    /// "Air traffic control": links to these hosts always open in the given space.
    pub routes: BTreeMap<String, SpaceId>,
    /// Installed extensions are on by default; the ones the user switched off are listed here.
    pub disabled_extensions: BTreeSet<String>,
    /// Default favorites the user removed from the start page, by site key.
    pub hidden_favorites: BTreeSet<String>,
    /// Version whose release notes the user was last shown (empty before the first launch).
    pub last_seen_version: String,
    /// Installed extensions also run in private windows (off by default: they can see pages).
    pub extensions_in_private: bool,
    /// The sidebar is pinned open (otherwise it slides in on hover). Remembered between launches.
    pub sidebar_open: bool,
    /// Tracker blocklist on (default) or off.
    pub tracker_blocking: bool,
    /// Ad server blocklist on (default) or off.
    pub ad_blocking: bool,
    /// What pages are asked to clean up (ad slots, video ads, cookie banners); all on by default.
    pub shields: Shields,
    /// Calendar sync: a toast when a meeting starts, and the next meeting in the sidebar.
    pub meeting_reminders: bool,
    pub meeting_card: bool,
    /// Joining a calendar meeting opens its notes in an Easel (the meeting copilot).
    pub meeting_notes: bool,
    /// Minutes without use before a wallet with a master password locks again (0: at quit).
    pub password_lock_minutes: u32,
    /// Sidebar sections folded by the user (`pinned`, `folders`). Remembered between launches.
    pub collapsed_sections: BTreeSet<String>,
}

/// Normalises a host so `www.example.com` and `example.com` share boosts and routes.
pub fn site_key(host: &str) -> String {
    host.trim().trim_start_matches("www.").to_ascii_lowercase()
}

/// Picks the least recently used entries to release so that at most `max` stay alive.
/// `protected` entries (visible tabs) are never chosen and count toward the budget.
pub fn overflow_to_release<K: Copy + PartialEq, T: Ord>(
    live: &[(K, T)],
    protected: &[K],
    max: usize,
) -> Vec<K> {
    let mut candidates: Vec<&(K, T)> = live
        .iter()
        .filter(|(id, _)| !protected.contains(id))
        .collect();
    candidates.sort_by(|a, b| a.1.cmp(&b.1));
    let kept = live.len() - candidates.len();
    let excess = (kept + candidates.len()).saturating_sub(max.max(kept));
    candidates.iter().take(excess).map(|(id, _)| *id).collect()
}

impl Settings {
    /// How many tabs may keep a live engine view (visible ones included).
    pub const LIVE_TABS_CHOICES: [u32; 6] = [2, 3, 4, 6, 8, 12];
    pub const DEFAULT_LIVE_TABS: u32 = 4;
    pub const SPARE_CHOICES: [u32; 3] = [0, 1, 2];
    pub const DEFAULT_SPARES: u32 = 2;
    pub const SLEEP_CHOICES: [u32; 3] = [5, 10, 15];
    /// Choices of `password_lock_minutes` (0: only when BlackHole quits).
    pub const LOCK_CHOICES: [u32; 4] = [5, 15, 60, 0];
    pub const DEFAULT_SLEEP_MINUTES: u32 = 10;
    pub const SIDEBAR_WIDTH_CHOICES: [u32; 3] = [200, 240, 280];
    pub const SECTIONS: [&'static str; 2] = ["pinned", "folders"];
    pub const DEFAULT_SIDEBAR_WIDTH: u32 = 240;
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            search_engine: SearchEngine::default(),
            sidebar_width: Self::DEFAULT_SIDEBAR_WIDTH,
            animations: true,
            hardware_acceleration: true,
            tab_sleep_minutes: Self::DEFAULT_SLEEP_MINUTES,
            max_live_tabs: Self::DEFAULT_LIVE_TABS,
            spare_views: Self::DEFAULT_SPARES,
            boosts: BTreeMap::new(),
            routes: BTreeMap::new(),
            disabled_extensions: BTreeSet::new(),
            hidden_favorites: BTreeSet::new(),
            last_seen_version: String::new(),
            extensions_in_private: false,
            sidebar_open: true,
            tracker_blocking: true,
            ad_blocking: true,
            shields: Shields::default(),
            meeting_reminders: true,
            meeting_card: true,
            meeting_notes: true,
            password_lock_minutes: 15,
            collapsed_sections: BTreeSet::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::overflow_to_release;

    #[test]
    fn releases_oldest_unprotected_tabs_over_budget() {
        let live = [(1, 50), (2, 10), (3, 30), (4, 20)];
        assert_eq!(overflow_to_release(&live, &[1], 3), vec![2]);
        assert_eq!(overflow_to_release(&live, &[1], 2), vec![2, 4]);
        assert!(overflow_to_release(&live, &[1], 4).is_empty());
    }

    #[test]
    fn never_releases_protected_tabs() {
        let live = [(1, 1), (2, 2)];
        assert!(overflow_to_release(&live, &[1, 2], 1).is_empty());
    }
}

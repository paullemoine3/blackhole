use crate::{SearchEngine, SpaceId};
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
    /// Tabs allowed to keep a live engine view at once (one of `LIVE_TABS_CHOICES`); the
    /// spare budget is used to preload neighbouring tabs, the least recently used are released.
    pub max_live_tabs: u32,
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

/// Indices of a list of `len` items ordered by distance from `active` (nearest first), the
/// active one excluded: the tabs most likely to be opened next.
pub fn nearest_first(len: usize, active: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).filter(|&i| i != active).collect();
    order.sort_by_key(|&i| (i.abs_diff(active), i));
    order
}

impl Settings {
    /// How many tabs may keep a live engine view (visible ones included).
    pub const LIVE_TABS_CHOICES: [u32; 6] = [2, 3, 4, 6, 8, 12];
    pub const DEFAULT_LIVE_TABS: u32 = 4;
    pub const SLEEP_CHOICES: [u32; 3] = [5, 10, 15];
    pub const DEFAULT_SLEEP_MINUTES: u32 = 10;
    pub const SIDEBAR_WIDTH_CHOICES: [u32; 3] = [200, 240, 280];
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
            boosts: BTreeMap::new(),
            routes: BTreeMap::new(),
            disabled_extensions: BTreeSet::new(),
            hidden_favorites: BTreeSet::new(),
            last_seen_version: String::new(),
            extensions_in_private: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{nearest_first, overflow_to_release};

    #[test]
    fn preloads_nearest_tabs_first() {
        assert_eq!(nearest_first(5, 2), vec![1, 3, 0, 4]);
        assert_eq!(nearest_first(3, 0), vec![1, 2]);
        assert!(nearest_first(1, 0).is_empty());
    }

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

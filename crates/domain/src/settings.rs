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
    /// Per-site CSS injected into matching pages ("Boosts"), keyed by host without `www.`.
    pub boosts: BTreeMap<String, String>,
    /// "Air traffic control": links to these hosts always open in the given space.
    pub routes: BTreeMap<String, SpaceId>,
    /// Installed extensions are on by default; the ones the user switched off are listed here.
    pub disabled_extensions: BTreeSet<String>,
    /// Default favorites the user removed from the start page, by site key.
    pub hidden_favorites: BTreeSet<String>,
}

/// Normalises a host so `www.example.com` and `example.com` share boosts and routes.
pub fn site_key(host: &str) -> String {
    host.trim().trim_start_matches("www.").to_ascii_lowercase()
}

impl Settings {
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
            boosts: BTreeMap::new(),
            routes: BTreeMap::new(),
            disabled_extensions: BTreeSet::new(),
            hidden_favorites: BTreeSet::new(),
        }
    }
}

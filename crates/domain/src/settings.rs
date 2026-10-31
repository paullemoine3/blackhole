use crate::{SearchEngine, SpaceId};
use std::collections::{BTreeMap, BTreeSet};

/// How the left sidebar behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarMode {
    /// Collapsed rail that expands while the cursor hovers it.
    #[default]
    Hover,
    /// Always expanded.
    Static,
}

impl SidebarMode {
    pub const ALL: [SidebarMode; 2] = [Self::Hover, Self::Static];

    pub fn id(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Static => "static",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Hover => "Expand on hover",
            Self::Static => "Always expanded",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Hover => Self::Static,
            Self::Static => Self::Hover,
        }
    }
}

/// User preferences. New settings are added here with a sensible private default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub search_engine: SearchEngine,
    pub sidebar_mode: SidebarMode,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            search_engine: SearchEngine::default(),
            sidebar_mode: SidebarMode::default(),
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

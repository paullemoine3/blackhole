//! Pure business rules. No I/O, no framework: only `url` for URL parsing and
//! `zeroize` to wipe secrets.

pub mod changelog;
pub mod clean_link;
pub mod easel;
pub mod error_page;
pub mod extension;
pub mod handoff;
pub mod highlights;
pub mod history;
pub mod http_auth;
pub mod i18n;
pub mod meetings;
pub mod omnibox;
pub mod passwords;
pub mod pilot;
pub mod pip;
pub mod popup;
pub mod privacy;
pub mod recall;
pub mod selection;
pub mod settings;
pub mod signals;
pub mod sync;
pub mod verbs;
pub mod watch;
pub mod workspace;
pub mod wormhole;

pub use easel::{Easel, Easels, Item as EaselItem, ItemKind as EaselItemKind};
pub use extension::{matches_pattern, ContentScript, Extension, Injection};
pub use history::{History, HistoryEntry};
pub use meetings::Meeting;
pub use omnibox::{is_extension_page, is_extension_popout, resolve_input, SearchEngine};
pub use privacy::{is_third_party, privacy_grade, BlockLog, BlockedHost, PrivacyDecision, PrivacyPolicy, Shields};
pub use settings::{next_zoom, overflow_to_release, site_key, Settings, ZOOM_STEPS};
pub use workspace::{
    Home,
    sanitize_icon, site_of_host, Folder, FolderId, Removed, Space, SpaceId, Tab, TabId, Tidied,
    TidyGroup, Workspace,
};

//! Pure business rules. No I/O, no framework: only `url` for URL parsing.

pub mod changelog;
pub mod easel;
pub mod extension;
pub mod history;
pub mod omnibox;
pub mod privacy;
pub mod settings;
pub mod workspace;

pub use easel::{Easel, Easels, Item as EaselItem, ItemKind as EaselItemKind};
pub use extension::{matches_pattern, ContentScript, Extension, Injection};
pub use history::{History, HistoryEntry};
pub use omnibox::{is_extension_page, is_extension_popout, resolve_input, SearchEngine};
pub use privacy::{BlockLog, BlockedHost, PrivacyDecision, PrivacyPolicy};
pub use settings::{overflow_to_release, site_key, Settings};
pub use workspace::{
    sanitize_icon, Folder, FolderId, Removed, Space, SpaceId, Tab, TabId, Workspace,
};

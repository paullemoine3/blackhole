//! Pure business rules. No I/O, no framework: only `url` for URL parsing.

pub mod extension;
pub mod history;
pub mod omnibox;
pub mod privacy;
pub mod settings;
pub mod workspace;

pub use extension::{matches_pattern, ContentScript, Extension, Injection};
pub use history::{History, HistoryEntry};
pub use omnibox::{resolve_input, SearchEngine};
pub use privacy::{BlockLog, BlockedHost, PrivacyDecision, PrivacyPolicy};
pub use settings::{site_key, Settings, SidebarMode};
pub use workspace::{sanitize_icon, Folder, FolderId, Space, SpaceId, Tab, TabId, Workspace};

//! Adapters: filesystem persistence and the embedded tracker blocklist.

use blackhole_application::{StorageError, WorkspaceRepository};
use blackhole_domain::{
    ContentScript, Extension, Folder, FolderId, History, HistoryEntry, PrivacyPolicy, SearchEngine,
    Settings, Space, SpaceId, Tab, TabId, Workspace,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use url::Url;

pub mod catalog;
pub mod import;
pub mod peers;
pub mod store;
pub mod updater;
mod writer;

pub use writer::{flush_writes, write_later};

const BLOCKLIST: &str = include_str!("../blocklist.txt");

pub fn default_privacy_policy() -> PrivacyPolicy {
    PrivacyPolicy::new(
        BLOCKLIST
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_owned),
    )
}

/// Per-user data directory (`~/Library/Application Support` on macOS, `%APPDATA%` on Windows).
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("BlackHole")
}

/// Size announced by the server for a download (anonymous HEAD request); `None` when unknown.
pub fn remote_size(url: &str) -> Option<u64> {
    if !url.starts_with("https://") {
        return None;
    }
    ureq::head(url)
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .ok()?
        .header("Content-Length")?
        .parse()
        .ok()
        .filter(|n| *n > 0)
}

/// Where downloaded files land.
pub fn downloads_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| std::env::temp_dir().join("BlackHole Downloads"))
}

/// `Name.ext` -> `Name (2).ext` ... until the path is free.
pub fn unique_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let name = std::path::Path::new(name).file_name().map_or_else(
        || "download".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let first = dir.join(&name);
    if !first.exists() {
        return first;
    }
    let path = std::path::Path::new(&name);
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// What kind of media a file name is, by extension.
pub fn media_kind(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" => Some("image"),
        "mp4" | "mov" | "webm" | "mkv" | "m4v" => Some("video"),
        _ => None,
    }
}

/// An image file as a `data:` URL for thumbnails; `None` when missing or larger than `max` bytes.
pub fn image_data_url(path: &std::path::Path, max: u64) -> Option<String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let name = path.file_name()?.to_string_lossy().into_owned();
    if media_kind(&name) != Some("image") || fs::metadata(path).ok()?.len() > max {
        return None;
    }
    let mime = match name.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        _ => "image/png",
    };
    let bytes = fs::read(path).ok()?;
    let mut out = format!("data:{mime};base64,");
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    Some(out)
}

/// Folder that holds one sub-folder per installed extension.
pub fn extensions_dir(data: &std::path::Path) -> PathBuf {
    data.join("extensions")
}

#[derive(Deserialize)]
struct ManifestDto {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    content_scripts: Vec<ContentScriptDto>,
}

#[derive(Deserialize)]
struct ContentScriptDto {
    #[serde(default)]
    matches: Vec<String>,
    #[serde(default)]
    css: Vec<String>,
    #[serde(default)]
    js: Vec<String>,
}

/// Largest script or stylesheet file an extension may ship; protects startup from huge files.
const MAX_EXTENSION_FILE: u64 = 2 * 1024 * 1024;

/// Reads every `<dir>/<id>/manifest.json`. A broken extension is skipped, never fatal. Files are
/// resolved inside the extension folder only (no absolute paths or `..`).
pub fn load_extensions(dir: &std::path::Path) -> Vec<Extension> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Extension> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| load_extension(&e.path()))
        .collect();
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

fn load_extension(root: &std::path::Path) -> Option<Extension> {
    let id = root.file_name()?.to_str()?.to_owned();
    let manifest: ManifestDto =
        serde_json::from_str(&fs::read_to_string(root.join("manifest.json")).ok()?).ok()?;
    let read = |rel: &String| -> Option<String> {
        let path = std::path::Path::new(rel);
        let safe = path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
        let full = root.join(path);
        if !safe || fs::metadata(&full).ok()?.len() > MAX_EXTENSION_FILE {
            return None;
        }
        fs::read_to_string(full).ok()
    };
    let scripts = manifest
        .content_scripts
        .iter()
        .map(|s| ContentScript {
            matches: s.matches.clone(),
            css: s.css.iter().filter_map(read).collect(),
            js: s.js.iter().filter_map(read).collect(),
        })
        .collect();
    Some(Extension {
        id,
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        scripts,
    })
}

#[derive(Serialize, Deserialize)]
struct TabDto {
    id: u64,
    title: String,
    url: String,
    pinned: bool,
    #[serde(default)]
    folder: Option<u64>,
    #[serde(default)]
    renamed: bool,
}

#[derive(Serialize, Deserialize)]
struct FolderDto {
    id: u64,
    name: String,
    collapsed: bool,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    parent: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct SpaceDto {
    id: u64,
    name: String,
    color: String,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    split: Option<u64>,
    active_tab: Option<u64>,
    tabs: Vec<TabDto>,
    #[serde(default)]
    folders: Vec<FolderDto>,
}

#[derive(Serialize, Deserialize)]
struct WorkspaceDto {
    version: u32,
    active_space: u64,
    spaces: Vec<SpaceDto>,
}

fn to_dto(ws: &Workspace) -> WorkspaceDto {
    WorkspaceDto {
        version: 1,
        active_space: ws.active_space().id.0,
        spaces: ws
            .spaces()
            .iter()
            .map(|s| SpaceDto {
                id: s.id.0,
                name: s.name.clone(),
                color: s.color.clone(),
                icon: s.icon.clone(),
                split: s.split.map(|t| t.0),
                active_tab: s.active_tab.map(|t| t.0),
                tabs: s
                    .tabs
                    .iter()
                    .map(|t| TabDto {
                        id: t.id.0,
                        title: t.title.clone(),
                        url: t.url.to_string(),
                        pinned: t.pinned,
                        folder: t.folder.map(|f| f.0),
                        renamed: t.renamed,
                    })
                    .collect(),
                folders: s
                    .folders
                    .iter()
                    .map(|f| FolderDto {
                        id: f.id.0,
                        name: f.name.clone(),
                        collapsed: f.collapsed,
                        icon: f.icon.clone(),
                        parent: f.parent.map(|p| p.0),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn from_dto(dto: WorkspaceDto) -> Workspace {
    let spaces = dto
        .spaces
        .into_iter()
        .map(|s| {
            let folders: Vec<Folder> = s
                .folders
                .into_iter()
                .map(|f| Folder {
                    id: FolderId(f.id),
                    name: f.name,
                    collapsed: f.collapsed,
                    icon: blackhole_domain::sanitize_icon(&f.icon),
                    parent: f.parent.map(FolderId),
                })
                .collect();
            // Drop dangling or self-referencing parents so the tree stays well formed.
            let ids: Vec<u64> = folders.iter().map(|f| f.id.0).collect();
            let folders: Vec<Folder> = folders
                .into_iter()
                .map(|mut f| {
                    if f.parent.is_some_and(|p| p == f.id || !ids.contains(&p.0)) {
                        f.parent = None;
                    }
                    f
                })
                .collect();
            let known = |id: u64| folders.iter().any(|f| f.id.0 == id);
            Space {
                id: SpaceId(s.id),
                name: s.name,
                color: s.color,
                icon: blackhole_domain::sanitize_icon(&s.icon),
                split: s.split.map(TabId),
                active_tab: s.active_tab.map(TabId),
                tabs: s
                    .tabs
                    .into_iter()
                    .filter_map(|t| {
                        Some(Tab {
                            id: TabId(t.id),
                            title: t.title,
                            url: Url::parse(&t.url).ok()?,
                            pinned: t.pinned,
                            folder: t.folder.filter(|f| known(*f) && !t.pinned).map(FolderId),
                            renamed: t.renamed,
                        })
                    })
                    .collect(),
                folders,
            }
        })
        .collect();
    Workspace::restore(spaces, SpaceId(dto.active_space))
}

pub struct JsonWorkspaceRepository {
    path: PathBuf,
}

impl JsonWorkspaceRepository {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            path: dir.join("workspace.json"),
        }
    }
}

impl WorkspaceRepository for JsonWorkspaceRepository {
    fn load(&self) -> Result<Option<Workspace>, StorageError> {
        writer::flush_writes();
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.path).map_err(|e| StorageError(e.to_string()))?;
        let dto: WorkspaceDto =
            serde_json::from_str(&raw).map_err(|e| StorageError(e.to_string()))?;
        Ok(Some(from_dto(dto)))
    }

    fn load_settings(&self) -> Result<Option<Settings>, StorageError> {
        writer::flush_writes();
        let path = self.path.with_file_name("settings.json");
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path).map_err(|e| StorageError(e.to_string()))?;
        let dto: SettingsDto =
            serde_json::from_str(&raw).map_err(|e| StorageError(e.to_string()))?;
        Ok(Some(Settings {
            search_engine: SearchEngine::from_id(&dto.search_engine).unwrap_or_default(),
            sidebar_width: if Settings::SIDEBAR_WIDTH_CHOICES.contains(&dto.sidebar_width) {
                dto.sidebar_width
            } else {
                Settings::DEFAULT_SIDEBAR_WIDTH
            },
            animations: dto.animations,
            hardware_acceleration: dto.hardware_acceleration,
            tab_sleep_minutes: if Settings::SLEEP_CHOICES.contains(&dto.tab_sleep_minutes) {
                dto.tab_sleep_minutes
            } else {
                Settings::DEFAULT_SLEEP_MINUTES
            },
            max_live_tabs: if Settings::LIVE_TABS_CHOICES.contains(&dto.max_live_tabs) {
                dto.max_live_tabs
            } else {
                Settings::DEFAULT_LIVE_TABS
            },
            spare_views: if Settings::SPARE_CHOICES.contains(&dto.spare_views) {
                dto.spare_views
            } else {
                Settings::DEFAULT_SPARES
            },
            boosts: dto.boosts,
            routes: dto
                .routes
                .into_iter()
                .map(|(host, id)| (host, SpaceId(id)))
                .collect(),
            disabled_extensions: dto.disabled_extensions.into_iter().collect(),
            hidden_favorites: dto.hidden_favorites.into_iter().collect(),
            last_seen_version: dto.last_seen_version,
            extensions_in_private: dto.extensions_in_private,
            sidebar_open: dto.sidebar_open,
            tracker_blocking: dto.tracker_blocking,
            collapsed_sections: dto.collapsed_sections.into_iter().collect(),
        }))
    }

    fn save_settings(&self, settings: &Settings) -> Result<(), StorageError> {
        let dto = SettingsDto {
            search_engine: settings.search_engine.id().to_owned(),
            sidebar_width: settings.sidebar_width,
            animations: settings.animations,
            hardware_acceleration: settings.hardware_acceleration,
            tab_sleep_minutes: settings.tab_sleep_minutes,
            max_live_tabs: settings.max_live_tabs,
            spare_views: settings.spare_views,
            boosts: settings.boosts.clone(),
            routes: settings
                .routes
                .iter()
                .map(|(host, id)| (host.clone(), id.0))
                .collect(),
            disabled_extensions: settings.disabled_extensions.iter().cloned().collect(),
            hidden_favorites: settings.hidden_favorites.iter().cloned().collect(),
            last_seen_version: settings.last_seen_version.clone(),
            extensions_in_private: settings.extensions_in_private,
            sidebar_open: settings.sidebar_open,
            tracker_blocking: settings.tracker_blocking,
            collapsed_sections: settings.collapsed_sections.iter().cloned().collect(),
        };
        let json = serde_json::to_string_pretty(&dto).map_err(|e| StorageError(e.to_string()))?;
        write_atomic(&self.path.with_file_name("settings.json"), &json)
    }

    fn load_history(&self) -> Result<Option<History>, StorageError> {
        writer::flush_writes();
        let path = self.path.with_file_name("history.json");
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path).map_err(|e| StorageError(e.to_string()))?;
        let dtos: Vec<HistoryDto> =
            serde_json::from_str(&raw).map_err(|e| StorageError(e.to_string()))?;
        let entries = dtos
            .into_iter()
            .filter_map(|d| {
                Some(HistoryEntry {
                    url: Url::parse(&d.url).ok()?,
                    title: d.title,
                    visited_at: d.visited_at,
                })
            })
            .collect();
        Ok(Some(History::restore(entries)))
    }

    fn save_history(&self, history: &History) -> Result<(), StorageError> {
        let dtos: Vec<HistoryDto> = history
            .entries()
            .iter()
            .map(|e| HistoryDto {
                url: e.url.to_string(),
                title: e.title.clone(),
                visited_at: e.visited_at,
            })
            .collect();
        let json = serde_json::to_string(&dtos).map_err(|e| StorageError(e.to_string()))?;
        write_atomic(&self.path.with_file_name("history.json"), &json)
    }

    fn save(&self, workspace: &Workspace) -> Result<(), StorageError> {
        let json = serde_json::to_string_pretty(&to_dto(workspace))
            .map_err(|e| StorageError(e.to_string()))?;
        write_atomic(&self.path, &json)
    }
}

#[derive(Serialize, Deserialize)]
struct HistoryDto {
    url: String,
    title: String,
    visited_at: u64,
}

#[derive(Serialize, Deserialize)]
struct SettingsDto {
    search_engine: String,
    #[serde(default)]
    sidebar_width: u32,
    #[serde(default = "default_true")]
    animations: bool,
    #[serde(default = "default_true")]
    hardware_acceleration: bool,
    #[serde(default)]
    tab_sleep_minutes: u32,
    #[serde(default)]
    max_live_tabs: u32,
    #[serde(default = "default_spares")]
    spare_views: u32,
    #[serde(default)]
    boosts: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    routes: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    disabled_extensions: Vec<String>,
    #[serde(default)]
    hidden_favorites: Vec<String>,
    #[serde(default)]
    last_seen_version: String,
    #[serde(default)]
    extensions_in_private: bool,
    #[serde(default = "default_true")]
    sidebar_open: bool,
    #[serde(default = "default_true")]
    tracker_blocking: bool,
    #[serde(default)]
    collapsed_sections: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_spares() -> u32 {
    Settings::DEFAULT_SPARES
}

/// Queues the write on the background thread (see `writer`).
fn write_atomic(path: &std::path::Path, contents: &str) -> Result<(), StorageError> {
    writer::write_later(path.to_path_buf(), contents.to_owned());
    Ok(())
}

fn write_atomic_now(path: &std::path::Path, contents: &str) -> Result<(), StorageError> {
    let err = |e: std::io::Error| StorageError(e.to_string());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(err)?;
    }
    // Write-then-rename so a crash never leaves a truncated file.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, contents).map_err(err)?;
    fs::rename(&tmp, path).map_err(err)
}

#[cfg(test)]
mod tests {
    #[test]
    fn download_helpers_pick_free_names_and_encode_images() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), [1u8, 2, 3, 4]).unwrap();
        assert_eq!(
            super::unique_path(dir.path(), "a.png").file_name().unwrap(),
            "a (2).png"
        );
        assert_eq!(
            super::unique_path(dir.path(), "../evil.txt")
                .parent()
                .unwrap(),
            dir.path()
        );
        assert_eq!(super::media_kind("Clip.MP4"), Some("video"));
        assert_eq!(super::media_kind("notes.txt"), None);
        let url = super::image_data_url(&dir.path().join("a.png"), 100).unwrap();
        assert_eq!(url, "data:image/png;base64,AQIDBA==");
        assert!(super::image_data_url(&dir.path().join("a.png"), 2).is_none());
    }

    use super::*;

    #[test]
    fn extensions_load_from_folders_and_skip_unsafe_or_broken_ones() {
        let dir = std::env::temp_dir().join(format!("bh_ext_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let ok = dir.join("dark");
        fs::create_dir_all(&ok).unwrap();
        fs::write(ok.join("manifest.json"), r#"{"name":"Dark","version":"1.0","content_scripts":[{"matches":["<all_urls>"],"css":["a.css","../secret.css"],"js":["a.js"]}]}"#).unwrap();
        fs::write(ok.join("a.css"), "body{x:1}").unwrap();
        fs::write(ok.join("a.js"), "var a=1").unwrap();
        fs::write(dir.join("secret.css"), "leak").unwrap();
        let broken = dir.join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("manifest.json"), "not json").unwrap();

        let found = load_extensions(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "dark");
        assert_eq!(found[0].scripts[0].css, vec!["body{x:1}".to_owned()]);
        assert_eq!(found[0].scripts[0].js, vec!["var a=1".to_owned()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn icons_and_split_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonWorkspaceRepository::new(dir.path().into());
        let mut ws = Workspace::new();
        let space = ws.active_space().id;
        let a = ws.open_tab(Url::parse("https://a.test").unwrap());
        ws.open_tab(Url::parse("https://b.test").unwrap());
        let folder = ws.add_folder("Work");
        ws.set_space_icon(space, "🚀");
        ws.set_folder_icon(folder, "📁");
        ws.set_split(a);
        repo.save(&ws).unwrap();
        let loaded = repo.load().unwrap().unwrap();
        assert_eq!(loaded, ws);
        assert_eq!(loaded.active_space().icon, "🚀");
        assert_eq!(loaded.active_space().folders[0].icon, "📁");
        assert_eq!(loaded.split_tab(), Some(a));
    }

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonWorkspaceRepository::new(dir.path().into());
        let mut ws = Workspace::new();
        ws.open_tab(Url::parse("https://example.com").unwrap());
        repo.save(&ws).unwrap();
        assert_eq!(repo.load().unwrap().unwrap(), ws);
    }

    #[test]
    fn settings_roundtrip_and_old_files_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonWorkspaceRepository::new(dir.path().into());
        let s = Settings {
            search_engine: SearchEngine::Brave,
            sidebar_width: 280,
            animations: false,
            hardware_acceleration: false,
            tab_sleep_minutes: 15,
            max_live_tabs: 8,
            spare_views: 0,
            boosts: [(                "example.com".to_owned(),
                "body{filter:invert(1)}".to_owned(),
            )]
            .into(),
            routes: [("github.com".to_owned(), SpaceId(2))].into(),
            disabled_extensions: ["dark".to_owned()].into(),
            hidden_favorites: ["proton.me".to_owned()].into(),
            last_seen_version: "0.3.0".to_owned(),
            extensions_in_private: true,
            sidebar_open: false,
            tracker_blocking: false,
            collapsed_sections: ["pinned".to_owned()].into(),
        };
        repo.save_settings(&s).unwrap();
        assert_eq!(repo.load_settings().unwrap().unwrap(), s);
        fs::write(
            dir.path().join("settings.json"),
            r#"{"search_engine":"qwant"}"#,
        )
        .unwrap();
        let old = repo.load_settings().unwrap().unwrap();
        assert_eq!(old.search_engine, SearchEngine::Qwant);
        assert_eq!(old.sidebar_width, Settings::DEFAULT_SIDEBAR_WIDTH);
        assert!(old.animations && old.hardware_acceleration);
        assert_eq!(old.tab_sleep_minutes, 10);
        assert_eq!(old.max_live_tabs, Settings::DEFAULT_LIVE_TABS);
        assert_eq!(old.spare_views, Settings::DEFAULT_SPARES);
        assert!(old.boosts.is_empty() && old.routes.is_empty());
        assert_eq!(old.last_seen_version, "");
        assert!(!old.extensions_in_private);
        assert!(old.sidebar_open, "pinned open by default");
        assert!(old.tracker_blocking, "blocker on by default");
        assert!(old.collapsed_sections.is_empty());
    }

    #[test]
    fn history_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JsonWorkspaceRepository::new(dir.path().into());
        let mut h = History::default();
        h.visit(&Url::parse("https://example.com/").unwrap(), 42);
        repo.save_history(&h).unwrap();
        assert_eq!(repo.load_history().unwrap().unwrap(), h);
    }

    #[test]
    fn blocklist_loads() {
        assert!(default_privacy_policy().is_blocked_host("stats.doubleclick.net"));
    }
}

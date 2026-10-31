//! Adapters: filesystem persistence and the embedded tracker blocklist.

use blackhole_application::{StorageError, WorkspaceRepository};
use blackhole_domain::{
    ContentScript, Extension, Folder, FolderId, History, HistoryEntry, PrivacyPolicy, SearchEngine,
    Settings, SidebarMode, Space, SpaceId, Tab, TabId, Workspace,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use url::Url;

pub mod catalog;
pub mod import;
pub mod store;

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
}

#[derive(Serialize, Deserialize)]
struct FolderDto {
    id: u64,
    name: String,
    collapsed: bool,
    #[serde(default)]
    icon: String,
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
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.path).map_err(|e| StorageError(e.to_string()))?;
        let dto: WorkspaceDto =
            serde_json::from_str(&raw).map_err(|e| StorageError(e.to_string()))?;
        Ok(Some(from_dto(dto)))
    }

    fn load_settings(&self) -> Result<Option<Settings>, StorageError> {
        let path = self.path.with_file_name("settings.json");
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path).map_err(|e| StorageError(e.to_string()))?;
        let dto: SettingsDto =
            serde_json::from_str(&raw).map_err(|e| StorageError(e.to_string()))?;
        Ok(Some(Settings {
            search_engine: SearchEngine::from_id(&dto.search_engine).unwrap_or_default(),
            sidebar_mode: SidebarMode::from_id(&dto.sidebar_mode).unwrap_or_default(),
            animations: dto.animations,
            hardware_acceleration: dto.hardware_acceleration,
            tab_sleep_minutes: if Settings::SLEEP_CHOICES.contains(&dto.tab_sleep_minutes) {
                dto.tab_sleep_minutes
            } else {
                Settings::DEFAULT_SLEEP_MINUTES
            },
            boosts: dto.boosts,
            routes: dto
                .routes
                .into_iter()
                .map(|(host, id)| (host, SpaceId(id)))
                .collect(),
            disabled_extensions: dto.disabled_extensions.into_iter().collect(),
            hidden_favorites: dto.hidden_favorites.into_iter().collect(),
        }))
    }

    fn save_settings(&self, settings: &Settings) -> Result<(), StorageError> {
        let dto = SettingsDto {
            search_engine: settings.search_engine.id().to_owned(),
            sidebar_mode: settings.sidebar_mode.id().to_owned(),
            animations: settings.animations,
            hardware_acceleration: settings.hardware_acceleration,
            tab_sleep_minutes: settings.tab_sleep_minutes,
            boosts: settings.boosts.clone(),
            routes: settings
                .routes
                .iter()
                .map(|(host, id)| (host.clone(), id.0))
                .collect(),
            disabled_extensions: settings.disabled_extensions.iter().cloned().collect(),
            hidden_favorites: settings.hidden_favorites.iter().cloned().collect(),
        };
        let json = serde_json::to_string_pretty(&dto).map_err(|e| StorageError(e.to_string()))?;
        write_atomic(&self.path.with_file_name("settings.json"), &json)
    }

    fn load_history(&self) -> Result<Option<History>, StorageError> {
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
    sidebar_mode: String,
    #[serde(default = "default_true")]
    animations: bool,
    #[serde(default = "default_true")]
    hardware_acceleration: bool,
    #[serde(default)]
    tab_sleep_minutes: u32,
    #[serde(default)]
    boosts: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    routes: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    disabled_extensions: Vec<String>,
    #[serde(default)]
    hidden_favorites: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn write_atomic(path: &std::path::Path, contents: &str) -> Result<(), StorageError> {
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
            sidebar_mode: SidebarMode::Static,
            animations: false,
            hardware_acceleration: false,
            tab_sleep_minutes: 15,
            boosts: [(
                "example.com".to_owned(),
                "body{filter:invert(1)}".to_owned(),
            )]
            .into(),
            routes: [("github.com".to_owned(), SpaceId(2))].into(),
            disabled_extensions: ["dark".to_owned()].into(),
            hidden_favorites: ["proton.me".to_owned()].into(),
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
        assert_eq!(old.sidebar_mode, SidebarMode::Hover);
        assert!(old.animations && old.hardware_acceleration);
        assert_eq!(old.tab_sleep_minutes, 10);
        assert!(old.boosts.is_empty() && old.routes.is_empty());
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

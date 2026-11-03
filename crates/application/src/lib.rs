//! Use cases and ports. Depends only on the domain.

use blackhole_domain::{
    resolve_input, site_key, Extension, FolderId, History, HistoryEntry, Injection,
    PrivacyDecision, PrivacyPolicy, SearchEngine, Settings, SpaceId, TabId, Workspace,
};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

const DEFAULT_FAVORITES: [(&str, &str); 6] = [
    ("DuckDuckGo", "https://duckduckgo.com/"),
    ("Wikipedia", "https://www.wikipedia.org/"),
    ("GitHub", "https://github.com/"),
    ("OpenStreetMap", "https://www.openstreetmap.org/"),
    ("Proton", "https://proton.me/"),
    ("Mozilla", "https://www.mozilla.org/"),
];
const MAX_FAVORITES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    pub name: String,
    pub url: Url,
    /// The pinned tab behind this favorite, when it is one (the others are built-in defaults).
    pub tab: Option<TabId>,
}

#[derive(Debug, thiserror::Error)]
#[error("storage error: {0}")]
pub struct StorageError(pub String);

/// Port: persistence of the user's workspace and settings (implemented in infrastructure).
pub trait WorkspaceRepository: Send {
    fn load(&self) -> Result<Option<Workspace>, StorageError>;
    fn save(&self, workspace: &Workspace) -> Result<(), StorageError>;
    fn load_settings(&self) -> Result<Option<Settings>, StorageError>;
    fn save_settings(&self, settings: &Settings) -> Result<(), StorageError>;
    fn load_history(&self) -> Result<Option<History>, StorageError>;
    fn save_history(&self, history: &History) -> Result<(), StorageError>;
}

/// What the shell must do after a navigation request was evaluated.
#[derive(Debug, PartialEq)]
pub enum NavigationOutcome {
    Go(Url),
    Blocked(&'static str),
}

pub struct BrowserService<R: WorkspaceRepository> {
    workspace: Workspace,
    settings: Settings,
    history: History,
    policy: PrivacyPolicy,
    extensions: Vec<Extension>,
    repo: R,
    /// No settings file existed at startup: a brand-new installation.
    fresh: bool,
}

impl<R: WorkspaceRepository> BrowserService<R> {
    pub fn start(repo: R, policy: PrivacyPolicy) -> Self {
        // A corrupt or missing session must never prevent the browser from starting.
        let workspace = repo.load().ok().flatten().unwrap_or_default();
        let stored = repo.load_settings().ok().flatten();
        let fresh = stored.is_none();
        let settings = stored.unwrap_or_default();
        let history = repo.load_history().ok().flatten().unwrap_or_default();
        Self {
            workspace,
            settings,
            history,
            policy,
            extensions: Vec::new(),
            repo,
            fresh,
        }
    }

    /// Release notes to show once after an update; remembers `current` so they never come back.
    pub fn take_release_notes(
        &mut self,
        current: &str,
    ) -> Vec<(&'static str, &'static [&'static str])> {
        let notes = blackhole_domain::changelog::pending(
            &self.settings.last_seen_version,
            current,
            self.fresh,
        );
        if self.settings.last_seen_version != current {
            self.settings.last_seen_version = current.to_owned();
            let _ = self.repo.save_settings(&self.settings);
        }
        notes
    }

    /// Pinned tabs first, then the built-in defaults, one per host.
    pub fn favorites(&self) -> Vec<Favorite> {
        let pinned = self
            .workspace
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter().filter(|t| t.pinned))
            .map(|t| Favorite {
                name: if t.title.trim().is_empty() {
                    t.url.host_str().unwrap_or_default().to_owned()
                } else {
                    t.title.clone()
                },
                url: t.url.clone(),
                tab: Some(t.id),
            });
        let defaults = DEFAULT_FAVORITES.iter().filter_map(|(name, url)| {
            Some(Favorite {
                name: (*name).to_owned(),
                url: Url::parse(url).ok()?,
                tab: None,
            })
        });
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for fav in pinned.chain(defaults) {
            let host = fav
                .url
                .host_str()
                .unwrap_or_default()
                .trim_start_matches("www.")
                .to_owned();
            if !seen.contains(&host) && !self.settings.hidden_favorites.contains(&host) {
                seen.push(host);
                out.push(fav);
            }
        }
        out.truncate(MAX_FAVORITES);
        out
    }

    pub fn recent_activity(&self, n: usize) -> &[HistoryEntry] {
        self.history.recent(n)
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
        let _ = self.repo.save_history(&self.history);
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_search_engine(&mut self, engine: SearchEngine) {
        self.settings.search_engine = engine;
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn set_sidebar_width(&mut self, width: u32) {
        if Settings::SIDEBAR_WIDTH_CHOICES.contains(&width) {
            self.settings.sidebar_width = width;
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    pub fn set_hardware_acceleration(&mut self, on: bool) {
        self.settings.hardware_acceleration = on;
        self.persist();
    }

    pub fn set_extensions_in_private(&mut self, on: bool) {
        self.settings.extensions_in_private = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    /// Remembers whether the sidebar is pinned open, for the next launch.
    pub fn set_sidebar_open(&mut self, open: bool) {
        if self.settings.sidebar_open != open {
            self.settings.sidebar_open = open;
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    pub fn set_animations(&mut self, on: bool) {
        self.settings.animations = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn set_tab_sleep_minutes(&mut self, minutes: u32) {
        if Settings::SLEEP_CHOICES.contains(&minutes) {
            self.settings.tab_sleep_minutes = minutes;
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    pub fn set_extensions(&mut self, extensions: Vec<Extension>) {
        self.extensions = extensions;
    }

    pub fn extensions(&self) -> &[Extension] {
        &self.extensions
    }

    pub fn extension_enabled(&self, id: &str) -> bool {
        !self.settings.disabled_extensions.contains(id)
    }

    pub fn set_extension_enabled(&mut self, id: &str, enabled: bool) {
        if !self.extensions.iter().any(|e| e.id == id) {
            return;
        }
        self.set_enabled_flag(id, enabled);
    }

    /// Same switch for extensions the shell hosts itself (the catalog), which are not in
    /// `extensions`. The caller vouches for the id.
    pub fn set_enabled_flag(&mut self, id: &str, enabled: bool) {
        if enabled {
            self.settings.disabled_extensions.remove(id);
        } else {
            self.settings.disabled_extensions.insert(id.to_owned());
        }
        let _ = self.repo.save_settings(&self.settings);
    }

    /// What the enabled extensions inject into `url`.
    pub fn injection_for(&self, url: &Url) -> Injection {
        let mut out = Injection::default();
        for ext in self
            .extensions
            .iter()
            .filter(|e| self.extension_enabled(&e.id))
        {
            let part = ext.injection_for(url);
            out.css.push_str(&part.css);
            out.js.push_str(&part.js);
        }
        out
    }

    /// CSS the user attached to a site ("Boost"), if any.
    pub fn boost_for(&self, host: &str) -> Option<&str> {
        self.settings
            .boosts
            .get(&site_key(host))
            .map(String::as_str)
    }

    /// Stores (or, when blank, removes) the Boost of a site.
    pub fn set_boost(&mut self, host: &str, css: &str) {
        let key = site_key(host);
        if css.trim().is_empty() {
            self.settings.boosts.remove(&key);
        } else {
            self.settings.boosts.insert(key, css.to_owned());
        }
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn route_for(&self, host: &str) -> Option<SpaceId> {
        self.settings.routes.get(&site_key(host)).copied()
    }

    /// Air traffic control: a host always opens in `space`; `None` clears the rule.
    pub fn set_route(&mut self, host: &str, space: Option<SpaceId>) {
        let key = site_key(host);
        match space {
            Some(id) => self.settings.routes.insert(key, id),
            None => self.settings.routes.remove(&key),
        };
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn policy(&self) -> &PrivacyPolicy {
        &self.policy
    }

    pub fn evaluate(&self, url: &Url) -> NavigationOutcome {
        match self.policy.evaluate(url) {
            PrivacyDecision::Allow => NavigationOutcome::Go(url.clone()),
            PrivacyDecision::Rewrite(u) => NavigationOutcome::Go(u),
            PrivacyDecision::Block(why) => NavigationOutcome::Blocked(why),
        }
    }

    pub fn resolve(&self, input: &str) -> NavigationOutcome {
        self.evaluate(&resolve_input(input, self.settings.search_engine))
    }

    pub fn open_tab(&mut self, input: &str) -> Option<TabId> {
        match self.resolve(input) {
            NavigationOutcome::Go(url) => {
                let routed = url.host_str().and_then(|host| self.route_for(host));
                if let Some(space) = routed {
                    self.workspace.switch_space(space);
                }
                // Like Arc: a tab already open in this space is reused instead of duplicated.
                if let Some(id) = self.workspace.find_tab(&url) {
                    self.workspace.activate_tab(id);
                    self.persist();
                    return Some(id);
                }
                let id = self.workspace.open_tab(url);
                self.persist();
                Some(id)
            }
            NavigationOutcome::Blocked(_) => None,
        }
    }

    pub fn activate_tab(&mut self, id: TabId) -> bool {
        let ok = self.workspace.activate_tab(id);
        self.persist();
        ok
    }

    pub fn close_tab(&mut self, id: TabId) -> bool {
        let ok = self.workspace.close_tab(id);
        self.persist();
        ok
    }

    pub fn toggle_pin(&mut self, id: TabId) -> bool {
        let ok = self.workspace.toggle_pin(id);
        self.persist();
        ok
    }

    pub fn move_tab(
        &mut self,
        id: TabId,
        before: Option<TabId>,
        folder: Option<FolderId>,
        pinned: bool,
    ) -> bool {
        let ok = self.workspace.move_tab(id, before, folder, pinned);
        self.persist();
        ok
    }

    pub fn import_tabs(&mut self, space: SpaceId, links: Vec<(String, String, Url)>) -> usize {
        let added = self.workspace.import_tabs(space, links);
        if added > 0 {
            self.persist();
        }
        added
    }

    pub fn move_space(&mut self, id: SpaceId, before: Option<SpaceId>) -> bool {
        let ok = self.workspace.move_space(id, before);
        self.persist();
        ok
    }

    pub fn add_folder(&mut self, name: &str) -> FolderId {
        let id = self.workspace.add_folder(name);
        self.persist();
        id
    }

    pub fn rename_space(&mut self, id: SpaceId, name: &str) -> bool {
        let ok = self.workspace.rename_space(id, name);
        self.persist();
        ok
    }

    pub fn rename_folder(&mut self, id: FolderId, name: &str) -> bool {
        let ok = self.workspace.rename_folder(id, name);
        self.persist();
        ok
    }

    pub fn toggle_folder(&mut self, id: FolderId) -> bool {
        let ok = self.workspace.toggle_folder(id);
        self.persist();
        ok
    }

    pub fn add_subfolder(&mut self, name: &str, parent: FolderId) -> Option<FolderId> {
        let id = self.workspace.add_folder_in(name, Some(parent));
        self.persist();
        id
    }

    pub fn move_folder(
        &mut self,
        id: FolderId,
        parent: Option<FolderId>,
        before: Option<FolderId>,
    ) -> bool {
        let ok = self.workspace.move_folder(id, parent, before);
        self.persist();
        ok
    }

    pub fn delete_folder(&mut self, id: FolderId) -> bool {
        let ok = self.workspace.delete_folder(id);
        self.persist();
        ok
    }

    /// Deletes a space; returns its tab ids so the caller can release their views.
    pub fn delete_space(&mut self, id: SpaceId) -> Option<Vec<TabId>> {
        let removed = self.workspace.delete_space(id)?;
        self.settings.routes.retain(|_, space| *space != id);
        let _ = self.repo.save_settings(&self.settings);
        self.persist();
        Some(removed)
    }

    /// Removes a site from the start page grid; a pinned tab for it stays in the sidebar.
    pub fn hide_favorite(&mut self, url: &Url) {
        let host = url
            .host_str()
            .unwrap_or_default()
            .trim_start_matches("www.")
            .to_owned();
        self.settings.hidden_favorites.insert(host);
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn set_space_icon(&mut self, id: SpaceId, icon: &str) -> bool {
        let ok = self.workspace.set_space_icon(id, icon);
        self.persist();
        ok
    }

    pub fn set_folder_icon(&mut self, id: FolderId, icon: &str) -> bool {
        let ok = self.workspace.set_folder_icon(id, icon);
        self.persist();
        ok
    }

    pub fn set_split(&mut self, id: TabId) -> bool {
        let ok = self.workspace.set_split(id);
        self.persist();
        ok
    }

    pub fn clear_split(&mut self) {
        self.workspace.clear_split();
        self.persist();
    }

    pub fn add_space(&mut self, name: &str, color: &str) -> SpaceId {
        let id = self.workspace.add_space(name, color);
        self.workspace.switch_space(id);
        self.persist();
        id
    }

    pub fn switch_space(&mut self, id: SpaceId) -> bool {
        let ok = self.workspace.switch_space(id);
        self.persist();
        ok
    }

    pub fn page_changed(&mut self, id: TabId, url: Option<Url>, title: Option<String>) {
        if let Some(url) = &url {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            self.history.visit(url, now);
        }
        self.workspace.update_page(id, url, title.clone());
        if let Some(title) = title {
            let tab_url = self
                .workspace
                .spaces()
                .iter()
                .flat_map(|s| s.tabs.iter())
                .find(|t| t.id == id)
                .map(|t| t.url.clone());
            if let Some(tab_url) = tab_url {
                self.history.set_title(&tab_url, &title);
            }
        }
        let _ = self.repo.save_history(&self.history);
        self.persist();
    }

    fn persist(&self) {
        // Best effort: a failed save must not interrupt browsing.
        let _ = self.repo.save(&self.workspace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Memory(
        Arc<Mutex<Option<Workspace>>>,
        Arc<Mutex<Option<Settings>>>,
        Arc<Mutex<Option<History>>>,
    );

    impl WorkspaceRepository for Memory {
        fn load(&self) -> Result<Option<Workspace>, StorageError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, w: &Workspace) -> Result<(), StorageError> {
            *self.0.lock().unwrap() = Some(w.clone());
            Ok(())
        }
        fn load_settings(&self) -> Result<Option<Settings>, StorageError> {
            Ok(self.1.lock().unwrap().clone())
        }
        fn save_settings(&self, s: &Settings) -> Result<(), StorageError> {
            *self.1.lock().unwrap() = Some(s.clone());
            Ok(())
        }
        fn load_history(&self) -> Result<Option<History>, StorageError> {
            Ok(self.2.lock().unwrap().clone())
        }
        fn save_history(&self, h: &History) -> Result<(), StorageError> {
            *self.2.lock().unwrap() = Some(h.clone());
            Ok(())
        }
    }

    #[test]
    fn opening_an_open_address_reuses_its_tab() {
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        let a = svc.open_tab("https://a.example/").unwrap();
        svc.open_tab("https://b.example/");
        assert_eq!(svc.open_tab("https://a.example/"), Some(a));
        assert_eq!(svc.workspace().active_tab().unwrap().id, a);
        assert_eq!(svc.workspace().active_space().tabs.len(), 2);
    }

    #[test]
    fn visits_feed_recent_activity_and_can_be_cleared() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        let id = svc.open_tab("example.com").unwrap();
        svc.page_changed(
            id,
            Some(Url::parse("https://example.com/").unwrap()),
            Some("Example".into()),
        );
        let svc2 = BrowserService::start(repo, PrivacyPolicy::default());
        assert_eq!(svc2.recent_activity(5)[0].title, "Example");
        let mut svc2 = svc2;
        svc2.clear_history();
        assert!(svc2.recent_activity(5).is_empty());
    }

    #[test]
    fn favorites_put_pinned_first_without_duplicate_hosts() {
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        let id = svc.open_tab("https://github.com/rust-lang").unwrap();
        svc.toggle_pin(id);
        let favs = svc.favorites();
        assert_eq!(favs[0].url.host_str(), Some("github.com"));
        assert_eq!(
            favs.iter()
                .filter(|f| f.url.host_str() == Some("github.com"))
                .count(),
            1
        );
    }

    #[test]
    fn search_engine_defaults_to_duckduckgo_and_persists() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert_eq!(svc.settings().search_engine, SearchEngine::DuckDuckGo);
        svc.set_search_engine(SearchEngine::Brave);
        let svc2 = BrowserService::start(repo, PrivacyPolicy::default());
        assert_eq!(svc2.settings().search_engine, SearchEngine::Brave);
        match svc2.resolve("hello") {
            NavigationOutcome::Go(u) => assert_eq!(u.host_str(), Some("search.brave.com")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn opened_tabs_are_persisted_and_restored() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        svc.open_tab("example.com");
        let svc2 = BrowserService::start(repo, PrivacyPolicy::default());
        assert_eq!(svc2.workspace().active_space().tabs.len(), 1);
    }

    #[test]
    fn routed_sites_open_in_their_space_and_boosts_persist() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        let work = svc.add_space("Work", "#2fd4a0");
        svc.add_space("Perso", "#ff5c8a");
        svc.set_route("www.github.com", Some(work));
        svc.open_tab("https://github.com/rust-lang");
        assert_eq!(svc.workspace().active_space().id, work);
        svc.set_boost("github.com", "body{zoom:1.1}");
        let svc2 = BrowserService::start(repo, PrivacyPolicy::default());
        assert_eq!(svc2.boost_for("www.github.com"), Some("body{zoom:1.1}"));
        assert_eq!(svc2.route_for("github.com"), Some(work));
    }

    #[test]
    fn deleting_a_space_drops_its_routes_and_returns_its_tabs() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        let work = svc.add_space("Work", "#2fd4a0");
        svc.set_route("github.com", Some(work));
        let tab = svc.open_tab("https://github.com/x").unwrap();
        assert_eq!(svc.delete_space(work), Some(vec![tab]));
        assert_eq!(svc.route_for("github.com"), None);
        let svc2 = BrowserService::start(repo, PrivacyPolicy::default());
        assert_eq!(svc2.workspace().spaces().len(), 1);
        let last = svc.workspace().active_space().id;
        assert_eq!(svc.delete_space(last), None);
    }

    #[test]
    fn hidden_default_favorites_stay_hidden() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        let url = svc.favorites()[0].url.clone();
        svc.hide_favorite(&url);
        let svc2 = BrowserService::start(repo, PrivacyPolicy::default());
        assert!(svc2
            .favorites()
            .iter()
            .all(|f| f.url.host_str() != url.host_str()));
    }

    #[test]
    fn a_pinned_tab_can_be_removed_from_the_favorites_grid() {
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        let id = svc.open_tab("https://github.com/rust-lang").unwrap();
        svc.toggle_pin(id);
        let url = svc.favorites()[0].url.clone();
        svc.hide_favorite(&url);
        assert!(svc
            .favorites()
            .iter()
            .all(|f| f.url.host_str() != Some("github.com")));
        assert!(svc.workspace().active_space().tabs[0].pinned);
    }

    #[test]
    fn release_notes_are_shown_once_after_an_update() {
        let repo = Memory::default();
        let mut first = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert!(
            first.take_release_notes("0.3.0").is_empty(),
            "fresh install"
        );

        let mut updated = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert_eq!(updated.take_release_notes("0.4.0")[0].0, "0.4.0");

        let mut restarted = BrowserService::start(repo, PrivacyPolicy::default());
        assert!(restarted.take_release_notes("0.4.0").is_empty());
    }

    #[test]
    fn extensions_can_be_switched_off_and_the_choice_persists() {
        use blackhole_domain::ContentScript;
        let ext = Extension {
            id: "dark".into(),
            name: "Dark".into(),
            version: "1".into(),
            description: String::new(),
            scripts: vec![ContentScript {
                matches: vec!["<all_urls>".into()],
                css: vec!["body{x:1}".into()],
                js: vec![],
            }],
        };
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        svc.set_extensions(vec![ext.clone()]);
        let page = Url::parse("https://example.com/").unwrap();
        assert!(svc.injection_for(&page).css.contains("x:1"));
        svc.set_extension_enabled("dark", false);
        svc.set_extension_enabled("unknown", false);
        assert!(svc.injection_for(&page).is_empty());
        let mut svc2 = BrowserService::start(repo, PrivacyPolicy::default());
        svc2.set_extensions(vec![ext]);
        assert!(!svc2.extension_enabled("dark"));
        assert!(svc2.extension_enabled("unknown"));
    }

    #[test]
    fn icons_and_split_are_persisted_by_the_service() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        let a = svc.open_tab("https://a.example/").unwrap();
        let b = svc.open_tab("https://b.example/").unwrap();
        let space = svc.workspace().active_space().id;
        svc.set_space_icon(space, "🚀");
        assert!(svc.set_split(a));
        assert_eq!(svc.workspace().split_tab(), Some(a));
        svc.activate_tab(a);
        assert_eq!(svc.workspace().active_tab().map(|t| t.id), Some(a));
        assert_eq!(svc.workspace().split_tab(), Some(b));
        let svc2 = BrowserService::start(repo, PrivacyPolicy::default());
        assert_eq!(svc2.workspace().active_space().icon, "🚀");
        assert_eq!(svc2.workspace().split_tab(), Some(b));
    }

    #[test]
    fn blocked_navigation_opens_nothing() {
        let policy = PrivacyPolicy::new(["tracker.test".to_owned()]);
        let mut svc = BrowserService::start(Memory::default(), policy);
        assert!(svc.open_tab("https://tracker.test").is_none());
    }
}

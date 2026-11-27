//! Use cases and ports. Depends only on the domain.

use blackhole_domain::keywords::{self, Refused};
use blackhole_domain::permissions::Permission;
use blackhole_domain::{
    site_key, Extension, FolderId, History, HistoryEntry, Injection,
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

/// A link brought from another browser or a file, to be filed as a tab of its space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedLink {
    /// Its space; `None` (links saved before each space had its own) means the first space.
    pub space: Option<SpaceId>,
    /// Folder path, `/`-separated (empty: the "Imported" folder).
    pub folder: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, thiserror::Error)]
#[error("storage error: {0}")]
pub struct StorageError(pub String);

/// Port: persistence of the user's workspace and settings (implemented in infrastructure).
pub trait WorkspaceRepository: Send + Sync {
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
    /// Page events only mark the session stale; `flush` writes it (each event used to rewrite
    /// the whole history and workspace on the UI thread).
    unsaved_pages: bool,
    /// How the next page of a tab was asked for (typed, chosen, quick access): its visit weighs
    /// that much in the command bar's ranking. A tab with none follows a link.
    expected: std::collections::HashMap<TabId, blackhole_domain::VisitKind>,
    /// The last page each tab arrived on, and when (seconds): a page left within `BOUNCE_SECS`
    /// without the user asking was a step of a redirect.
    arrived: std::collections::HashMap<TabId, (Url, u64)>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A page left this quickly, without the user asking for the next one, was a step of a redirect
/// made by the page (its visit no longer counts for the ranking).
const BOUNCE_SECS: u64 = 3;

impl<R: WorkspaceRepository> Drop for BrowserService<R> {
    fn drop(&mut self) {
        self.flush();
    }
}

impl<R: WorkspaceRepository> BrowserService<R> {
    pub fn start(repo: R, policy: PrivacyPolicy) -> Self {
        // A corrupt or missing session must never prevent the browser from starting.
        // The three files are independent: they are read and parsed in parallel.
        let (workspace, stored, history) = std::thread::scope(|scope| {
            let workspace = scope.spawn(|| repo.load().ok().flatten());
            let stored = scope.spawn(|| repo.load_settings().ok().flatten());
            let history = scope.spawn(|| repo.load_history().ok().flatten());
            (
                workspace.join().ok().flatten().unwrap_or_default(),
                stored.join().ok().flatten(),
                history.join().ok().flatten().unwrap_or_default(),
            )
        });
        let fresh = stored.is_none();
        let settings = stored.unwrap_or_default();
        policy.set_blocking(settings.tracker_blocking);
        policy.set_ad_blocking(settings.ad_blocking);
        policy.set_shields(settings.shields);
        Self {
            workspace,
            settings,
            history,
            policy,
            extensions: Vec::new(),
            repo,
            fresh,
            unsaved_pages: false,
            expected: std::collections::HashMap::new(),
            arrived: std::collections::HashMap::new(),
        }
    }

    /// No settings existed when this service started: the first launch of the profile (the
    /// welcome tour is offered then, `domain::onboarding`).
    pub fn first_launch(&self) -> bool {
        self.fresh
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
    /// The start page of `space` (its own, or the settings' one as before 0.8.33).
    pub fn home_of(&self, space: SpaceId) -> blackhole_domain::SpaceHome {
        self.workspace
            .spaces()
            .iter()
            .find(|s| s.id == space)
            .and_then(|s| s.home.as_deref().cloned())
            .unwrap_or_else(|| blackhole_domain::SpaceHome::of_settings(&self.settings))
    }

    /// The start page of the space shown.
    pub fn home(&self) -> blackhole_domain::SpaceHome {
        self.home_of(self.workspace.active_space().id)
    }

    /// Changes the start page of the space shown (it gets its own from then on).
    fn edit_home(&mut self, change: impl FnOnce(&mut blackhole_domain::SpaceHome) -> bool) -> bool {
        let id = self.workspace.active_space().id;
        let mut home = self.home_of(id);
        if !change(&mut home) {
            return false;
        }
        let changed = self.workspace.set_space_home(id, home);
        if changed {
            self.persist();
        }
        changed
    }

    /// The favorites of the space shown: its quick access, then the built-in sites, without
    /// the ones removed from its start page, in the order dragged there.
    pub fn favorites(&self) -> Vec<Favorite> {
        let home = self.home();
        let pinned = self
            .workspace
            .active_space()
            .tabs
            .iter()
            .filter(|t| t.pinned)
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
            if !seen.contains(&host) && !home.hidden_favorites.contains(&host) {
                seen.push(host);
                out.push(fav);
            }
        }
        home.sort_favorites(&mut out, |f| blackhole_domain::site_key(f.url.host_str().unwrap_or_default()));
        out.truncate(MAX_FAVORITES);
        out
    }

    /// Order of the favorites of the space shown, dragged on its start page (sites).
    pub fn set_favorite_order(&mut self, sites: &[String]) -> bool {
        self.edit_home(|home| home.set_favorite_order(sites))
    }

    pub fn recent_activity(&self, n: usize) -> &[HistoryEntry] {
        self.history.recent(n)
    }

    /// Forgets one page of the history (Library → History).
    pub fn forget_history(&mut self, url: &Url) {
        // Remembered as forgotten even when absent here: a paired device must drop it too.
        self.history.forget(url, now_secs());
        let _ = self.repo.save_history(&self.history);
    }

    pub fn clear_history(&mut self) {
        self.history.clear_at(now_secs());
        let _ = self.repo.save_history(&self.history);
    }

    /// Forgets the pages visited since a date; returns those that are gone.
    pub fn clear_history_since(&mut self, since: u64) -> Vec<Url> {
        let gone = self.history.forget_since(since, now_secs());
        let _ = self.repo.save_history(&self.history);
        gone
    }

    /// What a site may use: allowed, blocked, or asked again (`None`).
    pub fn set_site_permission(&mut self, origin: &str, permission: Permission, allowed: Option<bool>) {
        if self.settings.site_permissions.set(origin, permission, allowed) {
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    /// A site (or every site) asks again for everything.
    pub fn forget_site_permissions(&mut self, origin: Option<&str>) {
        match origin {
            Some(origin) => {
                self.settings.site_permissions.forget_site(origin);
            }
            None => self.settings.site_permissions.clear(),
        }
        let _ = self.repo.save_settings(&self.settings);
    }

    /// A private window keeps only what the profile blocks (see `SitePermissions::for_private`).
    pub fn seal_permissions(&mut self) {
        self.settings.site_permissions = self.settings.site_permissions.for_private();
    }

    /// Adds a keyword engine, or replaces the one with its keyword.
    pub fn add_keyword_engine(&mut self, keyword: &str, name: &str, url: &str) -> Result<(), Refused> {
        let engine = keywords::clean(keyword, name, url)?;
        keywords::add(&mut self.settings.keyword_engines, engine)?;
        let _ = self.repo.save_settings(&self.settings);
        Ok(())
    }

    /// Pulsars shown and read, or not (Settings > General).
    pub fn set_pulsars(&mut self, on: bool) {
        self.settings.pulsars = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn set_pulsar_every(&mut self, minutes: u32) {
        if blackhole_domain::pulsar::EVERY_CHOICES.contains(&minutes) {
            self.settings.pulsar_every = minutes;
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    pub fn set_pulsar_lines(&mut self, lines: u32) {
        if blackhole_domain::pulsar::LINES_CHOICES.contains(&lines) {
            self.settings.pulsar_lines = lines;
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    pub fn set_pulsar_notices(&mut self, on: bool) {
        self.settings.pulsar_notices = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn remove_keyword_engine(&mut self, keyword: &str) {
        self.settings.keyword_engines.retain(|e| e.keyword != keyword);
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn reset_keyword_engines(&mut self) {
        self.settings.keyword_engines = keywords::defaults();
        let _ = self.repo.save_settings(&self.settings);
    }

    /// What changed in the history after `since` (this device's clock), for a paired device.
    pub fn history_changes_since(&self, since: u64, max: usize) -> blackhole_domain::SharedHistory {
        self.history.changes_since(since, max)
    }

    /// Merges a batch from a paired device (see `History::merge`). Written by the next `flush`,
    /// like the visits (not on the UI thread at once).
    pub fn merge_history(&mut self, other: &blackhole_domain::SharedHistory) -> bool {
        let changed = self.history.merge(other, now_secs());
        if changed {
            self.unsaved_pages = true;
        }
        changed
    }

    /// Whether paired devices offer each other their extensions (Settings > Devices).
    pub fn set_extension_sync(&mut self, on: bool) {
        self.settings.extension_sync = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    /// Whether paired devices exchange the history (Settings > Devices).
    pub fn set_history_sync(&mut self, on: bool) {
        self.settings.history_sync = on;
        let _ = self.repo.save_settings(&self.settings);
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

    pub fn set_private_extension_settings(&mut self, on: bool) {
        self.settings.private_extension_settings = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    /// Switches the tracker blocklist, for existing and future views alike.
    pub fn set_tracker_blocking(&mut self, on: bool) {
        self.settings.tracker_blocking = on;
        self.policy.set_blocking(on);
        let _ = self.repo.save_settings(&self.settings);
    }

    /// Learning new trackers while browsing on or off (what was learned stays blocked).
    /// Encrypted DNS on or off (applied by the shell: at the next launch on Windows).
    pub fn set_encrypted_dns(&mut self, on: bool) {
        self.settings.encrypted_dns = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    /// The resolver of encrypted DNS: a known resolver's id or an `https://` address; anything
    /// else is refused (`false`).
    pub fn set_dns_resolver(&mut self, choice: &str) -> bool {
        if blackhole_domain::dns::doh_template(choice).is_none() {
            return false;
        }
        self.settings.dns_resolver = choice.trim().to_owned();
        let _ = self.repo.save_settings(&self.settings);
        true
    }

    pub fn set_tracker_learning(&mut self, on: bool) {
        self.settings.tracker_learning = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    /// Size of the quick access tiles (`small`, `medium`, `large`); false for another value.
    pub fn set_quick_access_size(&mut self, size: &str) -> bool {
        if !blackhole_domain::is_quick_access_size(size) {
            return false;
        }
        self.settings.quick_access_size = size.to_owned();
        let _ = self.repo.save_settings(&self.settings);
        true
    }

    /// Shows or hides a section of the start page; false for an unknown section.
    pub fn set_home_section(&mut self, name: &str, shown: bool) -> bool {
        if !blackhole_domain::is_home_section(name) {
            return false;
        }
        self.edit_home(|home| {
            if shown {
                home.hidden.remove(name)
            } else {
                home.hidden.insert(name.to_owned())
            }
        });
        true
    }

    /// Order of the start page sections (dragged on the page); unknown names are dropped and a
    /// missing section keeps its default place (`domain::home_order`).
    pub fn set_home_order(&mut self, order: &[String]) {
        let order: Vec<String> = blackhole_domain::home_order(order).into_iter().map(str::to_owned).collect();
        let default = order.iter().map(String::as_str).eq(blackhole_domain::HOME_SECTIONS);
        let order = if default { Vec::new() } else { order };
        self.edit_home(|home| std::mem::replace(&mut home.order, order.clone()) != order);
    }

    /// Calendar options by name: `reminders` (a toast when a meeting starts) or `card` (the next
    /// meeting in the sidebar). False for an unknown name.
    pub fn set_meeting_option(&mut self, name: &str, on: bool) -> bool {
        match name {
            "reminders" => self.settings.meeting_reminders = on,
            "card" => self.settings.meeting_card = on,
            "notes" => self.settings.meeting_notes = on,
            _ => return false,
        }
        let _ = self.repo.save_settings(&self.settings);
        true
    }

    /// Minutes between two readings of the calendars; false (unchanged) for another value.
    pub fn set_calendar_sync(&mut self, minutes: u32) -> bool {
        if !Settings::CALENDAR_SYNC_CHOICES.contains(&minutes) {
            return false;
        }
        self.settings.calendar_sync_minutes = minutes;
        let _ = self.repo.save_settings(&self.settings);
        true
    }

    /// Interface language: a supported code or `i18n::SYSTEM`; false (unchanged) otherwise.
    /// Remembers the page zoom of `host`'s site (percent, 100 forgets it).
    pub fn set_site_zoom(&mut self, host: &str, percent: u32) {
        if self.settings.set_zoom(host, percent) {
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    pub fn set_language(&mut self, code: &str) -> bool {
        if !blackhole_domain::i18n::is_choice(code) {
            return false;
        }
        self.settings.language = code.to_owned();
        let _ = self.repo.save_settings(&self.settings);
        true
    }

    /// Recall on or off (see `domain::recall`).
    pub fn set_recall(&mut self, on: bool) {
        self.settings.recall = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    /// Adds a site to (or takes it off) the list Recall never remembers. `site` may be a host
    /// or an address; the site as listed is returned, `None` when it is not a site.
    pub fn set_recall_excluded(&mut self, site: &str, excluded: bool) -> Option<String> {
        let site = blackhole_domain::recall::site_of(site)?;
        if excluded {
            self.settings.recall_exclude.insert(site.clone());
        } else {
            self.settings.recall_exclude.remove(&site);
        }
        let _ = self.repo.save_settings(&self.settings);
        Some(site)
    }

    /// Minutes before a wallet with a master password locks again; ignored unless one of
    /// `Settings::LOCK_CHOICES`.
    /// The built-in password manager on or off (saved passwords are kept either way).
    pub fn set_password_manager(&mut self, on: bool) {
        self.settings.password_manager = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    /// The email alias service connected (empty: none).
    pub fn set_alias_provider(&mut self, id: &str) {
        self.settings.alias_provider = id.to_owned();
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn set_alias_suggest(&mut self, on: bool) {
        self.settings.alias_suggest = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn set_alias_share_site(&mut self, on: bool) {
        self.settings.alias_share_site = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn set_password_lock(&mut self, minutes: u32) {
        if Settings::LOCK_CHOICES.contains(&minutes) {
            self.settings.password_lock_minutes = minutes;
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    /// Switches one of the Privacy shields by name: `ads` (the ad server list), `hide_ads`,
    /// `video_ads` or `cookie_banners` (what pages clean up), `clean_links` (tracking parameters
    /// and redirect pages). False for an unknown name.
    pub fn set_shield(&mut self, name: &str, on: bool) -> bool {
        let shields = &mut self.settings.shields;
        match name {
            "ads" => {
                self.settings.ad_blocking = on;
                self.policy.set_ad_blocking(on);
            }
            "hide_ads" => shields.hide_ads = on,
            "video_ads" => shields.video_ads = on,
            "cookie_banners" => shields.cookie_banners = on,
            "clean_links" => shields.clean_links = on,
            "fingerprinting" => shields.fingerprinting = on,
            _ => return false,
        }
        self.policy.set_shields(self.settings.shields);
        let _ = self.repo.save_settings(&self.settings);
        true
    }

    /// Folds or unfolds a sidebar section (`Settings::SECTIONS`), remembered between launches.
    pub fn set_section_collapsed(&mut self, section: &str, collapsed: bool) {
        if !Settings::SECTIONS.contains(&section) {
            return;
        }
        let changed = if collapsed {
            self.settings.collapsed_sections.insert(section.to_owned())
        } else {
            self.settings.collapsed_sections.remove(section)
        };
        if changed {
            let _ = self.repo.save_settings(&self.settings);
        }
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

    /// The style of the interface: 3D (`true`) or flat.
    pub fn set_depth(&mut self, on: bool) {
        self.settings.depth = on;
        let _ = self.repo.save_settings(&self.settings);
    }

    pub fn set_tab_sleep_minutes(&mut self, minutes: u32) {
        if Settings::SLEEP_CHOICES.contains(&minutes) {
            self.settings.tab_sleep_minutes = minutes;
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    pub fn set_max_live_tabs(&mut self, tabs: u32) {
        if Settings::LIVE_TABS_CHOICES.contains(&tabs) {
            self.settings.max_live_tabs = tabs;
            let _ = self.repo.save_settings(&self.settings);
        }
    }

    pub fn set_spare_views(&mut self, count: u32) {
        if Settings::SPARE_CHOICES.contains(&count) {
            self.settings.spare_views = count;
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
        self.evaluate(&self.settings.resolve_input(input))
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

    /// Opens a black hole tab on `input` in the active space (no Air Traffic, no reuse of an
    /// open tab: its session is its own).
    pub fn open_burner(&mut self, input: &str) -> Option<TabId> {
        match self.resolve(input) {
            NavigationOutcome::Go(url) => Some(self.workspace.open_burner(url)),
            NavigationOutcome::Blocked(_) => None,
        }
    }

    /// "Duplicate" (tab menu): another tab on the same page, never the open one reused.
    pub fn duplicate_tab(&mut self, id: TabId) -> Option<TabId> {
        let copy = self.workspace.duplicate_tab(id)?;
        self.persist();
        Some(copy)
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

    /// Files a tab in the Pulsar it was opened from.
    pub fn set_pulsar(&mut self, id: TabId, pulsar: u64) {
        if self.workspace.set_pulsar(id, pulsar) {
            self.persist();
        }
    }

    /// The Pulsars there are (id, space): the tabs of the others are loose again.
    pub fn keep_pulsars(&mut self, known: &[(u64, u64)]) {
        if self.workspace.keep_pulsars(known) {
            self.persist();
        }
    }

    /// ⌘W on a pinned or folder tab: it keeps its place in the sidebar, another tab is shown.
    pub fn release_tab(&mut self, id: TabId) -> bool {
        let ok = self.workspace.release_tab(id);
        if ok {
            self.persist();
        }
        ok
    }

    /// The page of a quick access or folder tab went to sleep: it wakes up at its home.
    pub fn go_home(&mut self, id: TabId) -> bool {
        let ok = self.workspace.go_home(id);
        if ok {
            self.persist();
        }
        ok
    }

    /// The page shown by a quick access or folder tab becomes its home.
    pub fn set_home(&mut self, id: TabId) -> bool {
        let ok = self.workspace.set_home(id);
        if ok {
            self.persist();
        }
        ok
    }

    /// Anchors a tab (its page stays alive) or lets it sleep again.
    pub fn toggle_anchor(&mut self, id: TabId) -> bool {
        let ok = self.workspace.toggle_anchor(id);
        if ok {
            self.persist();
        }
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

    /// Files imported links as tabs, space by space; only http(s) addresses are kept.
    /// Returns how many tabs were added.
    pub fn file_links(&mut self, links: impl IntoIterator<Item = ImportedLink>) -> usize {
        let first = self.workspace.spaces().first().map_or(0, |s| s.id.0);
        let mut by_space: std::collections::BTreeMap<u64, Vec<(String, String, Url)>> =
            std::collections::BTreeMap::new();
        for link in links {
            let Ok(url) = Url::parse(&link.url) else {
                continue;
            };
            if matches!(url.scheme(), "http" | "https") {
                let space = link.space.map_or(first, |s| s.0);
                by_space.entry(space).or_default().push((link.folder, link.title, url));
            }
        }
        by_space
            .into_iter()
            .map(|(space, links)| self.import_tabs(SpaceId(space), links))
            .sum()
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

    pub fn rename_tab(&mut self, id: TabId, name: &str) -> bool {
        let ok = self.workspace.rename_tab(id, name);
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

    /// Files the loose tabs of each group into a folder named after its site.
    pub fn tidy(&mut self, groups: &[blackhole_domain::TidyGroup]) -> blackhole_domain::Tidied {
        let done = self.workspace.tidy(groups);
        self.persist();
        done
    }

    pub fn untidy(&mut self, done: &blackhole_domain::Tidied) {
        self.workspace.untidy(done);
        self.persist();
    }

    pub fn set_space_color(&mut self, id: SpaceId, color: &str) -> bool {
        let ok = self.workspace.set_space_color(id, color);
        self.persist();
        ok
    }

    pub fn move_tab_to_space(&mut self, id: TabId, to: SpaceId) -> bool {
        let ok = self.workspace.move_tab_to_space(id, to);
        self.persist();
        ok
    }

    /// Puts back a space or folder deleted a moment ago (see `Workspace::snapshot_space`).
    pub fn undo_removal(&mut self, removed: blackhole_domain::Removed) -> bool {
        let ok = self.workspace.undo_removal(removed);
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

    /// Removes a site from the start page grid of the space shown; a pinned tab for it stays
    /// in the sidebar.
    pub fn hide_favorite(&mut self, url: &Url) {
        let host = blackhole_domain::site_key(url.host_str().unwrap_or_default());
        self.edit_home(|home| home.hidden_favorites.insert(host));
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

    /// A tab dropped on a half of the page (see `Workspace::split_drop`).
    pub fn drop_into_split(&mut self, id: TabId, left: bool) -> bool {
        let ok = self.workspace.drop_into_split(id, left);
        if ok {
            self.persist();
        }
        ok
    }

    pub fn add_space(&mut self, name: &str, color: &str) -> SpaceId {
        let id = self.workspace.add_space(name, color);
        self.workspace.switch_space(id);
        self.persist();
        id
    }

    /// What a merge with a paired device decided (see `blackhole_domain::sync`); returns the
    /// tabs removed, whose views must go.
    pub fn apply_sync(&mut self, plan: &[blackhole_domain::sync::Action]) -> Vec<TabId> {
        if plan.is_empty() {
            return Vec::new();
        }
        let removed = self.workspace.apply_sync(plan);
        self.persist();
        removed
    }

    pub fn switch_space(&mut self, id: SpaceId) -> bool {
        let ok = self.workspace.switch_space(id);
        self.persist();
        ok
    }

    /// The next page of `tab` was asked for this way (an address typed, a result chosen, quick
    /// access): its visit counts as such.
    pub fn expect_visit(&mut self, tab: TabId, kind: blackhole_domain::VisitKind) {
        self.expected.insert(tab, kind);
    }

    pub fn page_changed(&mut self, id: TabId, url: Option<Url>, title: Option<String>) {
        // A black hole tab leaves no trace: its pages stay out of the history.
        if self.workspace.is_burner(id) {
            self.workspace.update_page(id, url, title);
            return;
        }
        if let Some(url) = &url {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let mut kind = self.expected.remove(&id).unwrap_or_default();
            // Left at once without being asked: the page before was a step of a redirect, and
            // this one is what the user was after (it takes over how that one was reached).
            if let Some((before, at)) = self.arrived.get(&id) {
                let moved = blackhole_domain::suggest::key(before) != blackhole_domain::suggest::key(url);
                if moved && kind == blackhole_domain::VisitKind::Link && now.saturating_sub(*at) <= BOUNCE_SECS {
                    if let Some(was) = self.history.bounced(before) {
                        kind = was;
                    }
                }
            }
            let space = self
                .workspace
                .spaces()
                .iter()
                .find(|s| s.tabs.iter().any(|t| t.id == id))
                .map(|s| s.id.0);
            // The tab's title before this page: a page of the same site often keeps it, and the
            // engine then reports no title change (the entry would stay named after its host).
            let before = self
                .workspace
                .spaces()
                .iter()
                .flat_map(|s| s.tabs.iter())
                .find(|t| t.id == id)
                .filter(|t| t.url.host_str() == url.host_str() && !t.title.trim().is_empty())
                .map(|t| t.title.clone());
            self.history.visit_as(url, now, kind, space);
            if let Some(before) = before {
                self.history.name_if_bare(url, &before);
            }
            self.arrived.insert(id, (url.clone(), now));
            if self.arrived.len() > 256 {
                self.arrived.retain(|_, (_, at)| now.saturating_sub(*at) <= BOUNCE_SECS);
            }
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
        self.unsaved_pages = true;
    }

    /// Writes what page events changed since the last call.
    pub fn flush(&mut self) {
        if std::mem::take(&mut self.unsaved_pages) {
            let _ = self.repo.save_history(&self.history);
            self.persist();
        }
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
    fn imported_links_are_filed_in_their_space_and_only_web_addresses_kept() {
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        let first = svc.workspace().spaces()[0].id;
        let link = |space: Option<SpaceId>, url: &str| ImportedLink {
            space,
            folder: "Work/Docs".into(),
            title: url.into(),
            url: url.into(),
        };
        let added = svc.file_links([
            link(None, "https://a.example/"),
            link(Some(first), "http://b.example/"),
            link(None, "javascript:alert(1)"),
            link(None, "ftp://c.example/"),
            link(None, "not a url"),
            link(Some(SpaceId(999_999)), "https://elsewhere.example/"),
            link(None, "https://a.example/"),
        ]);
        assert_eq!(added, 2);
        let tabs = &svc.workspace().spaces()[0].tabs;
        assert!(tabs.iter().any(|t| t.url.as_str() == "https://a.example/"));
        assert!(tabs.iter().any(|t| t.url.as_str() == "http://b.example/"));
    }

    #[test]
    fn only_a_usable_resolver_is_kept() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert!(!svc.settings().encrypted_dns);
        svc.set_encrypted_dns(true);
        assert!(svc.set_dns_resolver("mullvad"));
        assert!(!svc.set_dns_resolver("http://plain.example/dns"));
        assert!(svc.set_shield("fingerprinting", false));
        let again = BrowserService::start(repo, PrivacyPolicy::default());
        assert!(again.settings().encrypted_dns);
        assert_eq!(again.settings().dns_resolver, "mullvad");
        assert!(!again.settings().shields.fingerprinting);
    }

    #[test]
    fn the_calendars_are_read_again_at_a_chosen_pace() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert_eq!(svc.settings().calendar_sync_minutes, Settings::DEFAULT_CALENDAR_SYNC);
        assert!(svc.set_calendar_sync(30));
        assert!(!svc.set_calendar_sync(7), "only one of the choices");
        let again = BrowserService::start(repo, PrivacyPolicy::default());
        assert_eq!(again.settings().calendar_sync_minutes, 30);
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
    fn a_released_pinned_tab_stays_and_a_page_can_be_forgotten() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        let a = svc.open_tab("https://a.example/").unwrap();
        let p = svc.open_tab("https://p.example/").unwrap();
        svc.toggle_pin(p);
        assert!(svc.release_tab(p));
        assert_eq!(svc.workspace().active_tab().unwrap().id, a);
        let again = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert_eq!(again.workspace().active_space().tabs.len(), 2);

        let url = Url::parse("https://a.example/").unwrap();
        svc.page_changed(a, Some(url.clone()), None);
        svc.flush();
        svc.forget_history(&url);
        let again = BrowserService::start(repo, PrivacyPolicy::default());
        assert!(again.recent_activity(5).is_empty());
    }

    #[test]
    fn shields_are_remembered_and_drive_the_policy() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert!(svc.set_shield("ads", false));
        assert!(svc.set_shield("cookie_banners", false));
        assert!(!svc.set_shield("bogus", false));
        let again = BrowserService::start(repo, PrivacyPolicy::default());
        assert!(!again.settings().ad_blocking);
        assert!(!again.policy().is_blocking_ads());
        let shields = again.policy().shields();
        assert!(shields.hide_ads && shields.video_ads && !shields.cookie_banners);
    }

    #[test]
    fn the_language_is_remembered_and_checked() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert_eq!(svc.settings().language, "system");
        assert!(svc.set_language("fr"));
        assert!(!svc.set_language("klingon"));
        let mut again = BrowserService::start(repo, PrivacyPolicy::default());
        assert_eq!(again.settings().language, "fr");
        assert!(again.set_language("system"));
        assert_eq!(again.settings().language, "system");
    }

    #[test]
    fn blocker_and_folded_sections_are_remembered() {
        let repo = Memory::default();
        let mut svc = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        svc.set_tracker_blocking(false);
        svc.set_section_collapsed("pinned", true);
        svc.set_section_collapsed("bogus", true);
        let again = BrowserService::start(repo, PrivacyPolicy::default());
        assert!(!again.settings().tracker_blocking);
        assert!(!again.policy().is_blocking());
        assert_eq!(again.settings().collapsed_sections.len(), 1);
        assert!(again.settings().collapsed_sections.contains("pinned"));
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
        svc.flush();
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
    fn each_space_has_its_own_start_page() {
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        let id = svc.open_tab("https://rust-lang.org/learn").unwrap();
        svc.toggle_pin(id);
        let first = svc.workspace().active_space().id;
        assert!(svc.set_home_section("recent", false));
        let other = svc.add_space("Work", "#336699");
        svc.switch_space(other);
        assert!(svc.favorites().iter().all(|f| f.url.host_str() != Some("rust-lang.org")), "only this space's quick access");
        assert!(!svc.home().hidden.contains("recent"), "the other space keeps its sections");
        let sites: Vec<String> = svc.favorites().iter().rev().map(|f| f.url.host_str().unwrap().to_owned()).collect();
        assert!(svc.set_favorite_order(&sites));
        let shown: Vec<String> = svc.favorites().iter().map(|f| f.url.host_str().unwrap().to_owned()).collect();
        assert_eq!(shown[0], sites[0], "dragged order");
        svc.switch_space(first);
        assert_eq!(svc.favorites()[0].url.host_str(), Some("rust-lang.org"));
        assert!(svc.home().hidden.contains("recent"));
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
        assert!(first.first_launch());

        let mut updated = BrowserService::start(repo.clone(), PrivacyPolicy::default());
        assert_eq!(updated.take_release_notes("0.4.0")[0].0, "0.4.0");

        let mut restarted = BrowserService::start(repo, PrivacyPolicy::default());
        assert!(restarted.take_release_notes("0.4.0").is_empty());
        assert!(!restarted.first_launch());
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
    fn recall_is_off_at_first_and_its_exclusions_are_sites() {
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        assert!(!svc.settings().recall);
        svc.set_recall(true);
        assert_eq!(svc.set_recall_excluded("https://www.Bank.test/login", true).as_deref(), Some("bank.test"));
        assert_eq!(svc.set_recall_excluded("not a site", true), None);
        assert!(svc.settings().recall && svc.settings().recall_exclude.contains("bank.test"));
        svc.set_recall_excluded("bank.test", false);
        assert!(svc.settings().recall_exclude.is_empty());
    }

    #[test]
    fn a_page_of_the_same_site_keeps_the_tab_title_until_its_own_arrives() {
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        let tab = svc.open_tab("https://shop.test/a").expect("tab");
        svc.page_changed(tab, Some(Url::parse("https://shop.test/a").unwrap()), Some("Shop".into()));
        svc.page_changed(tab, Some(Url::parse("https://shop.test/a?page=2").unwrap()), None);
        assert_eq!(svc.recent_activity(1)[0].title, "Shop", "no title event: the tab's title");
        svc.page_changed(tab, None, Some("Shop, page 2".into()));
        assert_eq!(svc.recent_activity(1)[0].title, "Shop, page 2");
        svc.page_changed(tab, Some(Url::parse("https://other.test/").unwrap()), None);
        assert_eq!(svc.recent_activity(1)[0].title, "other.test", "another site never takes it");
    }

    #[test]
    fn a_typed_page_left_at_once_hands_its_kind_to_the_next() {
        use blackhole_domain::VisitKind;
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        let tab = svc.open_tab("https://step.test/").expect("tab");
        svc.expect_visit(tab, VisitKind::Typed);
        svc.page_changed(tab, Some(Url::parse("https://step.test/").unwrap()), None);
        svc.page_changed(tab, Some(Url::parse("https://final.test/").unwrap()), None);
        let kind_of = |svc: &BrowserService<Memory>, host: &str| {
            svc.recent_activity(10).iter().find(|e| e.url.host_str() == Some(host)).map(|e| e.recent[0].kind)
        };
        assert_eq!(kind_of(&svc, "step.test"), Some(VisitKind::Redirect));
        assert_eq!(kind_of(&svc, "final.test"), Some(VisitKind::Typed));
        let space = svc.workspace().active_space().id.0;
        assert_eq!(svc.recent_activity(1)[0].spaces, vec![space]);
    }

    #[test]
    fn a_black_hole_tab_stays_out_of_the_history() {
        let mut svc = BrowserService::start(Memory::default(), PrivacyPolicy::default());
        let plain = svc.open_tab("https://a.test/").unwrap();
        let hole = svc.open_burner("https://b.test/").unwrap();
        assert!(svc.workspace().is_burner(hole));
        svc.page_changed(plain, Some(Url::parse("https://a.test/").unwrap()), None);
        svc.page_changed(hole, Some(Url::parse("https://b.test/secret").unwrap()), Some("Secret".into()));
        let seen: Vec<&str> = svc.recent_activity(10).iter().map(|e| e.url.as_str()).collect();
        assert_eq!(seen, ["https://a.test/"]);
        // The tab itself still follows its page.
        let tab = svc.workspace().active_space().tabs.iter().find(|t| t.id == hole).unwrap();
        assert_eq!((tab.url.as_str(), tab.title.as_str()), ("https://b.test/secret", "Secret"));
    }

    #[test]
    fn blocked_navigation_opens_nothing() {
        let policy = PrivacyPolicy::new(["tracker.test".to_owned()]);
        let mut svc = BrowserService::start(Memory::default(), policy);
        assert!(svc.open_tab("https://tracker.test").is_none());
    }
}

//! The mobile browser's state: the desktop's `BrowserService` (Arc-like tabs, spaces, quick
//! access, history, settings, Shields) plus what only the phone has (private tabs, live views,
//! error pages). Every input is a JSON message; every output a list of [`Effect`]s.

use crate::bridge::{self, Effect, MobileCommand, NativeEvent, PageEvent};
use crate::live::Live;
use blackhole_application::{BrowserService, NavigationOutcome};
use blackhole_domain::error_page::{self, classify, Bypasses, ErrorPage, Failure};
use blackhole_domain::{
    privacy_grade, resolve_input, BlockLog, PrivacyDecision, PrivacyPolicy, SearchEngine, SpaceId,
    TabId,
};
use blackhole_infrastructure::{default_privacy_policy, flush_writes, write_later, JsonWorkspaceRepository};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

/// The desktop's error page template, shared as is (it adapts to narrow screens).
const ERROR_HTML: &str = include_str!("../../app/ui/error.html");
/// The desktop's page clean-up script (Shields), shared as is.
const SHIELDS_JS: &str = include_str!("../../app/ui/shields.js");
/// Document-start bridge of the pages: `window.__bhipc` for our error page.
const PAGE_JS: &str = include_str!("../ui/page.js");

/// Private tab ids start here: never a workspace id, never saved.
const PRIVATE_BASE: u64 = 1 << 48;
/// Live WebViews kept at once (the shown one included).
pub const MAX_LIVE: usize = 4;
/// Closed tabs that "Undo" can bring back.
const MAX_CLOSED: usize = 10;
const THEMES: [&str; 3] = ["system", "dark", "light"];

/// Preferences of the phone only, next to the shared `settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MobilePrefs {
    pub theme: String,
    /// Looks for a new release once a day.
    pub auto_update: bool,
    /// Last update check (Unix seconds).
    pub update_checked: i64,
    /// Text size of pages, in percent.
    pub text_zoom: u32,
    /// Pull down at the top of a page to reload it.
    pub pull_refresh: bool,
}

impl Default for MobilePrefs {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            auto_update: true,
            update_checked: 0,
            text_zoom: 100,
            pull_refresh: true,
        }
    }
}

/// Text sizes offered in Settings.
pub(crate) const TEXT_ZOOMS: [u32; 5] = [85, 100, 115, 130, 150];

/// Request blocking, called from WebView network threads (`shouldInterceptRequest`).
#[derive(Debug)]
pub struct Blocker {
    policy: PrivacyPolicy,
    log: Mutex<BlockLog>,
}

impl Blocker {
    /// Whether to refuse the request `url` made by the page `page`; a refusal is counted for
    /// the page's privacy report. The page's own address is never refused here (the shell
    /// shows the "blocked" page instead).
    pub fn should_block(&self, url: &str, page: &str) -> bool {
        let Some(host) = Url::parse(url).ok().filter(|u| matches!(u.scheme(), "http" | "https")) else {
            return false;
        };
        let Some(host) = host.host_str().map(str::to_owned) else {
            return false;
        };
        if !self.policy.is_blocked_host(&host) {
            return false;
        }
        let site = Url::parse(page)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        if let Ok(mut log) = self.log.lock() {
            log.record(&host, &site);
        }
        true
    }

    fn is_ad(&self, host: &str) -> bool {
        Url::parse(&format!("https://{host}/"))
            .is_ok_and(|u| self.policy.evaluate(&u) == PrivacyDecision::Block("ad blocked"))
    }
}

pub(crate) struct PrivateTab {
    id: u64,
    url: Url,
    title: String,
}

pub(crate) struct Closed {
    url: Url,
    private: bool,
}

pub struct Core {
    pub(crate) svc: BrowserService<JsonWorkspaceRepository>,
    pub(crate) blocker: Arc<Blocker>,
    pub(crate) data: PathBuf,
    pub(crate) prefs: MobilePrefs,
    pub(crate) private: Vec<PrivateTab>,
    pub(crate) next_private: u64,
    /// The tab on screen; `None`: the start page.
    pub(crate) current: Option<u64>,
    pub(crate) live: Live,
    pub(crate) closed: Vec<Closed>,
    /// Tabs showing our error page, with the address that failed and its page.
    pub(crate) errors: HashMap<u64, (Url, ErrorPage)>,
    /// Plain-http addresses the core upgraded to https, per tab.
    pub(crate) upgraded: HashMap<u64, Url>,
    pub(crate) bypasses: Bypasses,
    /// Address and title the WebView reported last, per tab (shown in the address bar).
    pub(crate) pages: HashMap<u64, (Url, String)>,
    /// Paired devices and tab sync (`None` until the shell enables them).
    pub(crate) nearby: Option<crate::devices::Nearby>,
    pub(crate) update: crate::update::Updater,
    pub(crate) icons: crate::icons::Icons,
    pub(crate) permissions: crate::permissions::Permissions,
    /// Tabs showing the desktop site.
    pub(crate) desktop: HashSet<u64>,
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn host_of(url: &Url) -> String {
    url.host_str().unwrap_or_default().to_owned()
}

fn web_url(raw: &str) -> Option<Url> {
    Url::parse(raw)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

/// What text shared by another app opens: the first web address in it ("Look at this
/// https://…"), else a search for it (or the address it reads as).
pub(crate) fn shared_target(text: &str, engine: SearchEngine) -> Option<Url> {
    let found = text.split_whitespace().find_map(|word| {
        let word = word.trim_matches(|c: char| "\"'“”‘’«»()[]<>,;!".contains(c));
        let lower = word.to_ascii_lowercase();
        (lower.starts_with("http://") || lower.starts_with("https://")).then(|| web_url(word)).flatten()
    });
    if found.is_some() {
        return found;
    }
    let clean = blackhole_domain::selection::clean_selection(text)?;
    Some(blackhole_domain::selection::selection_action(&clean, engine).url().clone())
}

/// The shell's error page for `config` (`<` escaped so no text of the site closes the script).
/// Its texts come from the catalog (`error.*`), in English like the rest of the mobile interface.
pub fn error_page(config: &Value) -> String {
    let mut config = config.clone();
    let strings = blackhole_domain::i18n::catalog("en").with_prefix("error.");
    config["i18n"] = serde_json::json!({"lang": "en", "s": strings});
    let literal = config.to_string().replace('<', "\\u003c");
    ERROR_HTML.replace("__BH_ERROR__", &literal)
}

/// Schemes another app may open, after the user confirmed.
fn is_external(url: &str) -> bool {
    let scheme = url.split(':').next().unwrap_or_default().to_ascii_lowercase();
    matches!(scheme.as_str(), "intent" | "market" | "mailto" | "tel" | "sms" | "geo")
}

impl Core {
    /// Loads the profile in `data` (the app's private files directory).
    pub fn start(data: &Path) -> Self {
        Self::with_policy(data, default_privacy_policy())
    }

    pub fn with_policy(data: &Path, policy: PrivacyPolicy) -> Self {
        let _ = std::fs::create_dir_all(data);
        let svc = BrowserService::start(JsonWorkspaceRepository::new(data.to_path_buf()), policy.clone());
        let prefs = std::fs::read_to_string(data.join("mobile.json"))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self {
            svc,
            blocker: Arc::new(Blocker {
                policy,
                log: Mutex::new(BlockLog::default()),
            }),
            data: data.to_path_buf(),
            prefs,
            private: Vec::new(),
            next_private: PRIVATE_BASE,
            current: None,
            live: Live::default(),
            closed: Vec::new(),
            errors: HashMap::new(),
            upgraded: HashMap::new(),
            bypasses: Bypasses::default(),
            pages: HashMap::new(),
            nearby: None,
            update: crate::update::Updater::default(),
            icons: crate::icons::Icons::load(data),
            permissions: crate::permissions::Permissions::default(),
            desktop: HashSet::new(),
        }
    }

    pub(crate) fn save_prefs(&self) {
        if let Ok(raw) = serde_json::to_string(&self.prefs) {
            write_later(self.data.join("mobile.json"), raw);
        }
    }

    fn web_prefs(&self) -> Effect {
        Effect::WebPrefs {
            text_zoom: self.prefs.text_zoom,
            pull_refresh: self.prefs.pull_refresh,
        }
    }

    fn icons_message(&self) -> Effect {
        Effect::Ui {
            msg: json!({"type": "icons", "icons": self.icons.all()}),
        }
    }

    pub fn blocker(&self) -> Arc<Blocker> {
        Arc::clone(&self.blocker)
    }

    /// The first screen: the start page, like a new day in Arc (tabs wait in the drawer).
    ///
    /// Also called when the system recreates the activity while the process (and this core)
    /// lives on: the new activity has no view, so none is alive any more and the tab that was on
    /// screen is opened again (otherwise its page stays black).
    pub fn boot(&mut self) -> Vec<Effect> {
        self.live = Live::default();
        self.errors.clear();
        let mut out = vec![self.web_prefs()];
        out.extend(match self.current.filter(|&tab| self.exists(tab)) {
            Some(tab) => self.show(tab, None),
            None => self.home(),
        });
        out.push(self.state());
        out
    }

    pub fn command_json(&mut self, json: &str) -> String {
        let effects = serde_json::from_str(json).map_or_else(|_| Vec::new(), |c| self.command(c));
        bridge::to_json(&effects)
    }

    pub fn event_json(&mut self, json: &str) -> String {
        let effects = serde_json::from_str(json).map_or_else(|_| Vec::new(), |e| self.event(e));
        bridge::to_json(&effects)
    }

    pub fn page_event_json(&mut self, tab: u64, json: &str) -> String {
        let effects = serde_json::from_str(json).map_or_else(|_| Vec::new(), |e| self.page_event(tab, e));
        bridge::to_json(&effects)
    }

    pub(crate) fn is_private(tab: u64) -> bool {
        tab >= PRIVATE_BASE
    }

    pub(crate) fn tab_url(&self, tab: u64) -> Option<Url> {
        if Self::is_private(tab) {
            return self.private.iter().find(|t| t.id == tab).map(|t| t.url.clone());
        }
        self.svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .find(|t| t.id == TabId(tab))
            .map(|t| t.url.clone())
    }

    pub(crate) fn exists(&self, tab: u64) -> bool {
        self.tab_url(tab).is_some()
    }

    /// WebView profile of a tab: one per space (the first is the default profile).
    fn profile_of(&self, tab: u64) -> String {
        if Self::is_private(tab) {
            return "private".into();
        }
        let space = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .find(|s| s.tabs.iter().any(|t| t.id == TabId(tab)))
            .map_or(1, |s| s.id.0);
        if space == 1 {
            "Default".into()
        } else {
            format!("space-{space}")
        }
    }

    fn page_script(&self) -> String {
        let shields = self.svc.policy().shields();
        let config = json!({
            "hide_ads": shields.hide_ads,
            "video_ads": shields.video_ads,
            "cookie_banners": shields.cookie_banners,
        });
        format!("{PAGE_JS}\n{}", SHIELDS_JS.replace("__SHIELDS__", &config.to_string()))
    }

    /// Puts `tab` on screen, creating its view (at `load`, or its saved address) when needed.
    pub(crate) fn show(&mut self, tab: u64, load: Option<Url>) -> Vec<Effect> {
        let mut out = Vec::new();
        let alive = self.live.contains(tab);
        let url = load.clone().or_else(|| self.tab_url(tab));
        if let Some(url) = url {
            if !alive {
                out.push(Effect::Open {
                    tab,
                    url: url.to_string(),
                    profile: self.profile_of(tab),
                    private: Self::is_private(tab),
                    script: self.page_script(),
                    desktop: self.desktop.contains(&tab),
                });
            } else if let Some(url) = load {
                out.push(Effect::Load {
                    tab,
                    url: url.to_string(),
                });
            }
        }
        for gone in self.live.touch(tab, MAX_LIVE) {
            out.push(Effect::Drop { tab: gone });
        }
        self.current = Some(tab);
        out.push(Effect::Show { tab: Some(tab) });
        out
    }

    pub(crate) fn home(&mut self) -> Vec<Effect> {
        self.current = None;
        vec![Effect::Show { tab: None }]
    }

    pub(crate) fn toast(text: &str, undo: bool) -> Effect {
        Effect::Ui {
            msg: json!({"type": "toast", "text": text, "undo": undo}),
        }
    }

    /// Evaluates an address the user or a page asked for: `None` when the Shields refuse it.
    /// Plain http is upgraded to https unless the user accepted the plain site.
    fn evaluate(&self, url: &Url) -> Result<Url, &'static str> {
        if url.scheme() == "http" && self.bypasses.allows(&host_of(url)) {
            return Ok(url.clone());
        }
        match self.svc.evaluate(url) {
            NavigationOutcome::Go(u) => Ok(u),
            NavigationOutcome::Blocked(why) => Err(why),
        }
    }

    fn remember_upgrade(&mut self, tab: u64, asked: &Url, going: &Url) {
        if asked.scheme() == "http" && going.scheme() == "https" {
            self.upgraded.insert(tab, asked.clone());
        } else {
            self.upgraded.remove(&tab);
        }
    }

    fn open_private(&mut self, url: Url) -> Vec<Effect> {
        let id = self.next_private;
        self.next_private += 1;
        self.private.insert(
            0,
            PrivateTab {
                id,
                url: url.clone(),
                title: String::new(),
            },
        );
        self.show(id, Some(url))
    }

    /// Opens `input` (address or search): in the current tab, or in a new one.
    pub(crate) fn navigate(&mut self, input: &str, new_tab: bool, private: bool) -> Vec<Effect> {
        let input = input.trim();
        if input.is_empty() {
            return Vec::new();
        }
        let asked = resolve_input(input, self.svc.settings().search_engine);
        let url = match self.evaluate(&asked) {
            Ok(u) => u,
            Err(why) => return vec![Self::toast(&format!("Shields: {why}"), false)],
        };
        let in_private = private || self.current.is_some_and(Self::is_private) && !new_tab;
        let mut out = match self.current {
            Some(tab) if !new_tab => {
                if let Some(t) = self.private.iter_mut().find(|t| t.id == tab) {
                    t.url = url.clone();
                }
                self.errors.remove(&tab);
                self.show(tab, Some(url.clone()))
            }
            _ if in_private => self.open_private(url.clone()),
            _ => match self.svc.open_tab(url.as_str()) {
                Some(id) => {
                    let fresh = !self.live.contains(id.0);
                    self.show(id.0, fresh.then(|| url.clone()))
                }
                None => vec![Self::toast("Shields: blocked", false)],
            },
        };
        if let Some(tab) = self.current {
            self.remember_upgrade(tab, &asked, &url);
        }
        out.push(self.state());
        out
    }

    fn close(&mut self, tab: u64) -> Vec<Effect> {
        let Some(url) = self.tab_url(tab) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let private = Self::is_private(tab);
        if private {
            self.private.retain(|t| t.id != tab);
            if self.private.is_empty() {
                out.push(Effect::ClearProfile {
                    profile: "private".into(),
                });
            }
        } else {
            let pinned = self
                .svc
                .workspace()
                .spaces()
                .iter()
                .flat_map(|s| s.tabs.iter())
                .any(|t| t.id == TabId(tab) && t.pinned);
            // Quick access tiles are put to sleep, never removed (like the desktop).
            let done = if pinned {
                self.svc.release_tab(TabId(tab))
            } else {
                self.svc.close_tab(TabId(tab))
            };
            if !done && !pinned {
                return Vec::new();
            }
        }
        self.live.remove(tab);
        self.errors.remove(&tab);
        self.upgraded.remove(&tab);
        self.pages.remove(&tab);
        self.desktop.remove(&tab);
        out.extend(self.permissions_of_closed(tab));
        out.push(Effect::Drop { tab });
        self.closed.push(Closed { url, private });
        if self.closed.len() > MAX_CLOSED {
            self.closed.remove(0);
        }
        if self.current == Some(tab) {
            // The workspace already chose the successor (recent, then neighbour).
            let next = if private {
                self.private.first().map(|t| t.id)
            } else {
                self.svc.workspace().active_tab().map(|t| t.id.0)
            };
            out.extend(match next {
                Some(next) => self.show(next, None),
                None => self.home(),
            });
        }
        out.push(Self::toast("Tab closed", true));
        out.push(self.state());
        out
    }

    fn undo_close(&mut self) -> Vec<Effect> {
        let Some(c) = self.closed.pop() else {
            return Vec::new();
        };
        let mut out = if c.private {
            self.open_private(c.url)
        } else {
            match self.svc.open_tab(c.url.as_str()) {
                Some(id) => self.show(id.0, Some(c.url)),
                None => Vec::new(),
            }
        };
        out.push(self.state());
        out
    }

    fn close_private(&mut self) -> Vec<Effect> {
        let ids: Vec<u64> = self.private.iter().map(|t| t.id).collect();
        self.private.clear();
        let mut out: Vec<Effect> = ids
            .iter()
            .inspect(|id| {
                self.live.remove(**id);
                self.errors.remove(id);
                self.pages.remove(id);
                self.desktop.remove(id);
            })
            .map(|&tab| Effect::Drop { tab })
            .collect();
        for id in ids {
            out.extend(self.permissions_of_closed(id));
        }
        self.closed.retain(|c| !c.private);
        out.push(Effect::ClearProfile {
            profile: "private".into(),
        });
        if self.current.is_some_and(Self::is_private) {
            out.extend(self.home());
        }
        out.push(self.state());
        out
    }

    fn reload(&mut self, tab: u64) -> Vec<Effect> {
        // On our error page, "reload" means trying the address that failed again.
        match self.errors.remove(&tab) {
            Some((url, _)) => vec![Effect::Load {
                tab,
                url: url.to_string(),
            }],
            None => vec![Effect::Nav {
                tab,
                action: "reload",
            }],
        }
    }

    fn set_setting(&mut self, name: &str, value: &Value) -> Vec<Effect> {
        let on = value.as_bool();
        match (name, on, value.as_str()) {
            ("engine", _, Some(id)) => match SearchEngine::from_id(id) {
                Some(engine) => self.svc.set_search_engine(engine),
                None => return Vec::new(),
            },
            ("animations", Some(on), _) => self.svc.set_animations(on),
            ("trackers", Some(on), _) => self.svc.set_tracker_blocking(on),
            ("ads" | "hide_ads" | "video_ads" | "cookie_banners", Some(on), _) => {
                self.svc.set_shield(name, on);
            }
            ("theme", _, Some(theme)) if THEMES.contains(&theme) => {
                self.prefs.theme = theme.to_owned();
                self.save_prefs();
            }
            ("auto_update", Some(on), _) => {
                self.prefs.auto_update = on;
                self.save_prefs();
            }
            ("pull_refresh", Some(on), _) => {
                self.prefs.pull_refresh = on;
                self.save_prefs();
                return vec![self.web_prefs(), self.state()];
            }
            ("text_zoom", _, _) => {
                let Some(zoom) = value.as_u64().and_then(|z| u32::try_from(z).ok()).filter(|z| TEXT_ZOOMS.contains(z)) else {
                    return Vec::new();
                };
                self.prefs.text_zoom = zoom;
                self.save_prefs();
                return vec![self.web_prefs(), self.state()];
            }
            _ => return Vec::new(),
        }
        vec![self.state()]
    }

    fn suggestions(&self, text: &str) -> Effect {
        let text = text.trim();
        let needle = text.to_lowercase();
        let mut items = Vec::new();
        if !text.is_empty() {
            let engine = self.svc.settings().search_engine;
            match blackhole_domain::omnibox::web_address(text) {
                Some(url) => {
                    // The whole address the user is about to open, without the scheme.
                    let shown = url.as_str().split_once("://").map_or(url.as_str(), |(_, rest)| rest);
                    let shown = shown.strip_suffix('/').unwrap_or(shown);
                    items.push(json!({"kind": "go", "title": shown, "url": url.as_str()}));
                }
                None => items.push(json!({"kind": "search", "title": text, "engine": engine.label()})),
            }
        }
        let matches = |title: &str, url: &Url| {
            needle.is_empty() || title.to_lowercase().contains(&needle) || url.as_str().to_lowercase().contains(&needle)
        };
        let mut seen: Vec<String> = Vec::new();
        for t in self.svc.workspace().active_space().tabs.iter().filter(|t| matches(&t.title, &t.url)) {
            seen.push(t.url.to_string());
            items.push(json!({"kind": "tab", "title": t.title, "url": t.url.as_str(), "tab": t.id.0}));
        }
        for f in self.svc.favorites().iter().filter(|f| matches(&f.name, &f.url)) {
            if !seen.contains(&f.url.to_string()) {
                seen.push(f.url.to_string());
                items.push(json!({"kind": "favorite", "title": f.name, "url": f.url.as_str()}));
            }
        }
        for h in self.svc.recent_activity(400).iter().filter(|h| matches(&h.title, &h.url)) {
            if !seen.contains(&h.url.to_string()) {
                seen.push(h.url.to_string());
                items.push(json!({"kind": "history", "title": h.title, "url": h.url.as_str()}));
            }
        }
        items.truncate(8);
        Effect::Ui {
            msg: json!({"type": "suggest", "text": text, "items": items}),
        }
    }

    fn history(&self, query: &str) -> Effect {
        let needle = query.trim().to_lowercase();
        let items: Vec<Value> = self
            .svc
            .recent_activity(2000)
            .iter()
            .filter(|h| {
                needle.is_empty()
                    || h.title.to_lowercase().contains(&needle)
                    || h.url.as_str().to_lowercase().contains(&needle)
            })
            .take(300)
            .map(|h| json!({"title": h.title, "url": h.url.as_str(), "at": h.visited_at}))
            .collect();
        Effect::Ui {
            msg: json!({"type": "history", "query": query, "items": items}),
        }
    }

    pub(crate) fn current_page(&self) -> Option<Url> {
        let tab = self.current?;
        if let Some((url, _)) = self.errors.get(&tab) {
            return Some(url.clone());
        }
        self.pages.get(&tab).map(|(u, _)| u.clone()).or_else(|| self.tab_url(tab))
    }

    fn privacy_report(&self) -> Effect {
        let site = self.current_page().map(|u| host_of(&u)).filter(|h| !h.is_empty());
        let log = self.blocker.log.lock().map(|l| l.clone()).unwrap_or_default();
        let msg = match site {
            Some(site) => {
                let hosts = log.for_site(&site);
                let (mut trackers, mut ads) = (0, 0);
                let list: Vec<Value> = hosts
                    .iter()
                    .map(|(host, n)| {
                        let ad = self.blocker.is_ad(host);
                        if ad {
                            ads += n;
                        } else {
                            trackers += n;
                        }
                        json!({"host": host, "count": n, "ad": ad})
                    })
                    .collect();
                json!({
                    "type": "report", "site": site, "grade": privacy_grade(trackers, ads).to_string(),
                    "trackers": trackers, "ads": ads, "hosts": list, "total": log.total(),
                    "insecure": self.current_page().is_some_and(|u| u.scheme() == "http"),
                })
            }
            None => json!({"type": "report", "site": null, "total": log.total()}),
        };
        Effect::Ui { msg }
    }

    fn error(&mut self, tab: u64, url: Url, page: ErrorPage) -> Vec<Effect> {
        let host = host_of(&url);
        let mut page = page.with_clock(now_unix());
        if self.upgraded.get(&tab).is_some_and(|u| host_of(u) == host) && url.scheme() == "https" {
            page = page.after_https_upgrade();
        }
        let shown = match page {
            // The plain address is what the user can still open.
            ErrorPage::HttpOnly => self.upgraded.get(&tab).cloned().unwrap_or_else(|| url.clone()),
            _ => url.clone(),
        };
        let mut config = json!({
            "kind": page.key(),
            "url": shown.as_str(),
            "host": host,
            "animations": self.svc.settings().animations,
            "hsts": page.can_bypass() && error_page::hsts_preloaded(&host),
        });
        if let ErrorPage::ServerError(status) = page {
            config["status"] = json!(status);
        }
        if let ErrorPage::Certificate(p) = page {
            config["reason"] = json!(p.key());
        }
        if page == ErrorPage::Blocked {
            config["why"] = json!(if self.blocker.is_ad(&host) { "ad" } else { "tracker" });
        }
        self.errors.insert(tab, (shown.clone(), page));
        vec![
            Effect::Html {
                tab,
                html: error_page(&config),
                url: shown.to_string(),
            },
            self.state(),
        ]
    }

    pub fn command(&mut self, command: MobileCommand) -> Vec<Effect> {
        use MobileCommand as C;
        match command {
            C::Ready => vec![self.web_prefs(), self.icons_message(), self.state()],
            C::Navigate { input, new_tab, private } => self.navigate(&input, new_tab, private),
            C::OpenLink { url, private } => match web_url(&url) {
                Some(url) => self.navigate(url.as_str(), true, private),
                None => Vec::new(),
            },
            C::Activate { tab } if self.exists(tab) => {
                if !Self::is_private(tab) {
                    self.svc.activate_tab(TabId(tab));
                }
                let mut out = self.show(tab, None);
                out.push(self.state());
                out
            }
            C::Activate { .. } => Vec::new(),
            C::Close { tab } => self.close(tab),
            C::UndoClose => self.undo_close(),
            C::ClosePrivate => self.close_private(),
            C::Back => self.current.map_or_else(Vec::new, |tab| vec![Effect::Nav { tab, action: "back" }]),
            C::Forward => self.current.map_or_else(Vec::new, |tab| vec![Effect::Nav { tab, action: "forward" }]),
            C::Reload => self.current.map_or_else(Vec::new, |tab| self.reload(tab)),
            C::Stop => self.current.map_or_else(Vec::new, |tab| vec![Effect::Nav { tab, action: "stop" }]),
            C::DesktopSite { tab } if self.exists(tab) => {
                let on = !self.desktop.remove(&tab);
                if on {
                    self.desktop.insert(tab);
                }
                let mut out = Vec::new();
                if self.live.contains(tab) {
                    out.push(Effect::DesktopMode { tab, on });
                }
                out.push(Self::toast(if on { "Desktop site" } else { "Mobile site" }, false));
                out.push(self.state());
                out
            }
            C::DesktopSite { .. } => Vec::new(),
            C::DownloadUrl { url } => self.event(NativeEvent::Download {
                url,
                mime: String::new(),
                disposition: String::new(),
            }),
            C::PermissionAnswer { id, allow } => self.permission_answer(id, allow),
            C::CheckUpdate { manual } => self.update_check(manual, now_unix()),
            C::InstallUpdate => self.update_install(),
            C::Home => {
                let mut out = self.home();
                out.push(self.state());
                out
            }
            C::Pin { tab } if !Self::is_private(tab) && self.exists(tab) => {
                self.svc.toggle_pin(TabId(tab));
                vec![self.state()]
            }
            C::Pin { .. } => Vec::new(),
            C::SwitchSpace { space } => {
                if !self.svc.switch_space(SpaceId(space)) {
                    return Vec::new();
                }
                let mut out = match self.svc.workspace().active_tab().map(|t| t.id.0) {
                    Some(tab) => self.show(tab, None),
                    None => self.home(),
                };
                out.push(self.state());
                out
            }
            C::AddSpace { name, color } => {
                let name = name.trim();
                let name = if name.is_empty() { "Space" } else { name };
                self.svc.add_space(&name.chars().take(40).collect::<String>(), &color);
                let mut out = self.home();
                out.push(self.state());
                out
            }
            C::SetSpaceColor { space, color } => {
                self.svc.set_space_color(SpaceId(space), &color);
                vec![self.state()]
            }
            C::Suggest { text } => vec![self.suggestions(&text)],
            C::History { query } => vec![self.history(&query)],
            C::ForgetHistory { url } => {
                if let Ok(url) = Url::parse(&url) {
                    self.svc.forget_history(&url);
                }
                vec![self.history(""), self.state()]
            }
            C::ClearHistory => {
                self.svc.clear_history();
                // Icons tell which sites were visited.
                self.icons.clear();
                vec![self.history(""), self.icons_message(), self.state()]
            }
            C::ClearSiteData => {
                if let Ok(mut log) = self.blocker.log.lock() {
                    log.clear();
                }
                self.permissions.clear();
                vec![
                    Effect::ClearProfile { profile: "*".into() },
                    Self::toast("Cookies and site data cleared", false),
                    self.state(),
                ]
            }
            C::PrivacyReport => vec![self.privacy_report()],
            C::SetSetting { name, value } => self.set_setting(&name, &value),
            C::ErrorProceed { tab } => {
                let Some((url, page)) = self.errors.get(&tab).cloned() else {
                    return Vec::new();
                };
                if !page.can_bypass() || !self.bypasses.allow(&host_of(&url)) {
                    return vec![Self::toast("This site cannot be opened without a secure connection", false)];
                }
                self.errors.remove(&tab);
                vec![Effect::Load {
                    tab,
                    url: url.to_string(),
                }]
            }
            C::OpenExternal { url } if is_external(&url) => vec![Effect::External { url }],
            C::OpenExternal { .. } => Vec::new(),
            C::DeviceShowCode
            | C::DeviceJoin { .. }
            | C::DeviceCancelPairing
            | C::DeviceForget { .. }
            | C::DeviceSync { .. }
            | C::DeviceSyncNow { .. }
            | C::SyncAnswer { .. }
            | C::ContinueOn { .. } => self.device_command(command),
        }
    }

    pub fn page_event(&mut self, tab: u64, event: PageEvent) -> Vec<Effect> {
        match event {
            // Only for our own error page, on screen: any page can post this message.
            PageEvent::ErrorMenu { .. } if self.current == Some(tab) => match self.errors.get(&tab) {
                Some((url, page)) if page.can_bypass() => vec![Effect::Ui {
                    msg: json!({
                        "type": "error_menu", "tab": tab, "host": host_of(url),
                        "hsts": error_page::hsts_preloaded(&host_of(url)),
                    }),
                }],
                _ => Vec::new(),
            },
            PageEvent::ErrorMenu { .. } => Vec::new(),
        }
    }

    pub fn event(&mut self, event: NativeEvent) -> Vec<Effect> {
        use NativeEvent as E;
        match event {
            E::Started { tab, url } => {
                let Some(url) = web_url(&url) else {
                    return Vec::new();
                };
                if self.errors.get(&tab).is_some_and(|(u, _)| *u != url) {
                    self.errors.remove(&tab);
                }
                let title = self.pages.get(&tab).map(|p| p.1.clone()).unwrap_or_default();
                self.pages.insert(tab, (url, title));
                if self.current == Some(tab) {
                    vec![self.state()]
                } else {
                    Vec::new()
                }
            }
            E::Finished { tab, url, title } => {
                let Some(url) = web_url(&url) else {
                    return Vec::new();
                };
                if self.errors.contains_key(&tab) {
                    return Vec::new();
                }
                let title = title.trim().to_owned();
                self.pages.insert(tab, (url.clone(), title.clone()));
                if let Some(t) = self.private.iter_mut().find(|t| t.id == tab) {
                    t.url = url;
                    t.title = title;
                } else if !Self::is_private(tab) && self.exists(tab) {
                    let title = (!title.is_empty()).then_some(title);
                    self.svc.page_changed(TabId(tab), Some(url), title);
                }
                vec![self.state()]
            }
            E::Title { tab, title } => {
                let title = title.trim().to_owned();
                if title.is_empty() || self.errors.contains_key(&tab) {
                    return Vec::new();
                }
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.1 = title.clone();
                }
                // A title can come before the page finished loading: the tab (and its history
                // entry) still has the previous address then, and `Finished` brings the title.
                let current = self.pages.get(&tab).map(|p| p.0.clone());
                if let Some(t) = self.private.iter_mut().find(|t| t.id == tab) {
                    t.title = title;
                } else if !Self::is_private(tab) && current.is_some() && self.tab_url(tab) == current {
                    self.svc.page_changed(TabId(tab), None, Some(title));
                }
                vec![self.state()]
            }
            E::Navigating { tab, url } => {
                if is_external(&url) {
                    return vec![
                        Effect::Cancel,
                        Effect::Ui {
                            msg: json!({"type": "external", "url": url}),
                        },
                    ];
                }
                let Ok(asked) = Url::parse(&url) else {
                    return vec![Effect::Cancel];
                };
                if matches!(asked.scheme(), "about" | "data" | "blob") {
                    return Vec::new();
                }
                match self.evaluate(&asked) {
                    Ok(going) if going == asked => {
                        self.remember_upgrade(tab, &asked, &going);
                        Vec::new()
                    }
                    Ok(going) => {
                        self.remember_upgrade(tab, &asked, &going);
                        vec![Effect::Cancel, Effect::Load { tab, url: going.to_string() }]
                    }
                    Err(_) if asked.host_str().is_some_and(|h| self.svc.policy().is_blocked_host(h)) => {
                        let mut out = vec![Effect::Cancel];
                        out.extend(self.error(tab, asked, ErrorPage::Blocked));
                        out
                    }
                    Err(_) => vec![Effect::Cancel],
                }
            }
            E::Failed { tab, url, code, online } => {
                let Some(url) = web_url(&url) else {
                    return Vec::new();
                };
                if self.errors.get(&tab).is_some_and(|(u, _)| *u == url) {
                    return Vec::new();
                }
                let page = if online { classify(Failure::Android(code)) } else { Some(ErrorPage::Offline) };
                match page {
                    Some(page) => self.error(tab, url, page),
                    None => Vec::new(),
                }
            }
            E::SslError { tab, url, primary } => {
                let Some(url) = web_url(&url) else {
                    return vec![Effect::Ssl { proceed: false }];
                };
                if self.bypasses.allows(&host_of(&url)) {
                    return vec![Effect::Ssl { proceed: true }];
                }
                let mut out = vec![Effect::Ssl { proceed: false }];
                if let Some(page) = classify(Failure::AndroidSsl(primary)) {
                    out.extend(self.error(tab, url, page));
                }
                out
            }
            E::BlockedPage { tab, url } => match web_url(&url) {
                Some(url) => self.error(tab, url, ErrorPage::Blocked),
                None => Vec::new(),
            },
            E::BackExhausted => match self.current {
                Some(_) => {
                    let mut out = self.home();
                    out.push(self.state());
                    out
                }
                None => vec![Effect::Exit],
            },
            E::Download { url, mime, disposition } => match web_url(&url) {
                Some(u) if u.scheme() == "https" => vec![
                    Effect::Download { url, mime, disposition },
                    Self::toast("Downloading…", false),
                ],
                _ => vec![Self::toast("Download refused: only secure (https) downloads are allowed", false)],
            },
            E::Intent { url } => match web_url(&url) {
                Some(url) => self.navigate(url.as_str(), true, false),
                None => Vec::new(),
            },
            E::Pause => {
                self.svc.flush();
                flush_writes();
                Vec::new()
            }
            E::Resume => self.update_due(now_unix()),
            E::Favicon { tab, data } => {
                // Private tabs leave no trace, icons included.
                if Self::is_private(tab) || self.errors.contains_key(&tab) {
                    return Vec::new();
                }
                let Some(host) = self.pages.get(&tab).map(|(u, _)| host_of(u)).filter(|h| !h.is_empty()) else {
                    return Vec::new();
                };
                if !self.icons.set(&host, &data) {
                    return Vec::new();
                }
                vec![Effect::Ui {
                    msg: json!({"type": "icon", "host": crate::icons::site_key(&host), "data": data}),
                }]
            }
            E::Permission { id, tab, origin, kinds } => self.permission_asked(id, tab, &origin, &kinds),
            E::Shared { text } => match shared_target(&text, self.svc.settings().search_engine) {
                Some(url) => self.navigate(url.as_str(), true, false),
                None => Vec::new(),
            },
            E::UpdateLatest { url } => self.update_latest(&url),
            E::UpdateSums { tag, sums, debug } => self.update_sums(&tag, &sums, debug),
            E::UpdateDownloaded { sha256 } => self.update_downloaded(&sha256),
            E::UpdateFailed { error } => self.update_failed(&error),
            E::Tick => self.tick(),
        }
    }

    /// Everything the shell interface draws.
    pub fn state(&self) -> Effect {
        let ws = self.svc.workspace();
        let space = ws.active_space();
        let tab_json = |id: u64, title: &str, url: &Url, pinned: bool| {
            let (url, title) = match self.pages.get(&id) {
                Some((u, t)) if !t.is_empty() => (u.clone(), t.clone()),
                Some((u, _)) => (u.clone(), title.to_owned()),
                None => (url.clone(), title.to_owned()),
            };
            json!({
                "id": id, "title": if title.is_empty() { host_of(&url) } else { title },
                "url": url.as_str(), "host": host_of(&url).trim_start_matches("www."),
                "pinned": pinned, "live": self.live.contains(id),
            })
        };
        let tabs: Vec<Value> = space
            .tabs
            .iter()
            .map(|t| tab_json(t.id.0, &t.title, &t.url, t.pinned))
            .collect();
        let private: Vec<Value> = self
            .private
            .iter()
            .map(|t| tab_json(t.id, &t.title, &t.url, false))
            .collect();
        let spaces: Vec<Value> = ws
            .spaces()
            .iter()
            .map(|s| json!({"id": s.id.0, "name": s.name, "color": s.color, "icon": s.icon, "tabs": s.tabs.len()}))
            .collect();
        let favorites: Vec<Value> = self
            .svc
            .favorites()
            .iter()
            .map(|f| json!({"name": f.name, "url": f.url.as_str(), "tab": f.tab.map(|t| t.0)}))
            .collect();
        let recent: Vec<Value> = self
            .svc
            .recent_activity(6)
            .iter()
            .map(|h| json!({"title": h.title, "url": h.url.as_str()}))
            .collect();
        let settings = self.svc.settings();
        let shields = settings.shields;
        let page = self.current_page();
        let site_blocked = page
            .as_ref()
            .map(|u| {
                self.blocker
                    .log
                    .lock()
                    .map(|l| l.for_site(&host_of(u)).iter().map(|(_, n)| n).sum::<u32>())
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let total = self.blocker.log.lock().map(|l| l.total()).unwrap_or(0);
        let error = self.current.and_then(|t| self.errors.get(&t)).map(|(_, p)| p.key());
        Effect::Ui {
            msg: json!({
                "type": "state",
                "version": env!("CARGO_PKG_VERSION"),
                "current": self.current,
                "private": self.current.is_some_and(Self::is_private),
                "url": page.as_ref().map(Url::as_str),
                "title": self.current.and_then(|t| self.pages.get(&t)).map(|p| p.1.clone()),
                "secure": page.as_ref().is_some_and(|u| u.scheme() == "https"),
                "error": error,
                "space": {"id": space.id.0, "name": space.name, "color": space.color},
                "spaces": spaces,
                "tabs": tabs,
                "private_tabs": private,
                "favorites": favorites,
                "recent": recent,
                "blocked": {"total": total, "site": site_blocked},
                "desktop": self.current.is_some_and(|t| self.desktop.contains(&t)),
                "update": self.update_json(),
                "devices": self.devices_json(),
                "settings": {
                    "engine": settings.search_engine.id(),
                    "engines": SearchEngine::ALL.iter().map(|e| json!({"id": e.id(), "label": e.label()})).collect::<Vec<_>>(),
                    "animations": settings.animations,
                    "theme": self.prefs.theme,
                    "trackers": settings.tracker_blocking,
                    "ads": settings.ad_blocking,
                    "hide_ads": shields.hide_ads,
                    "video_ads": shields.video_ads,
                    "cookie_banners": shields.cookie_banners,
                    "text_zoom": self.prefs.text_zoom,
                    "text_zooms": TEXT_ZOOMS,
                    "pull_refresh": self.prefs.pull_refresh,
                    "auto_update": self.prefs.auto_update,
                },
            }),
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.svc.flush();
        flush_writes();
    }
}

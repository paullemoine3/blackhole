//! The signed extension market: its list, what is installed, updates and installs.

use std::time::{Duration, Instant};

use blackhole_infrastructure::{catalog, market};
use serde_json::json;

use crate::i18n;
use crate::features::updates::updates_enabled;
use crate::state::Running;

/// The signed extension market (`market::fetch`): first look shortly after launch, then daily.
pub(crate) const FIRST_MARKET_CHECK: Duration = Duration::from_secs(20);

const MARKET_CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);

const MARKET_RETRY: Duration = Duration::from_secs(3600);

/// The extension market: the list in use, its background refresh, the last check.
pub(crate) struct Market {
    /// Extensions offered: the signed market list kept on this device, else the build's own.
    pub(crate) index: market::Index,
    pub(crate) tx: std::sync::mpsc::Sender<Result<market::Index, String>>,
    pub(crate) rx: std::sync::mpsc::Receiver<Result<market::Index, String>>,
    /// When the list is next refreshed in the background (`None`: never).
    pub(crate) next_check: Option<Instant>,
    /// A market download is running; `Some(true)` when "Check for updates" waits for it.
    pub(crate) fetching: Option<bool>,
    /// Why the last market download failed (the list in use stays).
    pub(crate) error: Option<String>,
    /// Outdated extensions found by the last "Check for updates"; `None` until it is pressed.
    pub(crate) outdated: Option<usize>,
}

impl Market {
    /// The list kept by the profile at `ext_src` (a private window reads the main profile's).
    pub(crate) fn new(ext_src: &std::path::Path) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            index: market::current(ext_src, market::now()),
            tx,
            rx,
            next_check: updates_enabled().then(|| Instant::now() + FIRST_MARKET_CHECK),
            fetching: None,
            error: None,
            outdated: None,
        }
    }
}

impl Running {
    /// Where the market list comes from, for the note under the catalog (Settings > Extensions).
    pub(crate) fn market_json(&self) -> serde_json::Value {
        json!({
            "signed": self.market.index.is_signed(),
            // `seq` is the UNIX time of the signature.
            "signedAt": self.market.index.is_signed().then_some(self.market.index.seq),
            "checking": self.market.fetching == Some(true),
            "error": self.market.error.as_deref().map(i18n::render),
            // The system the packages are chosen for (`None`: no extension host here).
            "platform": blackhole_domain::market::platform_key(std::env::consts::OS),
        })
    }

    /// What updating `e` would let it do that the installed version could not (the update then
    /// waits for a click that accepts it). Unknown installed permissions: everything it asks.
    pub(crate) fn update_adds(&self, e: &market::Entry) -> Vec<blackhole_domain::market::PermissionLine> {
        use blackhole_domain::market::{added_permission_lines, permission_lines};
        if !catalog::needs_update(&self.data, e) {
            return Vec::new();
        }
        match catalog::installed_permissions(&self.data, &e.id) {
            Some(old) => added_permission_lines(&old, &e.permissions),
            None => permission_lines(&e.permissions),
        }
    }

    /// The market's entries that have a package for this system.
    pub(crate) fn catalog_state(&self) -> Vec<serde_json::Value> {
        let lines = |lines: Vec<blackhole_domain::market::PermissionLine>| {
            lines
                .into_iter()
                .map(|l| json!({"key": l.key, "sensitive": l.sensitive, "items": l.items}))
                .collect::<Vec<_>>()
        };
        let describe = |e: &market::Entry| {
                let installed = catalog::is_installed(&self.data, &e.id);
                let loaded = self.webext.as_ref().and_then(|h| h.state(&e.id));
                let status = if self.installing.contains(&e.id) {
                    "installing"
                } else {
                    match loaded {
                        Some(blackhole_macos_webext::LoadState::Loading) => "loading",
                        Some(blackhole_macos_webext::LoadState::Loaded) => "active",
                        Some(blackhole_macos_webext::LoadState::Failed(_)) => "failed",
                        None if installed => "disabled",
                        None => "available",
                    }
                };
                let error = match self.webext.as_ref().and_then(|h| h.state(&e.id)) {
                    Some(blackhole_macos_webext::LoadState::Failed(why)) => Some(why),
                    _ => self.install_errors.get(&e.id).cloned(),
                };
                // The package this system gets (`for_this_system` keeps only entries that have one).
                let package = e.package();
                let file = package.and_then(|p| p.url.rsplit('/').next()).unwrap_or_default();
                // The market carries its own translations (English when the language is missing).
                let mut item = json!({
                    "id": e.id, "name": e.name, "description": e.description_in(i18n::lang()),
                    "version": e.version, "homepage": e.homepage, "category": e.category,
                    "installed": installed, "status": status, "error": error,
                    "installedVersion": catalog::installed_version(&self.data, &e.id),
                    "outdated": catalog::needs_update(&self.data, e),
                    "enabled": self.svc.extension_enabled(&e.id),
                    "supported": self.webext.is_some(),
                });
                item["repo"] = json!(e.source.as_ref().map(|s| s.repo.as_str()));
                item["file"] = json!(file);
                item["size"] = json!(package.map_or(0, |p| p.size));
                item["sha256"] = json!(package.map(|p| p.sha256.as_str()));
                item["permissions"] = json!(lines(blackhole_domain::market::permission_lines(&e.permissions)));
                item["updateAdds"] = json!(lines(self.update_adds(e)));
                item
        };
        let mut list: Vec<_> = self
            .market.index
            .for_this_system()
            .map(|e| {
                let mut item = describe(e);
                item["unverified"] = json!(false);
                item
            })
            .collect();
        // Added from GitHub: not reviewed; an update is a newer release, shown before it is taken.
        for (e, p) in self.github_entries() {
            let mut item = describe(&e);
            let update = self.github.updates.get(&p.id);
            item["unverified"] = json!(true);
            item["repo"] = json!(p.repo_path());
            item["outdated"] = json!(update.is_some());
            item["offered"] = json!(update.map(|(release, _)| release.tag.as_str()));
            list.push(item);
        }
        list
    }

    pub(crate) fn outdated_extensions(&self) -> Vec<String> {
        self.market.index
            .for_this_system()
            .filter(|e| catalog::needs_update(&self.data, e))
            .map(|e| e.id.clone())
            .collect()
    }

    pub(crate) fn install_catalog(&mut self, id: &str) {
        let Some(entry) = self.market.index.find(id).cloned() else {
            return;
        };
        if self.webext.is_none() || !self.installing.insert(id.to_owned()) {
            return;
        }
        self.install_errors.remove(id);
        let (tx, data) = (self.install_tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let result = catalog::install(&data, &entry);
            let _ = tx.send((entry.id, result));
        });
    }

    /// Downloads the signed market list off the UI thread (`market_rx`). `asked`: "Check for
    /// updates" waits for it before counting what is outdated.
    pub(crate) fn refresh_market(&mut self, asked: bool) {
        if let Some(waiting) = &mut self.market.fetching {
            *waiting |= asked;
            return;
        }
        self.market.fetching = Some(asked);
        let (tx, data) = (self.market.tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let _ = tx.send(market::fetch(&data, market::now()));
        });
    }

    /// Runs the daily market refresh and applies what the download thread reports.
    pub(crate) fn poll_market(&mut self) {
        if let Some(at) = self.market.next_check {
            if Instant::now() >= at {
                self.market.next_check = Some(Instant::now() + MARKET_CHECK_EVERY);
                self.refresh_market(false);
            }
        }
        while let Ok(result) = self.market.rx.try_recv() {
            match result {
                Ok(index) => {
                    self.market.error = None;
                    self.market.index = index;
                }
                Err(why) => {
                    tracing::warn!("extension market: {why}");
                    self.market.error = Some(why);
                    let retry = Instant::now() + MARKET_RETRY;
                    if let Some(at) = &mut self.market.next_check {
                        *at = (*at).min(retry);
                    }
                }
            }
            if self.market.fetching.take() == Some(true) {
                // The extensions from GitHub may still be looked up: `poll_github` counts then.
                if self.github.checking == 0 {
                    self.market.outdated = Some(self.outdated_extensions().len() + self.github.updates.len());
                }
            }
            self.ui_state();
        }
    }
}

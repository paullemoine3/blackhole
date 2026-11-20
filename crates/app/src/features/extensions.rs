//! Filesystem helpers for the extensions folder (composition-root concerns: opening Finder,
//! writing the bundled example).

use std::{fs, io, path::Path, process::Command};
use std::time::{Duration, Instant};

use blackhole_infrastructure::{app_extensions, catalog, extensions_dir, load_extensions};
use serde_json::json;

use crate::views;
use crate::ipc::UiCommand;
use crate::platform::{trace, web_view_ptr};
use crate::state::Running;

const SAMPLE_ID: &str = "cookie-banner-hider";
const SAMPLE_MANIFEST: &str = r#"{
  "name": "Cookie banner hider",
  "version": "1.0",
  "description": "Example extension: hides common cookie consent banners and re-enables scrolling.",
  "content_scripts": [
    { "matches": ["<all_urls>"], "css": ["hide.css"], "js": ["unlock.js"] }
  ]
}
"#;
/// Never `html` or `body`: some sites mark the page itself (glosbe.com: `<body class="cookie-consent">`),
/// and hiding it left them blank.
const SAMPLE_CSS: &str = ":is(#onetrust-banner-sdk, #CybotCookiebotDialog, .cookie-banner, .cookie-consent, .cc-window, [id*=\"cookie-banner\"], [class*=\"cookie-banner\"]):not(html, body) { display: none !important; }\n";
/// The example's style sheet up to 0.8.14, replaced where it was left as written.
const OLD_SAMPLE_CSS: [&str; 1] = [
    "#onetrust-banner-sdk, #CybotCookiebotDialog, .cookie-banner, .cookie-consent, .cc-window, [id*=\"cookie-banner\"], [class*=\"cookie-banner\"] { display: none !important; }\n",
];
const SAMPLE_JS: &str = "document.documentElement.style.overflow = document.documentElement.style.overflow === 'hidden' ? '' : document.documentElement.style.overflow;\n";

/// Writes the bundled example extension; existing files are left untouched.
pub fn install_sample(dir: &Path) -> io::Result<()> {
    let root = dir.join(SAMPLE_ID);
    fs::create_dir_all(&root)?;
    for (name, body) in [
        ("manifest.json", SAMPLE_MANIFEST),
        ("hide.css", SAMPLE_CSS),
        ("unlock.js", SAMPLE_JS),
    ] {
        let path = root.join(name);
        if !path.exists() {
            fs::write(path, body)?;
        }
    }
    Ok(())
}

/// Brings an installed example up to date when its style sheet is an old one left as written
/// (a sheet the user changed is theirs). Returns whether it was replaced.
pub fn refresh_sample(dir: &Path) -> bool {
    let css = dir.join(SAMPLE_ID).join("hide.css");
    let Ok(text) = fs::read_to_string(&css) else {
        return false;
    };
    // The same text with Windows line ends counts as left as written.
    let text = text.replace("\r\n", "\n");
    OLD_SAMPLE_CSS.contains(&text.as_str()) && fs::write(css, SAMPLE_CSS).is_ok()
}

/// Shows the folder in Finder / Explorer.
pub fn reveal(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "open"
    };
    let _ = Command::new(opener).arg(dir).spawn();
}

impl Running {
    /// Private window: each extension that finishes loading runs its first-install code now,
    /// so the time its welcome page is dropped starts over (`domain::welcome`).
    fn watch_extension_loads(&mut self) {
        let now = Instant::now();
        if !self.welcome.watching(now) {
            return;
        }
        let Some(host) = &self.webext else { return };
        let loaded = self
            .market
            .for_this_system()
            .map(|e| e.id.as_str())
            .chain(self.github.pinned.iter().map(|p| p.id.as_str()))
            .chain(self.app_ext.iter().map(|e| e.id.as_str()))
            .filter(|id| matches!(host.state(id), Some(blackhole_macos_webext::LoadState::Loaded)))
            .count();
        if loaded > self.ext_loaded {
            self.welcome.extension_loaded(now);
        }
        self.ext_loaded = loaded;
    }

    /// Applies finished downloads and refreshes the settings window when a state changed.
    pub(crate) fn poll_webext(&mut self) {
        self.poll_update();
        self.poll_market();
        self.poll_github();
        self.poll_downloads();
        self.poll_captures();
        self.poll_passwords();
        while let Ok((msg, _count, list)) = self.import_rx.try_recv() {
            if let Some(m) = msg.filter(|m| !m.is_empty()) {
                self.import_msg = m;
                if self.onboarding.is_some() {
                    self.onboarding_imports += 1;
                }
            }
            self.bookmark_list = self.bookmarks.load();
            self.file_bookmarks(list);
            self.ui_state();
        }
        while let Ok((id, result)) = self.install_rx.try_recv() {
            self.installing.remove(&id);
            match result {
                Ok(path) => {
                    self.install_errors.remove(&id);
                    self.svc.set_enabled_flag(&id, true);
                    if let Some(host) = &self.webext {
                        host.load(&id, &path);
                    }
                }
                Err(why) => {
                    self.install_errors.insert(id, why);
                }
            }
        }
        self.sync_extension_tab();
        // Pages an extension asked for itself (`tabs.create`, `windows.create`: Bitwarden's
        // security key sign-in) open as tabs, never as its popup again.
        let opened = self.webext.as_ref().map(|h| h.take_opened()).unwrap_or_default();
        self.watch_extension_loads();
        for url in opened {
            if self.welcome.drops(false, Instant::now()) {
                trace(|| format!("welcome page dropped {url}"));
                continue;
            }
            trace(|| format!("extension opens {url}"));
            self.open(&url);
        }
        if let Some(host) = &self.webext {
            if self.bar_exts_at.elapsed() > Duration::from_millis(800) {
                self.bar_exts_at = Instant::now();
                let list = json!(host
                    .actions()
                    .iter()
                    .filter(|a| a.enabled)
                    .map(|a| json!({
                        "id": a.id, "label": a.label, "badge": a.badge, "icon": a.icon,
                    }))
                    .collect::<Vec<_>>());
                if list != self.bar_exts {
                    self.bar_exts = list;
                    self.ui_bar();
                }
            }
        }
        let sig = serde_json::to_string(&json!([
            self.catalog_state(),
            self.app_ext
                .iter()
                .map(|e| self
                    .webext
                    .as_ref()
                    .and_then(|h| h.state(&e.id))
                    .map(|s| format!("{s:?}")))
                .collect::<Vec<_>>()
        ]))
        .unwrap_or_default();
        if sig != self.webext_sig {
            self.webext_sig = sig;
            self.ui_settings();
            // An extension the welcome tour installs is downloaded, installed or failed.
            if self.onboarding.is_some() && self.start_page_front() {
                self.ui_home();
            }
        }
    }

    /// Tells the extension host which tab is in front, so extensions see a real page.
    fn sync_extension_tab(&self) {
        let Some(host) = &self.webext else { return };
        let tab = if self.home_shown() {
            None
        } else {
            self.svc.workspace().active_tab()
        };
        let Some(tab) = tab else {
            host.set_active_tab(None, "", "");
            return;
        };
        let view = self.tabs.get(&tab.id).and_then(web_view_ptr);
        host.set_active_tab(view, tab.url.as_str(), &tab.title);
    }

    pub(crate) fn reload_extensions(&mut self) {
        self.svc
            .set_extensions(load_extensions(&extensions_dir(&self.ext_src)));
        self.reapply_extensions();
        self.ui_state();
    }

    /// Injects the enabled extensions into every open page (also clears the CSS of disabled ones).
    pub(crate) fn reapply_extensions(&self) {
        for tab in self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
        {
            if let Some(view) = self.tabs.get(&tab.id) {
                let injection = self.svc.injection_for(&tab.url);
                views::apply_injection(view, &injection.css, &injection.js);
            }
        }
    }
}

impl Running {
    /// Commands about extensions: market, GitHub, toolbar buttons and popups, the system's app extensions.
    pub(crate) fn handle_extensions(&mut self, command: UiCommand) {
        match command {
            UiCommand::InstallCatalog { id } => {
                self.install_catalog(&id);
                // The tour shows the download at once (`installing`).
                if self.onboarding.is_some() {
                    self.ui_home();
                }
            }
            UiCommand::GithubLookup { input } => self.github_lookup(&input),
            UiCommand::GithubPrepare { asset, update } => self.github_prepare(asset, update),
            UiCommand::GithubInstall => self.github_install(),
            UiCommand::GithubCancel => self.github_cancel(),
            UiCommand::CheckCatalogUpdates => {
                // The newest signed list first; the count comes when it has arrived (or failed).
                self.refresh_market(true);
                // And a newer release of every extension added from GitHub.
                self.github_check_updates();
                self.ui_state();
            }
            UiCommand::UpdateAllCatalog => {
                // An update that asks for more than the installed version had waits for its own
                // click (the market shows what it adds): "Update all" leaves it out.
                let mut waiting = 0;
                for id in self.outdated_extensions() {
                    let quiet = self.market.find(&id).is_some_and(|e| self.update_adds(e).is_empty());
                    if quiet {
                        self.install_catalog(&id);
                    } else {
                        waiting += 1;
                    }
                }
                self.catalog_checked = Some(waiting);
                self.ui_state();
            }
            UiCommand::ExtAction { id, x, y, w, h: hh } => {
                #[cfg(target_os = "macos")]
                self.show_native_popup(&id, (x, y, w, hh));
                #[cfg(windows)]
                {
                    let url = self.webext.as_ref().and_then(|host| host.popup_url(&id));
                    if let Some(url) = url {
                        self.open_ext_popup(&url, (x, y, w, hh));
                    }
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                let _ = (id, x, y, w, hh);
            }
            UiCommand::CloseExtPopup => {
                #[cfg(windows)]
                self.close_ext_popup();
            }
            UiCommand::ExtAnchors { anchors } => {
                #[cfg(windows)]
                {
                    self.ext_anchors = anchors;
                }
                #[cfg(not(windows))]
                let _ = anchors;
            }
            UiCommand::SetAppExtension { id, on } => {
                if let Some(e) = self.app_ext.iter().find(|e| e.id == id).cloned() {
                    app_extensions::set_enabled(&self.data, &id, on);
                    if let Some(host) = &self.webext {
                        if on {
                            host.load(&id, &e.path);
                        } else {
                            host.unload(&id);
                        }
                    }
                    self.ui_settings();
                }
            }
            UiCommand::RemoveCatalog { id } => {
                if let Some(host) = &self.webext {
                    host.unload(&id);
                }
                if self.github.find(&id).is_some() {
                    self.github_remove(&id);
                } else {
                    catalog::remove(&self.data, &id);
                }
                self.ui_state();
            }
            UiCommand::SetExtensionEnabled { id, enabled }
                if self.market.find(&id).is_some() || self.github.find(&id).is_some() =>
            {
                self.svc.set_enabled_flag(&id, enabled);
                if let Some(host) = &self.webext {
                    if enabled && catalog::is_installed(&self.data, &id) {
                        host.load(&id, &catalog::package_path(&self.data, &id));
                    } else {
                        host.unload(&id);
                    }
                }
                self.ui_state();
            }
            UiCommand::SetExtensionEnabled { id, enabled } => {
                self.svc.set_extension_enabled(&id, enabled);
                self.reapply_extensions();
                self.ui_state();
            }
            UiCommand::ReloadExtensions => self.reload_extensions(),
            UiCommand::OpenExtensionsFolder => reveal(&extensions_dir(&self.data)),
            UiCommand::InstallSampleExtension => {
                let installed = install_sample(&extensions_dir(&self.data)).is_ok();
                if installed {
                    self.reload_extensions();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_never_hides_the_page_and_old_copies_are_brought_up_to_date() {
        assert!(SAMPLE_CSS.contains(":not(html, body)"));
        let dir = std::env::temp_dir().join(format!("bh-sample-ext-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // Nothing installed: nothing to do.
        assert!(!refresh_sample(&dir));
        fs::create_dir_all(dir.join(SAMPLE_ID)).unwrap();
        let css = dir.join(SAMPLE_ID).join("hide.css");
        fs::write(&css, OLD_SAMPLE_CSS[0]).unwrap();
        assert!(refresh_sample(&dir));
        assert_eq!(fs::read_to_string(&css).unwrap(), SAMPLE_CSS);
        // Up to date, or changed by the user: left alone.
        assert!(!refresh_sample(&dir));
        fs::write(&css, ".cookie-consent { display: none !important; }").unwrap();
        assert!(!refresh_sample(&dir));
        assert!(fs::read_to_string(&css).unwrap().starts_with(".cookie-consent"));
        let _ = fs::remove_dir_all(&dir);
    }
}

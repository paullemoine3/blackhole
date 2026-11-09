//! Updates of the APK from the GitHub releases, like the desktop (`infrastructure/updater.rs`):
//! the latest release is the tag `github.com/<repo>/releases/latest` redirects to (never the
//! REST API and its anonymous quota), its `SHA256SUMS.txt` names the APK and its checksum.
//!
//! The Kotlin shell does the network (Android's own trust store; `ureq` finds no root
//! certificates there) and the installation (`PackageInstaller`); every decision is here: which
//! address to fetch, whether the tag is newer, which APK fits the app's signing key, whether the
//! downloaded file is the one the checksums list.

use crate::bridge::Effect;
use crate::core::Core;
use blackhole_infrastructure::updater::{self, Release};
use serde_json::{json, Value};

/// Automatic checks: at most one a day.
pub(crate) const CHECK_EVERY: i64 = 24 * 3600;
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Checking,
    /// Checked: nothing newer.
    Latest,
    Available(Release),
    Downloading(Release),
    Installing(Release),
    Failed(String),
}

#[derive(Debug, Default)]
pub(crate) struct Updater {
    pub(crate) phase: Phase,
    /// The check was asked for in Settings: its outcome is said in a toast.
    manual: bool,
    /// The last release found, kept to try again after a failed download.
    found: Option<Release>,
}

impl Updater {
    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Checking | Phase::Downloading(_) | Phase::Installing(_))
    }
}

/// Error texts come from the network or the system: one bounded line.
fn short(error: &str) -> String {
    let line = error.lines().next().unwrap_or_default().trim();
    let mut out: String = line.chars().take(160).collect();
    if out.is_empty() {
        out = "Unknown error".into();
    }
    out
}

impl Core {
    pub(crate) fn update_check(&mut self, manual: bool, now: i64) -> Vec<Effect> {
        if self.update.busy() {
            return Vec::new();
        }
        self.update.phase = Phase::Checking;
        self.update.manual = manual;
        self.prefs.update_checked = now;
        self.save_prefs();
        vec![
            Effect::UpdateProbe {
                url: updater::latest_url(),
            },
            self.state(),
        ]
    }

    /// The daily check, when the app comes back on screen.
    pub(crate) fn update_due(&mut self, now: i64) -> Vec<Effect> {
        let waited = now.saturating_sub(self.prefs.update_checked);
        // A clock set back (negative wait) checks again rather than never.
        let due = !(0..CHECK_EVERY).contains(&waited);
        if !self.prefs.auto_update || !due || self.update.busy() || matches!(self.update.phase, Phase::Available(_)) {
            return Vec::new();
        }
        self.update_check(false, now)
    }

    fn update_done(&mut self, phase: Phase, toast: Option<String>) -> Vec<Effect> {
        let manual = self.update.manual;
        self.update.phase = phase;
        let mut out = Vec::new();
        if let Some(text) = toast.filter(|_| manual) {
            out.push(Self::toast(&text, false));
        }
        out.push(self.state());
        out
    }

    pub(crate) fn update_latest(&mut self, url: &str) -> Vec<Effect> {
        if self.update.phase != Phase::Checking {
            return Vec::new();
        }
        let Some(tag) = updater::tag_of_latest(url).map(str::to_owned) else {
            // No release published (the redirect leads to the release list).
            return self.update_done(Phase::Latest, Some("BlackHole is up to date".into()));
        };
        if !updater::is_newer(&tag, VERSION) {
            return self.update_done(Phase::Latest, Some("BlackHole is up to date".into()));
        }
        match updater::sums_url(&tag) {
            Some(url) => vec![Effect::UpdateSums { tag, url }],
            None => self.update_done(Phase::Latest, None),
        }
    }

    pub(crate) fn update_sums(&mut self, tag: &str, sums: &str, debug: bool) -> Vec<Effect> {
        if self.update.phase != Phase::Checking {
            return Vec::new();
        }
        let kind = if debug { "android-debug" } else { "android" };
        let release = updater::release_from_sums(tag, sums, kind)
            .filter(|r| updater::is_newer(&r.version, VERSION) && updater::trusted_url(&r.url));
        match release {
            Some(r) => {
                // Said even after the automatic check (it is what the check is for), with a button.
                let toast = Effect::Ui {
                    msg: json!({
                        "type": "toast", "text": format!("BlackHole {} is available", r.version),
                        "undo": false, "action": "install_update",
                    }),
                };
                self.update.found = Some(r.clone());
                self.update.phase = Phase::Available(r);
                vec![toast, self.state()]
            }
            None => {
                let why = format!("Release {tag} has no APK signed like this app");
                self.update_done(Phase::Failed(why.clone()), Some(why))
            }
        }
    }

    pub(crate) fn update_install(&mut self) -> Vec<Effect> {
        let release = match &self.update.phase {
            Phase::Available(r) => Some(r.clone()),
            Phase::Failed(_) => self.update.found.clone(),
            _ => None,
        };
        let Some(r) = release.filter(|r| updater::trusted_url(&r.url)) else {
            return Vec::new();
        };
        self.update.manual = true;
        self.update.phase = Phase::Downloading(r.clone());
        vec![
            Effect::UpdateDownload {
                url: r.url,
                sha256: r.sha256,
                name: r.asset,
            },
            self.state(),
        ]
    }

    pub(crate) fn update_downloaded(&mut self, sha256: &str) -> Vec<Effect> {
        let Phase::Downloading(r) = &self.update.phase else {
            return Vec::new();
        };
        if r.sha256.eq_ignore_ascii_case(sha256.trim()) {
            let r = r.clone();
            self.update.phase = Phase::Installing(r);
            return vec![Effect::UpdateInstall, self.state()];
        }
        let why = "The download does not match the release checksum".to_owned();
        let mut out = vec![Effect::UpdateDiscard];
        out.extend(self.update_done(Phase::Failed(why.clone()), Some(why)));
        out
    }

    pub(crate) fn update_failed(&mut self, error: &str) -> Vec<Effect> {
        if !self.update.busy() {
            return Vec::new();
        }
        let why = short(error);
        self.update_done(Phase::Failed(why.clone()), Some(format!("Update: {why}")))
    }

    pub(crate) fn update_json(&self) -> Value {
        let (status, version, error) = match &self.update.phase {
            Phase::Idle => ("idle", None, None),
            Phase::Checking => ("checking", None, None),
            Phase::Latest => ("latest", None, None),
            Phase::Available(r) => ("available", Some(r.version.clone()), None),
            Phase::Downloading(r) => ("downloading", Some(r.version.clone()), None),
            Phase::Installing(r) => ("installing", Some(r.version.clone()), None),
            Phase::Failed(e) => ("failed", self.update.found.as_ref().map(|r| r.version.clone()), Some(e.clone())),
        };
        json!({
            "status": status,
            "version": version,
            "error": error,
            "current": VERSION,
            "checked": self.prefs.update_checked,
            "auto": self.prefs.auto_update,
            "retry": matches!(self.update.phase, Phase::Failed(_)) && self.update.found.is_some(),
        })
    }
}

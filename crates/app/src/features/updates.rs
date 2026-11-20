//! Self-update of the app: checks, download, install.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use blackhole_infrastructure::updater;
use blackhole_infrastructure::updater::Release;
use serde_json::json;

use crate::i18n;
use crate::shell::windows::private_mode;
use crate::state::Running;

pub(crate) const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(15);

const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// After a failed check (offline, GitHub unreachable), try again well before `UPDATE_CHECK_EVERY`.
const UPDATE_RETRY: Duration = Duration::from_secs(30 * 60);

/// Where the self-update stands; shown as a button at the bottom of the sidebar.
pub(crate) enum UpdateState {
    Idle,
    Available(Release),
    Downloading(Release),
    Failed(Release, String),
}

pub(crate) enum UpdateMsg {
    Found(Release),
    /// The check found nothing newer.
    Current,
    CheckFailed(String),
    Downloaded(PathBuf),
    Failed(String),
}

/// Scripted devtools runs and cargo builds never look for (or install) updates.
pub(crate) fn updates_enabled() -> bool {
    !cfg!(feature = "devtools") && can_self_update()
}

/// Whether this installation may download and install a release (Settings can still check).
fn can_self_update() -> bool {
    !private_mode() && updater::supported()
}

impl Running {
    /// Sidebar button: only for an installation that can replace itself.
    pub(crate) fn update_json(&self) -> serde_json::Value {
        if !can_self_update() {
            return json!(null);
        }
        match &self.update {
            UpdateState::Idle => json!(null),
            UpdateState::Available(r) => json!({"state": "available", "version": r.version}),
            UpdateState::Downloading(r) => json!({"state": "downloading", "version": r.version}),
            UpdateState::Failed(r, why) => {
                json!({"state": "error", "version": r.version, "error": i18n::render(why)})
            }
        }
    }

    /// Settings > Updates: the check and the install, step by step.
    pub(crate) fn app_update_json(&self) -> serde_json::Value {
        let (state, version, error) = match (&self.update, &self.update_checked) {
            (UpdateState::Downloading(r), _) => ("downloading", Some(&r.version), None),
            _ if self.update_checking => ("checking", None, None),
            (UpdateState::Available(r), _) => ("available", Some(&r.version), None),
            (UpdateState::Failed(r, why), _) => ("install_failed", Some(&r.version), Some(why)),
            (UpdateState::Idle, None) => ("idle", None, None),
            (UpdateState::Idle, Some(Ok(()))) => ("current", None, None),
            (UpdateState::Idle, Some(Err(why))) => ("check_failed", None, Some(why)),
        };
        json!({
            "state": state,
            "version": version,
            // A network failure is a message of the updater, said in the interface's language.
            "error": error.map(|why| i18n::render(why)),
            "installable": can_self_update(),
            "page": format!("https://github.com/{}/releases/latest", updater::REPO),
        })
    }

    /// Looks for a newer release off the UI thread; the answer comes back through `update_rx`.
    pub(crate) fn check_for_update(&mut self) {
        if self.update_checking || matches!(self.update, UpdateState::Downloading(_)) {
            return;
        }
        self.update_checking = true;
        let tx = self.update_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(match updater::check(env!("CARGO_PKG_VERSION")) {
                Ok(Some(release)) => UpdateMsg::Found(release),
                Ok(None) => UpdateMsg::Current,
                Err(why) => UpdateMsg::CheckFailed(why),
            });
        });
    }

    /// Runs the periodic release check and applies what the background threads report.
    pub(crate) fn poll_update(&mut self) {
        if let Some(at) = self.next_update_check {
            if Instant::now() >= at {
                self.next_update_check = Some(Instant::now() + UPDATE_CHECK_EVERY);
                self.check_for_update();
            }
        }
        while let Ok(msg) = self.update_rx.try_recv() {
            match msg {
                UpdateMsg::Found(release) => {
                    self.update_checking = false;
                    self.update_checked = Some(Ok(()));
                    if !matches!(self.update, UpdateState::Downloading(_)) {
                        self.update = UpdateState::Available(release);
                    }
                }
                UpdateMsg::Current => {
                    self.update_checking = false;
                    self.update_checked = Some(Ok(()));
                }
                UpdateMsg::CheckFailed(why) => {
                    tracing::warn!("update check failed: {why}");
                    self.update_checking = false;
                    self.update_checked = Some(Err(why));
                    // A failed check no longer waits for the next 6-hour round.
                    let retry = Instant::now() + UPDATE_RETRY;
                    if let Some(at) = &mut self.next_update_check {
                        *at = (*at).min(retry);
                    }
                }
                UpdateMsg::Downloaded(path) => {
                    if let UpdateState::Downloading(release) =
                        std::mem::replace(&mut self.update, UpdateState::Idle)
                    {
                        match updater::apply(&path) {
                            Ok(()) => {
                                self.quit = true;
                                self.update = UpdateState::Downloading(release);
                            }
                            Err(why) => self.update = UpdateState::Failed(release, why),
                        }
                    }
                }
                UpdateMsg::Failed(why) => {
                    if let UpdateState::Downloading(release) =
                        std::mem::replace(&mut self.update, UpdateState::Idle)
                    {
                        self.update = UpdateState::Failed(release, why);
                    }
                }
            }
            self.ui_state();
        }
    }

    /// Sidebar button: downloads the new version, then the installer takes over and restarts.
    pub(crate) fn start_update(&mut self) {
        if !can_self_update() {
            return;
        }
        let release = match &self.update {
            UpdateState::Available(r) | UpdateState::Failed(r, _) => r.clone(),
            _ => return,
        };
        self.update = UpdateState::Downloading(release.clone());
        let (tx, data) = (self.update_tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let _ = tx.send(match updater::download(&data, &release) {
                Ok(path) => UpdateMsg::Downloaded(path),
                Err(why) => UpdateMsg::Failed(why),
            });
        });
        self.ui_state();
    }
}

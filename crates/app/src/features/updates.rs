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

/// Self-update of the app: where it stands, the background check and its answers.
pub(crate) struct Updates {
    /// Where the update stands; shown as a button at the bottom of the sidebar.
    pub(crate) state: UpdateState,
    pub(crate) tx: std::sync::mpsc::Sender<UpdateMsg>,
    pub(crate) rx: std::sync::mpsc::Receiver<UpdateMsg>,
    /// When the next background check is due (`None`: no background checks).
    pub(crate) next_check: Option<Instant>,
    /// A release check is running (background or Settings > Updates).
    pub(crate) checking: bool,
    /// What the last finished check said: up to date, or why it failed (Settings > Updates).
    pub(crate) checked: Option<Result<(), String>>,
}

impl Updates {
    pub(crate) fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            state: UpdateState::Idle,
            tx,
            rx,
            next_check: updates_enabled().then(|| Instant::now() + FIRST_UPDATE_CHECK),
            checking: false,
            checked: None,
        }
    }
}

impl Running {
    /// Sidebar button: only for an installation that can replace itself.
    pub(crate) fn update_json(&self) -> serde_json::Value {
        if !can_self_update() {
            return json!(null);
        }
        match &self.updates.state {
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
        let (state, version, error) = match (&self.updates.state, &self.updates.checked) {
            (UpdateState::Downloading(r), _) => ("downloading", Some(&r.version), None),
            _ if self.updates.checking => ("checking", None, None),
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
        if self.updates.checking || matches!(self.updates.state, UpdateState::Downloading(_)) {
            return;
        }
        self.updates.checking = true;
        let tx = self.updates.tx.clone();
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
        if let Some(at) = self.updates.next_check {
            if Instant::now() >= at {
                self.updates.next_check = Some(Instant::now() + UPDATE_CHECK_EVERY);
                self.check_for_update();
            }
        }
        while let Ok(msg) = self.updates.rx.try_recv() {
            match msg {
                UpdateMsg::Found(release) => {
                    self.updates.checking = false;
                    self.updates.checked = Some(Ok(()));
                    if !matches!(self.updates.state, UpdateState::Downloading(_)) {
                        self.updates.state = UpdateState::Available(release);
                    }
                }
                UpdateMsg::Current => {
                    self.updates.checking = false;
                    self.updates.checked = Some(Ok(()));
                }
                UpdateMsg::CheckFailed(why) => {
                    tracing::warn!("update check failed: {why}");
                    self.updates.checking = false;
                    self.updates.checked = Some(Err(why));
                    // A failed check no longer waits for the next 6-hour round.
                    let retry = Instant::now() + UPDATE_RETRY;
                    if let Some(at) = &mut self.updates.next_check {
                        *at = (*at).min(retry);
                    }
                }
                UpdateMsg::Downloaded(path) => {
                    if let UpdateState::Downloading(release) =
                        std::mem::replace(&mut self.updates.state, UpdateState::Idle)
                    {
                        match updater::apply(&path) {
                            Ok(()) => {
                                self.quit = true;
                                self.updates.state = UpdateState::Downloading(release);
                            }
                            Err(why) => self.updates.state = UpdateState::Failed(release, why),
                        }
                    }
                }
                UpdateMsg::Failed(why) => {
                    if let UpdateState::Downloading(release) =
                        std::mem::replace(&mut self.updates.state, UpdateState::Idle)
                    {
                        self.updates.state = UpdateState::Failed(release, why);
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
        let release = match &self.updates.state {
            UpdateState::Available(r) | UpdateState::Failed(r, _) => r.clone(),
            _ => return,
        };
        self.updates.state = UpdateState::Downloading(release.clone());
        let (tx, data) = (self.updates.tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let _ = tx.send(match updater::download(&data, &release) {
                Ok(path) => UpdateMsg::Downloaded(path),
                Err(why) => UpdateMsg::Failed(why),
            });
        });
        self.ui_state();
    }
}

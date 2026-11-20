//! The app's other windows (each its own process): registry, new and private windows, tabs dropped out.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use blackhole_domain::TabId;
use blackhole_infrastructure::peers;

use crate::i18n::t;
use crate::ipc::UiCommand;
use crate::state::Running;

#[cfg(feature = "devtools")]
use crate::devtools;

/// How often the event loop wakes to refresh this window's place in the registry, look for
/// incoming tabs, spares to build and media to probe.
pub(crate) const PEERS_EVERY: Duration = Duration::from_millis(400);

/// A window's listing file is only rewritten this often (peers consider it stale after 5 s).
pub(crate) const PEERS_PUBLISH_EVERY: Duration = Duration::from_secs(2);

/// A private window runs in its own process with a throwaway profile (cookies, history, tabs,
/// bookmarks), deleted when it closes.
pub(crate) fn private_mode() -> bool {
    static PRIVATE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PRIVATE.get_or_init(|| std::env::args().any(|a| a == "--private"))
}

/// Whether this window opens beside others ("New window", a private window, the taskbar while
/// the app runs) rather than starting the app: no launch splash then.
pub(crate) fn is_new_window() -> bool {
    private_mode()
        || std::env::args().any(|a| a == "--new-window")
        || !peers::peers(&peers_dir(), std::process::id()).is_empty()
}

/// Dev builds keep their window registry in the scratch folder, away from the real app.
pub(crate) fn peers_dir() -> PathBuf {
    #[cfg(feature = "devtools")]
    {
        devtools::scratch().join("windows")
    }
    #[cfg(not(feature = "devtools"))]
    {
        peers::dir()
    }
}

pub(crate) fn private_data_dir() -> PathBuf {
    std::env::temp_dir().join(format!("BlackHole-private-{}", std::process::id()))
}

/// Each window is its own process: a private one gets `--private` and a temporary profile.
/// A window given a page opens it right away (`--open <url>`).
pub(crate) fn open_window(private: bool, url: Option<&str>) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--new-window");
    if private {
        cmd.arg("--private");
    }
    if let Some(url) = url {
        cmd.args(["--open", url]);
    }
    cmd.spawn().is_ok()
}

/// Windows taskbar: the tasks of the app's jump list, in the language of the interface (set at
/// launch and when the language changes). Nothing on the other systems.
pub(crate) fn window_tasks() {
    blackhole_macos_webext::set_window_tasks(&[
        (t("palette.new_window"), String::new()),
        (t("palette.new_private_window"), "--private".to_owned()),
    ]);
}

impl Running {
    /// Keeps this window listed for the others and picks up tabs they sent here.
    pub(crate) fn service_peers(&mut self) {
        if Instant::now() < self.peers_due {
            return;
        }
        self.peers_due = Instant::now() + PEERS_EVERY;
        let dir = peers_dir();
        let publish = self.peers_published.elapsed() >= PEERS_PUBLISH_EVERY;
        if let Some((x, y, w, h)) = self.screen_rect().filter(|_| publish) {
            self.peers_published = Instant::now();
            peers::publish(
                &dir,
                &peers::Peer {
                    pid: std::process::id(),
                    private: private_mode(),
                    x,
                    y,
                    w,
                    h,
                    updated: 0,
                },
            );
        }
        let incoming = peers::receive(&dir, std::process::id());
        if incoming.is_empty() {
            return;
        }
        for transfer in incoming {
            self.open(&transfer.url);
        }
        self.window.focus_window();
    }

    /// A tab released outside its sidebar: hands it to the window under the pointer, if any.
    pub(crate) fn drop_tab_out(&mut self, id: TabId, x: f64, y: f64) {
        if self
            .screen_rect()
            .is_some_and(|(wx, wy, w, h)| x >= wx && x < wx + w && y >= wy && y < wy + h)
        {
            return;
        }
        let dir = peers_dir();
        let others = peers::peers(&dir, std::process::id());
        let target = peers::at(&others, x, y).map(|p| p.pid);
        let Some(tab) = self
            .svc
            .workspace()
            .active_space()
            .tabs
            .iter()
            .find(|t| t.id == id)
        else {
            return;
        };
        let transfer = peers::Transfer {
            url: tab.url.to_string(),
            title: tab.title.clone(),
        };
        let moved = match target {
            Some(pid) => peers::send(&dir, pid, &transfer),
            // Released over nothing: the tab becomes a window of its own (same privacy).
            None => open_window(private_mode(), Some(&transfer.url)),
        };
        if moved {
            self.handle_ui(UiCommand::Close { id: id.0 });
        }
    }
}

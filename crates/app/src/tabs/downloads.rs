//! Downloads: the live cards of the sidebar, their record, opening a finished file.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use blackhole_domain::TabId;
use serde_json::json;

use crate::i18n::t;
use crate::ipc::UiCommand;
use crate::platform::{now_secs, trace};
use crate::shell::notices;
use crate::state::Running;

/// A download shown in the sidebar while it runs, and for a few seconds after it ends.
pub(crate) struct DownloadLive {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) received: u64,
    pub(crate) total: Option<u64>,
    pub(crate) finished: Option<(bool, Instant)>,
}

/// How long a finished download stays in the sidebar.
const DOWNLOAD_LINGER: Duration = Duration::from_secs(8);

pub(crate) const DOWNLOAD_POLL: Duration = Duration::from_millis(500);

/// How long after a download started the end of its navigation may still be reported.
pub(crate) const DOWNLOAD_NAV: Duration = Duration::from_secs(10);

/// How long a hidden sidebar shows itself for a download card.
pub(crate) const DOWNLOAD_REVEAL: Duration = Duration::from_secs(4);

/// Opens a downloaded file with its default app; programs are only revealed, never run.
pub(crate) fn open_download(path: &std::path::Path) {
    let risky = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(blackhole_domain::downloads::is_program);
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if risky {
            cmd.arg("-R");
        }
        let _ = cmd.arg(path).spawn();
    }
    #[cfg(windows)]
    {
        if risky {
            reveal_file(path);
        } else {
            let _ = std::process::Command::new("explorer").arg(path).spawn();
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (path, risky);
}

/// Shows a downloaded file selected in Finder / Explorer (Linux: its folder). A file that is
/// gone shows its folder.
pub(crate) fn reveal_file(path: &std::path::Path) {
    let folder = path.parent().unwrap_or(path);
    if !path.exists() {
        crate::features::extensions::reveal(folder);
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    // `/select,"…"` must reach Explorer as written: the quoting of `arg` breaks it on a path
    // with spaces.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
}

/// Downloads: their record, the cards of the running ones, and the tabs they came from.
pub(crate) struct Downloads {
    /// Every download, as kept in the profile (Library > Downloads).
    pub(crate) records: Vec<blackhole_infrastructure::store::DownloadRec>,
    /// Downloads shown in the sidebar while they run, and a few seconds after.
    pub(crate) live: HashMap<u64, DownloadLive>,
    /// Tabs opened for a file alone (no page shown), by download URL: closed once it is done
    /// (closing the view earlier would cancel the download).
    pub(crate) only_tabs: HashMap<String, TabId>,
    /// Tabs whose navigation just became a download: the engine ends that navigation as a
    /// failure, which is no error to show.
    pub(crate) navs: HashMap<TabId, Instant>,
    pub(crate) polled: Instant,
    /// Sizes announced by the servers, read in the background: `(download id, bytes)`.
    pub(crate) size_tx: std::sync::mpsc::Sender<(u64, u64)>,
    pub(crate) size_rx: std::sync::mpsc::Receiver<(u64, u64)>,
}

impl Downloads {
    pub(crate) fn new(data: &std::path::Path) -> Self {
        let (size_tx, size_rx) = std::sync::mpsc::channel();
        Self {
            records: blackhole_infrastructure::store::DownloadStore::new(data).load(),
            live: HashMap::new(),
            only_tabs: HashMap::new(),
            navs: HashMap::new(),
            polled: Instant::now(),
            size_tx,
            size_rx,
        }
    }
}

impl Running {
    pub(crate) fn downloads_json(&self) -> serde_json::Value {
        let mut live: Vec<_> = self.downloads.live.iter().collect();
        live.sort_by_key(|(id, _)| **id);
        json!(live
            .into_iter()
            .map(|(id, d)| json!({
                "id": id, "name": d.name, "received": d.received, "total": d.total,
                "state": d.finished.map(|(ok, _)| if ok { "ok" } else { "bad" }),
            }))
            .collect::<Vec<_>>())
    }

    /// Refreshes the sidebar's download cards from the size of the files being written.
    pub(crate) fn poll_downloads(&mut self) {
        if self.downloads.live.is_empty() {
            return;
        }
        let mut changed = false;
        while let Ok((id, total)) = self.downloads.size_rx.try_recv() {
            if let Some(d) = self.downloads.live.get_mut(&id) {
                d.total = Some(total);
                changed = true;
            }
        }
        if self.downloads.polled.elapsed() >= DOWNLOAD_POLL {
            self.downloads.polled = Instant::now();
            for d in self.downloads.live.values_mut().filter(|d| d.finished.is_none()) {
                let size = blackhole_infrastructure::partial_size(&d.path);
                if size != d.received {
                    d.received = size;
                    changed = true;
                }
            }
            let before = self.downloads.live.len();
            self.downloads.live.retain(|_, d| {
                d.finished
                    .is_none_or(|(_, at)| at.elapsed() < DOWNLOAD_LINGER)
            });
            changed |= before != self.downloads.live.len();
        }
        if changed {
            let list = self.downloads_json();
            self.ui(format!("window.bh&&bh.downloads({list})"));
        }
    }

    /// The navigation of `tab` just turned into a download.
    pub(crate) fn download_took(&self, tab: TabId) -> bool {
        self.downloads.navs.get(&tab).is_some_and(|at| at.elapsed() < DOWNLOAD_NAV)
    }

    pub(crate) fn save_downloads(&self) {
        let _ =
            blackhole_infrastructure::store::DownloadStore::new(&self.data).save(&self.downloads.records);
    }

    /// A navigation became a download: its card in the sidebar, its record, its size asked in the background.
    pub(crate) fn download_started(&mut self, tab: TabId, url: String, path: PathBuf) {
        trace(|| format!("download started tab={} {url}", tab.0));
        // The navigation became a download: the tab is not waiting for a page any more
        // (else "Site injoignable" after CONNECT_MAX). A tab opened for the file alone,
        // that never showed a page, closes once the file is done, as in other browsers.
        if self.awaiting.remove(&tab).is_some() && self.splash.contains_key(&tab) {
            self.downloads.only_tabs.insert(url.clone(), tab);
        }
        self.downloads.navs.retain(|_, at| at.elapsed() < DOWNLOAD_NAV);
        self.downloads.navs.insert(tab, Instant::now());
        if self.aborted.remove(&tab).is_some() {
            self.forget_failure(tab);
        }
        self.loading.remove(&tab);
        self.pending.remove(&tab);
        self.reveal_sidebar(DOWNLOAD_REVEAL);
        let id = self.downloads.records.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let remote = url.clone();
        self.downloads.records
            .push(blackhole_infrastructure::store::DownloadRec {
                id,
                name,
                path: path.to_string_lossy().into_owned(),
                url,
                at: now_secs(),
                done: false,
                ok: false,
            });
        self.save_downloads();
        self.downloads.live.insert(
            id,
            DownloadLive {
                name: self
                    .downloads.records
                    .last()
                    .map(|d| d.name.clone())
                    .unwrap_or_default(),
                path: path.clone(),
                received: 0,
                total: None,
                finished: None,
            },
        );
        self.downloads.polled = Instant::now();
        let tx = self.downloads.size_tx.clone();
        std::thread::spawn(move || {
            if let Some(total) = blackhole_infrastructure::remote_size(&remote) {
                let _ = tx.send((id, total));
            }
        });
        let list = self.downloads_json();
        self.ui(format!("window.bh&&bh.downloads({list})"));
        self.sync_bar_notice();
        self.notify(notices::Notice::Download, &t("toast.download_started"), serde_json::Value::Null);
        self.ui_library();
    }

    /// A file the shell wrote itself ("Save image", 0.8.25): a finished download, with its card
    /// in the sidebar and its line in Library > Downloads, like one of the engine.
    pub(crate) fn download_saved(&mut self, url: String, path: PathBuf) {
        let id = self.downloads.records.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        self.downloads.records.push(blackhole_infrastructure::store::DownloadRec {
            id,
            name: name.clone(),
            path: path.to_string_lossy().into_owned(),
            url,
            at: now_secs(),
            done: true,
            ok: true,
        });
        self.save_downloads();
        self.downloads.live.insert(
            id,
            DownloadLive { name, path, received: size, total: Some(size.max(1)), finished: Some((true, Instant::now())) },
        );
        let list = self.downloads_json();
        self.ui(format!("window.bh&&bh.downloads({list})"));
        self.reveal_sidebar(DOWNLOAD_REVEAL);
        self.sync_bar_notice();
        self.notify(notices::Notice::Done, &t("toast.image_saved"), serde_json::Value::Null);
        self.ui_library();
    }

    /// A download ended: its record and card updated, the tab opened for it alone closed.
    pub(crate) fn download_done(&mut self, url: String, path: Option<PathBuf>, ok: bool) {
        if let Some(tab) = self.downloads.only_tabs.remove(&url) {
            if self.tabs.contains_key(&tab) {
                self.handle_ui(UiCommand::Close { id: tab.0 });
            }
        }
        if let Some(d) = self
            .downloads.records
            .iter_mut()
            .rev()
            .find(|d| d.url == url && !d.done)
        {
            d.done = true;
            d.ok = ok;
            if let Some(p) = path {
                d.path = p.to_string_lossy().into_owned();
            }
            if let Some(live) = self.downloads.live.get_mut(&d.id) {
                live.finished = Some((ok, Instant::now()));
                live.path = PathBuf::from(&d.path);
                if ok {
                    let size = std::fs::metadata(&live.path).map_or(0, |m| m.len());
                    live.received = size;
                    live.total = Some(size.max(1));
                }
            }
        }
        self.save_downloads();
        let list = self.downloads_json();
        self.ui(format!("window.bh&&bh.downloads({list})"));
        self.reveal_sidebar(DOWNLOAD_REVEAL);
        self.sync_bar_notice();
        let done = self
            .downloads.records
            .iter()
            .rev()
            .find(|d| d.url == url && d.done)
            .map(|d| d.id);
        match done.filter(|_| ok) {
            Some(id) => self.notify_action(
                notices::Notice::Done,
                &t("toast.download_complete"),
                &t("library.show_in_folder"),
                serde_json::json!({"cmd": "reveal_download", "id": id}),
            ),
            None => self.notify(
                notices::Notice::Problem,
                &t("toast.download_failed"),
                serde_json::Value::Null,
            ),
        }
        self.ui_library();
    }
}

//! Downloads: the live cards of the sidebar, their record, opening a finished file.

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
        let mut cmd = std::process::Command::new("explorer");
        if risky {
            cmd.arg(format!("/select,{}", path.display()));
        } else {
            cmd.arg(path);
        }
        let _ = cmd.spawn();
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (path, risky);
}

impl Running {
    pub(crate) fn downloads_json(&self) -> serde_json::Value {
        let mut live: Vec<_> = self.dl_live.iter().collect();
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
        if self.dl_live.is_empty() {
            return;
        }
        let mut changed = false;
        while let Ok((id, total)) = self.dl_size_rx.try_recv() {
            if let Some(d) = self.dl_live.get_mut(&id) {
                d.total = Some(total);
                changed = true;
            }
        }
        if self.dl_polled.elapsed() >= DOWNLOAD_POLL {
            self.dl_polled = Instant::now();
            for d in self.dl_live.values_mut().filter(|d| d.finished.is_none()) {
                let size = blackhole_infrastructure::partial_size(&d.path);
                if size != d.received {
                    d.received = size;
                    changed = true;
                }
            }
            let before = self.dl_live.len();
            self.dl_live.retain(|_, d| {
                d.finished
                    .is_none_or(|(_, at)| at.elapsed() < DOWNLOAD_LINGER)
            });
            changed |= before != self.dl_live.len();
        }
        if changed {
            let list = self.downloads_json();
            self.ui(format!("window.bh&&bh.downloads({list})"));
        }
    }

    /// The navigation of `tab` just turned into a download.
    pub(crate) fn download_took(&self, tab: TabId) -> bool {
        self.dl_navs.get(&tab).is_some_and(|at| at.elapsed() < DOWNLOAD_NAV)
    }

    pub(crate) fn save_downloads(&self) {
        let _ =
            blackhole_infrastructure::store::DownloadStore::new(&self.data).save(&self.downloads);
    }

    /// A navigation became a download: its card in the sidebar, its record, its size asked in the background.
    pub(crate) fn download_started(&mut self, tab: TabId, url: String, path: PathBuf) {
        trace(|| format!("download started tab={} {url}", tab.0));
        // The navigation became a download: the tab is not waiting for a page any more
        // (else "Site injoignable" after CONNECT_MAX). A tab opened for the file alone,
        // that never showed a page, closes once the file is done, as in other browsers.
        if self.awaiting.remove(&tab).is_some() && self.splash.contains_key(&tab) {
            self.dl_only_tabs.insert(url.clone(), tab);
        }
        self.dl_navs.retain(|_, at| at.elapsed() < DOWNLOAD_NAV);
        self.dl_navs.insert(tab, Instant::now());
        if self.aborted.remove(&tab).is_some() {
            self.forget_failure(tab);
        }
        self.loading.remove(&tab);
        self.pending.remove(&tab);
        self.reveal_sidebar(DOWNLOAD_REVEAL);
        let id = self.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let remote = url.clone();
        self.downloads
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
        self.dl_live.insert(
            id,
            DownloadLive {
                name: self
                    .downloads
                    .last()
                    .map(|d| d.name.clone())
                    .unwrap_or_default(),
                path: path.clone(),
                received: 0,
                total: None,
                finished: None,
            },
        );
        self.dl_polled = Instant::now();
        let tx = self.dl_size_tx.clone();
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

    /// A download ended: its record and card updated, the tab opened for it alone closed.
    pub(crate) fn download_done(&mut self, url: String, path: Option<PathBuf>, ok: bool) {
        if let Some(tab) = self.dl_only_tabs.remove(&url) {
            if self.tabs.contains_key(&tab) {
                self.handle_ui(UiCommand::Close { id: tab.0 });
            }
        }
        if let Some(d) = self
            .downloads
            .iter_mut()
            .rev()
            .find(|d| d.url == url && !d.done)
        {
            d.done = true;
            d.ok = ok;
            if let Some(p) = path {
                d.path = p.to_string_lossy().into_owned();
            }
            if let Some(live) = self.dl_live.get_mut(&d.id) {
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
        let (kind, key) = if ok {
            (notices::Notice::Done, "toast.download_complete")
        } else {
            (notices::Notice::Problem, "toast.download_failed")
        };
        self.notify(kind, &t(key), serde_json::Value::Null);
        self.ui_library();
    }
}

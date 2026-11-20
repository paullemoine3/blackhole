//! Downloads: the live cards of the sidebar, their record, opening a finished file.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use blackhole_domain::TabId;
use serde_json::json;

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

/// Size of the file a download is writing: browsers use a temporary name until it completes.
fn partial_size(path: &std::path::Path) -> u64 {
    let size = |p: &std::path::Path| std::fs::metadata(p).map_or(0, |m| m.len());
    let with = |ext: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(ext);
        PathBuf::from(name)
    };
    size(path)
        .max(size(&with(".crdownload")))
        .max(size(&with(".partial")))
        .max(size(&with(".download")))
}

/// Opens a downloaded file with its default app; programs are only revealed, never run.
pub(crate) fn open_download(path: &std::path::Path) {
    let risky = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "exe"
                | "msi"
                | "bat"
                | "cmd"
                | "ps1"
                | "com"
                | "scr"
                | "app"
                | "command"
                | "sh"
                | "jar"
                | "vbs"
                | "js"
                | "lnk"
                | "pkg"
                | "dmg"
        )
    });
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
                let size = partial_size(&d.path);
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
}

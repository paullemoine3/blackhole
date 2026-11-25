//! The Library: media, downloads, easels, history, highlights, watches, archive, and the
//! Shields report (0.8.28; no spaces since 0.8.27: the sidebar already lists them).

use blackhole_domain::TabId;
use serde_json::json;

use crate::platform::now_secs;
use crate::state::Running;

/// The sections of the Library page, in its rail's order.
const LIBRARY_SECTIONS: [&str; 8] =
    ["media", "downloads", "easels", "history", "highlights", "watches", "archive", "shields"];

impl Running {
    /// Everything the Library page shows; it picks the section and filters client-side.
    pub(crate) fn ui_library(&self) {
        if !self.library {
            return;
        }
        let ws = self.svc.workspace();
        let active = ws.active_space();
        let exists = |d: &blackhole_infrastructure::store::DownloadRec| {
            std::path::Path::new(&d.path).exists()
        };
        let downloads: Vec<_> = self.downloads.records.iter().rev().collect();
        let mut thumbs = self.thumbs.borrow_mut();
        let mut budget = 60;
        let media: Vec<_> = downloads
            .iter()
            .filter(|d| d.done && d.ok && blackhole_infrastructure::media_kind(&d.name).is_some())
            .filter(|d| exists(d))
            .map(|d| {
                let kind = blackhole_infrastructure::media_kind(&d.name);
                let thumb = if kind == Some("image") && budget > 0 {
                    budget -= 1;
                    thumbs
                        .entry(d.id)
                        .or_insert_with(|| {
                            blackhole_infrastructure::image_data_url(
                                std::path::Path::new(&d.path),
                                1_500_000,
                            )
                        })
                        .clone()
                } else {
                    None
                };
                json!({"id": d.id, "name": d.name, "kind": kind, "thumb": thumb, "at": d.at})
            })
            .collect();
        let state = json!({
            "accent": active.color,
            "media": media,
            "downloads": downloads.iter().map(|d| json!({
                "acts": self.downloads.acts_of(d.id), "paused": self.downloads.paused(d.id),
                "id": d.id, "name": d.name, "url": d.url, "at": d.at,
                "done": d.done, "ok": d.ok, "exists": exists(d),
            })).collect::<Vec<_>>(),
            "easels": self.easels.all().iter().filter(|e| self.easel_space(e) == active.id.0).map(|e| json!({
                "id": e.id, "name": e.name, "count": e.items.len(),
                "preview": e.items.iter().map(|i| i.text.as_str()).find(|t| !t.is_empty()).unwrap_or(""),
            })).collect::<Vec<_>>(),
            "archive": self.archive.iter().enumerate().rev().map(|(i, a)| json!({
                "index": i, "title": a.title, "url": a.url, "at": a.at,
            })).collect::<Vec<_>>(),
            "history": self.svc.recent_activity(usize::MAX).iter().map(|e| json!({
                "title": e.title, "url": e.url.as_str(), "at": e.visited_at,
            })).collect::<Vec<_>>(),
            "now": now_secs(),
            // A section to show, applied once per request (the page keeps its own choice after).
            "jump": json!([self.library_jump.0, self.library_jump.1]),
            "animations": self.svc.settings().animations,
            "depth": self.svc.settings().depth,
            "watches": self.watches_json(),
            // The Easel ticked in the "Add to an Easel" menus.
            "easelCurrent": self.easel_current,
            // Paired devices: a downloaded file can be sent to one (the Wormhole).
            "devices": self.devices_brief(),
            "highlights": self.marks_json(),
            "shields": self.shields_json(),
        });
        if let Some(panel) = &self.library_panel.view {
            let _ = panel.evaluate_script(&format!("window.bh&&bh.render({state})"));
        }
    }

    pub(crate) fn open_library(&mut self, section: Option<&str>) {
        // Only the Library's own sections: the page is told which one to show.
        if let Some(name) = section.and_then(|s| LIBRARY_SECTIONS.iter().find(|n| **n == s)) {
            self.library_jump = (self.library_jump.0 + 1, name);
        }
        self.close_palette();
        self.close_peek();
        self.close_easel_tab();
        self.settings_shown = false;
        self.library = true;
        let fresh = self.library_panel.view.is_none();
        self.sync();
        self.ui_library();
        // A page still loading plays its intro when it reports `library_ready`.
        if !fresh {
            self.library_intro();
        }
    }

    pub(crate) fn library_intro(&self) {
        if let Some(panel) = &self.library_panel.view {
            let _ = panel.evaluate_script("window.bh&&bh.intro()");
        }
    }

    pub(crate) fn close_library(&mut self) {
        if self.library {
            self.marks_edit_done();
            self.library = false;
            self.settings_shown = false;
            self.sync();
        }
    }

    pub(crate) fn save_archive(&self) {
        let _ = blackhole_infrastructure::store::ArchiveStore::new(&self.data).save(&self.archive);
    }

    /// Remembers a tab that is about to be closed so the Library can bring it back.
    /// Works for any space: deleting a space that is not shown archives its tabs too.
    pub(crate) fn archive_tab(&mut self, id: TabId) {
        let ws = self.svc.workspace();
        let Some((space, tab)) = ws
            .spaces()
            .iter()
            .find_map(|s| s.tabs.iter().find(|t| t.id == id).map(|t| (s, t)))
        else {
            return;
        };
        // A black hole tab leaves nothing behind, not even in the archive.
        if !matches!(tab.url.scheme(), "http" | "https") || ws.is_burner(id) {
            return;
        }
        let rec = blackhole_infrastructure::store::ArchivedTab {
            title: tab.title.clone(),
            url: tab.url.to_string(),
            space: space.id.0,
            at: now_secs(),
            folder: tab.folder.map(|f| f.0),
        };
        self.archive
            .retain(|a| !(a.url == rec.url && a.space == rec.space));
        self.archive.push(rec);
        self.save_archive();
    }
}

//! The one thing just deleted from the Library or an Easel (a highlight, a watched area, an
//! archived tab, a card, a whole Easel), kept `UNDO_FOR` for the toast's Undo. Deleting stays
//! one click; what the click took can come back.

use crate::i18n::t;
use crate::{notices, Running, UNDO_FOR};
use blackhole_domain::highlights::TakenMark;
use blackhole_domain::watch::Watch;
use blackhole_domain::{Easel, EaselItem};
use blackhole_infrastructure::store::ArchivedTab;
use serde_json::json;
use std::time::Instant;

pub(crate) enum Trashed {
    Highlight(TakenMark),
    Watch(usize, Watch),
    Archived(usize, ArchivedTab),
    Card { easel: u64, at: usize, item: EaselItem },
    Easel(usize, Easel),
}

impl Running {
    /// Keeps `what` (replacing what was kept before) and says `message` with an Undo button.
    pub(crate) fn trash(&mut self, message: &str, what: Trashed) {
        self.trashed = Some((Instant::now(), what));
        self.notify(
            notices::Notice::Undo,
            message,
            json!([{"label": t("toast.undo"), "run": {"cmd": "undo_trash"}}]),
        );
    }

    /// The toast's Undo: what was deleted last comes back in its place.
    pub(crate) fn undo_trash(&mut self) {
        let Some((at, what)) = self.trashed.take() else {
            return;
        };
        if at.elapsed() > UNDO_FOR {
            return;
        }
        let back = match what {
            Trashed::Highlight(mark) => self.marks_put_back(mark),
            Trashed::Watch(at, watch) => self.watch_put_back(at, watch),
            Trashed::Archived(at, rec) => {
                let known = self.archive.iter().any(|a| a.url == rec.url && a.space == rec.space);
                if !known {
                    let at = at.min(self.archive.len());
                    self.archive.insert(at, rec);
                    self.save_archive();
                }
                !known
            }
            Trashed::Card { easel, at, item } => {
                let ok = self.easels.put_item_back(easel, at, item);
                self.easel_changed(ok);
                ok
            }
            Trashed::Easel(at, easel) => {
                let ok = self.easels.put_back(at, easel);
                self.easel_changed(ok);
                ok
            }
        };
        if back {
            self.ui_library();
            self.ui_easel();
            self.toast(&t("toast.restored"));
        }
    }
}

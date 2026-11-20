//! Importing bookmarks and passwords from other browsers or files.

use std::collections::HashMap;

use blackhole_application::BrowserService;
use blackhole_infrastructure::{import, JsonWorkspaceRepository};
use blackhole_infrastructure::store::{BookmarkStore, Vault};

use crate::i18n::message;
use crate::state::Running;

/// Result of an import job: message, vault size, and the bookmarks to file as tabs.
pub(crate) type ImportDone = (Option<String>, usize, Vec<import::Bookmark>);

pub(crate) fn file_bookmarks(svc: &mut BrowserService<JsonWorkspaceRepository>, list: Vec<import::Bookmark>) {
    let first = svc.workspace().spaces().first().map_or(0, |s| s.id.0);
    let mut by_space: HashMap<u64, Vec<(String, String, url::Url)>> = HashMap::new();
    for b in list {
        let Ok(u) = url::Url::parse(&b.url) else {
            continue;
        };
        if matches!(u.scheme(), "http" | "https") {
            by_space
                .entry(b.space.unwrap_or(first))
                .or_default()
                .push((b.folder, b.title, u));
        }
    }
    for (space, links) in by_space {
        svc.import_tabs(blackhole_domain::SpaceId(space), links);
    }
}

pub(crate) fn store_bookmarks(
    store: &BookmarkStore,
    mut list: Vec<import::Bookmark>,
    from: &str,
    space: u64,
) -> (String, Vec<import::Bookmark>) {
    list.iter_mut().for_each(|b| b.space = Some(space));
    let total = list.len();
    match store.merge(list.clone()) {
        Ok(new) => (
            message(
                "settings.import_done",
                &[("from", from), ("n", &new.to_string()), ("known", &(total - new).to_string())],
            ),
            list,
        ),
        Err(e) => (message("settings.import_failed", &[("from", from), ("why", &e.to_string())]), Vec::new()),
    }
}

pub(crate) fn import_file(
    bookmarks: &BookmarkStore,
    logins: &std::sync::mpsc::Sender<Vec<import::Login>>,
    kind: &str,
    space: u64,
) -> (String, Vec<import::Bookmark>) {
    let (label, exts): (&str, &[&str]) = if kind == "logins" {
        ("CSV", &["csv"])
    } else {
        ("HTML", &["html", "htm"])
    };
    let Some(path) = rfd::FileDialog::new().add_filter(label, exts).pick_file() else {
        return (String::new(), Vec::new());
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (message("settings.import_unreadable", &[]), Vec::new());
    };
    if kind == "logins" {
        // Added to the wallet on the UI thread, which sets the message.
        let msg = match import::parse_logins_csv(&text) {
            Ok(list) => {
                let _ = logins.send(list);
                String::new()
            }
            Err(e) => e,
        };
        return (msg, Vec::new());
    }
    store_bookmarks(bookmarks, import::parse_html(&text), &message("settings.import_from_html", &[]), space)
}

impl Running {
    /// Imported bookmarks are ordinary tabs (drag & drop, close, folders) filed in folders.
    pub(crate) fn file_bookmarks(&mut self, list: Vec<import::Bookmark>) {
        file_bookmarks(&mut self.svc, list);
    }

    pub(crate) fn store_dropped(&mut self, list: Vec<import::Bookmark>) -> String {
        let space = self.svc.workspace().active_space().id.0;
        let (msg, list) = store_bookmarks(&self.bookmarks, list, &message("settings.import_from_drop", &[]), space);
        self.bookmark_list = self.bookmarks.load();
        self.file_bookmarks(list);
        self.ui_state();
        self.toast(&msg);
        msg
    }

    /// Runs blocking import work (file dialog, Keychain) off the UI thread: modal panels
    /// opened inside the event-loop callback crash AppKit.
    pub(crate) fn spawn_import(
        &self,
        job: impl FnOnce(&BookmarkStore, &Vault) -> Option<(String, Vec<import::Bookmark>)>
            + Send
            + 'static,
    ) {
        let (tx, data) = (self.import_tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let (bookmarks, vault) = (BookmarkStore::new(&data), Vault::new(&data));
            let (msg, list) = match job(&bookmarks, &vault) {
                Some((m, l)) => (Some(m), l),
                None => (None, Vec::new()),
            };
            let _ = tx.send((msg, vault.count(), list));
        });
    }
}

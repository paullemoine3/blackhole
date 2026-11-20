//! Importing bookmarks and passwords from other browsers or files.

use blackhole_application::{BrowserService, ImportedLink};
use blackhole_domain::SpaceId;
use blackhole_infrastructure::{import, JsonWorkspaceRepository};
use blackhole_infrastructure::store::{BookmarkStore, Vault};

use crate::i18n::message;
use crate::ipc::UiCommand;
use crate::state::Running;

/// Result of an import job: message, vault size, and the bookmarks to file as tabs.
pub(crate) type ImportDone = (Option<String>, usize, Vec<import::Bookmark>);

/// Files bookmarks as tabs of their space (`BrowserService::file_links`).
pub(crate) fn file_bookmarks(svc: &mut BrowserService<JsonWorkspaceRepository>, list: Vec<import::Bookmark>) {
    svc.file_links(list.into_iter().map(|b| ImportedLink {
        space: b.space.map(SpaceId),
        folder: b.folder,
        title: b.title,
        url: b.url,
    }));
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

/// Bookmarks kept by the profile, the browsers found to import from, and imports running.
pub(crate) struct Imports {
    pub(crate) store: BookmarkStore,
    /// The bookmarks imported so far (filed as tabs of their space).
    pub(crate) list: Vec<import::Bookmark>,
    /// Browsers and profiles found on this computer.
    pub(crate) sources: Vec<import::Source>,
    /// What the last import said (Settings > Import).
    pub(crate) msg: String,
    pub(crate) tx: std::sync::mpsc::Sender<ImportDone>,
    pub(crate) rx: std::sync::mpsc::Receiver<ImportDone>,
}

impl Imports {
    pub(crate) fn new(data: &std::path::Path) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let store = BookmarkStore::new(data);
        Self {
            list: store.load(),
            store,
            sources: import::detect(),
            msg: String::new(),
            tx,
            rx,
        }
    }
}

impl Running {
    /// Imported bookmarks are ordinary tabs (drag & drop, close, folders) filed in folders.
    pub(crate) fn file_bookmarks(&mut self, list: Vec<import::Bookmark>) {
        file_bookmarks(&mut self.svc, list);
    }

    pub(crate) fn store_dropped(&mut self, list: Vec<import::Bookmark>) -> String {
        let space = self.svc.workspace().active_space().id.0;
        let (msg, list) = store_bookmarks(&self.imports.store, list, &message("settings.import_from_drop", &[]), space);
        self.imports.list = self.imports.store.load();
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
        let (tx, data) = (self.imports.tx.clone(), self.data.clone());
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

impl Running {
    /// Commands that import bookmarks and passwords.
    pub(crate) fn handle_imports(&mut self, command: UiCommand) {
        match command {
            UiCommand::ImportScan => {
                self.imports.sources = import::detect();
                self.ui_settings();
                self.spawn_import(|_, _| None);
            }
            UiCommand::ImportBookmarks { source } => {
                let found = self.imports.sources.iter().find(|s| s.id == source).cloned();
                let space = self.svc.workspace().active_space().id.0;
                self.spawn_import(move |bookmarks, _| {
                    Some(match found {
                        Some(s) => match import::read_bookmarks(&s) {
                            Ok(list) => store_bookmarks(bookmarks, list, &s.browser, space),
                            Err(e) => (message("settings.import_failed", &[("from", &s.browser), ("why", &e.to_string())]), Vec::new()),
                        },
                        None => (message("settings.import_no_source", &[]), Vec::new()),
                    })
                });
            }
            UiCommand::AddBookmark { title, url, folder } => {
                let allowed =
                    url::Url::parse(&url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
                self.imports.msg = if allowed {
                    let title = if title.trim().is_empty() {
                        url.clone()
                    } else {
                        title
                    };
                    self.store_dropped(vec![import::Bookmark {
                        title,
                        url,
                        folder,
                        space: None,
                    }])
                } else {
                    String::new()
                };
            }
            UiCommand::ImportHtml { text } => {
                let list = import::parse_html(&text);
                self.imports.msg = self.store_dropped(list);
            }
            UiCommand::ImportFile { kind } => {
                let space = self.svc.workspace().active_space().id.0;
                let logins = self.passwords.import_sender();
                self.spawn_import(move |bookmarks, _| {
                    Some(import_file(bookmarks, &logins, &kind, space))
                });
            }
            _ => {}
        }
    }
}

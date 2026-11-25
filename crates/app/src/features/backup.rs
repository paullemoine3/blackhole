//! Settings > Import and export: bookmarks and passwords as files other browsers read, and the
//! sealed backup of the profile (`infrastructure::backup`). File dialogs and sealing run off the
//! UI thread (a modal panel inside the event loop crashes AppKit).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use blackhole_infrastructure::{backup, import::Login};
use serde_json::json;
use zeroize::Zeroizing;

use crate::i18n::{self, message};
use crate::ipc::UiCommand;
use crate::shell::windows::private_mode;
use crate::state::Running;

enum Done {
    /// What to say; empty when the user closed the dialog.
    Said(String),
    /// A backup is ready for the next launch, with the passwords it holds.
    Staged(Vec<Login>),
}

pub(crate) struct Backups {
    tx: Sender<Done>,
    rx: Receiver<Done>,
    busy: bool,
    status: String,
    /// A restore waits for the next launch.
    staged: bool,
}

impl Backups {
    pub(crate) fn new() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, busy: false, status: String::new(), staged: false }
    }
}

/// Asks where to save, then writes.
fn save(name: &str, label: &str, extension: &str, bytes: &[u8]) -> String {
    let Some(path) = rfd::FileDialog::new().set_file_name(name).add_filter(label, &[extension]).save_file() else {
        return String::new();
    };
    match backup::write_export(&path, bytes) {
        Ok(()) => said(&path),
        Err(why) => message("msg.export_failed", &[("why", &why)]),
    }
}

fn said(path: &Path) -> String {
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    message("msg.exported", &[("file", &name)])
}

impl Running {
    pub(crate) fn backup_json(&self) -> serde_json::Value {
        json!({
            "busy": self.backups.busy,
            "status": i18n::render(&self.backups.status),
            "staged": self.backups.staged,
            "private": private_mode(),
            "passwords": self.passwords.entries().is_some(),
        })
    }

    fn backup_job(&mut self, job: impl FnOnce(PathBuf) -> Done + Send + 'static) {
        self.backups.busy = true;
        self.backups.status.clear();
        let (tx, data) = (self.backups.tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(data));
        });
        self.ui_settings();
    }

    pub(crate) fn handle_backup(&mut self, command: UiCommand) {
        if self.backups.busy || private_mode() {
            return;
        }
        match command {
            UiCommand::ExportBookmarks => {
                let html = backup::bookmarks_html(self.svc.workspace());
                self.backup_job(move |_| Done::Said(save("BlackHole.html", "HTML", "html", html.as_bytes())));
            }
            UiCommand::ExportPasswords => {
                let Some(csv) = self.passwords.entries().map(backup::logins_csv) else {
                    self.backup_says(message("msg.export_locked", &[]));
                    return;
                };
                self.backup_job(move |_| {
                    Done::Said(match csv {
                        Ok(csv) => save("BlackHole passwords.csv", "CSV", "csv", &csv),
                        Err(why) => message("msg.export_failed", &[("why", &why)]),
                    })
                });
            }
            UiCommand::BackupExport { password } => {
                // A wallet that is locked would leave the passwords out without a word.
                let Some(entries) = self.passwords.entries().map(<[_]>::to_vec) else {
                    self.backup_says(message("msg.export_locked", &[]));
                    return;
                };
                self.svc.flush();
                self.save_downloads();
                let password = Zeroizing::new(password.as_str().to_owned());
                self.backup_job(move |data| {
                    Done::Said(match backup::seal(&data, &entries, &password) {
                        Ok(bytes) => save("BlackHole.bhbackup", "BlackHole", backup::EXTENSION, &bytes),
                        Err(why) => why,
                    })
                });
            }
            UiCommand::BackupRestore { password } => {
                let password = Zeroizing::new(password.as_str().to_owned());
                self.backup_job(move |data| {
                    let Some(path) =
                        rfd::FileDialog::new().add_filter("BlackHole", &[backup::EXTENSION]).pick_file()
                    else {
                        return Done::Said(String::new());
                    };
                    let staged = backup::read(&path, &password).and_then(|b| backup::stage(&data, &b).map(|()| b));
                    match staged {
                        Ok(b) => Done::Staged(b.logins()),
                        Err(why) => Done::Said(why),
                    }
                });
            }
            UiCommand::QuitApp => self.quit = true,
            _ => {}
        }
    }

    fn backup_says(&mut self, message: String) {
        self.backups.status = message;
        self.ui_settings();
    }

    pub(crate) fn service_backup(&mut self) {
        while let Ok(done) = self.backups.rx.try_recv() {
            self.backups.busy = false;
            match done {
                Done::Said(status) => self.backups.status = status,
                Done::Staged(logins) => {
                    self.backups.staged = true;
                    // Passwords are added to the wallet now, like an import (which says when it
                    // is locked): the wallet itself is sealed to this computer.
                    if !logins.is_empty() {
                        let _ = self.passwords.import_sender().send(logins);
                    }
                }
            }
            self.ui_settings();
        }
    }
}

//! The password wallet in the shell: autofill of the page's login fields, the offer to save
//! after a login form was submitted, and Settings › Passwords. The rules are in
//! `domain::passwords`, the encrypted file in `infrastructure::wallet`.
//!
//! A page only ever receives usernames until the user picks one in the menu drawn by
//! `ui/autofill.js` (main frame, exact origin re-read from the engine, https or localhost).
//! Secrets never go to a page otherwise, nor to the logs or a URL; the Settings page (a shell
//! view) gets one password at a time, on an explicit click.

use crate::i18n::{message, t, tf};
use crate::{views, Running, TabId};
use blackhole_domain::passwords::{display_host, origin_of, origin_of_saved, Draft, Entry, Kind, Offer, Recipe};
use blackhole_domain::Settings;
use blackhole_infrastructure::import::Login;
use blackhole_infrastructure::store::Vault;
use blackhole_infrastructure::wallet::{self, OpenError, Protection, Unlocked, Wallet};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

/// A copied password is wiped from the clipboard after this long (if it is still there).
const CLIPBOARD_FOR: Duration = Duration::from_secs(30);
/// The credentials of a submitted form wait this long for "Save".
const OFFER_FOR: Duration = Duration::from_secs(120);

/// A secret received over IPC: wiped when dropped, never printed.
#[derive(Deserialize, Default)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Settings › Passwords and the save toast (shell views only).
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PasswordAction {
    Unlock {
        password: Secret,
    },
    Lock,
    /// Sends the password and notes of entry `id` to the Settings page (show or edit).
    Reveal {
        id: String,
    },
    /// `field`: `username` or `password`.
    Copy {
        id: String,
        field: String,
    },
    /// A new entry (`id` empty) or an edit.
    Save {
        #[serde(default)]
        id: String,
        origin: String,
        #[serde(default)]
        username: String,
        password: Secret,
        #[serde(default)]
        notes: Secret,
    },
    Delete {
        id: String,
    },
    Undo,
    Generate {
        #[serde(default)]
        length: usize,
        #[serde(default)]
        digits: bool,
        #[serde(default)]
        symbols: bool,
    },
    SetMaster {
        password: Secret,
    },
    RemoveMaster,
    LockAfter {
        minutes: u32,
    },
    /// The save toast: `choice` is `save` or `never`.
    Offer {
        tab: u64,
        choice: String,
    },
}

enum Done {
    /// At launch: open with the keychain, or waiting for the master password.
    Started(Result<Option<Unlocked>, String>),
    Unlocked(Result<Unlocked, OpenError>),
    /// The data key was wrapped again (master password set).
    Protected(Box<Unlocked>, Result<(), String>),
    /// Logins read from a CSV export (Import page).
    Imported(Vec<Login>),
}

/// A JSON string literal of `s`, wiped when dropped.
fn quoted(s: &str) -> Zeroizing<String> {
    Zeroizing::new(serde_json::to_string(s).unwrap_or_default())
}

struct Offered {
    origin: String,
    ids: Vec<String>,
    fresh: bool,
    locked: bool,
    /// One-time proof that a message answers this menu (see `autofill.js`).
    token: String,
}

struct Pending {
    origin: String,
    username: String,
    password: Zeroizing<String>,
    update: Option<String>,
    at: Instant,
}

pub struct Passwords {
    wallet: Wallet,
    /// A private window reads the profile's wallet but never writes it (another process does).
    read_only: bool,
    open: Option<Unlocked>,
    protection: Option<Protection>,
    used: Instant,
    busy: bool,
    status: String,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    offered: HashMap<TabId, Offered>,
    pending: HashMap<TabId, Pending>,
    undo: Option<Entry>,
    clipboard: Option<(Instant, Zeroizing<String>)>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl Passwords {
    /// Opens the profile's wallet in the background (migrating `vault.bin` the first time).
    pub fn new(profile: &Path, read_only: bool) -> Self {
        let (tx, rx) = channel();
        let wallet = Wallet::new(profile);
        let (w, vault, start) = (wallet.clone(), Vault::new(profile), tx.clone());
        std::thread::spawn(move || {
            let opened = if read_only {
                match w.protection() {
                    Ok(Some(Protection::Keychain)) => w.open(None).map(Some).map_err(|e| e.to_string()),
                    Ok(_) => Ok(None),
                    Err(e) => Err(e),
                }
            } else {
                wallet::open_at_start(&w, &vault, now_ms())
            };
            let _ = start.send(Done::Started(opened));
        });
        Self {
            wallet,
            read_only,
            open: None,
            protection: None,
            used: Instant::now(),
            busy: true,
            status: String::new(),
            tx,
            rx,
            offered: HashMap::new(),
            pending: HashMap::new(),
            undo: None,
            clipboard: None,
        }
    }

    /// For the Import page: logins parsed from a CSV file come back through here.
    pub fn import_sender(&self) -> Sender<Vec<Login>> {
        let (tx, rx) = channel::<Vec<Login>>();
        let done = self.tx.clone();
        // Imports are rare: a short-lived relay keeps `Done` private.
        std::thread::spawn(move || {
            if let Ok(list) = rx.recv() {
                let _ = done.send(Done::Imported(list));
            }
        });
        tx
    }

    fn lock(&mut self) {
        self.open = None;
        self.offered.clear();
        self.pending.clear();
        self.undo = None;
    }

    fn persist(&mut self) {
        if self.read_only {
            return;
        }
        if let Some(u) = &self.open {
            match u.seal() {
                Ok(text) => blackhole_infrastructure::write_later(self.wallet.path().to_owned(), text),
                Err(e) => self.status = e,
            }
        }
    }

    fn touch(&mut self) {
        self.used = Instant::now();
    }

    /// The state shown by Settings › Passwords: never a password or a note.
    pub fn state_json(&self, settings: &Settings) -> Value {
        let state = if self.busy {
            "busy"
        } else if self.open.is_some() {
            "open"
        } else if self.protection == Some(Protection::Password) {
            "locked"
        } else {
            "error"
        };
        let entries: Vec<Value> = self
            .open
            .as_ref()
            .map(|u| {
                u.data
                    .live()
                    .into_iter()
                    .map(|e| {
                        json!({
                            "id": e.id, "origin": e.origin, "host": display_host(&e.origin),
                            "username": e.username,
                            "realm": match &e.kind { Kind::Web => None, Kind::HttpAuth { realm } => Some(realm) },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        json!({
            "state": state,
            "master": self.protection == Some(Protection::Password),
            "readOnly": self.read_only,
            "status": crate::i18n::render(&self.status),
            "lockMinutes": settings.password_lock_minutes,
            "lockChoices": Settings::LOCK_CHOICES,
            "entries": entries,
        })
    }

    pub fn count(&self) -> usize {
        self.open.as_ref().map_or(0, |u| u.data.live().len())
    }
}

#[cfg(windows)]
fn copy_text(text: &str, secret: bool) -> Result<(), arboard::Error> {
    use arboard::SetExtWindows;
    let mut clipboard = arboard::Clipboard::new()?;
    let set = clipboard.set();
    if secret {
        set.exclude_from_history().exclude_from_cloud().text(text)
    } else {
        set.text(text)
    }
}

#[cfg(target_os = "macos")]
fn copy_text(text: &str, secret: bool) -> Result<(), arboard::Error> {
    use arboard::SetExtApple;
    let mut clipboard = arboard::Clipboard::new()?;
    let set = clipboard.set();
    if secret {
        set.exclude_from_history().text(text)
    } else {
        set.text(text)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn copy_text(text: &str, _secret: bool) -> Result<(), arboard::Error> {
    arboard::Clipboard::new()?.set_text(text)
}

/// Wipes the clipboard if it still holds `text` (the user may have copied something since).
fn clear_clipboard(text: &str) {
    if let Ok(mut c) = arboard::Clipboard::new() {
        if c.get_text().is_ok_and(|mut now| {
            let same = now == text;
            now.zeroize();
            same
        }) {
            let _ = c.clear();
        }
    }
}

impl Running {
    /// The exact origin of what the tab shows, read from the engine (never from the page).
    fn page_origin(&self, tab: TabId) -> Option<String> {
        let raw = self.tabs.get(&tab)?.url().ok()?;
        origin_of(&Url::parse(&raw).ok()?)
    }

    fn foreground(&self, tab: TabId) -> bool {
        let active = self.svc.workspace().active_tab().map(|t| t.id);
        Some(tab) == active || Some(tab) == self.split_tab()
    }

    /// A login field got focus: offer the usernames saved for exactly this origin. `fresh`: a
    /// new-password field, where a generated password is offered too.
    pub(crate) fn autofill_query(&mut self, tab: TabId, fresh: bool) {
        if !self.foreground(tab) {
            return;
        }
        // A loaded password-manager extension owns the login fields: two menus would collide.
        let manager_active = self.webext.as_ref().is_some_and(|h| {
            matches!(
                h.state("bitwarden"),
                Some(blackhole_macos_webext::LoadState::Loaded)
            )
        });
        let origin = if manager_active { None } else { self.page_origin(tab) };
        let p = &mut self.passwords;
        let (names, ids, locked) = match (&origin, &p.open) {
            (None, _) => (Vec::new(), Vec::new(), false),
            (Some(o), Some(u)) => {
                let list = u.data.for_origin(o);
                (
                    list.iter().map(|e| e.username.clone()).collect::<Vec<_>>(),
                    list.iter().map(|e| e.id.clone()).collect(),
                    false,
                )
            }
            (Some(_), None) => (Vec::new(), Vec::new(), p.protection == Some(Protection::Password) && !p.busy),
        };
        let generate = fresh && origin.is_some() && p.open.is_some() && !p.read_only;
        let token = wallet::new_id();
        if let Some(view) = self.tabs.get(&tab) {
            views::autofill_show(
                view,
                &json!({"names": names, "locked": locked, "generate": generate, "token": token}),
            );
        }
        match origin {
            Some(origin) => {
                let offered = Offered { origin, ids, fresh: generate, locked, token };
                p.offered.insert(tab, offered);
            }
            None => {
                p.offered.remove(&tab);
            }
        }
    }

    /// The last menu of the tab, once, if `token` is the one it carried and the tab still
    /// shows the same origin: a page script cannot answer the menu in the user's place.
    fn take_offer(&mut self, tab: TabId, token: &str) -> Option<Offered> {
        let origin = self.page_origin(tab)?;
        let offered = self.passwords.offered.remove(&tab)?;
        (!token.is_empty() && offered.token == token && offered.origin == origin).then_some(offered)
    }

    /// The user picked the `index`-th username of the menu: fill it with its password.
    pub(crate) fn autofill_fill(&mut self, tab: TabId, index: usize, token: &str) {
        let Some(offered) = self.take_offer(tab, token) else { return };
        let p = &mut self.passwords;
        let entry = offered
            .ids
            .get(index)
            .and_then(|id| p.open.as_ref()?.data.get(id))
            .filter(|e| e.origin == offered.origin);
        if let (Some(e), Some(view)) = (entry, self.tabs.get(&tab)) {
            views::autofill_fill(view, &e.username, &e.password);
            p.used = Instant::now();
        }
    }

    /// "Unlock saved passwords…" in the page's menu: Settings › Passwords asks for the master
    /// password (never the page).
    pub(crate) fn autofill_unlock(&mut self, tab: TabId, token: &str) {
        if self.take_offer(tab, token).is_some_and(|o| o.locked) {
            self.settings_tab = Some("passwords".into());
            self.settings_seq += 1;
            self.settings_wanted = true;
        }
    }

    /// "Use a strong password" on a new-password field: generated here, written into the page.
    pub(crate) fn autofill_generate(&mut self, tab: TabId, token: &str) {
        if !self.take_offer(tab, token).is_some_and(|o| o.fresh) {
            return;
        }
        let password = Zeroizing::new(wallet::random_password(Recipe::default()));
        if let Some(view) = self.tabs.get(&tab) {
            views::autofill_generated(view, &password);
        }
    }

    /// A login form was submitted with these credentials (the page already has them): offer
    /// to save or update them in the sidebar.
    pub(crate) fn login_submitted(&mut self, tab: TabId, username: &str, password: &Secret) {
        let Some(origin) = self.page_origin(tab) else { return };
        let p = &mut self.passwords;
        let Some(u) = p.open.as_ref().filter(|_| !p.read_only) else { return };
        let username: String = username.chars().take(256).collect();
        let (update, verb) = match u.data.offer(&origin, &username, password.as_str()) {
            Offer::Nothing => return,
            Offer::Save => (None, false),
            Offer::Update { id } => (Some(id), true),
        };
        p.pending.insert(
            tab,
            Pending {
                origin: origin.clone(),
                username: username.clone(),
                password: Zeroizing::new(password.as_str().to_owned()),
                update,
                at: Instant::now(),
            },
        );
        let host = display_host(&origin);
        let message = match (verb, username.is_empty()) {
            (false, true) => tf("toast.pw_save", &[("host", &host)]),
            (false, false) => tf("toast.pw_save_for", &[("host", &host), ("user", &username)]),
            (true, true) => tf("toast.pw_update", &[("host", &host)]),
            (true, false) => tf("toast.pw_update_for", &[("host", &host), ("user", &username)]),
        };
        let run = |choice: &str| json!({"cmd": "passwords", "action": {"op": "offer", "tab": tab.0, "choice": choice}});
        let actions = json!([
            {"label": t(if verb { "toast.pw_update_button" } else { "toast.pw_save_button" }), "run": run("save")},
            {"label": t("toast.pw_never"), "run": run("never")},
        ]);
        self.ui(format!("window.bh&&bh.toast({},{actions})", json!(message)));
    }

    fn settings_js(&self, js: &str) {
        if let Some(win) = &self.settings {
            let _ = win.view.evaluate_script(js);
        }
    }

    /// Background work finished, the lock delay, the clipboard: called from the event loop.
    pub(crate) fn poll_passwords(&mut self) {
        let mut changed = false;
        while let Ok(done) = self.passwords.rx.try_recv() {
            changed = true;
            self.password_done(done);
        }
        let minutes = self.svc.settings().password_lock_minutes;
        let p = &mut self.passwords;
        if p.open.is_some()
            && p.protection == Some(Protection::Password)
            && minutes > 0
            && p.used.elapsed() >= Duration::from_secs(u64::from(minutes) * 60)
        {
            p.lock();
            p.status = message("settings.pw_locked_idle", &[]);
            changed = true;
        }
        p.pending.retain(|_, x| x.at.elapsed() < OFFER_FOR);
        if p.clipboard.as_ref().is_some_and(|(at, _)| at.elapsed() >= CLIPBOARD_FOR) {
            if let Some((_, text)) = p.clipboard.take() {
                clear_clipboard(&text);
            }
        }
        if changed {
            self.ui_settings();
        }
    }

    fn password_done(&mut self, done: Done) {
        let p = &mut self.passwords;
        match done {
            Done::Started(result) => {
                p.busy = false;
                match result {
                    Ok(Some(mut u)) => {
                        p.protection = Some(u.protection());
                        u.data.purge_tombstones(now_ms());
                        p.open = Some(u);
                    }
                    Ok(None) => p.protection = p.wallet.protection().ok().flatten(),
                    Err(e) => p.status = message("settings.pw_unavailable", &[("why", &e)]),
                }
            }
            Done::Unlocked(result) => {
                p.busy = false;
                match result {
                    Ok(mut u) => {
                        u.data.purge_tombstones(now_ms());
                        p.open = Some(u);
                        p.status.clear();
                        p.touch();
                    }
                    Err(e) => p.status = e.to_string(),
                }
            }
            Done::Protected(u, result) => {
                p.busy = false;
                p.protection = Some(u.protection());
                p.open = Some(*u);
                p.touch();
                match result {
                    Ok(()) => {
                        p.status = message("settings.pw_master_set", &[]);
                        p.persist();
                    }
                    Err(e) => p.status = e,
                }
            }
            Done::Imported(mut list) => {
                let device = p.open.as_ref().map(|u| u.device().to_owned());
                let message = match (p.open.as_mut(), device) {
                    (Some(u), Some(device)) if !p.read_only => {
                        let logins: Vec<(&str, &str, &str)> = list
                            .iter()
                            .map(|l| (l.url.as_str(), l.username.as_str(), l.password.as_str()))
                            .collect();
                        let added = u.data.import(&logins, wallet::new_id, now_ms(), &device);
                        message(
                            "settings.pw_imported",
                            &[("n", &added.to_string()), ("skipped", &(list.len() - added).to_string())],
                        )
                    }
                    _ => message("settings.pw_unlock_first", &[]),
                };
                for l in &mut list {
                    l.password.zeroize();
                }
                p.persist();
                self.import_msg = message;
            }
        }
    }

    pub(crate) fn password_action(&mut self, action: PasswordAction) {
        let now = now_ms();
        match action {
            PasswordAction::Unlock { password } => {
                let p = &mut self.passwords;
                if p.busy || p.open.is_some() {
                    return;
                }
                p.busy = true;
                p.status.clear();
                blackhole_infrastructure::flush_writes();
                let (w, tx) = (p.wallet.clone(), p.tx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Done::Unlocked(w.open(Some(password.as_str()))));
                });
            }
            PasswordAction::Lock => {
                let p = &mut self.passwords;
                if p.protection == Some(Protection::Password) {
                    p.lock();
                    p.status = message("settings.pw_locked", &[]);
                }
            }
            PasswordAction::Reveal { id } => {
                let found = self.passwords.open.as_ref().and_then(|u| u.data.get(&id)).map(|e| {
                    Zeroizing::new(format!(
                        "window.bh&&bh.passwordSecret({{\"id\":{},\"password\":{},\"notes\":{}}})",
                        quoted(&e.id).as_str(),
                        quoted(&e.password).as_str(),
                        quoted(&e.notes).as_str()
                    ))
                });
                if let Some(js) = found {
                    self.passwords.touch();
                    self.settings_js(&js);
                }
                return;
            }
            PasswordAction::Copy { id, field } => {
                let p = &mut self.passwords;
                let Some(e) = p.open.as_ref().and_then(|u| u.data.get(&id)) else { return };
                let secret = field == "password";
                let text = Zeroizing::new(if secret { e.password.clone() } else { e.username.clone() });
                match copy_text(&text, secret) {
                    Ok(()) => {
                        p.status = if secret {
                            message("settings.pw_copied", &[])
                        } else {
                            message("settings.pw_user_copied", &[])
                        };
                        if secret {
                            p.clipboard = Some((Instant::now(), text));
                        }
                    }
                    Err(e) => p.status = message("settings.pw_copy_failed", &[("why", &e.to_string())]),
                }
                p.touch();
            }
            PasswordAction::Save { id, origin, username, password, notes } => {
                let p = &mut self.passwords;
                let typed = origin.trim();
                let with_scheme = if typed.contains("://") { typed.to_owned() } else { format!("https://{typed}") };
                let Some(origin) = origin_of_saved(&with_scheme) else {
                    p.status = message("settings.pw_enter_site", &[]);
                    return self.ui_settings();
                };
                let (Some(u), false) = (p.open.as_mut(), p.read_only) else { return };
                let device = u.device().to_owned();
                let kind = u.data.get(&id).map_or(Kind::Web, |e| e.kind.clone());
                let draft = Draft {
                    origin: &origin,
                    username: username.trim(),
                    password: password.as_str(),
                    notes: notes.as_str(),
                    kind,
                };
                if id.is_empty() {
                    u.data.add(wallet::new_id(), &draft, now, &device);
                } else if !u.data.edit(&id, &draft, now, &device) {
                    return;
                }
                p.status = message("settings.pw_saved", &[]);
                p.touch();
                p.persist();
            }
            PasswordAction::Delete { id } => {
                let p = &mut self.passwords;
                let (Some(u), false) = (p.open.as_mut(), p.read_only) else { return };
                let device = u.device().to_owned();
                let Some(removed) = u.data.remove(&id, now, &device) else { return };
                let host = display_host(&removed.origin);
                p.undo = Some(removed);
                p.status.clear();
                p.touch();
                p.persist();
                self.toast_action(&tf("toast.pw_deleted", &[("host", &host)]), &t("toast.undo"), json!({"cmd": "passwords", "action": {"op": "undo"}}));
            }
            PasswordAction::Undo => {
                let p = &mut self.passwords;
                let (Some(entry), Some(u)) = (p.undo.take(), p.open.as_mut()) else { return };
                let device = u.device().to_owned();
                u.data.restore(entry, now, &device);
                p.persist();
            }
            PasswordAction::Generate { length, digits, symbols } => {
                let recipe = Recipe { length: if length == 0 { 20 } else { length }, digits, symbols };
                let password = Zeroizing::new(wallet::random_password(recipe));
                let js = Zeroizing::new(format!("window.bh&&bh.passwordGenerated({})", quoted(&password).as_str()));
                self.settings_js(&js);
                return;
            }
            PasswordAction::SetMaster { password } => {
                let p = &mut self.passwords;
                if p.busy || p.read_only {
                    return;
                }
                if password.as_str().chars().count() < 8 {
                    p.status = message("msg.master_too_short", &[]);
                    return self.ui_settings();
                }
                let Some(mut u) = p.open.take() else { return };
                p.busy = true;
                p.status = message("settings.pw_protecting", &[]);
                let tx = p.tx.clone();
                std::thread::spawn(move || {
                    let result = u.protect_with_password(password.as_str());
                    let _ = tx.send(Done::Protected(Box::new(u), result));
                });
            }
            PasswordAction::RemoveMaster => {
                let p = &mut self.passwords;
                let (Some(u), false) = (p.open.as_mut(), p.read_only) else { return };
                match u.protect_with_keychain(&p.wallet) {
                    Ok(()) => {
                        p.protection = Some(Protection::Keychain);
                        p.status = message("settings.pw_master_removed", &[]);
                        p.persist();
                    }
                    Err(e) => p.status = e,
                }
            }
            PasswordAction::LockAfter { minutes } => self.svc.set_password_lock(minutes),
            PasswordAction::Offer { tab, choice } => {
                let p = &mut self.passwords;
                let Some(pending) = p.pending.remove(&TabId(tab)) else { return };
                let (Some(u), false) = (p.open.as_mut(), p.read_only) else { return };
                let device = u.device().to_owned();
                if choice == "never" {
                    u.data.never_for(&pending.origin);
                } else if let Some(id) = &pending.update {
                    u.data.set_password(id, &pending.password, now, &device);
                } else {
                    let draft = Draft {
                        origin: &pending.origin,
                        username: &pending.username,
                        password: &pending.password,
                        notes: "",
                        kind: Kind::Web,
                    };
                    u.data.add(wallet::new_id(), &draft, now, &device);
                }
                p.touch();
                p.persist();
                if choice != "never" {
                    self.toast(&t("toast.pw_saved"));
                }
            }
        }
        self.ui_settings();
    }
}

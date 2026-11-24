//! Email aliases: Settings › Email aliases, and "Create an alias for this site" in the menu of
//! the email fields (`ui/autofill.js`). The rules are in `domain::aliases`, the services' APIs in
//! `infrastructure::aliases`; the account's key and the aliases are in the password wallet.
//!
//! Nothing leaves before a service is connected; after that, only on a click (the menu entry,
//! with its one-time token, or a button of the Settings). The key never goes to a page.

use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};

use blackhole_domain::aliases::{Account, Alias, Aliases as Kept, Provider};
use blackhole_domain::passwords::display_host;
use blackhole_domain::{Settings, TabId};
use blackhole_infrastructure::{aliases as api, wallet};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::features::passwords::{copy_text, now_ms, Secret};
use crate::i18n::{message, render, t, tf};
use crate::shell::notices::Notice;
use crate::state::Running;
use crate::views;

/// Settings › Email aliases and the notice of a new alias (shell views only).
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AliasAction {
    /// Checks the key with the service, then keeps it. `server`: empty for the service's own.
    Connect {
        provider: String,
        #[serde(default)]
        server: String,
        key: Secret,
    },
    /// The kept key, checked again.
    Test,
    Disconnect,
    Suggest {
        on: bool,
    },
    ShareSite {
        on: bool,
    },
    Enable {
        id: String,
        on: bool,
    },
    /// On the service too.
    Delete {
        id: String,
    },
    /// Only here: the service of that alias is not the one connected.
    Forget {
        id: String,
    },
    Copy {
        id: String,
    },
    /// The service's page where the key is found, in a tab.
    KeyPage {
        provider: String,
    },
}

enum Done {
    /// The key was checked (`None`: the kept one, nothing to save).
    Checked(Option<Account>, Result<(), String>),
    Created {
        tab: TabId,
        token: String,
        origin: String,
        account: Account,
        result: Result<api::Made, String>,
    },
    Enabled(String, bool, Result<(), String>),
    Deleted(String, Result<(), String>),
}

pub struct Aliases {
    tx: Sender<Done>,
    rx: Receiver<Done>,
    /// The key is being checked.
    checking: bool,
    status: String,
    /// The last status says why something did not happen.
    failed: bool,
    /// Tabs with an alias being made: one at a time.
    making: HashSet<TabId>,
    /// Aliases a service is changing.
    busy: HashSet<String>,
    /// Made or changed while the wallet was closed: written as soon as it opens.
    unsaved: Vec<Alias>,
}

impl Aliases {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            checking: false,
            status: String::new(),
            failed: false,
            making: HashSet::new(),
            busy: HashSet::new(),
            unsaved: Vec::new(),
        }
    }

    fn say(&mut self, text: String, failed: bool) {
        self.status = text;
        self.failed = failed;
    }

    /// The state shown by Settings › Email aliases: never the key.
    pub fn state_json(&self, kept: Option<&Kept>, settings: &Settings) -> Value {
        let account = kept.and_then(|k| k.account.as_ref());
        json!({
            "open": kept.is_some(),
            "providers": Provider::ALL.map(|p| json!({
                "id": p.id(), "name": p.name(), "server": p.default_server(), "selfHosted": p.self_hosted(),
            })),
            "connected": account.map(|a| json!({
                "id": a.provider.id(), "name": a.provider.name(), "server": a.server,
            })),
            "checking": self.checking,
            "status": render(&self.status),
            "failed": self.failed,
            "suggest": settings.alias_suggest,
            "shareSite": settings.alias_share_site,
            "list": kept.map(|k| k.sorted().into_iter().map(|a| json!({
                "id": a.id, "host": display_host(&a.origin), "email": a.email, "enabled": a.enabled,
                "service": a.provider.name(), "reachable": k.reaches(a), "busy": self.busy.contains(&a.id),
            })).collect::<Vec<_>>()).unwrap_or_default(),
        })
    }
}

impl Running {
    /// Settings › Passwords, with Settings › Email aliases (both live in the wallet).
    pub(crate) fn passwords_json(&self, settings: &Settings) -> Value {
        let mut state = self.passwords.state_json(settings);
        state["aliases"] = self.aliases.state_json(self.passwords.aliases(), settings);
        state
    }

    /// The user picked "Create an alias for this site" in the menu of `tab` (`token` is checked
    /// by the caller).
    pub(crate) fn make_alias(&mut self, tab: TabId, origin: String, token: String) {
        let Some(account) = self.passwords.aliases().and_then(|a| a.account.clone()) else { return };
        if !self.aliases.making.insert(tab) {
            return;
        }
        let site = self.svc.settings().alias_share_site.then(|| display_host(&origin));
        let tx = self.aliases.tx.clone();
        std::thread::spawn(move || {
            let result = api::create(&account, site.as_deref());
            let _ = tx.send(Done::Created { tab, token, origin, account, result });
        });
    }

    /// Answers of the services: called from the event loop.
    pub(crate) fn poll_aliases(&mut self) {
        let mut changed = false;
        if !self.aliases.unsaved.is_empty() && self.passwords.aliases().is_some() {
            for alias in std::mem::take(&mut self.aliases.unsaved) {
                self.passwords.edit_aliases(|a| a.add(alias));
            }
            changed = true;
        }
        while let Ok(done) = self.aliases.rx.try_recv() {
            changed = true;
            self.alias_done(done);
        }
        if changed {
            self.ui_settings();
        }
    }

    fn alias_done(&mut self, done: Done) {
        match done {
            Done::Checked(account, result) => {
                self.aliases.checking = false;
                match (result, account) {
                    (Err(why), _) => self.aliases.say(why, true),
                    (Ok(()), None) => self.aliases.say(message("settings.alias_key_ok", &[]), false),
                    (Ok(()), Some(account)) => {
                        let provider = account.provider;
                        if self.passwords.edit_aliases(|a| a.account = Some(account)) {
                            self.svc.set_alias_provider(provider.id());
                            self.aliases.say(message("settings.alias_connected", &[("name", provider.name())]), false);
                        } else {
                            self.aliases.say(message("msg.passwords_locked", &[]), true);
                        }
                    }
                }
            }
            Done::Created { tab, token, origin, account, result } => {
                self.aliases.making.remove(&tab);
                let same_page = self.page_origin(tab).as_deref() == Some(origin.as_str());
                let made = match result {
                    Ok(made) => made,
                    Err(why) => {
                        if let Some(view) = self.tabs.get(&tab) {
                            views::autofill_alias(view, &token, "");
                        }
                        let text = tf("toast.alias_failed", &[("why", &render(&why))]);
                        return self.notify(Notice::Problem, &text, Value::Null);
                    }
                };
                if let Some(view) = self.tabs.get(&tab).filter(|_| same_page) {
                    views::autofill_alias(view, &token, &made.email);
                }
                let host = display_host(&origin);
                let id = wallet::new_id();
                let alias = Alias {
                    id: id.clone(),
                    provider: account.provider,
                    server: account.server.clone(),
                    remote: made.remote,
                    email: made.email,
                    origin,
                    enabled: true,
                    created: now_ms(),
                };
                let kept = alias.clone();
                if !self.passwords.edit_aliases(|a| a.add(kept)) {
                    self.aliases.unsaved.push(alias);
                }
                self.passwords.touch();
                self.notify_action(
                    Notice::Password,
                    &tf("toast.alias_made", &[("host", &host)]),
                    &t("toast.undo"),
                    json!({"cmd": "aliases", "action": {"op": "delete", "id": id}}),
                );
            }
            Done::Enabled(id, on, result) => {
                self.aliases.busy.remove(&id);
                match result {
                    Ok(()) => {
                        // Locked meanwhile: the list says the older state until it is switched again.
                        self.passwords.edit_aliases(|a| a.set_enabled(&id, on));
                        self.aliases.say(String::new(), false);
                    }
                    Err(why) => self.aliases.say(why, true),
                }
            }
            Done::Deleted(id, result) => {
                self.aliases.busy.remove(&id);
                match result {
                    Ok(()) => {
                        let mut gone = None;
                        self.passwords.edit_aliases(|a| gone = a.remove(&id));
                        // Undo right after it was written: a form is not left with a dead address.
                        if let Some(alias) = gone {
                            for view in self.tabs.values() {
                                views::autofill_alias_gone(view, &alias.email);
                            }
                        }
                        self.aliases.say(String::new(), false);
                        self.notify(Notice::Done, &t("toast.alias_deleted"), Value::Null);
                    }
                    Err(why) => {
                        let text = tf("toast.alias_failed", &[("why", &render(&why))]);
                        self.notify(Notice::Problem, &text, Value::Null);
                        self.aliases.say(why, true);
                    }
                }
            }
        }
    }

    /// The alias `id` with the account that made it, unless the service is already changing it.
    fn alias_target(&mut self, id: &str) -> Option<(Account, Alias)> {
        let kept = self.passwords.aliases()?;
        let alias = kept.get(id)?.clone();
        let account = kept.account.clone().filter(|_| kept.reaches(&alias))?;
        (!self.passwords.read_only() && self.aliases.busy.insert(id.to_owned())).then_some((account, alias))
    }

    pub(crate) fn alias_action(&mut self, action: AliasAction) {
        match action {
            AliasAction::Connect { provider, server, key } => {
                let Some(provider) = Provider::from_id(&provider) else { return };
                if self.aliases.checking || self.passwords.read_only() {
                    return;
                }
                let typed = key.as_str().trim();
                if self.passwords.aliases().is_none() {
                    self.aliases.say(message("msg.passwords_locked", &[]), true);
                } else if typed.is_empty() || !typed.chars().all(|c| c.is_ascii_graphic()) {
                    self.aliases.say(message("settings.alias_enter_key", &[]), true);
                } else if let Some(server) = provider.server(&server) {
                    let account = Account { provider, server, key: typed.to_owned() };
                    self.aliases.checking = true;
                    self.aliases.say(String::new(), false);
                    let tx = self.aliases.tx.clone();
                    std::thread::spawn(move || {
                        let result = api::check(&account);
                        let _ = tx.send(Done::Checked(Some(account), result));
                    });
                } else {
                    self.aliases.say(message("settings.alias_bad_server", &[]), true);
                }
            }
            AliasAction::Test => {
                let Some(account) = self.passwords.aliases().and_then(|a| a.account.clone()) else { return };
                if self.aliases.checking {
                    return;
                }
                self.aliases.checking = true;
                self.aliases.say(String::new(), false);
                let tx = self.aliases.tx.clone();
                std::thread::spawn(move || {
                    let result = api::check(&account);
                    let _ = tx.send(Done::Checked(None, result));
                });
            }
            AliasAction::Disconnect => {
                if self.passwords.edit_aliases(|a| a.account = None) {
                    self.svc.set_alias_provider("");
                    self.aliases.say(message("settings.alias_disconnected", &[]), false);
                }
            }
            AliasAction::Suggest { on } => self.svc.set_alias_suggest(on),
            AliasAction::ShareSite { on } => self.svc.set_alias_share_site(on),
            AliasAction::Enable { id, on } => {
                let Some((account, alias)) = self.alias_target(&id) else { return };
                let tx = self.aliases.tx.clone();
                std::thread::spawn(move || {
                    let result = api::set_enabled(&account, &alias.remote, on);
                    let _ = tx.send(Done::Enabled(id, on, result));
                });
            }
            AliasAction::Delete { id } => {
                let Some((account, alias)) = self.alias_target(&id) else { return };
                let tx = self.aliases.tx.clone();
                std::thread::spawn(move || {
                    let result = api::delete(&account, &alias.remote);
                    let _ = tx.send(Done::Deleted(id, result));
                });
            }
            AliasAction::Forget { id } => {
                self.passwords.edit_aliases(|a| {
                    a.remove(&id);
                });
            }
            AliasAction::KeyPage { provider } => {
                if let Some(p) = Provider::from_id(&provider) {
                    self.handle_ui(crate::ipc::UiCommand::Open { input: p.key_page().to_owned(), via: None });
                }
                return;
            }
            AliasAction::Copy { id } => {
                let Some(email) = self.passwords.aliases().and_then(|a| a.get(&id)).map(|a| a.email.clone()) else {
                    return;
                };
                match copy_text(&email, false) {
                    Ok(()) => self.aliases.say(message("settings.alias_copied", &[]), false),
                    Err(e) => self.aliases.say(message("settings.pw_copy_failed", &[("why", &e.to_string())]), true),
                }
            }
        }
        self.ui_settings();
    }
}

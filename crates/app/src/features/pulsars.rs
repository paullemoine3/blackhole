//! Pulsars in the shell: the folders of the sidebar fed by a site, their form, and the reading of
//! their feeds (one at a time, off the UI thread). Rules in `domain::pulsar`, network and file in
//! `infrastructure::feeds`. Off (Settings > General) or in a private window: nothing is shown,
//! nothing is asked.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use blackhole_domain::pulsar::{self, GithubKind, GithubQuery, Pulsars};
use blackhole_infrastructure::feeds::{self, Answer, PulsarStore};
use blackhole_infrastructure::github_issues::{self as github, Account, AccountStore, Failed};
use serde_json::{json, Value};
use url::Url;

use crate::i18n::{self, message, t, tf, tn};
use crate::ipc::UiCommand;
use crate::platform::now_secs;
use crate::shell::notices::Notice;
use crate::shell::windows::private_mode;
use crate::state::Running;

/// How often the feeds that are due are looked for.
const LOOK_EVERY: Duration = Duration::from_secs(5);
/// Nothing is asked while the app starts.
const FIRST_LOOK: Duration = Duration::from_secs(12);

enum Done {
    Read(u64, Result<Answer, String>),
    /// The form numbered so: the feed found for it, for a space.
    Found(u64, u64, Box<Result<(Url, Answer), String>>),
    Github(u64, Result<Vec<pulsar::Entry>, Failed>),
    /// A token tried: the account it opens.
    Connected(Result<Account, String>),
    /// At launch: the account kept, if any.
    Account(Option<Account>),
}

/// The form of a new Pulsar (palette).
#[derive(Default)]
struct Form {
    seq: u64,
    prefill: String,
    busy: bool,
    error: String,
}

pub(crate) struct Pulsing {
    list: Pulsars,
    store: PulsarStore,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    reading: Option<u64>,
    /// The one being read was asked by hand: the sidebar says when nothing came.
    by_hand: bool,
    /// Asked by hand, before the ones that are due.
    wanted: Vec<u64>,
    look_at: Instant,
    form: Form,
    github: Github,
}

/// The GitHub account, and what the Settings say of it.
struct Github {
    store: std::sync::Arc<AccountStore>,
    account: Option<Account>,
    busy: bool,
    status: String,
    /// Where a token is authorized for an organization that asks for its sign-on.
    sso: Option<Url>,
    /// What GitHub last answered to a Pulsar it refused.
    why: std::collections::HashMap<u64, String>,
}

/// GitHub's API; the scripted runs of the dev build have one of their own on this machine.
fn api() -> String {
    #[cfg(feature = "devtools")]
    if let Some(local) = std::env::var("BH_GITHUB_API").ok().filter(|u| u.starts_with("http://127.0.0.1:")) {
        return local;
    }
    github::API.to_owned()
}

impl Pulsing {
    pub(crate) fn new(data: &std::path::Path) -> Self {
        let (tx, rx) = channel();
        let store = PulsarStore::new(data);
        let list = if private_mode() { Pulsars::default() } else { store.load() };
        let keep = std::sync::Arc::new(AccountStore::new(data));
        // The system's keychain may be slow: never on the UI thread.
        if !private_mode() {
            let (keep, tx) = (keep.clone(), tx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Done::Account(keep.load()));
            });
        }
        Self {
            github: Github { store: keep, account: None, busy: false, status: String::new(), sso: None, why: Default::default() },
            list,
            store,
            tx,
            rx,
            reading: None,
            by_hand: false,
            wanted: Vec::new(),
            look_at: Instant::now() + FIRST_LOOK,
            form: Form::default(),
        }
    }

    fn save(&self) {
        if !private_mode() {
            self.store.save(&self.list);
        }
    }
}

fn now() -> i64 {
    i64::try_from(now_secs()).unwrap_or(i64::MAX)
}

impl Running {
    pub(crate) fn pulsars_on(&self) -> bool {
        self.svc.settings().pulsars && !private_mode()
    }

    /// The Pulsars of the space shown, as the sidebar draws them.
    pub(crate) fn pulsars_json(&self) -> Value {
        if !self.pulsars_on() {
            return json!([]);
        }
        let space = self.svc.workspace().active_space().id.0;
        let max = self.svc.settings().pulsar_lines as usize;
        self.pulsing
            .list
            .in_space(space)
            .map(|p| {
                json!({
                    "id": p.id, "name": p.name, "pinned": p.pinned, "collapsed": p.collapsed,
                    "host": p.feed.host_str(), "unread": p.unread(), "failed": p.failures > 0 || self.pulsar_waits(p),
                    "github": p.github.is_some(), "more": p.total().saturating_sub(max), "waits": self.pulsar_waits(p),
                    // Asked by hand, it turns at once, before its turn comes.
                    "busy": self.pulsing.reading == Some(p.id) || self.pulsing.wanted.contains(&p.id),
                    "asked": self.pulsing.wanted.contains(&p.id) || (self.pulsing.by_hand && self.pulsing.reading == Some(p.id)),
                    "checked": p.checked_at,
                    "next": if self.pulsar_waits(p) { 0 } else { p.next_at(self.svc.settings().pulsar_every) },
                    "lines": p.shown(max).map(|l| json!({"key": l.entry.key, "title": l.entry.title, "host": l.entry.url.host_str(), "url": l.entry.url.as_str(), "at": l.entry.at, "note": l.entry.note, "read": l.read})).collect::<Vec<_>>(),
                })
            })
            .collect()
    }

    /// A Pulsar of GitHub while no account is connected.
    fn pulsar_waits(&self, p: &pulsar::Pulsar) -> bool {
        p.github.is_some() && self.pulsing.github.account.is_none()
    }

    /// Settings > Pulsars: the options, the GitHub account, the Pulsars of every space.
    pub(crate) fn pulsars_settings(&self) -> Value {
        let ws = self.svc.workspace();
        let s = self.svc.settings();
        let g = &self.pulsing.github;
        json!({
            "on": s.pulsars,
            "private": private_mode(),
            "every": s.pulsar_every, "everyChoices": pulsar::EVERY_CHOICES,
            "lines": s.pulsar_lines, "linesChoices": pulsar::LINES_CHOICES,
            "notices": s.pulsar_notices,
            "space": ws.active_space().id.0,
            "spaces": ws.spaces().iter().map(|s| json!({"id": s.id.0, "name": s.name})).collect::<Vec<_>>(),
            "busy": self.pulsing.form.busy,
            "error": i18n::render(&self.pulsing.form.error),
            "github": {
                "login": g.account.as_ref().map(|a| a.login.clone()),
                "busy": g.busy,
                "status": i18n::render(&g.status),
                "sso": g.sso.is_some(),
                "kinds": GithubKind::ALL.map(GithubKind::id),
            },
            "list": self.pulsing.list.all().iter().map(|p| json!({
                "id": p.id, "name": p.name, "failed": p.failures > 0, "waits": self.pulsar_waits(p),
                "why": g.why.get(&p.id).map(|w| i18n::render(w)),
                "source": p.github.as_ref().map_or_else(|| p.feed.to_string(), |g| format!("GitHub · {}", github_label(g))),
                "github": p.github.is_some(), "unread": p.unread(), "pinned": p.pinned, "space": p.space,
                "busy": self.pulsing.reading == Some(p.id),
            })).collect::<Vec<_>>(),
        })
    }

    /// The form as the palette draws it.
    pub(crate) fn pulsar_form(&self) -> Value {
        let f = &self.pulsing.form;
        json!({"seq": f.seq, "prefill": f.prefill, "busy": f.busy, "error": i18n::render(&f.error)})
    }

    fn tab_ids(&self) -> std::collections::HashSet<blackhole_domain::TabId> {
        self.svc.workspace().spaces().iter().flat_map(|s| s.tabs.iter().map(|t| t.id)).collect()
    }

    /// The tabs of a Pulsar that is gone, or in another space, are loose again (its id may be
    /// given to a new one).
    fn free_pulsar_tabs(&mut self) {
        let known: Vec<(u64, u64)> = self.pulsing.list.all().iter().map(|p| (p.id, p.space)).collect();
        self.svc.keep_pulsars(&known);
    }

    fn pulsars_changed(&mut self) {
        self.pulsing.save();
        self.ui_state();
    }

    pub(crate) fn handle_pulsars(&mut self, command: UiCommand) {
        // Removing and switching stay possible when off; the rest needs them on.
        match command {
            UiCommand::SetPulsars { on } => {
                self.svc.set_pulsars(on);
                self.ui_state();
                return;
            }
            UiCommand::SetPulsarEvery { minutes } => {
                self.svc.set_pulsar_every(minutes);
                self.ui_settings();
                return;
            }
            UiCommand::SetPulsarLines { lines } => {
                self.svc.set_pulsar_lines(lines);
                self.ui_state();
                return;
            }
            UiCommand::SetPulsarNotices { on } => {
                self.svc.set_pulsar_notices(on);
                self.ui_settings();
                return;
            }
            UiCommand::PulsarGithubDisconnect => {
                let g = &mut self.pulsing.github;
                if !private_mode() {
                    g.account = None;
                    g.sso = None;
                    g.why.clear();
                    g.status = g.store.remove().err().unwrap_or_default();
                }
                self.ui_state();
                return;
            }
            UiCommand::PulsarRemove { id } => {
                if self.pulsing.list.remove(id) {
                    // What was said of it goes with it.
                    self.github_answered(id, None);
                    self.free_pulsar_tabs();
                    self.pulsars_changed();
                }
                return;
            }
            _ if !self.pulsars_on() => return,
            _ => {}
        }
        match command {
            UiCommand::PulsarForm => {
                // The site shown is the likely one (never a black hole tab's).
                let ws = self.svc.workspace();
                let shown = ws.active_tab().filter(|t| !self.home_shown() && !ws.is_burner(t.id));
                let prefill = shown
                    .filter(|t| matches!(t.url.scheme(), "http" | "https"))
                    .map(|t| t.url.to_string())
                    .unwrap_or_default();
                let seq = self.pulsing.form.seq + 1;
                self.pulsing.form = Form { seq, prefill, ..Form::default() };
                self.open_palette("pulsar");
            }
            UiCommand::PulsarGithubSso => {
                if let Some(url) = self.pulsing.github.sso.clone() {
                    self.open(url.as_str());
                }
            }
            UiCommand::PulsarGithubConnect { token } => self.github_connect(token.as_str()),
            UiCommand::PulsarGithubNew { kind, repo, space } => self.github_new(&kind, &repo, space),
            UiCommand::PulsarMove { id, space } => {
                if self.svc.workspace().spaces().iter().any(|s| s.id.0 == space) {
                    if !self.pulsing.list.move_to(id, space) {
                        self.toast(&t("toast.pulsar_there"));
                    }
                    self.free_pulsar_tabs();
                    self.pulsars_changed();
                }
            }
            UiCommand::PulsarNew { input, space } => {
                if self.pulsing.form.busy {
                    return;
                }
                let form = &mut self.pulsing.form;
                form.busy = true;
                form.error.clear();
                let space = space.unwrap_or_else(|| self.svc.workspace().active_space().id.0);
                let (seq, tx) = (form.seq, self.pulsing.tx.clone());
                std::thread::spawn(move || {
                    let found = feeds::find(&input).map_err(|why| {
                        // A private repository has no public feed.
                        let host = Url::parse(&input).or_else(|_| Url::parse(&format!("https://{input}")));
                        let github = host.is_ok_and(|u| u.host_str() == Some("github.com"));
                        if github { message("msg.github_private", &[]) } else { why }
                    });
                    let _ = tx.send(Done::Found(seq, space, Box::new(found)));
                });
                self.push_pulsar_form();
                self.ui_settings();
            }
            UiCommand::PulsarToggle { id } => {
                self.pulsing.list.toggle(id);
                self.pulsars_changed();
            }
            UiCommand::PulsarPin { id, pinned } => {
                self.pulsing.list.set_pinned(id, pinned);
                self.pulsars_changed();
            }
            UiCommand::PulsarRename { id, name } => {
                self.pulsing.list.rename(id, &name);
                self.pulsars_changed();
            }
            UiCommand::PulsarEdit { id } => self.ui(format!("window.bh&&bh.editPulsarId({id})")),
            UiCommand::PulsarReadAll { id } => {
                self.pulsing.list.read_all(id);
                self.pulsars_changed();
            }
            UiCommand::PulsarRefresh { id } => {
                // Being read already: that reading is the answer.
                if self.pulsing.reading == Some(id) || self.pulsing.wanted.contains(&id) {
                    return;
                }
                self.pulsing.wanted.push(id);
                self.pulsing.look_at = Instant::now();
                self.ui_state();
            }
            UiCommand::PulsarOpen { id, key, behind } => {
                let Some(url) = self.pulsing.list.open(id, &key) else { return };
                self.pulsing.save();
                let before = self.tab_ids();
                if behind {
                    self.open_in_background(url.as_str());
                } else {
                    self.open(url.as_str());
                }
                // A tab that was open already stays where the user has it.
                let home = self.pulsing.list.get(id).map(|p| p.space);
                let ws = self.svc.workspace();
                let here = ws.spaces().iter().find(|s| Some(s.id.0) == home);
                let made = here.and_then(|s| s.tabs.iter().map(|t| t.id).find(|t| !before.contains(t)));
                if let Some(tab) = made {
                    self.svc.set_pulsar(tab, id);
                }
                self.ui_state();
            }
            _ => {}
        }
    }

    /// Tries a token before it is kept.
    fn github_connect(&mut self, token: &str) {
        let g = &mut self.pulsing.github;
        if g.busy {
            return;
        }
        let Some(token) = github::clean_token(token).map(|t| zeroize::Zeroizing::new(t.to_owned())) else {
            g.status = message("msg.github_token", &[]);
            self.ui_settings();
            return;
        };
        g.busy = true;
        g.status.clear();
        let (store, tx) = (g.store.clone(), self.pulsing.tx.clone());
        std::thread::spawn(move || {
            let account = github::login(&api(), &token).map_err(|e| e.text()).and_then(|login| {
                let account = Account { token: token.to_string(), login };
                store.save(&account).map(|()| account)
            });
            let _ = tx.send(Done::Connected(account));
        });
        self.ui_settings();
    }

    fn github_new(&mut self, kind: &str, repo: &str, space: Option<u64>) {
        let Some(kind) = GithubKind::from_id(kind) else { return };
        let space = space.unwrap_or_else(|| self.svc.workspace().active_space().id.0);
        if self.pulsing.github.account.is_none() || !self.svc.workspace().spaces().iter().any(|s| s.id.0 == space) {
            return;
        }
        self.pulsing.github.status = match GithubQuery::new(kind, repo) {
            None => message("msg.github_repo_format", &[]),
            Some(query) => match self.pulsing.list.add_github(space, &github_label(&query), query) {
                None => message("msg.pulsar_full", &[("n", &pulsar::MAX.to_string())]),
                Some(id) => {
                    self.pulsing.wanted.push(id);
                    self.pulsing.look_at = Instant::now();
                    String::new()
                }
            },
        };
        self.pulsars_changed();
    }

    /// What GitHub answered to a Pulsar: the status shows what is still refused, if anything.
    fn github_answered(&mut self, id: u64, failed: Option<github::Failed>) {
        let g = &mut self.pulsing.github;
        let Some(why) = failed else {
            if let Some(was) = g.why.remove(&id) {
                if g.status == was {
                    g.status.clear();
                    g.sso = None;
                }
            }
            return;
        };
        g.status = why.text();
        g.why.insert(id, why.text());
        g.sso = match why {
            github::Failed::Sso(_, url) => Some(url),
            _ => None,
        };
    }

    /// New lines, said when the user asked for it.
    fn pulsar_tell(&mut self, id: u64, new: usize) {
        let Some(p) = self.pulsing.list.get(id).filter(|_| new > 0 && self.svc.settings().pulsar_notices) else {
            return;
        };
        let text = tf("toast.pulsar_new", &[("name", &p.name), ("n", &new.to_string())]);
        self.notify(Notice::Watch, &text, Value::Null);
    }

    fn push_pulsar_form(&self) {
        if self.palette.open && self.palette.mode == "pulsar" {
            self.push_palette();
        }
    }

    /// A space is made with an id a deleted one may have had: nothing of that one is left.
    pub(crate) fn pulsars_new_space(&mut self, space: u64) {
        if self.pulsing.list.forget_space(space) {
            self.free_pulsar_tabs();
            self.pulsing.save();
        }
    }

    /// Takes what the threads read, then asks the next feed (asked by hand, or due).
    pub(crate) fn service_pulsars(&mut self) {
        let mut changed = false;
        while let Ok(done) = self.pulsing.rx.try_recv() {
            match done {
                Done::Read(id, answer) => {
                    self.pulsing.reading = None;
                    self.pulsar_answer(id, answer);
                    changed = true;
                }
                Done::Github(id, answer) => {
                    self.pulsing.reading = None;
                    match answer {
                        Ok(entries) => {
                            self.github_answered(id, None);
                            let new = self.pulsing.list.merge(id, entries, now());
                            self.pulsar_tell(id, new);
                        }
                        Err(why) => {
                            self.pulsing.list.failed(id, now());
                            self.github_answered(id, Some(why));
                        }
                    }
                    changed = true;
                }
                Done::Connected(answer) => {
                    self.pulsing.github.busy = false;
                    match answer {
                        Ok(account) => {
                            self.pulsing.github.account = Some(account);
                            self.pulsing.github.status.clear();
                            self.pulsing.github.sso = None;
                            let mine = self.pulsing.list.all().iter().filter(|p| p.github.is_some()).map(|p| p.id);
                            self.pulsing.wanted.extend(mine);
                            self.pulsing.look_at = Instant::now();
                        }
                        Err(why) => self.pulsing.github.status = why,
                    }
                    changed = true;
                }
                Done::Account(account) => {
                    self.pulsing.github.account = account;
                    changed |= self.pulsing.github.account.is_some();
                }
                Done::Found(seq, space, found) => {
                    // The form may have been closed meanwhile: the Pulsar is made all the same,
                    // unless it was switched off.
                    if self.pulsars_on() {
                        self.pulsar_found(seq, space, *found);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.pulsars_changed();
        }
        if self.pulsing.reading.is_some() || Instant::now() < self.pulsing.look_at || !self.pulsars_on() {
            return;
        }
        self.pulsing.look_at = Instant::now() + LOOK_EVERY;
        let (id, by_hand) = match self.pulsing.wanted.pop() {
            Some(id) => (Some(id), true),
            None => (
                self.pulsing
                    .list
                    .next_due(now(), self.svc.settings().pulsar_every, self.pulsing.github.account.is_some())
                    .map(|p| p.id),
                false,
            ),
        };
        let Some(p) = id.and_then(|id| self.pulsing.list.get(id)) else { return };
        let tx = self.pulsing.tx.clone();
        if let Some(query) = p.github.clone() {
            let (id, account) = (p.id, self.pulsing.github.account.clone());
            let Some(account) = account else {
                // Asked by hand with no account: its star stops.
                if by_hand {
                    self.ui_state();
                }
                return;
            };
            self.pulsing.reading = Some(id);
            self.pulsing.by_hand = by_hand;
            std::thread::spawn(move || {
                let _ = tx.send(Done::Github(id, github::search(&api(), &account.token, &query)));
            });
            // The sidebar turns its star while it reads.
            self.ui_state();
            return;
        }
        let (id, feed) = (p.id, p.feed.clone());
        let (etag, modified) = (p.etag.clone(), p.modified.clone());
        self.pulsing.reading = Some(id);
        self.pulsing.by_hand = by_hand;
        std::thread::spawn(move || {
            let _ = tx.send(Done::Read(id, feeds::refresh(&feed, &etag, &modified)));
        });
        self.ui_state();
    }

    fn pulsar_answer(&mut self, id: u64, answer: Result<Answer, String>) {
        match answer {
            Ok(Answer { feed: Some(feed), etag, modified }) => {
                let first = self.pulsing.list.get(id).is_some_and(|p| p.checked_at == 0);
                let new = self.pulsing.list.merge(id, feed.entries, now());
                if !first {
                    self.pulsar_tell(id, new);
                }
                self.pulsing.list.set_marks(id, etag, modified);
            }
            Ok(_) => self.pulsing.list.unchanged(id, now()),
            Err(_) => self.pulsing.list.failed(id, now()),
        }
    }

    fn pulsar_found(&mut self, seq: u64, space: u64, found: Result<(Url, Answer), String>) {
        let mine = self.pulsing.form.seq == seq;
        let known = self.svc.workspace().spaces().iter().any(|s| s.id.0 == space);
        let error = match found {
            Ok(_) if !known => String::new(),
            Ok((url, answer)) => {
                let title = answer.feed.as_ref().map(|f| f.title.clone()).unwrap_or_default();
                match self.pulsing.list.add(space, &title, url) {
                    Some(id) => {
                        self.pulsar_answer(id, Ok(answer));
                        let unread = self.pulsing.list.get(id).map_or(0, pulsar::Pulsar::unread);
                        self.notify(Notice::Done, &tn("toast.pulsar_made", unread, &[]), Value::Null);
                        String::new()
                    }
                    None => message("msg.pulsar_full", &[("n", &pulsar::MAX.to_string())]),
                }
            }
            Err(why) => why,
        };
        self.pulsing.form.busy = false;
        if !mine {
            return;
        }
        if error.is_empty() {
            if self.palette.open && self.palette.mode == "pulsar" {
                self.close_palette();
            }
        } else if self.palette.open && self.palette.mode == "pulsar" {
            self.pulsing.form.error = error;
            self.push_pulsar_form();
        } else {
            if !self.settings_shown {
                self.notify(Notice::Problem, &i18n::render(&error), Value::Null);
            }
            self.pulsing.form.error = error;
        }
    }
}

/// What a Pulsar of GitHub gathers, in the interface language.
fn github_label(query: &GithubQuery) -> String {
    let kind = t(match query.kind {
        GithubKind::Reviews => "pulsar.github_reviews",
        GithubKind::Mine => "pulsar.github_mine",
        GithubKind::Assigned => "pulsar.github_assigned",
        GithubKind::Mentions => "pulsar.github_mentions",
    });
    if query.repo.is_empty() { kind } else { format!("{kind} · {}", query.repo) }
}

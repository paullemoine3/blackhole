//! The bridges between features: what can be done with a page, a link or a tab (one rule,
//! `domain::verbs`, for every menu and the command bar), the Easel as the place where things
//! converge (a Recall result, a watched area's change, the comparator's table, a text received
//! from another device), the signals that last (cards of the sidebar, `domain::signals`), the
//! start page's "Now", the command bar's grouped results, and the saved logins offered to an
//! HTTP sign-in.

use crate::i18n::{t, tf};
use crate::{ipc::UiCommand, now_secs, Running, TabId};
use blackhole_domain::easel::Easels;
use blackhole_domain::signals::Kind;
use blackhole_domain::verbs::{Context, Target, Verb};
use serde_json::{json, Value};

/// How many results of each kind the command bar may get to search.
const FOUND_MAX: usize = 300;
/// The start page lists this many changed areas, and the tab received from another device
/// for this long (seconds).
const HOME_AREAS: usize = 4;
const HOME_RECEIVED_FOR: i64 = 12 * 3600;

fn now() -> i64 {
    now_secs() as i64
}

impl Running {
    /// What the shell knows of a tab's page, for `domain::verbs`: read from the engine and
    /// the workspace, never from the page.
    pub(crate) fn verb_context(&self, tab: TabId) -> Context {
        let ws = self.svc.workspace();
        Context {
            front: ws.active_tab().is_some_and(|t| t.id == tab) && !self.home_shown(),
            web: self
                .view_of(tab)
                .and_then(|v| v.url().ok())
                .is_some_and(|u| u.starts_with("https://") || u.starts_with("http://")),
            burner: ws.is_burner(tab),
            error: self.error_shown(tab),
            paired: self.paired(),
            ready: false,
        }
    }

    /// Whether `verb` may be done with the page in front (the start page has none).
    pub(crate) fn front_page_allows(&self, verb: Verb) -> bool {
        let tab = self.svc.workspace().active_tab().map(|t| t.id);
        tab.filter(|_| !self.home_shown())
            .is_some_and(|tab| blackhole_domain::verbs::allows(Target::Page, self.verb_context(tab), verb))
    }

    /// Keeps something in the Easel shown (a new one when there is none) and says where.
    fn keep(&mut self, add: impl FnOnce(&mut Easels, u64) -> Result<u64, &'static str>) -> bool {
        let Some(easel) = self.easel_or_create() else {
            self.toast(&t("toast.too_many_easels"));
            return false;
        };
        match add(&mut self.easels, easel) {
            Ok(_) => {
                let name = self.easels.get(easel).map(|e| e.name.clone()).unwrap_or_default();
                self.easel_changed(true);
                self.toast(&tf("toast.captured_to", &[("name", &name)]));
                true
            }
            Err(why) => {
                self.toast(&t(why));
                false
            }
        }
    }

    /// Makes the Easel chosen in a menu the current one before something is kept in it:
    /// `easel` (when it still exists), a new one (`new`), or the current one (neither).
    fn pick_easel(&mut self, easel: Option<u64>, new: bool) -> bool {
        if new {
            match self.create_easel("") {
                Some(id) => {
                    self.easel_current = Some(id);
                    self.tag_easel_space();
                }
                None => {
                    self.toast(&t("toast.too_many_easels"));
                    return false;
                }
            }
        } else if let Some(id) = easel.filter(|id| self.easels.get(*id).is_some()) {
            self.easel_current = Some(id);
        }
        true
    }

    /// A watched area to the Easel: a clip of its page with the value before and after.
    fn watch_to_easel(&mut self, id: u64) -> bool {
        let Some((url, title, before, after)) = self.watch_of(id) else {
            return false;
        };
        let text = if before.is_empty() {
            after
        } else {
            tf("easel.watch_change", &[("before", &before), ("after", &after)])
        };
        self.keep(|e, easel| e.capture(easel, &url, &title, &text))
    }

    /// The comparator's table to the Easel, as a note: one paragraph per page picked.
    fn compare_to_easel(&mut self, ids: &[u64]) -> bool {
        let items = self.comparing.result["items"].as_array().cloned().unwrap_or_default();
        let field = |i: &Value, k: &str| i[k].as_str().map(str::trim).unwrap_or_default().to_owned();
        let mut text = vec![t("palette.compare_title")];
        for item in items.iter().filter(|i| ids.iter().any(|id| i["id"] == json!(id))) {
            let name = [field(item, "name"), field(item, "title"), field(item, "url")]
                .into_iter()
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            let price = format!("{} {}", field(item, "price"), field(item, "currency"));
            let facts: Vec<String> = [
                (t("palette.cmp_price"), price.trim().to_owned()),
                (t("palette.cmp_rating"), field(item, "rating")),
                (t("palette.cmp_reviews"), field(item, "reviews")),
                (t("palette.cmp_brand"), field(item, "brand")),
                (t("palette.cmp_availability"), field(item, "availability")),
            ]
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
            text.push(String::new());
            text.push(name);
            if !facts.is_empty() {
                text.push(facts.join(" · "));
            }
            text.push(field(item, "url"));
        }
        if text.len() == 1 {
            self.toast(&t("palette.cmp_pick"));
            return false;
        }
        let text = text.join("\n");
        self.keep(|e, easel| e.add_text(easel, &text))
    }

    // ---- signals that last ----------------------------------------------------------------

    /// A card of the sidebar that stays until answered or dismissed.
    pub(crate) fn signal(&mut self, kind: Kind, key: &str, title: &str, detail: &str) {
        self.signals.push(kind, key, title, detail, now());
        self.ui_state();
    }

    pub(crate) fn signals_forget(&mut self, kind: Kind, key: &str) {
        if self.signals.forget(kind, key) {
            self.ui_state();
        }
    }

    pub(crate) fn signals_forget_kind(&mut self, kind: Kind) {
        if self.signals.forget_kind(kind) {
            self.ui_state();
        }
    }

    /// Called every turn of the event loop: cards too old to matter leave.
    pub(crate) fn service_signals(&mut self) {
        if self.signals.expire(now()) {
            self.ui_state();
        }
    }

    /// The cards, for the sidebar: their buttons are named by the kind.
    pub(crate) fn signals_json(&self) -> Value {
        json!(self
            .signals
            .all()
            .iter()
            .map(|s| {
                let acts: &[(&str, &str)] = match s.kind {
                    Kind::AreaChanged => &[("open", "toast.open"), ("easel", "toast.to_easel")],
                    Kind::TextReceived => &[("copy", "menu.copy"), ("easel", "toast.to_easel")],
                    Kind::Notes => &[("notes", "toast.take_notes")],
                };
                json!({
                    "id": s.id, "kind": s.kind.id(), "title": s.title, "detail": s.detail, "at": s.at,
                    "acts": acts.iter().map(|(act, label)| json!({"act": act, "label": t(label)})).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>())
    }

    fn signal_act(&mut self, id: u64, act: &str) {
        let Some(signal) = self.signals.take(id) else { return };
        match (signal.kind, act) {
            (Kind::AreaChanged, "open") => {
                if let Some((url, ..)) = signal.key.parse().ok().and_then(|id| self.watch_of(id)) {
                    self.open(&url);
                }
            }
            (Kind::AreaChanged, "easel") => {
                if let Ok(id) = signal.key.parse() {
                    self.watch_to_easel(id);
                }
            }
            (Kind::TextReceived, "copy") => {
                if let Ok(id) = signal.key.parse() {
                    self.handle_devices(UiCommand::BeamCopy { id });
                }
            }
            (Kind::TextReceived, "easel") => {
                if let Ok(id) = signal.key.parse() {
                    self.handle_bridges(UiCommand::BeamToEasel { id });
                }
            }
            (Kind::Notes, "notes") => {
                if let Some((start, url)) = signal.key.split_once('|') {
                    if let Ok(start) = start.parse() {
                        self.take_meeting_notes(url, start);
                    }
                }
            }
            _ => {}
        }
        self.ui_state();
    }

    // ---- the start page's "Now" ----------------------------------------------------------

    /// What happens now: the next meeting, the areas that changed, the tab that came from
    /// another device.
    pub(crate) fn home_now_json(&self) -> Value {
        let received = self
            .received
            .as_ref()
            .filter(|(_, _, at)| now() - at < HOME_RECEIVED_FOR)
            .map(|(from, url, at)| json!({"from": from, "url": url, "at": at}));
        let space = self.svc.workspace().active_space().id.0;
        json!({
            "meeting": self.calendar.card(),
            "areas": self.changed_watches_json(HOME_AREAS),
            "received": received,
            // The Easels a changed area can go to ("Add to an Easel"), the current one ticked.
            "easels": self.easels.all().iter().filter(|e| self.easel_space(e) == space)
                .map(|e| json!({"id": e.id, "name": e.name})).collect::<Vec<_>>(),
            "easel": self.easel_current,
        })
    }

    // ---- the command bar's grouped results -------------------------------------------------

    /// What the command bar searches besides tabs, favorites, bookmarks and history: the
    /// highlights, the watched areas, the archive and the downloads (Recall answers apart).
    pub(crate) fn found_json(&self) -> Value {
        json!({
            "highlights": self.marks_found_json(FOUND_MAX),
            "watches": self.watches_json(),
            "archive": self.archive.iter().enumerate().rev().take(FOUND_MAX)
                .map(|(i, a)| json!({"index": i, "title": a.title, "url": a.url}))
                .collect::<Vec<_>>(),
            "downloads": self.downloads.iter().rev().filter(|d| d.done && d.ok).take(FOUND_MAX)
                .map(|d| json!({"id": d.id, "name": d.name, "url": d.url}))
                .collect::<Vec<_>>(),
        })
    }

    // ---- commands ---------------------------------------------------------------------------

    pub(crate) fn handle_bridges(&mut self, command: UiCommand) {
        match command {
            UiCommand::SignalAct { id, act } => self.signal_act(id, &act),
            UiCommand::SignalDismiss { id } => {
                if self.signals.take(id).is_some() {
                    self.ui_state();
                }
            }
            UiCommand::LinkToEasel { url, title } => {
                self.keep(|e, easel| e.capture(easel, &url, &title, ""));
            }
            UiCommand::FoundToEasel { url, title, text } => {
                self.keep(|e, easel| e.capture(easel, &url, &title, &text));
            }
            UiCommand::WatchToEasel { id, easel, new } => {
                if self.pick_easel(easel, new) {
                    self.watch_to_easel(id);
                }
            }
            UiCommand::CompareToEasel { ids } => {
                self.compare_to_easel(&ids);
            }
            UiCommand::BeamToEasel { id } => match self.received_text(id) {
                Some(text) => {
                    if self.keep(|e, easel| e.add_text(easel, &text)) {
                        self.signals_forget(Kind::TextReceived, &id.to_string());
                    }
                }
                None => self.toast(&t("toast.beam_text_gone")),
            },
            UiCommand::EaselWatch { item, on } => {
                let live = self
                    .current_easel()
                    .and_then(|e| self.easels.get(e))
                    .and_then(|e| e.items.iter().find(|i| i.id == item))
                    .filter(|i| i.kind == blackhole_domain::EaselItemKind::Live)
                    .and_then(|i| Some((i.url.clone()?, i.selector.clone(), i.title.clone())));
                if let Some((url, selector, title)) = live {
                    if on {
                        self.watch_url(&url, &selector, &title, None);
                    } else {
                        self.unwatch(&url, &selector);
                    }
                    self.ui_easel();
                }
            }
            UiCommand::AuthSaved { id, index } => {
                let (Some(origin), Some(realm)) = (
                    self.auth_origins.get(&id).cloned(),
                    self.auth.get(id).map(|c| c.realm.clone()),
                ) else {
                    return;
                };
                if let Some((user, password)) = self.passwords.auth_login(&origin, &realm, index) {
                    self.answer_auth(id, Some((user, password.to_string())));
                }
            }
            _ => {}
        }
    }

    /// The user names saved for the sign-in prompt `id` (never a password).
    pub(crate) fn auth_saved_users(&self, id: u64) -> Vec<String> {
        match (self.auth_origins.get(&id), self.auth.get(id)) {
            (Some(origin), Some(c)) => self.passwords.auth_users(origin, &c.realm),
            _ => Vec::new(),
        }
    }
}

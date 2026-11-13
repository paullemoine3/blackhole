//! Watched areas of pages: the element picked with the area picker is read again every half
//! hour in a hidden view (one at a time, sharing the profile so signed-in pages work), and a
//! change is told with a toast. The rules live in `domain::watch`.

use crate::i18n::{t, tf};
use crate::{ipc::UiCommand, views, Running, TabId};
use blackhole_domain::signals::Kind;
use blackhole_domain::watch::{Reading, Watches};
use blackhole_infrastructure::store::WatchStore;
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;
use wry::WebView;

/// How often a watched area is read again.
const EVERY: i64 = 30 * 60;
/// A hidden page is read this long after it started loading, and dropped a little later.
const READ_AFTER: Duration = Duration::from_secs(8);
const DROP_AFTER: Duration = Duration::from_secs(14);

pub struct Watching {
    list: Watches,
    store: WatchStore,
    tx: Sender<(u64, Option<String>)>,
    rx: Receiver<(u64, Option<String>)>,
    /// The page being read now: watch id, its hidden view, when it started, read asked, answered.
    probe: Option<(u64, WebView, Instant, bool, bool)>,
}

impl Watching {
    pub fn new(data: &std::path::Path) -> Self {
        let (tx, rx) = channel();
        let store = WatchStore::new(data);
        Self {
            list: store.load(),
            store,
            tx,
            rx,
            probe: None,
        }
    }

    fn save(&self) {
        let _ = self.store.save(&self.list);
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Script that returns the text of the watched element, or `null` when it is not there.
fn read_js(selector: &str) -> String {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_owned());
    format!("(()=>{{try{{const e=document.querySelector({selector});return e?(e.innerText||e.textContent||''):null}}catch(_){{return null}}}})()")
}

/// Reads the element in `view`; the answer comes back through `tx`.
fn read(view: &WebView, id: u64, selector: &str, tx: &Sender<(u64, Option<String>)>) {
    let tx = tx.clone();
    let _ = view.evaluate_script_with_callback(&read_js(selector), move |raw| {
        let text = serde_json::from_str::<Option<String>>(&raw).ok().flatten();
        let _ = tx.send((id, text));
    });
}

impl Running {
    /// "Watch this area for changes" (the area picker's menu): the area picked becomes a
    /// watch, read at once from the page shown to set its reference.
    pub(crate) fn watch_area(&mut self, tab: TabId, selector: &str) {
        let Some(url) = self
            .tabs
            .get(&tab)
            .and_then(|v| v.url().ok())
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        else {
            self.toast(&t("toast.cannot_watch"));
            return;
        };
        let title = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .find(|t| t.id == tab)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        self.watch_url(&url, selector, &title, Some(tab));
    }

    /// Starts watching `selector` of `url` (unless it is watched already). With `tab`, the
    /// area is read at once from that page; otherwise a hidden view reads it soon.
    pub(crate) fn watch_url(&mut self, url: &str, selector: &str, title: &str, tab: Option<TabId>) -> bool {
        if self.watching.list.find(url, selector).is_some() {
            return true;
        }
        let Some(id) = self.watching.list.add(url, selector, title, now()) else {
            self.toast(&t("toast.too_many_watches"));
            return false;
        };
        if let Some(view) = tab.and_then(|tab| self.tabs.get(&tab)) {
            read(view, id, selector, &self.watching.tx);
        }
        self.watching.save();
        self.toast_action(
            &t("toast.watching"),
            &t("toast.see_watches"),
            json!({"cmd": "action", "id": "open_watches"}),
        );
        self.ui_library();
        true
    }

    /// Whether `selector` of `url` is watched (a live card of the Easel shows it).
    pub(crate) fn is_watched(&self, url: &str, selector: &str) -> bool {
        self.watching.list.find(url, selector).is_some()
    }

    /// Stops watching `selector` of `url`.
    pub(crate) fn unwatch(&mut self, url: &str, selector: &str) {
        if let Some(id) = self.watching.list.find(url, selector).map(|w| w.id) {
            self.handle_watch(UiCommand::WatchRemove { id });
        }
    }

    /// A watch, for the Easel: its address, title, and its value before and after the last
    /// change (`previous` is empty before any change).
    pub(crate) fn watch_of(&self, id: u64) -> Option<(String, String, String, String)> {
        let w = self.watching.list.get(id)?;
        Some((w.url.clone(), w.title.clone(), w.previous.clone(), w.value.clone()))
    }

    /// The areas that changed since the user last looked (the start page lists them).
    pub(crate) fn changed_watches_json(&self, limit: usize) -> Value {
        json!(self
            .watching
            .list
            .changed(limit)
            .iter()
            .map(|w| json!({"id": w.id, "url": w.url, "title": w.title, "before": w.previous, "after": w.value, "at": w.changed_at}))
            .collect::<Vec<_>>())
    }

    /// Called every turn of the event loop: readings that came back, the hidden page being
    /// read, and the next watch due.
    pub(crate) fn service_watches(&mut self) {
        let mut changed = false;
        while let Ok((id, text)) = self.watching.rx.try_recv() {
            if let Some(probe) = self.watching.probe.as_mut().filter(|p| p.0 == id) {
                probe.4 = true;
            }
            let reading = self.watching.list.record(id, text.as_deref(), now());
            if let (Reading::Changed { before, after }, Some(w)) = (&reading, self.watching.list.get(id)) {
                let title = if w.title.is_empty() { t("toast.watched_area") } else { w.title.clone() };
                let message = tf("toast.watch_changed", &[("title", &title), ("before", before), ("after", after)]);
                let url = w.url.clone();
                self.toast_action(&message, &t("toast.open"), json!({"cmd": "open", "input": url}));
                // Told again by a card of the sidebar, for when nobody saw the toast.
                let detail = format!("{before} → {after}");
                self.signal(Kind::AreaChanged, &id.to_string(), &title, &detail);
            }
            changed = true;
        }
        if let Some((id, view, started, asked, answered)) = self.watching.probe.as_mut() {
            let elapsed = started.elapsed();
            if !*asked && elapsed >= READ_AFTER {
                *asked = true;
                if let Some(w) = self.watching.list.get(*id) {
                    read(view, *id, &w.selector, &self.watching.tx);
                }
            }
            if elapsed >= DROP_AFTER {
                let (id, answered) = (*id, *answered);
                self.watching.probe = None;
                if !answered {
                    self.watching.list.record(id, None, now());
                    changed = true;
                }
            }
        } else if let Some(w) = self.watching.list.due(now(), EVERY).cloned() {
            match Url::parse(&w.url) {
                Ok(url) => match views::live_view(
                    &self.window,
                    &mut self.web_context,
                    &url,
                    &w.selector,
                    self.svc.policy(),
                    &self.mailbox,
                ) {
                    Ok(view) => {
                        self.watching.probe = Some((w.id, view, Instant::now(), false, false));
                        self.ui_library();
                    }
                    Err(_) => {
                        self.watching.list.record(w.id, None, now());
                        changed = true;
                    }
                },
                Err(_) => {
                    self.watching.list.remove(w.id);
                    changed = true;
                }
            }
        }
        if changed {
            self.watching.save();
            self.ui_library();
            self.ui_easel();
            if self.home_shown() {
                self.ui_home();
            }
        }
    }

    /// The Library's "Watches" section. `checking`: a reading was asked for ("Check now") or
    /// its page is being read now; the Library says so until the reading is in.
    pub(crate) fn watches_json(&self) -> Value {
        // The hidden page being read, until its answer is in (it lingers a little after).
        let reading = self.watching.probe.as_ref().filter(|p| !p.4).map(|p| p.0);
        json!(self
            .watching
            .list
            .all()
            .iter()
            .map(|w| json!({
                "id": w.id, "url": w.url, "title": w.title, "value": w.value, "previous": w.previous,
                "checked": w.checked_at, "changed": w.changed_at, "unread": w.unread,
                "checking": w.checked_at == 0 || reading == Some(w.id),
            }))
            .collect::<Vec<_>>())
    }

    pub(crate) fn handle_watch(&mut self, command: UiCommand) {
        match command {
            UiCommand::WatchRemove { id } => {
                if self.watching.probe.as_ref().is_some_and(|p| p.0 == id) {
                    self.watching.probe = None;
                }
                self.watching.list.remove(id);
            }
            UiCommand::WatchCheck { id } => {
                // Its page already answered and only lingers: read it again in a fresh one.
                if self.watching.probe.as_ref().is_some_and(|p| p.0 == id && p.4) {
                    self.watching.probe = None;
                }
                self.watching.list.check_soon(id);
            }
            UiCommand::WatchesSeen => {
                self.watching.list.mark_read();
                self.signals_forget_kind(Kind::AreaChanged);
            }
            _ => return,
        }
        self.watching.save();
        self.ui_library();
        self.ui_easel();
    }
}

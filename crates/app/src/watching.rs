//! Watched areas of pages: the element picked with the area picker is read again every half
//! hour in a hidden view (one at a time, sharing the profile so signed-in pages work), and a
//! change is told with a toast. The rules live in `domain::watch`.

use crate::i18n::{t, tf};
use crate::{ipc::UiCommand, views, Running, TabId};
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
        let Some(view) = self.tabs.get(&tab) else { return };
        let Some(url) = view.url().ok().filter(|u| u.starts_with("https://") || u.starts_with("http://")) else {
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
        let Some(id) = self.watching.list.add(&url, selector, &title, now()) else {
            self.toast(&t("toast.too_many_watches"));
            return;
        };
        read(view, id, selector, &self.watching.tx);
        self.watching.save();
        self.toast_action(
            &t("toast.watching"),
            &t("toast.see_watches"),
            json!({"cmd": "action", "id": "open_watches"}),
        );
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
                    Ok(view) => self.watching.probe = Some((w.id, view, Instant::now(), false, false)),
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
        }
    }

    /// The Library's "Watches" section.
    pub(crate) fn watches_json(&self) -> Value {
        json!(self
            .watching
            .list
            .all()
            .iter()
            .map(|w| json!({
                "id": w.id, "url": w.url, "title": w.title, "value": w.value, "previous": w.previous,
                "checked": w.checked_at, "changed": w.changed_at, "unread": w.unread,
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
            UiCommand::WatchCheck { id } => self.watching.list.check_soon(id),
            UiCommand::WatchesSeen => self.watching.list.mark_read(),
            _ => return,
        }
        self.watching.save();
        self.ui_library();
    }
}

//! The tab comparator: every loaded tab of the space is asked what product it shows
//! (`ui/compare.js`), then the command bar shows them side by side (`palette.html`, mode
//! `compare`). Pages answer asynchronously: the table opens once all did, or after a moment.

use crate::i18n::t;
use crate::{views, Running, TabId};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// Pages that have not answered by then are left out (asleep, busy, or not a product).
const WAIT: Duration = Duration::from_millis(1500);

pub struct Comparing {
    tx: Sender<(u64, String)>,
    rx: Receiver<(u64, String)>,
    waiting: HashSet<u64>,
    deadline: Option<Instant>,
    items: Vec<Value>,
    /// What the command bar shows (`null` when closed).
    pub result: Value,
}

impl Comparing {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            waiting: HashSet::new(),
            deadline: None,
            items: Vec::new(),
            result: Value::Null,
        }
    }
}

impl Running {
    /// "Compare tabs": asks every loaded page of the space what it shows.
    pub(crate) fn compare_start(&mut self) {
        let tabs: Vec<(TabId, String, String)> = self
            .svc
            .workspace()
            .active_space()
            .tabs
            .iter()
            .filter(|t| self.tabs.contains_key(&t.id) && matches!(t.url.scheme(), "http" | "https"))
            .map(|t| (t.id, t.title.clone(), t.url.to_string()))
            .collect();
        if tabs.len() < 2 {
            self.toast(&t("toast.compare_two"));
            return;
        }
        self.comparing.items.clear();
        self.comparing.waiting.clear();
        for (id, title, url) in tabs {
            let Some(view) = self.tabs.get(&id) else { continue };
            let tx = self.comparing.tx.clone();
            self.comparing.waiting.insert(id.0);
            self.comparing.items.push(json!({"id": id.0, "title": title, "url": url}));
            let _ = view.evaluate_script_with_callback(views::COMPARE_JS, move |raw| {
                let _ = tx.send((id.0, raw));
            });
        }
        self.comparing.deadline = Some(Instant::now() + WAIT);
    }

    /// Collects the answers and opens the table when they are in.
    pub(crate) fn service_compare(&mut self) {
        while let Ok((id, raw)) = self.comparing.rx.try_recv() {
            self.comparing.waiting.remove(&id);
            // The script returns a JSON string, which the engine hands back JSON-encoded.
            let data = serde_json::from_str::<String>(&raw)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok());
            if let (Some(Value::Object(data)), Some(item)) = (
                data,
                self.comparing.items.iter_mut().find(|i| i["id"] == json!(id)),
            ) {
                for (k, v) in data {
                    item[k.as_str()] = v;
                }
            }
        }
        let Some(deadline) = self.comparing.deadline else { return };
        if !self.comparing.waiting.is_empty() && Instant::now() < deadline {
            return;
        }
        self.comparing.deadline = None;
        let items: Vec<Value> = std::mem::take(&mut self.comparing.items)
            .into_iter()
            .filter(|i| i.get("name").is_some())
            .collect();
        if items.len() < 2 {
            self.toast(&t("toast.compare_unreadable"));
            return;
        }
        self.comparing.result = json!({"items": items});
        self.open_palette("compare");
    }
}

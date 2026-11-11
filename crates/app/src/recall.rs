//! Recall: the pages read can be searched by what they said. The rules live in `domain::recall`
//! and the encrypted store in `infrastructure::recall`; this module reads the text of a page a
//! few seconds after it loaded (once, and only while it is on screen), hands it to the store and
//! shows the store's answers in the command bar.
//!
//! Off unless the user turns it on. Never in a private window nor a black hole tab, never on
//! one of BlackHole's own pages, never a page with a password field (the script returns nothing
//! there), never a site on the user's list.

use crate::i18n::{t, tf};
use crate::{ipc::UiCommand, Running, TabId};
use blackhole_domain::recall::{self, page_key};
use blackhole_infrastructure::recall::{Event, Handle, Request};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

/// A page is read this long after it finished loading (late content is in, and a page left at
/// once is never read).
const READ_AFTER: Duration = Duration::from_secs(4);
/// The same address is not read again before this long.
const READ_AGAIN: Duration = Duration::from_secs(10 * 60);
/// Pages waiting to be read, and addresses remembered as read this session.
const MAX_DUE: usize = 64;
const MAX_RECENT: usize = 500;

/// What a page says of itself: nothing when it is hidden or has a password field.
const READ_JS: &str = "(()=>{try{if(document.visibilityState!=='visible')return null;\
if(document.querySelector('input[type=password]'))return null;const b=document.body;if(!b)return null;\
return{title:String(document.title||'').slice(0,300),text:String(b.innerText||'').slice(0,60000)}}catch(_){return null}})()";

pub struct Recall {
    /// The store's thread, started the first time it is needed.
    handle: Option<Handle>,
    data: std::path::PathBuf,
    private: bool,
    /// Pages to read: when, and the address they had when they loaded.
    due: HashMap<TabId, (Instant, String)>,
    tx: Sender<(Url, String)>,
    rx: Receiver<(Url, String)>,
    /// Addresses read this session, and when.
    recent: HashMap<String, Instant>,
    /// A search was sent and its answer is not in yet.
    searching: bool,
    /// Pages remembered, as last counted (`None`: to be asked).
    count: Option<usize>,
    counting: bool,
}

impl Recall {
    pub fn new(data: &std::path::Path, private: bool) -> Self {
        let (tx, rx) = channel();
        Self {
            handle: None,
            data: data.to_path_buf(),
            private,
            due: HashMap::new(),
            tx,
            rx,
            recent: HashMap::new(),
            searching: false,
            count: None,
            counting: false,
        }
    }

    fn store(&mut self) -> &Handle {
        let data = &self.data;
        self.handle.get_or_insert_with(|| blackhole_infrastructure::recall::spawn(data))
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Running {
    /// Recall is on (the setting) and this window may use it.
    pub(crate) fn recall_on(&self) -> bool {
        self.svc.settings().recall && !self.recall.private
    }

    /// Whether the page of `tab`, at `url`, may be remembered.
    fn recall_may(&self, tab: TabId, url: &Url) -> bool {
        self.recall_on()
            && recall::indexable(url)
            && !self.svc.workspace().is_burner(tab)
            && !self.error_shown(tab)
            && !url
                .host_str()
                .is_some_and(|h| recall::excluded(h, &self.svc.settings().recall_exclude))
    }

    /// A page finished loading (or a single-page app moved on): it will be read in a moment.
    pub(crate) fn recall_seen(&mut self, tab: TabId) {
        let url = self
            .tabs
            .get(&tab)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok());
        match url.filter(|u| self.recall_may(tab, u)) {
            Some(url) if self.recall.due.len() < MAX_DUE || self.recall.due.contains_key(&tab) => {
                self.recall.due.insert(tab, (Instant::now() + READ_AFTER, page_key(&url)));
            }
            _ => {
                self.recall.due.remove(&tab);
            }
        }
    }

    /// Called every turn of the event loop. Returns how soon it wants to be called again
    /// (a page is about to be read, or a search is on its way).
    pub(crate) fn service_recall(&mut self) -> Option<Duration> {
        let now_at = Instant::now();
        // Pages whose time has come are read if they are on screen; a hidden one waits there.
        let on_screen: Vec<TabId> = if self.home_shown() {
            Vec::new()
        } else {
            self.shown.into_iter().chain(self.split_tab()).collect()
        };
        let ready: Vec<TabId> = self
            .recall
            .due
            .iter()
            .filter(|(tab, (at, _))| *at <= now_at && on_screen.contains(tab))
            .map(|(tab, _)| *tab)
            .collect();
        for tab in ready {
            let Some((_, key)) = self.recall.due.remove(&tab) else { continue };
            let url = self
                .tabs
                .get(&tab)
                .and_then(|v| v.url().ok())
                .and_then(|raw| Url::parse(&raw).ok())
                .filter(|u| page_key(u) == key && self.recall_may(tab, u));
            let fresh = self.recall.recent.get(&key).is_some_and(|at| at.elapsed() < READ_AGAIN);
            if let (Some(url), false, Some(view)) = (url, fresh, self.tabs.get(&tab)) {
                let tx = self.recall.tx.clone();
                let _ = view.evaluate_script_with_callback(READ_JS, move |raw| {
                    let _ = tx.send((url.clone(), raw));
                });
            }
        }
        self.recall.due.retain(|tab, _| self.tabs.contains_key(tab));
        // What the pages answered (untrusted text, bounded by the domain).
        while let Ok((url, raw)) = self.recall.rx.try_recv() {
            let answer: Value = serde_json::from_str(&raw).unwrap_or_default();
            let text = |key| answer.get(key).and_then(Value::as_str).unwrap_or_default();
            // The setting or the list may have changed while the page was being read.
            let allowed = self.recall_on()
                && !url
                    .host_str()
                    .is_some_and(|h| recall::excluded(h, &self.svc.settings().recall_exclude));
            if let (true, Some(page)) = (allowed, recall::page(&url, text("title"), text("text"), now())) {
                if self.recall.recent.len() >= MAX_RECENT {
                    self.recall.recent.retain(|_, at| at.elapsed() < READ_AGAIN);
                }
                self.recall.recent.insert(page.url.clone(), Instant::now());
                self.recall.count = None;
                self.recall.store().send(Request::Index(page));
            }
        }
        // The Settings window shows how many pages are remembered: asked when it is not known.
        if self.settings.is_some() && self.recall.count.is_none() && !self.recall.counting && !self.recall.private {
            self.recall.counting = true;
            self.recall.store().send(Request::Count);
        }
        // What the store answered.
        let mut counted = false;
        while let Some(event) = self.recall.handle.as_ref().and_then(Handle::poll) {
            match event {
                Event::Results { id, hits } => {
                    self.recall.searching = false;
                    if self.palette_open && self.palette_mode == "recall" {
                        let list: Vec<Value> = hits
                            .iter()
                            .map(|h| json!({"url": h.url, "title": h.title, "snippet": h.snippet, "at": h.at}))
                            .collect();
                        let answer = json!({"seq": id, "hits": list, "now": now()});
                        let _ = self
                            .palette
                            .evaluate_script(&format!("window.bh&&bh.recall&&bh.recall({answer})"));
                    }
                }
                Event::Count(n) => {
                    self.recall.count = Some(n);
                    self.recall.counting = false;
                    counted = true;
                }
            }
        }
        if counted {
            self.ui_settings();
        }
        let next_read = self
            .recall
            .due
            .iter()
            .filter(|(tab, _)| on_screen.contains(tab))
            .map(|(_, (at, _))| at.saturating_duration_since(now_at))
            .min();
        let waiting = (self.recall.searching || self.recall.counting).then_some(Duration::from_millis(40));
        match (next_read, waiting) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Settings > Privacy > Recall, and the command bar.
    pub(crate) fn recall_state(&self) -> Value {
        json!({
            "on": self.svc.settings().recall,
            "available": !self.recall.private,
            "count": self.recall.count,
            "exclude": self.svc.settings().recall_exclude,
        })
    }

    /// The history forgot a page, or everything: Recall forgets it too.
    pub(crate) fn recall_forget(&mut self, url: Option<&str>) {
        if self.recall.private {
            return;
        }
        match url {
            Some(url) => {
                if let Ok(parsed) = Url::parse(url) {
                    self.recall.recent.remove(&page_key(&parsed));
                }
                self.recall.store().send(Request::Forget(url.to_owned()));
            }
            None => {
                self.recall.recent.clear();
                self.recall.due.clear();
                self.recall.store().send(Request::Clear);
            }
        }
        self.recall.count = None;
    }

    pub(crate) fn handle_recall(&mut self, command: UiCommand) {
        match command {
            UiCommand::RecallSearch { query, seq } => {
                if !self.recall_on() {
                    return;
                }
                self.recall.searching = true;
                self.recall.store().send(Request::Search { id: seq, query });
            }
            UiCommand::SetRecall { on } => {
                if self.recall.private {
                    return;
                }
                self.svc.set_recall(on);
                if !on {
                    self.recall.due.clear();
                }
                // The command bar's own switch: it shows the search at once.
                if self.palette_open && self.palette_mode == "recall" {
                    self.push_palette();
                }
                self.ui_state();
            }
            UiCommand::RecallExclude { site, on } => {
                match self.svc.set_recall_excluded(&site, on) {
                    // What was remembered of the site goes with it.
                    Some(site) if on => {
                        self.recall.recent.clear();
                        self.recall.count = None;
                        self.recall.store().send(Request::ForgetSite(site.clone()));
                        self.toast(&tf("toast.recall_excluded", &[("site", &site)]));
                    }
                    Some(_) => {}
                    None => self.toast(&t("toast.recall_not_a_site")),
                }
                self.ui_state();
            }
            UiCommand::RecallForget { url } => {
                self.recall_forget(Some(&url));
                self.ui_state();
            }
            UiCommand::RecallClear => {
                self.recall_forget(None);
                self.toast(&t("toast.recall_cleared"));
                self.ui_state();
            }
            _ => {}
        }
    }
}

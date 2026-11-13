//! Highlights and notes on pages. The rules and limits live in `domain::highlights`; this module
//! asks the page for the selection (`ui/highlights.js`), keeps the list (`highlights.json`) and
//! gives each page shown its own highlights to paint. A page never drives the list: it only
//! answers the shell's questions, and its answers are bounded by the domain.
//!
//! Nothing is recorded in a private window nor for a black hole tab.

use crate::i18n::{t, tf};
use crate::{ipc::UiCommand, Running, TabId};
use blackhole_domain::highlights::{page_key, Highlights};
use blackhole_domain::verbs::{self, Target, Verb};
use blackhole_infrastructure::store::HighlightStore;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

/// A selection captured when the page menu opened stays usable this long.
const PENDING_FOR: Duration = Duration::from_secs(120);
/// A highlight opened from the Library is scrolled to if its page loads within this time.
const FOCUS_FOR: Duration = Duration::from_secs(20);

/// What the page says of its selection (untrusted, bounded by `Highlights::add`).
#[derive(Deserialize, Default)]
struct Captured {
    #[serde(default)]
    quote: String,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    suffix: String,
    #[serde(default)]
    title: String,
}

/// An answer of a page: the tab, its address read from the engine when the question was
/// asked, whether to highlight at once, and the raw answer.
type Answer = (TabId, Url, bool, String);

pub struct Marks {
    list: Highlights,
    store: HighlightStore,
    /// Off in a private window: nothing is read nor written.
    enabled: bool,
    tx: Sender<Answer>,
    rx: Receiver<Answer>,
    /// The selection captured when the page menu opened, for its "Highlight" entry.
    pending: Option<(TabId, Url, Captured, Instant)>,
    /// Tabs whose page was given highlights (it must be told when they go).
    painted: HashSet<TabId>,
    /// The highlight to scroll to once its tab has loaded.
    focus: Option<(TabId, u64, Instant)>,
    /// Library: the highlight whose note is being written, with a request number.
    edit: (u32, u64),
}

impl Marks {
    pub fn new(data: &std::path::Path, private: bool) -> Self {
        let (tx, rx) = channel();
        let store = HighlightStore::new(data);
        Self {
            list: if private { Highlights::default() } else { store.load() },
            store,
            enabled: !private,
            tx,
            rx,
            pending: None,
            painted: HashSet::new(),
            focus: None,
            edit: (0, 0),
        }
    }

    fn save(&self) {
        if self.enabled {
            self.store.save(&self.list);
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Running {
    /// Whether highlights may be made in `tab`: not in a private window, not in a black hole
    /// tab, and on a web page.
    pub(crate) fn can_mark(&self, tab: TabId) -> bool {
        self.marks.enabled && !self.svc.workspace().is_burner(tab) && self.mark_page(tab).is_some()
    }

    /// The address of the page of `tab`, read from the engine.
    fn mark_page(&self, tab: TabId) -> Option<Url> {
        let raw = self.tabs.get(&tab)?.url().ok()?;
        Url::parse(&raw).ok().filter(|u| page_key(u).is_some())
    }

    /// Asks the page of `tab` for its selection. `add`: highlight it as soon as it answers;
    /// otherwise it waits for the menu's "Highlight" entry.
    pub(crate) fn mark_capture(&mut self, tab: TabId, add: bool) {
        if !self.can_mark(tab) {
            return;
        }
        let (Some(view), Some(url)) = (self.tabs.get(&tab), self.mark_page(tab)) else {
            return;
        };
        let tx = self.marks.tx.clone();
        let _ = view.evaluate_script_with_callback(
            "window.__bhMarks?window.__bhMarks.capture():null",
            move |raw| {
                let _ = tx.send((tab, url.clone(), add, raw));
            },
        );
    }

    /// The "Highlight" entry of the page menu of `menu_tab` (its selection was read when the
    /// menu opened), or the command bar: the selection of the page shown, read now.
    pub(crate) fn mark_selection(&mut self, menu_tab: Option<TabId>) {
        let pending = self.marks.pending.take();
        let Some(tab) = menu_tab.or(self.shown) else { return };
        match pending {
            Some((owner, url, captured, at))
                if menu_tab == Some(owner) && at.elapsed() < PENDING_FOR =>
            {
                self.mark_add(tab, &url, &captured);
            }
            _ if self.can_mark(tab) => self.mark_capture(tab, true),
            _ => self.toast(&t("toast.cannot_highlight")),
        }
    }

    fn mark_add(&mut self, tab: TabId, url: &Url, captured: &Captured) {
        // The page may have gone elsewhere since it answered.
        if self.mark_page(tab).and_then(|u| page_key(&u)) != page_key(url) {
            return;
        }
        let added = self.marks.list.add(
            url,
            &captured.title,
            &captured.quote,
            &captured.prefix,
            &captured.suffix,
            now(),
        );
        match added {
            Some(id) => {
                self.marks.save();
                self.marks_repaint(url);
                self.ui_library();
                self.notify_action(
                    crate::notices::Notice::Done,
                    &t("toast.highlighted"),
                    &t("toast.add_note"),
                    json!({"cmd": "highlight_edit", "id": id}),
                );
            }
            None => self.toast(&t("toast.nothing_highlighted")),
        }
    }

    /// Called every turn of the event loop: the selections the pages answered with.
    pub(crate) fn service_marks(&mut self) {
        while let Ok((tab, url, add, raw)) = self.marks.rx.try_recv() {
            let Some(captured) = serde_json::from_str::<Option<Captured>>(&raw).ok().flatten() else {
                if add {
                    self.toast(&t("toast.nothing_highlighted"));
                }
                continue;
            };
            if add {
                self.mark_add(tab, &url, &captured);
            } else {
                self.marks.pending = Some((tab, url, captured, Instant::now()));
            }
        }
    }

    /// Gives the page of `tab` its highlights (or takes them back). `loaded`: the page just
    /// finished loading, so a highlight to scroll to is shown now.
    pub(crate) fn marks_push(&mut self, tab: TabId, loaded: bool) {
        if !self.marks.enabled || self.svc.workspace().is_burner(tab) {
            return;
        }
        let Some(view) = self.tabs.get(&tab) else {
            self.marks.painted.remove(&tab);
            return;
        };
        let marks: Vec<Value> = self
            .mark_page(tab)
            .map(|url| {
                self.marks
                    .list
                    .for_page(&url)
                    .iter()
                    .map(|m| json!({"id": m.id, "quote": m.quote, "prefix": m.prefix, "suffix": m.suffix, "note": !m.note.is_empty()}))
                    .collect()
            })
            .unwrap_or_default();
        // A page without highlights is left alone, unless it was painted before.
        if marks.is_empty() && !self.marks.painted.remove(&tab) {
            return;
        }
        if !marks.is_empty() {
            self.marks.painted.insert(tab);
        }
        let list = Value::Array(marks);
        let _ = view.evaluate_script(&format!("window.__bhMarks&&window.__bhMarks.set({list})"));
        match self.marks.focus {
            Some((owner, id, at)) if owner == tab && at.elapsed() < FOCUS_FOR => {
                let _ = view.evaluate_script(&format!("window.__bhMarks&&window.__bhMarks.show({id})"));
                if loaded {
                    self.marks.focus = None;
                }
            }
            Some((_, _, at)) if at.elapsed() >= FOCUS_FOR => self.marks.focus = None,
            _ => {}
        }
    }

    /// The highlights of a page changed: every tab showing it paints them again.
    fn marks_repaint(&mut self, url: &Url) {
        let key = page_key(url);
        let showing: Vec<TabId> = self
            .tabs
            .keys()
            .copied()
            .filter(|tab| self.mark_page(*tab).and_then(|u| page_key(&u)) == key)
            .collect();
        for tab in showing {
            self.marks_push(tab, false);
        }
    }

    /// The entries of the page menu for the highlight `id` under the pointer (none when the
    /// id, which comes from the page, is not a highlight of that page).
    pub(crate) fn mark_menu_items(&self, tab: TabId, id: u64) -> Vec<Value> {
        let Some((page, mark)) = self.marks.list.get(id) else {
            return Vec::new();
        };
        if self.mark_page(tab).and_then(|u| page_key(&u)).as_deref() != Some(page.url.as_str()) {
            return Vec::new();
        }
        let note = if mark.note.is_empty() { "menu.highlight_add_note" } else { "menu.highlight_edit_note" };
        let mut items = vec![
            json!({"icon": "edit", "label": t(note), "run": {"cmd": "highlight_edit", "id": id}}),
            json!({"icon": "copy", "label": t("menu.copy"), "copy": mark.quote}),
        ];
        // The same verbs as everywhere else (`domain::verbs`): keep it, send it.
        let ctx = self.verb_context(tab);
        for verb in verbs::verbs(Target::Highlight, ctx) {
            match verb {
                Verb::Keep => items.push(json!({"icon": "easel", "label": t("library.to_easel"), "run": {"cmd": "highlight_to_easel", "id": id}})),
                Verb::Send => {
                    let quote = mark.quote.clone();
                    items.extend(self.send_items("menu.beam_to", |d| json!({"cmd": "beam_text", "device": d, "text": quote})));
                }
                _ => {}
            }
        }
        items.push(json!({"icon": "trash", "label": t("menu.highlight_remove"), "run": {"cmd": "highlight_remove", "id": id}}));
        items
    }

    /// Every highlight, for the command bar's search: page, quote and note.
    pub(crate) fn marks_found_json(&self, limit: usize) -> Value {
        json!(self
            .marks
            .list
            .pages()
            .iter()
            .flat_map(|p| p.marks.iter().map(move |m| json!({"id": m.id, "url": p.url, "title": p.title, "quote": m.quote, "note": m.note})))
            .take(limit)
            .collect::<Vec<_>>())
    }

    /// The Library's "Highlights" section.
    pub(crate) fn marks_json(&self) -> Value {
        json!({
            "edit": [self.marks.edit.0, self.marks.edit.1],
            "pages": self.marks.list.pages().iter().map(|p| json!({
                "url": p.url, "title": p.title,
                "marks": p.marks.iter().map(|m| json!({"id": m.id, "quote": m.quote, "note": m.note, "at": m.created})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    /// The Library closed: no note is being written any more.
    pub(crate) fn marks_edit_done(&mut self) {
        self.marks.edit.1 = 0;
    }

    pub(crate) fn marks_clear(&mut self) {
        self.marks.list.clear();
        self.marks.save();
        let painted: Vec<TabId> = self.marks.painted.iter().copied().collect();
        for tab in painted {
            self.marks_push(tab, false);
        }
    }

    pub(crate) fn handle_marks(&mut self, command: UiCommand) {
        let page_of = |list: &Highlights, id| list.get(id).and_then(|(p, _)| Url::parse(&p.url).ok());
        match command {
            UiCommand::HighlightSelection { tab } => self.mark_selection(tab.map(TabId)),
            UiCommand::HighlightRemove { id } => {
                let page = page_of(&self.marks.list, id);
                if self.marks.list.remove(id) {
                    self.marks.save();
                    if let Some(page) = page {
                        self.marks_repaint(&page);
                    }
                    self.ui_library();
                }
            }
            UiCommand::HighlightNote { id, note } => {
                if self.marks.list.set_note(id, &note) {
                    self.marks.edit.1 = 0;
                    self.marks.save();
                    if let Some(page) = page_of(&self.marks.list, id) {
                        self.marks_repaint(&page);
                    }
                    self.ui_library();
                }
            }
            // Its note is written in the Library, on the highlight's own row.
            UiCommand::HighlightEdit { id } => {
                if self.marks.list.get(id).is_some() {
                    self.marks.edit = (self.marks.edit.0 + 1, id);
                    self.open_library(Some("highlights"));
                }
            }
            UiCommand::HighlightOpen { id } => {
                let Some(page) = page_of(&self.marks.list, id) else { return };
                self.open(page.as_str());
                if let Some(tab) = self.svc.workspace().active_tab().map(|t| t.id) {
                    self.marks.focus = Some((tab, id, Instant::now()));
                    // Already on screen and loaded: scrolled to at once.
                    self.marks_push(tab, false);
                }
            }
            UiCommand::HighlightToEasel { id } => {
                let Some((page, mark)) = self.marks.list.get(id) else { return };
                let text = if mark.note.is_empty() {
                    mark.quote.clone()
                } else {
                    format!("{}\n\n{}", mark.quote, mark.note)
                };
                let (url, title) = (page.url.clone(), page.title.clone());
                let Some(easel) = self.easel_or_create() else {
                    self.toast(&t("toast.too_many_easels"));
                    return;
                };
                match self.easels.capture(easel, &url, &title, &text) {
                    Ok(_) => {
                        let name = self.easels.get(easel).map(|e| e.name.clone()).unwrap_or_default();
                        self.easel_changed(true);
                        self.notify(crate::notices::Notice::Easel, &tf("toast.captured_to", &[("name", &name)]), serde_json::Value::Null);
                    }
                    Err(why) => self.toast(&t(why)),
                }
            }
            _ => {}
        }
    }
}

//! The command bar's ranking, done here at every key press (`UiCommand::Suggest`): the palette
//! sends what is typed and draws the rows it gets back, with the completion of the field. Rules
//! in `domain::suggest`; the rows are built once when the palette opens (tabs of every space,
//! quick access, bookmarks, the history pool, the commands). What the user chooses is learned
//! (`suggest.json`), never in a private window.

use crate::{now_secs, private_mode, Running};
use blackhole_domain::suggest::{self, Adaptive, Context, Row, RowKind};
use blackhole_infrastructure::store::ChoiceStore;
use serde_json::{json, Value};
use std::cell::RefCell;
use url::Url;

/// Rows under the field besides the first (search or address): the palette shows nine at most.
const ROOM: usize = 8;
/// Commands that keep a place however many sites match.
const ACTION_ROWS: usize = 3;
/// Searches made before shown at most.
const SEARCH_ROWS: usize = 2;

pub struct Choices {
    store: ChoiceStore,
    pub(crate) data: Adaptive,
    /// The candidates of the palette opened last, and its commands as the palette knows them.
    rows: RefCell<(Vec<Row>, Vec<Value>)>,
}

impl Choices {
    pub fn new(data: &std::path::Path) -> Self {
        let store = ChoiceStore::new(data);
        let data = if private_mode() { Adaptive::default() } else { store.load() };
        Self { store, data, rows: RefCell::new((Vec::new(), Vec::new())) }
    }

    pub(crate) fn save(&self) {
        if !private_mode() {
            self.store.save(&self.data);
        }
    }
}

fn kind_name(kind: RowKind) -> &'static str {
    match kind {
        RowKind::Tab => "tab",
        RowKind::OtherTab => "other",
        RowKind::Favorite => "favorite",
        RowKind::Bookmark => "bookmark",
        RowKind::History => "history",
        RowKind::Search => "search",
        RowKind::Action => "action",
    }
}

impl Running {
    /// Builds the candidates of the palette being opened (`actions`: its commands, as sent).
    pub(crate) fn suggest_prepare(&self, actions: &[Value]) {
        let ws = self.svc.workspace();
        let shown = ws.active_space().id;
        let mut rows = Vec::new();
        for space in ws.spaces() {
            for t in &space.tabs {
                // A black hole tab is offered in its own space only, like in the sidebar.
                if space.id != shown && ws.is_burner(t.id) {
                    continue;
                }
                rows.push(Row {
                    kind: if space.id == shown { RowKind::Tab } else { RowKind::OtherTab },
                    id: t.id.0.to_string(),
                    title: t.title.clone(),
                    key: suggest::key(&t.url),
                    url: Some(t.url.clone()),
                    frecency: 0.0,
                    spaces: vec![space.id.0],
                });
            }
        }
        for f in self.svc.favorites() {
            rows.push(Row {
                kind: RowKind::Favorite,
                id: String::new(),
                title: f.name.clone(),
                key: suggest::key(&f.url),
                url: Some(f.url.clone()),
                frecency: 0.0,
                spaces: Vec::new(),
            });
        }
        for b in self.bookmarks.load() {
            let Ok(url) = Url::parse(&b.url) else { continue };
            rows.push(Row {
                kind: RowKind::Bookmark,
                id: String::new(),
                title: b.title.clone(),
                key: suggest::key(&url),
                url: Some(url),
                frecency: 0.0,
                spaces: Vec::new(),
            });
        }
        for s in suggest::pool(self.svc.recent_activity(usize::MAX), now_secs()) {
            rows.push(Row {
                kind: if s.search.is_some() { RowKind::Search } else { RowKind::History },
                id: String::new(),
                title: s.title,
                url: Some(s.url),
                key: s.key,
                frecency: s.frecency,
                spaces: s.spaces,
            });
        }
        for a in actions {
            let (Some(id), Some(label)) = (a["id"].as_str(), a["label"].as_str()) else { continue };
            rows.push(Row {
                kind: RowKind::Action,
                id: id.to_owned(),
                title: label.to_owned(),
                url: None,
                key: format!("action:{id}"),
                frecency: 0.0,
                spaces: Vec::new(),
            });
        }
        *self.choices.rows.borrow_mut() = (rows, actions.to_vec());
    }

    /// Answers the palette's query numbered `seq` with its rows and the completion of the field.
    pub(crate) fn suggest_answer(&self, query: &str, seq: u64) {
        let (rows, actions) = &*self.choices.rows.borrow();
        let ws = self.svc.workspace();
        let ctx = Context { space: ws.active_space().id.0, adaptive: &self.choices.data, now: now_secs() };
        let ranked = suggest::rank_rows(rows, query, &ctx);
        let picked = suggest::pick(rows, &ranked, ROOM, ACTION_ROWS, SEARCH_ROWS);
        let space_name = |id: u64| ws.spaces().iter().find(|s| s.id.0 == id).map(|s| s.name.clone());
        let out: Vec<Value> = picked
            .iter()
            .map(|r| {
                let row = &rows[r.index];
                let mut v = match row.kind {
                    // The command as the palette knows it (keys, hint, confirmation).
                    RowKind::Action => actions.iter().find(|a| a["id"].as_str() == Some(row.id.as_str())).cloned().unwrap_or(json!({})),
                    _ => json!({}),
                };
                v["kind"] = json!(kind_name(row.kind));
                v["id"] = json!(row.id);
                v["title"] = json!(row.title);
                v["key"] = json!(row.key);
                v["url"] = json!(row.url.as_ref().map(Url::as_str));
                if row.kind == RowKind::OtherTab {
                    v["space"] = json!(row.spaces.first().and_then(|id| space_name(*id)));
                }
                v
            })
            .collect();
        let complete = suggest::completion(rows, &ranked, query, &ctx).map(|c| {
            let key = suggest::key(&c.url);
            json!({"text": c.text, "url": c.url.as_str(), "key": key})
        });
        let answer = json!({"seq": seq, "query": query, "rows": out, "complete": complete});
        let _ = self.palette.evaluate_script(&format!("window.bh&&bh.suggested&&bh.suggested({answer})"));
    }

    /// The user took the row `key` for what they typed: it comes first next time.
    pub(crate) fn suggest_chosen(&mut self, input: &str, key: &str) {
        if private_mode() || self.burner_next {
            return;
        }
        if self.choices.data.record(input, key, now_secs()) {
            self.choices.save();
        }
    }

    /// The history was cleared, or one page forgotten: its choices go too.
    pub(crate) fn suggest_forget(&mut self, url: Option<&Url>) {
        match url {
            Some(url) => self.choices.data.forget(&suggest::key(url)),
            None => self.choices.data.clear(),
        }
        self.choices.save();
    }
}

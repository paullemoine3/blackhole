//! Erasing browsing data (Settings > Privacy > Data). What the app keeps goes at once; cookies
//! and cache are erased by the engine, in the profile of each space. Rules in `domain::clearing`.

use std::time::{Duration, Instant};

use blackhole_domain::clearing::{DataKind, Period};
use blackhole_domain::SpaceId;
use wry::WebView;

use crate::i18n::t;
use crate::ipc::Msg;
use crate::platform::now_secs;
use crate::state::Running;
use crate::views::{self, Store};

/// An engine that never answers does not keep its view longer than this.
const ENGINE_MAX: Duration = Duration::from_secs(60);

/// The erasing under way in the engine.
#[derive(Default)]
pub(crate) struct Clearing {
    run: u64,
    waiting: usize,
    /// Views made for the spaces that had none: kept until the engine is done.
    views: Vec<WebView>,
    since: Option<Instant>,
    /// Settings: what is erased now.
    pub(crate) busy: bool,
}

impl Running {
    pub(crate) fn clear_data(&mut self, kinds: &[String], period: &str, all_spaces: bool) {
        let kinds: Vec<DataKind> = kinds.iter().filter_map(|k| DataKind::from_id(k)).collect();
        let Some(period) = Period::from_id(period) else { return };
        if kinds.is_empty() || self.clearing.busy {
            return;
        }
        let since = period.since(now_secs());
        for kind in &kinds {
            self.clear_kept(*kind, since);
        }
        let what = wry::SiteData {
            cookies: kinds.contains(&DataKind::Cookies),
            cache: kinds.contains(&DataKind::Cache),
        };
        if what == wry::SiteData::default() {
            self.cleared();
            return;
        }
        let ws = self.svc.workspace();
        let spaces: Vec<SpaceId> = if all_spaces {
            ws.spaces().iter().map(|s| s.id).collect()
        } else {
            vec![ws.active_space().id]
        };
        self.clearing.run += 1;
        self.clearing.busy = true;
        self.clearing.since = Some(Instant::now());
        let mut stores: Vec<Store> = Vec::new();
        let mut failed = false;
        for space in spaces {
            let store = self.store_of_space(space);
            if !stores.contains(&store) {
                stores.push(store);
                failed |= !self.clear_engine(space, store, what, since);
            }
        }
        if failed {
            self.notify(
                crate::shell::notices::Notice::Problem,
                &t("toast.data_failed"),
                serde_json::Value::Null,
            );
        }
        if self.clearing.waiting == 0 {
            self.clearing.busy = false;
        }
        self.ui_settings();
    }

    /// What the app itself keeps.
    fn clear_kept(&mut self, kind: DataKind, since: Option<u64>) {
        let before = |at: u64| since.is_some_and(|since| at < since);
        match kind {
            DataKind::History => match since {
                None => {
                    self.svc.clear_history();
                    self.suggest_forget(None);
                    self.recall_forget(None);
                }
                Some(since) => {
                    for url in self.svc.clear_history_since(since) {
                        self.suggest_forget(Some(&url));
                        self.recall_forget(Some(url.as_str()));
                    }
                }
            },
            DataKind::Downloads => {
                self.downloads.records.retain(|d| !d.done || before(d.at));
                self.thumbs.borrow_mut().clear();
                self.save_downloads();
            }
            DataKind::Archive => {
                self.archive.retain(|a| before(a.at));
                self.save_archive();
            }
            DataKind::Permissions => self.handle_permissions(crate::ipc::UiCommand::PermissionForget { origin: None }),
            DataKind::Cookies | DataKind::Cache => {}
        }
    }

    /// Erases in the profile of `space`, through a page of it, or a view made for it.
    fn clear_engine(&mut self, space: SpaceId, store: Store, what: wry::SiteData, since: Option<u64>) -> bool {
        let run = self.clearing.run;
        let mailbox = self.mailbox.clone();
        let done = move || mailbox.push(Msg::DataCleared(run));
        let ws = self.svc.workspace();
        let open = ws
            .spaces()
            .iter()
            .filter(|s| s.id == space)
            .flat_map(|s| &s.tabs)
            .filter(|t| !ws.is_burner(t.id))
            .find_map(|t| self.tabs.get(&t.id));
        let started = if let Some(view) = open {
            view.clear_site_data(what, since, done).is_ok()
        } else {
            let made = views::spare_tab_view(
                &self.window,
                &mut self.web_context,
                self.svc.policy(),
                &self.mailbox,
                self.webext.as_ref(),
                store,
            );
            match made {
                Ok((_, view)) => {
                    let ok = view.clear_site_data(what, since, done).is_ok();
                    self.clearing.views.push(view);
                    ok
                }
                Err(_) => false,
            }
        };
        if started {
            self.clearing.waiting += 1;
        }
        started
    }

    /// The engine finished one of the profiles.
    pub(crate) fn on_data_cleared(&mut self, run: u64) {
        if run != self.clearing.run || self.clearing.waiting == 0 {
            return;
        }
        self.clearing.waiting -= 1;
        if self.clearing.waiting == 0 {
            self.clearing_over();
            self.cleared();
        }
    }

    pub(crate) fn service_clearing(&mut self) {
        if self.clearing.since.is_some_and(|at| at.elapsed() > ENGINE_MAX) {
            self.clearing_over();
            self.notify(
                crate::shell::notices::Notice::Problem,
                &t("toast.data_failed"),
                serde_json::Value::Null,
            );
            self.ui_settings();
        }
    }

    fn clearing_over(&mut self) {
        self.clearing.views.clear();
        self.clearing.waiting = 0;
        self.clearing.since = None;
        self.clearing.busy = false;
    }

    fn cleared(&mut self) {
        self.notify(crate::shell::notices::Notice::Done, &t("toast.data_cleared"), serde_json::Value::Null);
        self.ui_state();
        self.ui_library();
        self.ui_home();
        self.ui_settings();
    }
}

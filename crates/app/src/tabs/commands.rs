//! Commands about the tabs of the window: open, show, close, move, split, tidy.

use blackhole_domain::{FolderId, SpaceId, TabId, VisitKind};
use serde_json::json;
use url::Url;

use crate::views;
use crate::i18n::{t, tf, tn};
use crate::ipc::UiCommand;
use crate::shell::notices;
use crate::shell::render::TIDY_MIN;
use crate::shell::windows::open_window;
use crate::state::Running;
use crate::tabs::lifecycle::MAX_BURNERS;

impl Running {
    /// Commands about tabs: opening, showing, closing, moving, splitting and tidying them.
    pub(crate) fn handle_tabs(&mut self, command: UiCommand) {
        match command {
            UiCommand::Open { input, via } => {
                self.next_visit = Some(VisitKind::asked_via(via.as_deref()));
                self.open(&input);
                self.next_visit = None;
            }
            UiCommand::Home => {
                self.close_palette();
                self.close_peek();
                self.home_visible = true;
                self.easel_tab = false;
                self.library = false;
                self.settings_shown = false;
                self.sync();
            }
            UiCommand::NewTab => self.new_tab(),
            UiCommand::Navigate { input, via } => {
                self.next_visit = Some(VisitKind::asked_via(via.as_deref()));
                self.navigate(&input);
                self.next_visit = None;
            }
            UiCommand::Activate { id } => {
                self.close_palette();
                self.close_peek();
                // A tab of another space (offered by the command bar): its space first.
                let elsewhere = self
                    .svc
                    .workspace()
                    .spaces()
                    .iter()
                    .find(|s| s.tabs.iter().any(|t| t.id.0 == id))
                    .map(|s| s.id)
                    .filter(|s| *s != self.svc.workspace().active_space().id);
                if let Some(space) = elsewhere {
                    self.svc.switch_space(space);
                }
                self.svc.activate_tab(TabId(id));
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.settings_shown = false;
                self.sync();
            }
            // A quick access tab is never taken off the list by closing it: it only stops.
            UiCommand::Close { id }
                if self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .any(|t| t.id.0 == id && t.pinned) =>
            {
                self.handle_ui(UiCommand::CloseInstance { id });
            }
            UiCommand::Close { id } => {
                self.archive_tab(TabId(id));
                self.svc.close_tab(TabId(id));
                self.drop_view(TabId(id));
                self.sync();
            }
            UiCommand::CloseInstance { id } => {
                let id = TabId(id);
                self.drop_view(id);
                if self.split_tab() == Some(id) {
                    self.svc.clear_split();
                }
                // The tab seen before takes over, as when it is closed; the start page otherwise.
                if self.svc.workspace().active_tab().map(|t| t.id) == Some(id)
                    && !self.svc.release_tab(id)
                {
                    self.home_visible = true;
                    self.easel_tab = false;
                    self.library = false;
                    self.settings_shown = false;
                }
                self.sync();
            }
            UiCommand::Split { id } => {
                if self.svc.set_split(TabId(id)) {
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                    self.settings_shown = false;
                }
                self.sync();
            }
            UiCommand::Tidy { skip } => {
                let groups = blackhole_domain::tidy_kept(
                    self.svc.workspace().tidy_suggestion(TIDY_MIN),
                    &skip,
                );
                if groups.is_empty() {
                    self.toast(&t("toast.nothing_to_tidy"));
                    return;
                }
                let done = self.svc.tidy(&groups);
                let message = tn(
                    "toast.tidied",
                    groups.len(),
                    &[("tabs", &done.tabs().to_string())],
                );
                self.tidied = Some(done);
                self.sync();
                self.notify(notices::Notice::Undo, &message, json!([{"label": t("toast.undo"), "run": {"cmd": "untidy"}}]));
            }
            UiCommand::Untidy => {
                if let Some(done) = self.tidied.take() {
                    self.svc.untidy(&done);
                    self.sync();
                }
            }
            UiCommand::DismissTidy => {
                self.tidy_dismissed = true;
                self.ui_state();
            }
            UiCommand::DismissDownload { id } => {
                // A download still running keeps its card.
                if self.downloads.live.get(&id).is_some_and(|d| d.finished.is_some()) {
                    self.downloads.live.remove(&id);
                    let list = self.downloads_json();
                    self.ui(format!("window.bh&&bh.downloads({list})"));
                }
            }
            UiCommand::OpenLink { url, target } => match target.as_str() {
                "window" | "private" => {
                    if matches!(Url::parse(&url), Ok(u) if matches!(u.scheme(), "http" | "https")) {
                        open_window(target == "private", Some(&url));
                    }
                }
                "burner" => {
                    if matches!(Url::parse(&url), Ok(u) if matches!(u.scheme(), "http" | "https")) {
                        self.open_burner(&url);
                    }
                }
                // A new tab shown at once (the selection's search or address).
                "front" => {
                    if matches!(Url::parse(&url), Ok(u) if matches!(u.scheme(), "http" | "https")) {
                        self.open(&url);
                    }
                }
                _ => self.open_in_background(&url),
            },
            UiCommand::TabHome { id } => {
                let id = TabId(id);
                if self.svc.go_home(id) {
                    let home = self
                        .svc
                        .workspace()
                        .spaces()
                        .iter()
                        .flat_map(|s| s.tabs.iter())
                        .find(|t| t.id == id)
                        .map(|t| t.url.clone());
                    if let (Some(url), Some(view)) = (home, self.tabs.get(&id)) {
                        let _ = view.load_url(url.as_str());
                    }
                    self.sync();
                }
            }
            UiCommand::TabAnchor { id } => {
                if self.svc.toggle_anchor(TabId(id)) {
                    self.ui_state();
                }
            }
            UiCommand::TabSetHome { id } => {
                if self.svc.set_home(TabId(id)) {
                    self.ui_state();
                }
            }
            UiCommand::ReloadTab { id } => {
                if let Some(view) = self.tabs.get(&TabId(id)) {
                    views::reload(view);
                }
                if self.svc.workspace().active_tab().is_some_and(|t| t.id == TabId(id)) {
                    self.spin_reload();
                }
            }
            // Not `open`: opening an address shows the tab already on it instead of a copy.
            UiCommand::DuplicateTab { id } => {
                let ws = self.svc.workspace();
                if ws.is_burner(TabId(id)) && ws.burner_count() >= MAX_BURNERS {
                    self.toast(&t("toast.too_many_burners"));
                    return;
                }
                if self.svc.duplicate_tab(TabId(id)).is_some() {
                    self.close_palette();
                    self.close_peek();
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                    self.settings_shown = false;
                    self.sync();
                }
            }
            UiCommand::MoveTabToSpace { id, space } => {
                let name = self
                    .svc
                    .workspace()
                    .spaces()
                    .iter()
                    .find(|s| s.id.0 == space)
                    .map(|s| s.name.clone());
                // Spaces have their own cookies: the view is rebuilt on the new space's profile
                // the next time the tab is shown.
                if self.svc.move_tab_to_space(TabId(id), SpaceId(space)) {
                    self.drop_view(TabId(id));
                    self.sync();
                    self.toast(&tf("toast.moved_to", &[("name", &name.unwrap_or_default())]));
                }
            }
            UiCommand::CloseOtherTabs { id } => {
                let others: Vec<TabId> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| t.id.0 != id && !t.pinned && t.folder.is_none())
                    .map(|t| t.id)
                    .collect();
                // Taken first: the toast's Undo puts them all back, in their places.
                let snapshot = self.svc.workspace().snapshot_tabs(&others);
                for &tab in &others {
                    self.archive_tab(tab);
                    self.svc.close_tab(tab);
                    self.drop_view(tab);
                }
                self.svc.activate_tab(TabId(id));
                self.home_visible = false;
                self.sync();
                let n = others.len();
                match snapshot {
                    Some(snapshot) => self.toast_undo(&tn("toast.tabs_closed", n, &[]), snapshot),
                    None => self.toast(&tn("toast.tabs_closed", n, &[])),
                }
            }
            UiCommand::ClosePane { side } if self.easel_side() => {
                // Closing the tab's pane leaves the easel alone, in the content area.
                let easel = self.easel_beside.map(|(_, id)| id);
                self.close_easel_side();
                if let Some(id) = easel.filter(|_| side == "left") {
                    self.open_easel_tab(id);
                }
            }
            UiCommand::ClosePane { side } => {
                if side == "left" {
                    if let Some(other) = self.split_tab() {
                        self.svc.activate_tab(other);
                    }
                }
                self.svc.clear_split();
                self.sync();
            }
            UiCommand::CloseSplit => {
                self.svc.clear_split();
                self.sync();
            }
            UiCommand::Pin { id } => {
                self.svc.toggle_pin(TabId(id));
                self.ui_state();
            }
            UiCommand::OpenAt {
                url,
                before,
                folder,
                pinned,
            } => {
                self.open(&url);
                if let Some(id) = self.svc.workspace().active_tab().map(|t| t.id) {
                    self.svc
                        .move_tab(id, before.map(TabId), folder.map(FolderId), pinned);
                }
                self.sync();
            }
            UiCommand::MoveTab {
                id,
                before,
                folder,
                pinned,
            } => {
                self.svc
                    .move_tab(TabId(id), before.map(TabId), folder.map(FolderId), pinned);
                self.ui_state();
            }
            UiCommand::TabDropOut { id, x, y } => self.drop_tab_out(TabId(id), x, y),
            UiCommand::TabDrag { id, x, y, card } => self.tab_drag(TabId(id), x, y, card),
            UiCommand::TabDragEnd => self.tab_drag_end(),
            UiCommand::TabDragReady => self.tab_drag_ready(),
            UiCommand::TabDragGone => self.tab_drag_gone(),
            UiCommand::TabDragArea { area } => self.tab_drag_area(area),
            UiCommand::MediaCtl { id, action } => self.media_ctl(TabId(id), &action),
            UiCommand::Back => {
                // On our error page, "back" is the page seen before the failure (the entry
                // just before is the address that failed).
                let on_error = self.svc.workspace().active_tab().is_some_and(|t| self.error_shown(t.id));
                self.with_active_tab(if on_error { views::error_back } else { views::go_back });
            }
            UiCommand::Forward => self.with_active_tab(views::go_forward),
            UiCommand::Reload => {
                self.spin_reload();
                self.with_active_tab(views::reload);
            }
            UiCommand::HardReload => {
                self.spin_reload();
                self.with_active_tab(views::hard_reload);
            }
            _ => {}
        }
    }
}

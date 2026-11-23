//! Easels: boards of notes and page captures, in their window, as a tab or beside one.

use blackhole_domain::{FolderId, SpaceId, TabId};
use serde_json::json;
use url::Url;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};
use wry::WebView;

use crate::{i18n, views};
use crate::features::trash;
use crate::i18n::{t, tf};
use crate::ipc::UiCommand;
use crate::shell::layout::{card_rect, SPLIT_GAP, SPLIT_HEADER};
use crate::shell::pages::EASEL_HTML;
use crate::state::Running;
use crate::tabs::downloads::open_download;

pub(crate) mod live;

/// The Easel window, laid out like the settings window.
pub(crate) struct EaselWindow {
    pub(crate) view: WebView,
    pub(crate) window: Window,
}

/// A page snapshot read by `capture_to_easel`: address from the engine, the rest from the page.
pub(crate) struct Capture {
    pub(crate) url: String,
    pub(crate) page: String,
}

impl Running {
    /// Whether an Easel fills the right pane beside the tab shown (`easel_beside`).
    pub(crate) fn easel_side(&self) -> bool {
        !self.home_shown()
            && self.easel_beside.is_some_and(|(tab, easel)| {
                self.easel_current == Some(easel)
                    && self.svc.workspace().active_tab().is_some_and(|t| t.id == tab)
            })
    }

    /// Where the embedded Easel page starts in the window (live views are placed from there).
    fn easel_origin(&self) -> (f64, f64) {
        let (x, y, width, _) = card_rect(&self.sidebar, &self.window_size());
        if self.easel_side() {
            let half = ((width - SPLIT_GAP) / 2.0).max(1.0);
            (x + half + SPLIT_GAP, y + SPLIT_HEADER)
        } else {
            (x, y)
        }
    }

    /// The space an easel shows in: its folder's, else the one it was created in.
    pub(crate) fn easel_space(&self, easel: &blackhole_domain::easel::Easel) -> u64 {
        let ws = self.svc.workspace();
        let first = ws.spaces().first().map_or(0, |s| s.id.0);
        let in_folder = easel.folder.and_then(|f| {
            ws.spaces()
                .iter()
                .find(|s| s.folders.iter().any(|x| x.id.0 == f))
        });
        in_folder.map_or_else(|| easel.space.unwrap_or(first), |s| s.id.0)
    }

    /// Opens (or focuses) the Easel window; called from the event loop, which creates windows.
    pub(crate) fn open_easel(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(win) = &self.easel {
            win.window.set_minimized(false);
            win.window.focus_window();
            return;
        }
        let attrs = self.panel_window_attributes(&i18n::t("window.easel"), (1100.0, 720.0));
        let Ok(window) = event_loop.create_window(attrs) else {
            return;
        };
        window.focus_window();
        match views::panel_view(&window, &mut self.web_context, EASEL_HTML, &self.mailbox) {
            Ok(view) => self.easel = Some(EaselWindow { view, window }),
            Err(err) => tracing::error!("cannot create easel window: {err}"),
        }
    }

    pub(crate) fn close_easel(&mut self) {
        if !self.easel_tab {
            self.live.clear();
            self.live_slots.clear();
        }
        if let Some(EaselWindow { view, window }) = self.easel.take() {
            drop(view);
            self.retire_window(window);
        }
        self.ui_state();
    }

    /// Shows an easel in the content area like a web page, with its own controls.
    pub(crate) fn open_easel_tab(&mut self, id: u64) {
        if self.easels.get(id).is_none() {
            return;
        }
        self.close_palette();
        self.close_peek();
        if !self.easel_tab || self.easel_current != Some(id) {
            self.live.clear();
            self.live_slots.clear();
        }
        self.easel_current = Some(id);
        self.easel_tab = true;
        self.library = false;
        self.settings_shown = false;
        self.sync();
    }

    pub(crate) fn close_easel_tab(&mut self) {
        if !self.easel_tab {
            return;
        }
        self.live.clear();
        self.live_slots.clear();
        self.easel_tab = false;
        self.library = false;
        self.settings_shown = false;
        self.sync();
    }

    /// Shows a tab with an easel in the right pane beside it (a meeting's notes beside the
    /// call). It replaces the tab's split view; going back to that tab shows the easel again.
    pub(crate) fn open_easel_side(&mut self, tab: TabId, id: u64) {
        let known = self.svc.workspace().active_space().tabs.iter().any(|t| t.id == tab);
        if self.easels.get(id).is_none() || !known {
            return;
        }
        self.close_palette();
        self.close_peek();
        if self.easel_tab || self.easel_current != Some(id) {
            self.live.clear();
            self.live_slots.clear();
        }
        self.easel_current = Some(id);
        self.easel_tab = false;
        self.library = false;
        self.settings_shown = false;
        self.home_visible = false;
        self.svc.activate_tab(tab);
        self.svc.clear_split();
        self.easel_beside = Some((tab, id));
        self.sync();
    }

    pub(crate) fn close_easel_side(&mut self) {
        if self.easel_beside.take().is_none() {
            return;
        }
        if !self.easel_tab {
            self.live.clear();
            self.live_slots.clear();
        }
        self.sync();
    }

    pub(crate) fn easel_resized(&self) {
        if let Some(win) = &self.easel {
            let size = win
                .window
                .inner_size()
                .to_logical::<f64>(win.window.scale_factor());
            let _ = win
                .view
                .set_bounds(views::rect(0.0, 0.0, size.width, size.height));
        }
    }

    pub(crate) fn easel_window_id(&self) -> Option<WindowId> {
        self.easel.as_ref().map(|w| w.window.id())
    }

    /// The easel being edited; falls back to the first one when the selection is gone.
    pub(crate) fn current_easel(&mut self) -> Option<u64> {
        if !self
            .easel_current
            .is_some_and(|id| self.easels.get(id).is_some())
        {
            self.easel_current = self.easels.all().first().map(|e| e.id);
        }
        self.easel_current
    }

    pub(crate) fn tag_easel_space(&mut self) {
        if let Some(id) = self.easel_current {
            let space = self.svc.workspace().active_space().id.0;
            self.easels.set_space(id, Some(space));
        }
    }

    /// A new Easel; a blank name becomes "Easel N" in the interface language.
    pub(crate) fn create_easel(&mut self, name: &str) -> Option<u64> {
        let name = if name.trim().is_empty() {
            tf("easel.default_name", &[("n", &(self.easels.all().len() + 1).to_string())])
        } else {
            name.to_owned()
        };
        self.easels.create(&name)
    }

    /// The current easel, created on first use so a capture never fails for lack of a board.
    pub(crate) fn easel_or_create(&mut self) -> Option<u64> {
        if self.current_easel().is_none() {
            self.easel_current = self.create_easel("");
            self.tag_easel_space();
        }
        self.easel_current
    }

    pub(crate) fn ui_easel(&self) {
        let active = self.svc.workspace().active_space().id.0;
        let state = |embedded: bool| {
            json!({
                "embedded": embedded,
                "current": self.easel_current,
                "easels": self.easels.all().iter().filter(|e| self.easel_space(e) == active).map(|e| json!({
                    "id": e.id,
                    "name": e.name,
                    "items": e.items.iter().map(|i| json!({
                        "id": i.id, "kind": i.kind.id(), "x": i.x, "y": i.y, "w": i.w, "h": i.h,
                        "text": i.text, "title": i.title, "url": i.url, "color": i.color,
                        "selector": i.selector, "playing": self.live.contains_key(&i.id),
                        "watched": i.kind == blackhole_domain::EaselItemKind::Live
                            && i.url.as_deref().is_some_and(|u| self.is_watched(u, &i.selector)),
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "colors": blackhole_domain::easel::COLORS,
                // The Easel window's own top bar (not drawn when embedded as a tab).
                "os": std::env::consts::OS,
                "spaceColor": self.svc.workspace().active_space().color,
                "animations": self.svc.settings().animations,
                "depth": self.svc.settings().depth,
                "maximized": self.easel.as_ref().is_some_and(|w| w.window.is_maximized()),
            })
        };
        if let Some(win) = &self.easel {
            let _ = win
                .view
                .evaluate_script(&format!("window.bh&&bh.render({})", state(false)));
        }
        if self.easel_tab || self.easel_side() {
            if let Some(panel) = &self.easel_panel.view {
                let _ = panel.evaluate_script(&format!("window.bh&&bh.render({})", state(true)));
            }
        }
    }

    /// Persists after a change and refreshes the window; `false` changes nothing.
    pub(crate) fn easel_changed(&mut self, changed: bool) {
        if changed {
            self.easel_persist();
            self.sync_live();
            if self.easel_tab && self.current_easel().is_none() {
                self.close_easel_tab();
            }
            if self.easel_beside.is_some_and(|(_, id)| self.easels.get(id).is_none()) {
                self.close_easel_side();
            }
            self.ui_state();
        }
    }

    fn easel_persist(&mut self) {
        if let Err(e) = self.easel_store.save(&self.easels) {
            tracing::error!("cannot save easels: {e}");
            self.toast(&t("toast.easels_not_saved"));
        }
    }

    /// Reads the page in front. The address comes from the engine; the title and the selection
    /// come from the page and are only ever stored as text.
    fn capture_to_easel(&mut self) {
        // Nothing of a black hole tab is kept (`domain::verbs`), whatever led here.
        let burner = self.svc.workspace().active_tab().is_some_and(|t| self.svc.workspace().is_burner(t.id));
        if burner && !self.home_shown() {
            self.toast(&t("toast.burner_keeps_nothing"));
            return;
        }
        let Some(view) = (if self.home_visible {
            None
        } else {
            self.svc
                .workspace()
                .active_tab()
                .and_then(|t| self.tabs.get(&t.id))
        }) else {
            self.toast(&t("toast.open_page_to_capture"));
            return;
        };
        let Ok(url) = view.url() else { return };
        let tx = self.capture_tx.clone();
        let result = view.evaluate_script_with_callback(
            "({title:document.title,text:String(getSelection()||'').slice(0,2000)})",
            move |page| {
                let _ = tx.send(Capture {
                    url: url.clone(),
                    page,
                });
            },
        );
        if result.is_err() {
            self.notify(crate::shell::notices::Notice::Problem, &t("toast.cannot_capture"), serde_json::Value::Null);
        }
    }

    pub(crate) fn poll_captures(&mut self) {
        while let Ok(cap) = self.capture_rx.try_recv() {
            let page: serde_json::Value = serde_json::from_str(&cap.page).unwrap_or_default();
            let text = |key| page.get(key).and_then(|v| v.as_str()).unwrap_or_default();
            let Some(easel) = self.easel_or_create() else {
                self.toast(&t("toast.too_many_easels"));
                continue;
            };
            match self
                .easels
                .capture(easel, &cap.url, text("title"), text("text"))
            {
                Ok(_) => {
                    let name = self
                        .easels
                        .get(easel)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    self.easel_changed(true);
                    self.notify(crate::shell::notices::Notice::Easel, &tf("toast.captured_to", &[("name", &name)]), serde_json::Value::Null);
                }
                Err(why) => self.toast(&t(why)),
            }
        }
    }

    pub(crate) fn handle_easel(&mut self, command: UiCommand) {
        match command {
            UiCommand::OpenEasel { id } => {
                if let Some(id) = id.filter(|id| self.easels.get(*id).is_some()) {
                    self.easel_current = Some(id);
                }
                self.easel_or_create();
                self.easel_changed(true);
                self.easel_wanted = true;
            }
            UiCommand::CloseEasel => self.close_easel(),
            UiCommand::OpenEaselTab { id } => self.open_easel_tab(id),
            // The easel beside a tab closes its pane; the tab stays.
            UiCommand::CloseEaselTab if self.easel_side() => self.close_easel_side(),
            UiCommand::CloseEaselTab => self.close_easel_tab(),
            UiCommand::OpenLibrary { section } => self.open_library(section.as_deref()),
            UiCommand::CloseLibrary => self.close_library(),
            UiCommand::LibraryOpenFile { id } => {
                let Some(path) = self
                    .downloads
                    .records
                    .iter()
                    .find(|d| d.id == id)
                    .map(|d| std::path::PathBuf::from(&d.path))
                else {
                    return;
                };
                // A PDF, a picture, a video, a sound or a text: in a tab, like other browsers.
                // macOS: WKWebView needs read access granted to file pages, so the system opens it.
                let viewed = !cfg!(target_os = "macos")
                    && path.is_file()
                    && url::Url::from_file_path(&path)
                        .ok()
                        .filter(blackhole_domain::downloads::viewable_file)
                        .map(|url| self.open(url.as_str()))
                        .is_some();
                if !viewed {
                    open_download(&path);
                }
            }
            UiCommand::RevealDownload { id } => {
                if let Some(d) = self.downloads.records.iter().find(|d| d.id == id) {
                    crate::tabs::downloads::reveal_file(std::path::Path::new(&d.path));
                }
            }
            UiCommand::LibraryDeleteDownload { id } => {
                self.downloads.records.retain(|d| d.id != id);
                self.thumbs.borrow_mut().remove(&id);
                self.save_downloads();
                self.ui_library();
            }
            UiCommand::LibraryClear { kind } => {
                match kind.as_str() {
                    "downloads" => {
                        self.downloads.records.clear();
                        self.thumbs.borrow_mut().clear();
                        self.save_downloads();
                    }
                    "archive" => {
                        self.archive.clear();
                        self.save_archive();
                    }
                    "history" => {
                        self.svc.clear_history();
                        self.recall_forget(None);
                    }
                    "highlights" => self.marks_clear(),
                    _ => {}
                }
                self.ui_library();
            }
            UiCommand::ReopenArchived { index } => {
                if index < self.archive.len() {
                    let rec = self.archive.remove(index);
                    self.save_archive();
                    // Back in the space it was closed from, when that space still exists.
                    let space = SpaceId(rec.space);
                    if self.svc.workspace().spaces().iter().any(|s| s.id == space) {
                        self.svc.switch_space(space);
                    }
                    self.open(&rec.url);
                    // And in its folder, when the folder is still there.
                    let ws = self.svc.workspace();
                    let folder = rec
                        .folder
                        .map(FolderId)
                        .filter(|f| ws.active_space().id == space && ws.active_space().folders.iter().any(|x| x.id == *f));
                    let tab = ws.active_tab().filter(|t| t.folder.is_none() && !t.pinned).map(|t| t.id);
                    if let (Some(folder), Some(tab)) = (folder, tab) {
                        self.svc.move_tab(tab, None, Some(folder), false);
                        self.sync();
                    }
                }
            }
            UiCommand::ForgetHistory { url } => {
                if let Ok(url) = Url::parse(&url) {
                    self.svc.forget_history(&url);
                    self.suggest_forget(Some(&url));
                    self.recall_forget(Some(url.as_str()));
                    self.ui_library();
                }
            }
            UiCommand::DeleteArchived { index } => {
                if index < self.archive.len() {
                    let rec = self.archive.remove(index);
                    self.save_archive();
                    self.ui_library();
                    self.trash(&t("toast.archived_removed"), trash::Trashed::Archived(index, rec));
                }
            }
            UiCommand::ClearTabs => {
                let loose: Vec<TabId> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| !t.pinned && t.folder.is_none())
                    .map(|t| t.id)
                    .collect();
                for id in loose {
                    self.archive_tab(id);
                    self.svc.close_tab(id);
                    self.drop_view(id);
                }
                self.sync();
            }
            UiCommand::NewEasel { name } => {
                match self.create_easel(&name) {
                    Some(id) => {
                        self.easel_current = Some(id);
                        self.tag_easel_space();
                    }
                    None => self.toast(&t("toast.too_many_easels")),
                }
                self.easel_changed(true);
            }
            UiCommand::SelectEasel { id } => {
                if self.easels.get(id).is_some() {
                    self.easel_current = Some(id);
                    self.sync_live();
                    self.ui_state();
                }
            }
            UiCommand::RenameEasel { id, name } => {
                let ok = self.easels.rename(id, &name);
                self.easel_changed(ok);
            }
            UiCommand::MoveEasel { id, folder } => {
                let known = folder.is_none()
                    || folder.is_some_and(|f| {
                        self.svc
                            .workspace()
                            .spaces()
                            .iter()
                            .any(|s| s.folders.iter().any(|x| x.id.0 == f))
                    });
                let ok = known && self.easels.set_folder(id, folder);
                self.easel_changed(ok);
            }
            UiCommand::DeleteEasel { id } => {
                let taken = self.easels.take(id);
                self.easel_changed(taken.is_some());
                if let Some((at, easel)) = taken {
                    let name = easel.name.clone();
                    self.trash(&tf("toast.easel_deleted", &[("name", &name)]), trash::Trashed::Easel(at, easel));
                }
            }
            UiCommand::CaptureToEasel => self.capture_to_easel(),
            UiCommand::LivePick => {
                self.pick_watches = false;
                self.live_pick_start();
            }
            UiCommand::CaptureLive { selector, w, h, vw } => {
                let tab = self.svc.workspace().active_tab().map(|t| t.id);
                match tab {
                    Some(tab) if !self.home_shown() => {
                        self.live_picked(tab, selector, (w, h, vw));
                    }
                    _ => self.toast(&t("toast.open_page_for_area")),
                }
            }
            UiCommand::EaselLivePlay { item } => self.live_play(item),
            UiCommand::EaselLiveStop { item } => {
                self.live.remove(&item);
                self.ui_easel();
            }
            UiCommand::EaselLayout { slots, embedded }
                if embedded == (self.easel_tab || self.easel_side()) =>
            {
                self.live_slots = slots.into_iter().map(|s| (s.item, s)).collect();
                self.apply_slots();
            }
            UiCommand::EaselLayout { .. } => {}
            UiCommand::EaselAddNote { x, y } => {
                if let Some(easel) = self.easel_or_create() {
                    let at = x.zip(y);
                    let added = self.easels.add_note(easel, at).is_some();
                    self.easel_changed(added);
                }
            }
            UiCommand::EaselOpenLink { item } => {
                let url = self
                    .current_easel()
                    .and_then(|id| self.easels.get(id))
                    .and_then(|e| e.items.iter().find(|i| i.id == item))
                    .and_then(|i| i.url.clone());
                if let Some(url) = url {
                    self.open(&url);
                    self.toast(&t("toast.opened_in_tab"));
                }
            }
            other => {
                let Some(easel) = self.current_easel() else {
                    return;
                };
                // Text, moves and resizes are already on screen: re-rendering would steal the
                // caret or interrupt a drag.
                let silent = match &other {
                    UiCommand::EaselSetText { item, text } => {
                        self.easels.set_text(easel, *item, text)
                    }
                    UiCommand::EaselMove { item, x, y } => {
                        self.easels.move_item(easel, *item, *x, *y)
                    }
                    UiCommand::EaselResize { item, w, h } => {
                        self.easels.resize_item(easel, *item, *w, *h)
                    }
                    _ => false,
                };
                if silent {
                    self.easel_persist();
                    return;
                }
                let changed = match other {
                    UiCommand::EaselColor { item, color } => {
                        self.easels.set_color(easel, item, &color)
                    }
                    UiCommand::EaselRemove { item } => match self.easels.take_item(easel, item) {
                        Some((at, item)) => {
                            self.easel_changed(true);
                            self.trash(&t("toast.card_removed"), trash::Trashed::Card { easel, at, item });
                            return;
                        }
                        None => false,
                    },
                    _ => false,
                };
                self.easel_changed(changed);
            }
        }
    }
}

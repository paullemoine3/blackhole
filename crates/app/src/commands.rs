//! Commands sent by the shell's pages (`UiCommand`), routed to the module that handles them.

use std::time::Instant;

use blackhole_domain::{FolderId, Removed, SearchEngine, SpaceId, TabId};
use blackhole_infrastructure::{catalog, extensions_dir, import};
use serde_json::json;
use url::Url;
use wry::WebView;

use crate::views;
use crate::features::extensions;
use crate::features::extensions::set_app_extension_enabled;
use crate::features::imports::{import_file, store_bookmarks};
use crate::i18n::{message, t, tf, tn};
use crate::ipc::UiCommand;
use crate::platform::{default_browser_unavailable, drag_window, trace_startup, visit_kind};
use crate::shell::notices;
use crate::shell::layout::{BAR_HEIGHT, card_rect};
use crate::shell::render::TIDY_MIN;
use crate::shell::windows::open_window;
use crate::state::Running;
use crate::tabs::lifecycle::{MAX_BURNERS, SPARES_DELAY};

impl Running {
    pub(crate) fn handle_ui(&mut self, command: UiCommand) {
        match command {
            UiCommand::Ready => {
                trace_startup("an interface page is ready");
                self.sync()
            }
            UiCommand::LibraryReady => {
                self.ui_library();
                if self.library {
                    self.library_intro();
                }
            }
            UiCommand::Open { input, via } => {
                self.next_visit = Some(visit_kind(via.as_deref()));
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
            UiCommand::Palette { mode } => {
                if matches!(mode.as_str(), "url" | "new") {
                    self.open_palette(&mode);
                }
            }
            UiCommand::PaletteReady => {
                self.palette_ready = true;
                if self.palette_open {
                    self.push_palette();
                }
            }
            UiCommand::PaletteClose => self.close_palette(),
            UiCommand::AuthAnswer { id, user, password } => {
                self.answer_auth(id, Some((user, password)));
            }
            UiCommand::AuthCancel { id } => self.answer_auth(id, None),
            UiCommand::ErrorProceed { tab } => self.error_proceed(TabId(tab)),
            UiCommand::Action { id } => self.run_action(&id),
            UiCommand::SaveBoost { css } => self.save_boost(&css),
            UiCommand::ClosePeek => self.close_peek(),
            UiCommand::SetTabSleep { minutes } => {
                self.svc.set_tab_sleep_minutes(minutes);
                self.ui_state();
            }
            UiCommand::SetMaxLiveTabs { tabs } => {
                self.svc.set_max_live_tabs(tabs);
                self.discard_idle_tabs();
                self.ui_state();
            }
            UiCommand::SetSpareViews { count } => {
                self.svc.set_spare_views(count);
                self.trim_spares();
                self.spares_due = Some(Instant::now());
                self.ui_state();
            }
            UiCommand::SetHardwareAcceleration { on } => {
                self.svc.set_hardware_acceleration(on);
                self.ui_state();
            }
            UiCommand::SetTrackerBlocking { on } => {
                self.svc.set_tracker_blocking(on);
                // Spare views carry the blocklist of their creation time.
                self.spares.clear();
                self.spares_due = Some(Instant::now() + SPARES_DELAY);
                self.ui_state();
            }
            UiCommand::SetShield { name, on } => {
                if self.svc.set_shield(&name, on) {
                    // Spare views carry the lists and page scripts of their creation time.
                    self.spares.clear();
                    self.spares_due = Some(Instant::now() + SPARES_DELAY);
                    self.ui_state();
                }
            }
            UiCommand::SetSectionCollapsed { section, collapsed } => {
                self.svc.set_section_collapsed(&section, collapsed);
            }
            UiCommand::SetExtensionsInPrivate { on } => {
                self.svc.set_extensions_in_private(on);
                self.ui_state();
            }
            UiCommand::SetAnimations { on } => {
                self.svc.set_animations(on);
                self.ui_state();
            }
            UiCommand::SetDepth { on } => {
                self.svc.set_depth(on);
                self.ui_state();
                self.ui_easel();
            }
            UiCommand::SetLanguage { language } => {
                if self.svc.set_language(&language) {
                    self.language_changed();
                }
            }
            UiCommand::ClearHistory => {
                self.svc.clear_history();
                self.suggest_forget(None);
                self.recall_forget(None);
                self.ui_state();
            }
            UiCommand::Navigate { input, via } => {
                self.next_visit = Some(visit_kind(via.as_deref()));
                self.navigate(&input);
                self.next_visit = None;
            }
            UiCommand::Suggest { query, seq } => self.suggest_answer(&query, seq),
            UiCommand::SuggestChosen { input, key } => self.suggest_chosen(&input, &key),
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
            UiCommand::Menu {
                origin,
                x,
                y,
                items,
            } => {
                let dx = if origin == "home" || origin == "library" {
                    card_rect(&self.sidebar, &self.window_size()).0
                } else {
                    0.0
                };
                self.menu = Some(json!({"x": x + dx, "y": y + BAR_HEIGHT, "items": items}));
                self.open_palette("menu");
                // A menu of the floating sidebar (the space colour picker above all): the
                // overlay covers the flyout, which must not slide away behind the menu.
                self.sidebar.hold_for_menu(origin == "sidebar");
            }
            UiCommand::DeleteSpace { id } => {
                // Its tabs go to the archive, like those of a deleted folder: nothing is lost.
                let doomed: Vec<TabId> = self
                    .svc
                    .workspace()
                    .spaces()
                    .iter()
                    .find(|s| s.id.0 == id)
                    .map(|s| s.tabs.iter().map(|t| t.id).collect())
                    .unwrap_or_default();
                if self.svc.workspace().spaces().len() > 1 {
                    for &tab in &doomed {
                        self.archive_tab(tab);
                    }
                }
                let snapshot = self.svc.workspace().snapshot_space(SpaceId(id));
                if let Some(tabs) = self.svc.delete_space(SpaceId(id)) {
                    for tab in tabs {
                        self.drop_view(tab);
                    }
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                    self.settings_shown = false;
                    self.sync();
                    if let Some(snapshot @ Removed::Space { .. }) = snapshot {
                        let name = match &snapshot {
                            Removed::Space { space, .. } => space.name.clone(),
                            Removed::Folder { .. } | Removed::Tabs { .. } => String::new(),
                        };
                        self.toast_undo(&tf("toast.space_deleted", &[("name", &name)]), snapshot);
                    }
                }
            }
            UiCommand::UndoRemoval => self.undo_removal(),
            UiCommand::UndoTrash => self.undo_trash(),
            UiCommand::NoticeDone => self.notice_done(),
            UiCommand::PrivacyReport { x, y } => self.privacy_report((x, y + BAR_HEIGHT)),
            UiCommand::WatchPick => {
                self.live_pick_start();
                self.pick_watches = self.live_pick.is_some();
            }
            UiCommand::WatchArea => {
                if let Some((tab, selector, _)) = self.live_pending.take() {
                    self.watch_area(tab, &selector);
                }
            }
            command @ (UiCommand::RecallSearch { .. }
            | UiCommand::SetRecall { .. }
            | UiCommand::RecallExclude { .. }
            | UiCommand::RecallForget { .. }
            | UiCommand::RecallClear) => self.handle_recall(command),
            command @ (UiCommand::SetTrackerLearning { .. }
            | UiCommand::TrackerAllow { .. }
            | UiCommand::TrackerBlock { .. }
            | UiCommand::TrackersForget) => self.handle_trackers(command),
            command @ (UiCommand::HighlightSelection { .. }
            | UiCommand::HighlightRemove { .. }
            | UiCommand::HighlightNote { .. }
            | UiCommand::HighlightEdit { .. }
            | UiCommand::HighlightOpen { .. }
            | UiCommand::HighlightToEasel { .. }) => self.handle_marks(command),
            command @ (UiCommand::SignalAct { .. }
            | UiCommand::SignalDismiss { .. }
            | UiCommand::LinkToEasel { .. }
            | UiCommand::FoundToEasel { .. }
            | UiCommand::WatchToEasel { .. }
            | UiCommand::CompareToEasel { .. }
            | UiCommand::BeamToEasel { .. }
            | UiCommand::EaselWatch { .. }
            | UiCommand::AuthSaved { .. }) => self.handle_bridges(command),
            command @ (UiCommand::WatchRemove { .. }
            | UiCommand::WatchCheck { .. }
            | UiCommand::WatchesSeen) => self.handle_watch(command),
            UiCommand::Tidy => {
                let groups = self.svc.workspace().tidy_suggestion(TIDY_MIN);
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
                if self.dl_live.get(&id).is_some_and(|d| d.finished.is_some()) {
                    self.dl_live.remove(&id);
                    let list = self.downloads_json();
                    self.ui(format!("window.bh&&bh.downloads({list})"));
                }
            }
            command @ (UiCommand::OpenMeet { .. }
            | UiCommand::MeetingNotes { .. }
            | UiCommand::MeetingDismiss { .. }
            | UiCommand::CalendarAdd { .. }
            | UiCommand::CalendarRemove { .. }
            | UiCommand::CalendarSync
            | UiCommand::SetMeetingOption { .. }) => self.handle_calendar(command),
            command @ (UiCommand::DeviceShowCode
            | UiCommand::DeviceJoin { .. }
            | UiCommand::DeviceCancelPairing
            | UiCommand::DeviceForget { .. }
            | UiCommand::DeviceSync { .. }
            | UiCommand::DeviceSyncNow { .. }
            | UiCommand::SetHistorySync { .. }
            | UiCommand::HistorySyncNow
            | UiCommand::SyncAnswer { .. }
            | UiCommand::BeamText { .. }
            | UiCommand::BeamClipboard { .. }
            | UiCommand::BeamFile { .. }
            | UiCommand::BeamAnswer { .. }
            | UiCommand::BeamCopy { .. }
            | UiCommand::SendLink { .. }
            | UiCommand::SendDownload { .. }
            | UiCommand::ContinueOn { .. }) => self.handle_devices(command),
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
            UiCommand::OpenPeek { url } => {
                if let Ok(url) = Url::parse(&url) {
                    self.open_peek(&url);
                }
            }
            UiCommand::LiveCaptureTo { easel } => {
                if let Some((tab, selector, size)) = self.live_pending.take() {
                    self.capture_live(tab, &selector, size, easel);
                }
            }
            UiCommand::SetSpaceColor { id, color } => {
                if self.svc.set_space_color(SpaceId(id), &color) {
                    self.sync();
                }
            }
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
            UiCommand::PinSite { url } => {
                self.open(&url);
                // The site may already be open as a quick access tab: keep it pinned.
                let tab = self.svc.workspace().active_tab().filter(|t| !t.pinned).map(|t| t.id);
                if let Some(id) = tab {
                    self.svc.toggle_pin(id);
                }
                self.sync();
            }
            UiCommand::DismissReleaseNotes => {
                self.release_notes.clear();
                self.ui_home();
            }
            UiCommand::HideFavorite { url } => {
                if let Ok(url) = Url::parse(&url) {
                    self.svc.hide_favorite(&url);
                    self.ui_home();
                }
            }
            UiCommand::CloseSplit => {
                self.svc.clear_split();
                self.sync();
            }
            UiCommand::Pilot { id } => self.toggle_pilot(id.map(TabId)),
            UiCommand::PickIcon { kind, id } => {
                self.icon_target = Some((kind, id));
                self.open_palette("emoji");
            }
            UiCommand::SetSpaceIcon { id, icon } => {
                self.svc.set_space_icon(SpaceId(id), &icon);
                self.ui_state();
            }
            UiCommand::SetFolderIcon { id, icon } => {
                self.svc.set_folder_icon(FolderId(id), &icon);
                self.ui_state();
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
            UiCommand::MediaCtl { id, action } => self.media_ctl(TabId(id), &action),
            UiCommand::NewFolder { parent } => {
                match parent {
                    Some(p) => {
                        let made = self.svc.add_subfolder(&t("palette.new_folder"), FolderId(p));
                        // Show the new subfolder: a collapsed parent would hide it.
                        if self
                            .svc
                            .workspace()
                            .active_space()
                            .folders
                            .iter()
                            .any(|f| f.id == FolderId(p) && f.collapsed)
                        {
                            self.svc.toggle_folder(FolderId(p));
                        }
                        self.ui_state();
                        if let Some(id) = made {
                            self.ui(format!("window.bh&&bh.editFolderId({})", id.0));
                        }
                        return;
                    }
                    None => {
                        self.svc.add_folder(&t("palette.new_folder"));
                    }
                }
                self.ui_state();
            }
            UiCommand::RenameFolder { id, name } => {
                self.svc.rename_folder(FolderId(id), &name);
                self.ui_state();
            }
            UiCommand::RenameSpace { id, name } => {
                self.svc.rename_space(SpaceId(id), &name);
                self.ui_state();
            }
            UiCommand::EditSpace { id } => self.ui(format!("window.bh&&bh.editSpace({id})")),
            UiCommand::RenameTab { id, name } => {
                self.svc.rename_tab(TabId(id), &name);
                self.ui_state();
            }
            UiCommand::EditTab { id } => self.ui(format!("window.bh&&bh.editTab({id})")),
            UiCommand::EditFolder { id } => {
                self.ui(format!("window.bh&&bh.editFolderId({id})"))
            }
            UiCommand::ToggleFolder { id } => {
                self.svc.toggle_folder(FolderId(id));
                self.ui_state();
            }
            UiCommand::MoveFolder { id, parent, before } => {
                self.svc
                    .move_folder(FolderId(id), parent.map(FolderId), before.map(FolderId));
                self.ui_state();
            }
            UiCommand::DeleteFolder { id } => {
                // The folder takes its tabs with it (they go to the archive like any closed tab).
                let doomed = self.svc.workspace().folder_tree(FolderId(id));
                let inside: Vec<TabId> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| t.folder.is_some_and(|f| doomed.contains(&f)))
                    .map(|t| t.id)
                    .collect();
                let snapshot = self.svc.workspace().snapshot_folder(FolderId(id));
                for tab in inside {
                    self.archive_tab(tab);
                    self.svc.close_tab(tab);
                    self.drop_view(tab);
                }
                if self.svc.delete_folder(FolderId(id)) {
                    if let Some(snapshot) = snapshot {
                        let name = match &snapshot {
                            Removed::Folder { folders, .. } => {
                                folders.first().map(|(_, f)| f.name.clone()).unwrap_or_default()
                            }
                            Removed::Space { .. } | Removed::Tabs { .. } => String::new(),
                        };
                        self.toast_undo(&tf("toast.folder_deleted", &[("name", &name)]), snapshot);
                    }
                }
                self.sync();
            }
            UiCommand::Space { id } => {
                self.svc.switch_space(SpaceId(id));
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.settings_shown = false;
                self.sync();
            }
            UiCommand::MoveSpace { id, before } => {
                self.svc.move_space(SpaceId(id), before.map(SpaceId));
                self.ui_state();
            }
            UiCommand::NewSpace { name, color } => {
                let name = if name.trim().is_empty() {
                    t("sidebar.space")
                } else {
                    name
                };
                self.svc.add_space(&name, &color);
                self.sync();
            }
            UiCommand::SetEngine { id } => {
                if let Some(engine) = SearchEngine::from_id(&id) {
                    self.svc.set_search_engine(engine);
                    self.ui_state();
                }
            }
            UiCommand::Lock => self.toggle_sidebar(),
            UiCommand::Peek { on } => {
                self.sidebar.peek(on);
                self.layout(false);
            }
            UiCommand::SetSidebarWidth { width } => self.set_sidebar_width(width),
            UiCommand::SetQuickAccessSize { size } => {
                if self.svc.set_quick_access_size(&size) {
                    self.ui_state();
                    self.ui_settings();
                }
            }
            UiCommand::SetHomeSection { section, shown } => {
                if self.svc.set_home_section(&section, shown) {
                    self.ui_home();
                    self.ui_settings();
                }
            }
            UiCommand::SetHomeOrder { order } => {
                self.svc.set_home_order(&order);
                self.ui_home();
                self.ui_settings();
            }
            UiCommand::FocusContent => self.focus_content(),
            UiCommand::DragWindow => drag_window(&self.window),
            UiCommand::WindowMinimize => self.window.set_minimized(true),
            UiCommand::WindowMaximize => self.window.set_maximized(!self.window.is_maximized()),
            UiCommand::WindowClose => self.quit = true,
            UiCommand::PanelWindow { window, action } => {
                self.panel_window_action(&window, &action)
            }
            UiCommand::Shortcut { key } => self.shortcut(&key),
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
            UiCommand::Passwords { action } => self.password_action(action),
            UiCommand::OpenSettings { tab } => {
                self.settings_tab = tab;
                self.settings_seq += 1;
                self.settings_wanted = true;
            }
            UiCommand::ClearBlocked => {
                self.blocked.clear();
                self.ui_state();
            }
            UiCommand::CloseSettings => self.close_settings(),
            command @ (UiCommand::OpenEasel { .. }
            | UiCommand::CloseEasel
            | UiCommand::OpenEaselTab { .. }
            | UiCommand::CloseEaselTab
            | UiCommand::NewEasel { .. }
            | UiCommand::SelectEasel { .. }
            | UiCommand::RenameEasel { .. }
            | UiCommand::DeleteEasel { .. }
            | UiCommand::MoveEasel { .. }
            | UiCommand::CaptureToEasel
            | UiCommand::LivePick
            | UiCommand::CaptureLive { .. }
            | UiCommand::EaselLivePlay { .. }
            | UiCommand::EaselLiveStop { .. }
            | UiCommand::EaselLayout { .. }
            | UiCommand::EaselAddNote { .. }
            | UiCommand::EaselSetText { .. }
            | UiCommand::EaselMove { .. }
            | UiCommand::EaselResize { .. }
            | UiCommand::EaselColor { .. }
            | UiCommand::EaselRemove { .. }
            | UiCommand::EaselOpenLink { .. }
            | UiCommand::OpenLibrary { .. }
            | UiCommand::CloseLibrary
            | UiCommand::LibraryOpenFile { .. }
            | UiCommand::LibraryDeleteDownload { .. }
            | UiCommand::LibraryClear { .. }
            | UiCommand::ReopenArchived { .. }
            | UiCommand::DeleteArchived { .. }
            | UiCommand::ForgetHistory { .. }
            | UiCommand::LibraryOpenTab { .. }
            | UiCommand::ClearTabs) => self.handle_easel(command),
            UiCommand::InstallCatalog { id } => {
                self.install_catalog(&id);
                // The tour shows the download at once (`installing`).
                if self.onboarding.is_some() {
                    self.ui_home();
                }
            }
            UiCommand::OnboardingOpen { on } => {
                self.onboarding_open = on;
                // The extensions it installs must not open their welcome pages over it.
                let welcome = on && self.onboarding == Some(blackhole_domain::onboarding::Mode::Welcome);
                self.welcome.tour_open(welcome, Instant::now());
                self.sync_dim();
            }
            UiCommand::OnboardingDone => {
                self.onboarding = None;
                self.onboarding_open = false;
                self.welcome.tour_open(false, Instant::now());
                self.default_asked = false;
                self.sync_dim();
                self.ui_home();
            }
            UiCommand::ReplayTour => {
                self.onboarding = Some(blackhole_domain::onboarding::Mode::Tour);
                self.handle_ui(UiCommand::Home);
            }
            UiCommand::SetDefaultBrowser => {
                self.default_asked = blackhole_macos_url::set_default_browser();
                if !self.default_asked {
                    self.toast(&default_browser_unavailable());
                }
                self.ui_home();
            }
            UiCommand::GithubLookup { input } => self.github_lookup(&input),
            UiCommand::GithubPrepare { asset, update } => self.github_prepare(asset, update),
            UiCommand::GithubInstall => self.github_install(),
            UiCommand::GithubCancel => self.github_cancel(),
            UiCommand::CheckCatalogUpdates => {
                // The newest signed list first; the count comes when it has arrived (or failed).
                self.refresh_market(true);
                // And a newer release of every extension added from GitHub.
                self.github_check_updates();
                self.ui_state();
            }
            UiCommand::UpdateAllCatalog => {
                // An update that asks for more than the installed version had waits for its own
                // click (the market shows what it adds): "Update all" leaves it out.
                let mut waiting = 0;
                for id in self.outdated_extensions() {
                    let quiet = self.market.find(&id).is_some_and(|e| self.update_adds(e).is_empty());
                    if quiet {
                        self.install_catalog(&id);
                    } else {
                        waiting += 1;
                    }
                }
                self.catalog_checked = Some(waiting);
                self.ui_state();
            }
            UiCommand::ApplyUpdate => self.start_update(),
            UiCommand::CheckAppUpdate => {
                self.check_for_update();
                self.ui_state();
            }
            UiCommand::ExtAction { id, x, y, w, h: hh } => {
                #[cfg(target_os = "macos")]
                self.show_native_popup(&id, (x, y, w, hh));
                #[cfg(windows)]
                {
                    let url = self.webext.as_ref().and_then(|host| host.popup_url(&id));
                    if let Some(url) = url {
                        self.open_ext_popup(&url, (x, y, w, hh));
                    }
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                let _ = (id, x, y, w, hh);
            }
            UiCommand::CloseExtPopup => {
                #[cfg(windows)]
                self.close_ext_popup();
            }
            UiCommand::ExtAnchors { anchors } => {
                #[cfg(windows)]
                {
                    self.ext_anchors = anchors;
                }
                #[cfg(not(windows))]
                let _ = anchors;
            }
            UiCommand::SetAppExtension { id, on } => {
                if let Some(e) = self.app_ext.iter().find(|e| e.id == id).cloned() {
                    set_app_extension_enabled(&self.data, &id, on);
                    if let Some(host) = &self.webext {
                        if on {
                            host.load(&id, &e.path);
                        } else {
                            host.unload(&id);
                        }
                    }
                    self.ui_settings();
                }
            }
            UiCommand::ImportScan => {
                self.import_sources = import::detect();
                self.ui_settings();
                self.spawn_import(|_, _| None);
            }
            UiCommand::ImportBookmarks { source } => {
                let found = self.import_sources.iter().find(|s| s.id == source).cloned();
                let space = self.svc.workspace().active_space().id.0;
                self.spawn_import(move |bookmarks, _| {
                    Some(match found {
                        Some(s) => match import::read_bookmarks(&s) {
                            Ok(list) => store_bookmarks(bookmarks, list, &s.browser, space),
                            Err(e) => (message("settings.import_failed", &[("from", &s.browser), ("why", &e.to_string())]), Vec::new()),
                        },
                        None => (message("settings.import_no_source", &[]), Vec::new()),
                    })
                });
            }
            UiCommand::AddBookmark { title, url, folder } => {
                let allowed =
                    url::Url::parse(&url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
                self.import_msg = if allowed {
                    let title = if title.trim().is_empty() {
                        url.clone()
                    } else {
                        title
                    };
                    self.store_dropped(vec![import::Bookmark {
                        title,
                        url,
                        folder,
                        space: None,
                    }])
                } else {
                    String::new()
                };
            }
            UiCommand::ImportHtml { text } => {
                let list = import::parse_html(&text);
                self.import_msg = self.store_dropped(list);
            }
            UiCommand::ImportFile { kind } => {
                let space = self.svc.workspace().active_space().id.0;
                let logins = self.passwords.import_sender();
                self.spawn_import(move |bookmarks, _| {
                    Some(import_file(bookmarks, &logins, &kind, space))
                });
            }
            UiCommand::RemoveCatalog { id } => {
                if let Some(host) = &self.webext {
                    host.unload(&id);
                }
                if self.github.find(&id).is_some() {
                    self.github_remove(&id);
                } else {
                    catalog::remove(&self.data, &id);
                }
                self.ui_state();
            }
            UiCommand::SetExtensionEnabled { id, enabled }
                if self.market.find(&id).is_some() || self.github.find(&id).is_some() =>
            {
                self.svc.set_enabled_flag(&id, enabled);
                if let Some(host) = &self.webext {
                    if enabled && catalog::is_installed(&self.data, &id) {
                        host.load(&id, &catalog::package_path(&self.data, &id));
                    } else {
                        host.unload(&id);
                    }
                }
                self.ui_state();
            }
            UiCommand::SetExtensionEnabled { id, enabled } => {
                self.svc.set_extension_enabled(&id, enabled);
                self.reapply_extensions();
                self.ui_state();
            }
            UiCommand::ReloadExtensions => self.reload_extensions(),
            UiCommand::OpenExtensionsFolder => extensions::reveal(&extensions_dir(&self.data)),
            UiCommand::InstallSampleExtension => {
                if extensions::install_sample(&extensions_dir(&self.data)).is_ok() {
                    self.reload_extensions();
                }
            }
        }
    }

    pub(crate) fn shortcut(&mut self, key: &str) {
        match key {
            "escape" => {
                if self.palette_open {
                    self.close_palette();
                } else {
                    self.close_peek();
                }
                return;
            }
            "enter" => {
                if self.peek.is_some() {
                    self.peek_to_tab();
                } else if self.palette_open && self.palette_mode == "boost" {
                    if let Some(palette) = &self.palette {
                        let _ = palette.evaluate_script("window.bh&&bh.save&&bh.save()");
                    }
                }
                return;
            }
            _ if self.peek.is_some() || (self.palette_open && !matches!(key, "l" | "t")) => return,
            _ => {}
        }
        match key {
            "l" => self.open_palette("url"),
            "t" => self.new_tab(),
            "n" => {
                open_window(false, None);
            }
            "shift+n" => {
                open_window(true, None);
            }
            // The Settings are shown like a tab: Mod+W closes them.
            "w" if self.settings_shown => self.close_settings(),
            "w" => self.close_active_tab(),
            "shift+t" => self.reopen_closed(),
            "recent_tab" | "next_tab" | "prev_tab" => self.switch_tab(key),
            _ if key.starts_with("tab_") => {
                let n = key["tab_".len()..].parse().unwrap_or(0);
                if let Some(id) = self.svc.workspace().nth_tab(n) {
                    self.handle_ui(UiCommand::Activate { id: id.0 });
                }
            }
            "r" | "reload" => self.handle_ui(UiCommand::Reload),
            "shift+r" => self.handle_ui(UiCommand::HardReload),
            "b" => self.handle_ui(UiCommand::Lock),
            "[" => self.handle_ui(UiCommand::Back),
            "]" => self.handle_ui(UiCommand::Forward),
            "," => self.settings_wanted = true,
            "shift+e" => self.handle_ui(UiCommand::OpenEasel { id: None }),
            "shift+s" => self.handle_ui(UiCommand::CaptureToEasel),
            "shift+l" => self.handle_ui(UiCommand::LivePick),
            "shift+f" => self.open_palette("recall"),
            "\\" => self.toggle_split(),
            "zoom_in" | "zoom_out" | "zoom_reset" => self.zoom_active(key),
            "f" => self.with_active_tab(|v| {
                let _ = v.evaluate_script("window.__bhFind&&__bhFind()");
            }),
            "g" | "shift+g" => self.with_active_tab(|v| {
                let call = if key == "g" { "next" } else { "prev" };
                let _ = v.evaluate_script(&format!("window.__bhFind&&__bhFind('{call}')"));
            }),
            "p" => self.with_active_tab(|v| {
                let _ = v.print();
            }),
            // The engine's inspector for the page shown: elements and styles, console, network.
            "devtools" => self.with_active_tab(WebView::open_devtools),
            _ => {}
        }
    }
}

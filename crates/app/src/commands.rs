//! Commands sent by the shell's pages (`UiCommand`), routed to the module that handles them.

use blackhole_domain::TabId;
use serde_json::json;
use url::Url;
use wry::WebView;

use crate::ipc::UiCommand;
use crate::platform::{drag_window, trace_startup};
use crate::shell::layout::{BAR_HEIGHT, card_rect};
use crate::shell::windows::open_window;
use crate::state::Running;

impl Running {
    /// Routes a command of the shell's pages: the window's own commands here, the others to the
    /// module that handles them (`handle_tabs`, `handle_settings`, `handle_easel`…).
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
            UiCommand::Palette { mode } => {
                if matches!(mode.as_str(), "url" | "new") {
                    self.open_palette(&mode);
                }
            }
            UiCommand::PaletteReady => {
                self.palette.ready = true;
                if self.palette.open {
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
            UiCommand::Suggest { query, seq } => self.suggest_answer(&query, seq),
            UiCommand::SuggestChosen { input, key } => self.suggest_chosen(&input, &key),
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
                self.palette.menu = Some(json!({"x": x + dx, "y": y + BAR_HEIGHT, "items": items}));
                self.open_palette("menu");
                // A menu of the floating sidebar (the space colour picker above all): the
                // overlay covers the flyout, which must not slide away behind the menu.
                self.sidebar.hold_for_menu(origin == "sidebar");
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
            UiCommand::Pilot { id } => self.toggle_pilot(id.map(TabId)),
            UiCommand::Lock => self.toggle_sidebar(),
            UiCommand::Peek { on } => {
                self.sidebar.peek(on);
                self.layout(false);
            }
            UiCommand::SetSidebarWidth { width } => self.set_sidebar_width(width),
            UiCommand::FocusContent => self.focus_content(),
            UiCommand::DragWindow => drag_window(&self.window),
            UiCommand::WindowMinimize => self.window.set_minimized(true),
            UiCommand::WindowMaximize => self.window.set_maximized(!self.window.is_maximized()),
            UiCommand::WindowClose => self.quit = true,
            UiCommand::PanelWindow { window, action } => {
                self.panel_window_action(&window, &action)
            }
            UiCommand::Shortcut { key } => self.shortcut(&key),
            UiCommand::Passwords { action } => self.password_action(action),
            // Everything else belongs to a module: each one handles its own commands.
            command @ (UiCommand::SignalAct { .. }
            | UiCommand::SignalDismiss { .. }
            | UiCommand::LinkToEasel { .. }
            | UiCommand::FoundToEasel { .. }
            | UiCommand::WatchToEasel { .. }
            | UiCommand::CompareToEasel { .. }
            | UiCommand::BeamToEasel { .. }
            | UiCommand::EaselWatch { .. }
            | UiCommand::AuthSaved { .. }) => self.handle_bridges(command),
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
            command @ (UiCommand::InstallCatalog { .. }
            | UiCommand::GithubLookup { .. }
            | UiCommand::GithubPrepare { .. }
            | UiCommand::GithubInstall
            | UiCommand::GithubCancel
            | UiCommand::CheckCatalogUpdates
            | UiCommand::UpdateAllCatalog
            | UiCommand::ExtAction { .. }
            | UiCommand::CloseExtPopup
            | UiCommand::ExtAnchors { .. }
            | UiCommand::SetAppExtension { .. }
            | UiCommand::RemoveCatalog { .. }
            | UiCommand::SetExtensionEnabled { .. }
            | UiCommand::ReloadExtensions
            | UiCommand::OpenExtensionsFolder
            | UiCommand::InstallSampleExtension) => self.handle_extensions(command),
            command @ (UiCommand::ImportScan
            | UiCommand::ImportBookmarks { .. }
            | UiCommand::AddBookmark { .. }
            | UiCommand::ImportHtml { .. }
            | UiCommand::ImportFile { .. }) => self.handle_imports(command),
            command @ (UiCommand::HighlightSelection { .. }
            | UiCommand::HighlightRemove { .. }
            | UiCommand::HighlightNote { .. }
            | UiCommand::HighlightEdit { .. }
            | UiCommand::HighlightOpen { .. }
            | UiCommand::HighlightToEasel { .. }) => self.handle_marks(command),
            command @ (UiCommand::OnboardingOpen { .. }
            | UiCommand::OnboardingDone
            | UiCommand::ReplayTour
            | UiCommand::SetDefaultBrowser) => self.handle_onboarding(command),
            command @ (UiCommand::DeleteSpace { .. }
            | UiCommand::SetSpaceColor { .. }
            | UiCommand::PinSite { .. }
            | UiCommand::HideFavorite { .. }
            | UiCommand::PickIcon { .. }
            | UiCommand::SetSpaceIcon { .. }
            | UiCommand::SetFolderIcon { .. }
            | UiCommand::NewFolder { .. }
            | UiCommand::RenameFolder { .. }
            | UiCommand::RenameSpace { .. }
            | UiCommand::EditSpace { .. }
            | UiCommand::RenameTab { .. }
            | UiCommand::EditTab { .. }
            | UiCommand::EditFolder { .. }
            | UiCommand::ToggleFolder { .. }
            | UiCommand::MoveFolder { .. }
            | UiCommand::DeleteFolder { .. }
            | UiCommand::Space { .. }
            | UiCommand::MoveSpace { .. }
            | UiCommand::NewSpace { .. }) => self.handle_organize(command),
            command @ (UiCommand::RecallSearch { .. }
            | UiCommand::SetRecall { .. }
            | UiCommand::RecallExclude { .. }
            | UiCommand::RecallForget { .. }
            | UiCommand::RecallClear) => self.handle_recall(command),
            command @ (UiCommand::SetTabSleep { .. }
            | UiCommand::SetMaxLiveTabs { .. }
            | UiCommand::SetSpareViews { .. }
            | UiCommand::SetHardwareAcceleration { .. }
            | UiCommand::SetEncryptedDns { .. }
            | UiCommand::SetDnsResolver { .. }
            | UiCommand::InstallDnsProfile
            | UiCommand::SetTrackerBlocking { .. }
            | UiCommand::SetShield { .. }
            | UiCommand::SetSectionCollapsed { .. }
            | UiCommand::SetExtensionsInPrivate { .. }
            | UiCommand::SetAnimations { .. }
            | UiCommand::SetDepth { .. }
            | UiCommand::SetLanguage { .. }
            | UiCommand::ClearHistory
            | UiCommand::DismissReleaseNotes
            | UiCommand::SetEngine { .. }
            | UiCommand::SetQuickAccessSize { .. }
            | UiCommand::SetHomeSection { .. }
            | UiCommand::SetHomeOrder { .. }
            | UiCommand::OpenSettings { .. }
            | UiCommand::ClearBlocked
            | UiCommand::CloseSettings
            | UiCommand::ApplyUpdate
            | UiCommand::CheckAppUpdate) => self.handle_settings(command),
            command @ (UiCommand::Open { .. }
            | UiCommand::Home
            | UiCommand::NewTab
            | UiCommand::Navigate { .. }
            | UiCommand::Activate { .. }
            | UiCommand::Close { .. }
            | UiCommand::CloseInstance { .. }
            | UiCommand::Split { .. }
            | UiCommand::Tidy
            | UiCommand::Untidy
            | UiCommand::DismissTidy
            | UiCommand::DismissDownload { .. }
            | UiCommand::OpenLink { .. }
            | UiCommand::TabHome { .. }
            | UiCommand::TabAnchor { .. }
            | UiCommand::TabSetHome { .. }
            | UiCommand::ReloadTab { .. }
            | UiCommand::DuplicateTab { .. }
            | UiCommand::MoveTabToSpace { .. }
            | UiCommand::CloseOtherTabs { .. }
            | UiCommand::ClosePane { .. }
            | UiCommand::CloseSplit
            | UiCommand::Pin { .. }
            | UiCommand::OpenAt { .. }
            | UiCommand::MoveTab { .. }
            | UiCommand::TabDropOut { .. }
            | UiCommand::MediaCtl { .. }
            | UiCommand::Back
            | UiCommand::Forward
            | UiCommand::Reload
            | UiCommand::HardReload) => self.handle_tabs(command),
            command @ (UiCommand::SetTrackerLearning { .. }
            | UiCommand::TrackerAllow { .. }
            | UiCommand::TrackerBlock { .. }
            | UiCommand::TrackersForget) => self.handle_trackers(command),
            command @ (UiCommand::WatchRemove { .. }
            | UiCommand::WatchCheck { .. }
            | UiCommand::WatchesSeen) => self.handle_watch(command),
        }
    }

    pub(crate) fn shortcut(&mut self, key: &str) {
        match key {
            "escape" => {
                if self.palette.open {
                    self.close_palette();
                } else {
                    self.close_peek();
                }
                return;
            }
            "enter" => {
                if self.peek.is_some() {
                    self.peek_to_tab();
                } else if self.palette.open && self.palette.mode == "boost" {
                    if let Some(palette) = &self.palette.view {
                        let _ = palette.evaluate_script("window.bh&&bh.save&&bh.save()");
                    }
                }
                return;
            }
            _ if self.peek.is_some() || (self.palette.open && !matches!(key, "l" | "t")) => return,
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

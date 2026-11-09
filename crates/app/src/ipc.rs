//! Messages flowing from the web views into the shell. Handlers run on the event-loop thread, so
//! they only enqueue; `Running::pump` drains the queue and mutates state.

use blackhole_domain::TabId;
use serde::Deserialize;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use url::Url;
use winit::event_loop::EventLoopProxy;

/// Commands sent by BlackHole's own pages (sidebar, top bar, start page).
#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum UiCommand {
    Ready,
    LibraryReady,
    Open {
        input: String,
    },
    Home,
    ClearHistory,
    NewTab,
    SetAnimations {
        on: bool,
    },
    /// Interface language: a code of `i18n::LANGUAGES` or "system".
    SetLanguage {
        language: String,
    },
    SetHardwareAcceleration {
        on: bool,
    },
    SetExtensionsInPrivate {
        on: bool,
    },
    SetTrackerBlocking {
        on: bool,
    },
    SetSectionCollapsed {
        section: String,
        collapsed: bool,
    },
    SetTabSleep {
        minutes: u32,
    },
    SetMaxLiveTabs {
        tabs: u32,
    },
    SetSpareViews {
        count: u32,
    },
    Navigate {
        input: String,
    },
    Activate {
        id: u64,
    },
    Close {
        id: u64,
    },
    /// Frees a tab's page (a pinned one stays in the sidebar) and goes back to the start page.
    CloseInstance {
        id: u64,
    },
    Pin {
        id: u64,
    },
    /// A bookmark dropped on the tab list: opens it there (position as in `MoveTab`).
    OpenAt {
        url: String,
        before: Option<u64>,
        folder: Option<u64>,
        pinned: bool,
    },
    MoveTab {
        id: u64,
        before: Option<u64>,
        folder: Option<u64>,
        pinned: bool,
    },
    /// Creates a folder, nested in `parent` when given.
    NewFolder {
        #[serde(default)]
        parent: Option<u64>,
    },
    RenameFolder {
        id: u64,
        name: String,
    },
    RenameSpace {
        id: u64,
        name: String,
    },
    /// Asks the sidebar to turn a space's name into an input.
    EditSpace {
        id: u64,
    },
    RenameTab {
        id: u64,
        name: String,
    },
    /// Asks the sidebar to turn a tab's title into an input.
    EditTab {
        id: u64,
    },
    /// Asks the sidebar to turn a folder's name into an input.
    EditFolder {
        id: u64,
    },
    /// Tab menu: reloads that tab (not only the one shown).
    ReloadTab {
        id: u64,
    },
    /// Tab menu: opens the same page in a new tab.
    DuplicateTab {
        id: u64,
    },
    /// Tab menu: moves the tab to another space (its view is rebuilt on that space's profile).
    MoveTabToSpace {
        id: u64,
        space: u64,
    },
    /// Tab menu: closes the other loose tabs of the space (quick access and folders are kept).
    CloseOtherTabs {
        id: u64,
    },
    SetSpaceColor {
        id: u64,
        color: String,
    },
    /// Puts back the space or folder deleted a moment ago (the toast's Undo button).
    UndoRemoval,
    /// Link menu: `target` is "tab" (background), "front" (new tab shown), "window" or "private".
    OpenLink {
        url: String,
        target: String,
    },
    /// The area just picked becomes a watched area (the picker's menu).
    WatchArea,
    /// Library > Watches: stop watching, read again now, everything seen.
    WatchRemove {
        id: u64,
    },
    WatchCheck {
        id: u64,
    },
    WatchesSeen,
    /// Files the loose tabs into folders by site (the sidebar's suggestion); `Untidy` undoes
    /// it, `DismissTidy` hides the suggestion for this session.
    Tidy,
    Untidy,
    DismissTidy,
    /// The sidebar's shield: the privacy report of the site shown, as a menu at (x, y).
    PrivacyReport {
        x: f64,
        y: f64,
    },
    /// Opens Google Meet: a room (`url`), or a new meeting. Reuses the Meet tab of quick access
    /// or of a folder when there is one (never one in a call).
    OpenMeet {
        #[serde(default)]
        url: Option<String>,
    },
    /// The toast offered on joining a calendar meeting: its notes in an Easel beside the call.
    MeetingNotes {
        url: String,
        start: i64,
    },
    /// Settings > Calendar: a secret iCal address of Google Calendar.
    CalendarAdd {
        url: String,
    },
    CalendarRemove {
        index: usize,
    },
    CalendarSync,
    /// Settings > Calendar: `reminders` or `card`.
    SetMeetingOption {
        name: String,
        on: bool,
    },
    /// Settings > Privacy: `name` is `ads`, `hide_ads`, `video_ads` or `cookie_banners`.
    SetShield {
        name: String,
        on: bool,
    },
    /// Settings > Devices: shows a pairing code, or pairs with the code of the other device.
    DeviceShowCode,
    DeviceJoin {
        code: String,
    },
    DeviceCancelPairing,
    DeviceForget {
        id: String,
    },
    /// Settings > Devices: tab synchronization with a device, `ask`, `auto` or `off`.
    DeviceSync {
        id: String,
        mode: String,
    },
    /// Settings > Devices: sends this device's tabs and asks for the other's.
    DeviceSyncNow {
        id: String,
    },
    /// The proposal to synchronize with a device: `merge` (once), `always` or `later`.
    SyncAnswer {
        id: String,
        answer: String,
    },
    /// Tab menu and command bar: opens the tab on a paired device.
    ContinueOn {
        id: u64,
        device: String,
    },
    /// Link menu: previews the link in a floating window (Peek).
    OpenPeek {
        url: String,
    },
    /// The area picked for a live capture goes to this Easel (`None`: a new one).
    LiveCaptureTo {
        easel: Option<u64>,
    },
    ToggleFolder {
        id: u64,
    },
    DeleteFolder {
        id: u64,
    },
    /// Drag & drop in the sidebar: moves a folder under `parent` (`null`: top level), before
    /// the sibling `before` (`null` puts it last).
    MoveFolder {
        id: u64,
        #[serde(default)]
        parent: Option<u64>,
        #[serde(default)]
        before: Option<u64>,
    },
    Space {
        id: u64,
    },
    MoveSpace {
        id: u64,
        before: Option<u64>,
    },
    Split {
        id: u64,
    },
    CloseSplit,
    PickIcon {
        kind: String,
        id: u64,
    },
    SetSpaceIcon {
        id: u64,
        icon: String,
    },
    SetFolderIcon {
        id: u64,
        icon: String,
    },
    NewSpace {
        name: String,
        color: String,
    },
    SetEngine {
        id: String,
    },
    Lock,
    Peek {
        on: bool,
    },
    SetSidebarWidth {
        width: u32,
    },
    Palette {
        mode: String,
    },
    PaletteReady,
    PaletteClose,
    /// The sign-in prompt of an HTTP authentication (palette): credentials for the server.
    /// Never logged, never stored: they go straight to the engine.
    AuthAnswer {
        id: u64,
        user: String,
        password: String,
    },
    AuthCancel {
        id: u64,
    },
    /// From the shell's own menu on an error page: go on despite the certificate, open the
    /// http version, or let the blocked site through, for this run of the app only. Honoured
    /// only for the error page the shell shows in that tab (never sent by a web page).
    ErrorProceed {
        tab: u64,
    },
    Action {
        id: String,
    },
    SaveBoost {
        css: String,
    },
    ClosePeek,
    CloseExtPopup,
    FocusContent,
    DragWindow,
    WindowMinimize,
    WindowMaximize,
    WindowClose,
    /// The top bar of a secondary window ("settings" or "easel") drawn by its page: `action` is
    /// "drag", "minimize", "maximize" (toggles) or "close".
    PanelWindow {
        window: String,
        action: String,
    },
    Shortcut {
        key: String,
    },
    Back,
    Forward,
    Reload,
    /// Settings › Passwords and the save toast (see `passwords::PasswordAction`).
    Passwords {
        action: crate::passwords::PasswordAction,
    },
    OpenSettings {
        /// Section to show (`privacy`, `extensions`…).
        #[serde(default)]
        tab: Option<String>,
    },
    ClearBlocked,
    CloseSettings,
    SetExtensionEnabled {
        id: String,
        enabled: bool,
    },
    ReloadExtensions,
    OpenExtensionsFolder,
    InstallSampleExtension,
    /// Download, verify and install a catalog extension (real WebExtension).
    InstallCatalog {
        id: String,
    },
    /// Compares installed catalog extensions with the versions pinned by this build.
    CheckCatalogUpdates,
    /// Updates every outdated catalog extension.
    UpdateAllCatalog,
    /// Sidebar update button: installs the release found by the background check.
    ApplyUpdate,
    /// Toolbar click on an extension: opens its popup under the button (window coordinates).
    ExtAction {
        id: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// Enable or remove an extension found inside an installed app (Safari web extension).
    SetAppExtension {
        id: String,
        on: bool,
    },
    /// Refresh the list of browsers whose data can be imported.
    ImportScan,
    /// Import the bookmarks of a detected browser profile.
    ImportBookmarks {
        source: String,
    },
    /// A link dropped on the sidebar becomes a bookmark (`folder` is a `/`-separated path).
    AddBookmark {
        title: String,
        url: String,
        #[serde(default)]
        folder: String,
    },
    /// Bookmarks export (Netscape HTML) dropped on the sidebar.
    ImportHtml {
        text: String,
    },
    /// Pick an export file: `kind` is "bookmarks" (HTML) or "logins" (CSV).
    ImportFile {
        kind: String,
    },
    /// Context menu requested by a UI page: shown by the command-bar overlay, which is the only
    /// view as large as the window. `items` is rendered as-is; each item's `run` is a `UiCommand`.
    Menu {
        origin: String,
        x: f64,
        y: f64,
        items: serde_json::Value,
    },
    DeleteSpace {
        id: u64,
    },
    /// Closes one pane of the split view (`left` or `right`); the tab stays in the sidebar.
    ClosePane {
        side: String,
    },
    /// Opens a site and pins it.
    PinSite {
        url: String,
    },
    /// Removes a built-in favorite from the start page.
    HideFavorite {
        url: String,
    },
    DismissReleaseNotes,
    RemoveCatalog {
        id: String,
    },
    /// Opens (or focuses) the Easel window, optionally on a given easel.
    OpenEasel {
        #[serde(default)]
        id: Option<u64>,
    },
    CloseEasel,
    /// Shows an easel in the content area, like a tab.
    OpenEaselTab {
        id: u64,
    },
    CloseEaselTab,
    /// A tab was released outside the sidebar; the point is in screen coordinates. When another
    /// window lies under it, the tab moves there.
    TabDropOut {
        id: u64,
        x: f64,
        y: f64,
    },
    /// Controls the audio/video of a tab from the sidebar card: toggle, mute, pip, cam, mic or close.
    MediaCtl {
        id: u64,
        action: String,
    },
    /// Shows the Library (media, downloads, easels, spaces, archived tabs) over the window.
    OpenLibrary,
    CloseLibrary,
    LibraryOpenFile {
        id: u64,
    },
    LibraryDeleteDownload {
        id: u64,
    },
    /// Empties the download history, the archive or the browsing history.
    LibraryClear {
        kind: String,
    },
    ReopenArchived {
        index: usize,
    },
    DeleteArchived {
        index: usize,
    },
    /// Removes one page from the browsing history (Library → History).
    ForgetHistory {
        url: String,
    },
    LibraryOpenTab {
        space: u64,
        id: u64,
    },
    /// Closes the loose (not pinned, not in a folder) tabs of the active space.
    ClearTabs,
    NewEasel {
        #[serde(default)]
        name: String,
    },
    SelectEasel {
        id: u64,
    },
    RenameEasel {
        id: u64,
        name: String,
    },
    DeleteEasel {
        id: u64,
    },
    /// Drag & drop in the sidebar: files an easel in a folder (`null`: loose).
    MoveEasel {
        id: u64,
        #[serde(default)]
        folder: Option<u64>,
    },
    /// Clips the page in front (address, title, selected text) to the current easel.
    CaptureToEasel,
    EaselAddNote {
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
    },
    EaselSetText {
        item: u64,
        text: String,
    },
    EaselMove {
        item: u64,
        x: f64,
        y: f64,
    },
    EaselResize {
        item: u64,
        w: f64,
        h: f64,
    },
    EaselColor {
        item: u64,
        color: String,
    },
    EaselRemove {
        item: u64,
    },
    /// A clip's link was clicked: opens it in a tab of the main window.
    EaselOpenLink {
        item: u64,
    },
    /// Starts the area picker on the page in front (live capture).
    LivePick,
    /// Live-captures the region `selector` (`w` x `h` page pixels) of the page in front.
    CaptureLive {
        selector: String,
        #[serde(default)]
        w: f64,
        #[serde(default)]
        h: f64,
    },
    EaselLivePlay {
        item: u64,
    },
    EaselLiveStop {
        item: u64,
    },
    /// Where each live card's slot is in the easel window (window coordinates).
    EaselLayout {
        slots: Vec<LiveSlot>,
        /// Sent by the page shown in the content area rather than the Easel window.
        #[serde(default)]
        embedded: bool,
    },
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct LiveSlot {
    pub item: u64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub visible: bool,
}

/// Events a web page may raise. Pages are untrusted: this is deliberately a tiny vocabulary
/// and none of it carries data the shell trusts (the URL is re-read from the engine).
#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum PageEvent {
    Shortcut {
        key: String,
    },
    Location,
    /// Console errors and failed requests, printed by debug builds only.
    DebugLog {
        #[serde(default)]
        msg: String,
    },
    /// The icon the page declares for itself (untrusted: validated by `Favicons`).
    Favicon {
        url: String,
    },
    /// A request to `host` was blocked inside the page (untrusted, sanitised by `BlockLog`).
    Blocked {
        #[serde(default)]
        host: String,
    },
    /// Shift+click on a link: preview it in a floating window.
    Peek {
        url: String,
    },
    /// Middle click or Ctrl/Cmd+click on a link: open it in a background tab (http(s) only).
    OpenLink {
        url: String,
    },
    /// Right click on a link or on selected text: the shell shows its menu at (x, y) in the
    /// page. `url` is empty off a link; `text` (untrusted, bounded by `clean_selection`) is the
    /// selection under the pointer.
    LinkMenu {
        #[serde(default)]
        url: String,
        #[serde(default)]
        text: String,
        x: f64,
        y: f64,
    },
    /// A login field got focus: ask for the saved usernames of this origin (re-read from the
    /// engine). `fresh`: a new-password field (sign-up, change of password).
    AutofillQuery {
        #[serde(default)]
        fresh: bool,
    },
    /// The user picked the `index`-th suggestion from the last `AutofillQuery`. `token`: the
    /// one-time value the shell put in that menu (a page script posting this itself lacks it).
    AutofillFill {
        index: usize,
        #[serde(default)]
        token: String,
    },
    /// "Unlock saved passwords…" was clicked in the menu (only after a locked query).
    AutofillUnlock {
        #[serde(default)]
        token: String,
    },
    /// "Use a strong password" was clicked (only after a `fresh` query).
    AutofillGenerate {
        #[serde(default)]
        token: String,
    },
    /// A login form was submitted. The page already knows these credentials; the shell only
    /// offers to save them, and the password is wiped when the event is dropped.
    LoginSubmitted {
        #[serde(default)]
        username: String,
        password: crate::passwords::Secret,
    },
    /// The risky button of BlackHole's error page: the shell opens its own menu at (x, y) to
    /// ask. Acted on only while that tab shows the error page; it grants nothing by itself.
    ErrorMenu {
        x: f64,
        y: f64,
    },
    /// A video left Picture in Picture (untrusted: only acted on for a tab the shell floated).
    PipLeft {},
    /// The user clicked an area while the picker was armed (honoured only then; untrusted).
    LivePick {
        selector: String,
        #[serde(default)]
        w: f64,
        #[serde(default)]
        h: f64,
    },
}

pub enum Msg {
    Ui(UiCommand),
    Page(TabId, PageEvent),
    Url(TabId, Url),
    /// A page started loading: the progress bar runs until `Loaded`.
    LoadStart(TabId),
    /// The page finished loading: extensions are injected now that the DOM exists.
    Loaded(TabId, Url),
    /// The page could not be reached: the tab shows the error page.
    Failed(TabId, Url),
    /// Why a navigation failed (before `Failed`), or its HTTP error status (before `Loaded`).
    LoadFailure(TabId, wry::LoadFailure),
    /// A certificate the engine refused (details for the error page).
    ServerTrust(TabId, wry::ServerTrustChallenge),
    /// The http address the shell upgraded to https in this tab.
    Upgraded(TabId, Url),
    /// The tab itself was sent to a blocked tracker or ad server.
    BlockedPage(TabId, Url, &'static str),
    /// The server asks for a user name and password (HTTP Basic or Digest).
    Auth(TabId, wry::HttpAuthChallenge, wry::HttpAuthResponder),
    Title(TabId, String),
    /// A page asked for a new window (`target=_blank`, `window.open`).
    OpenTab(Url),
    /// A link opened from another application (we are the default browser).
    External(Url),
    Redirect(TabId, Url),
    DownloadStarted {
        url: String,
        path: std::path::PathBuf,
    },
    DownloadDone {
        url: String,
        path: Option<std::path::PathBuf>,
        ok: bool,
    },
    Blocked {
        why: &'static str,
        host: String,
    },
}

/// Wakes the event loop so queued messages are handled promptly.
#[derive(Debug)]
pub struct Wake;

#[derive(Clone)]
pub struct Mailbox {
    queue: Rc<RefCell<VecDeque<Msg>>>,
    proxy: EventLoopProxy<Wake>,
}

impl Mailbox {
    pub fn new(proxy: EventLoopProxy<Wake>) -> Self {
        Self {
            queue: Rc::default(),
            proxy,
        }
    }

    pub fn push(&self, msg: Msg) {
        self.queue.borrow_mut().push_back(msg);
        let _ = self.proxy.send_event(Wake);
    }

    pub fn pop(&self) -> Option<Msg> {
        self.queue.borrow_mut().pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_popup_commands_parse() {
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"ext_action","id":"x","x":1,"y":2,"w":3,"h":4}"#)
                .unwrap();
        assert!(matches!(c, UiCommand::ExtAction { id, w, .. } if id == "x" && w == 3.0));
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"set_app_extension","id":"app-a","on":true}"#).unwrap();
        assert!(matches!(c, UiCommand::SetAppExtension { on: true, .. }));
    }

    #[test]
    fn folder_commands_parse() {
        let c: UiCommand = serde_json::from_str(r#"{"cmd":"new_folder"}"#).unwrap();
        assert!(matches!(c, UiCommand::NewFolder { parent: None }));
        let c: UiCommand = serde_json::from_str(r#"{"cmd":"new_folder","parent":4}"#).unwrap();
        assert!(matches!(c, UiCommand::NewFolder { parent: Some(4) }));
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"move_folder","id":2,"parent":1,"before":null}"#)
                .unwrap();
        assert!(matches!(
            c,
            UiCommand::MoveFolder {
                id: 2,
                parent: Some(1),
                before: None
            }
        ));
    }

    #[test]
    fn drop_commands_parse() {
        let c: UiCommand = serde_json::from_str(
            r#"{"cmd":"open_at","url":"https://a.com","before":null,"folder":null,"pinned":true}"#,
        )
        .unwrap();
        assert!(matches!(c, UiCommand::OpenAt { pinned: true, .. }));
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"add_bookmark","title":"T","url":"https://a.com"}"#)
                .unwrap();
        assert!(matches!(c, UiCommand::AddBookmark { folder, .. } if folder.is_empty()));
        assert!(matches!(
            serde_json::from_str::<UiCommand>(r#"{"cmd":"import_html","text":"x"}"#).unwrap(),
            UiCommand::ImportHtml { .. }
        ));
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"tab_drop_out","id":4,"x":1500.5,"y":30}"#).unwrap();
        assert!(matches!(c, UiCommand::TabDropOut { id: 4, .. }));
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"media_ctl","id":4,"action":"mute"}"#).unwrap();
        assert!(matches!(c, UiCommand::MediaCtl { id: 4, .. }));
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"rename_space","id":2,"name":"Work"}"#).unwrap();
        assert!(matches!(c, UiCommand::RenameSpace { id: 2, .. }));
        assert!(matches!(
            serde_json::from_str::<UiCommand>(r#"{"cmd":"edit_space","id":2}"#).unwrap(),
            UiCommand::EditSpace { id: 2 }
        ));
    }

    #[test]
    fn import_commands_parse() {
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"import_file","kind":"logins"}"#).unwrap();
        assert!(matches!(c, UiCommand::ImportFile { kind } if kind == "logins"));
        assert!(matches!(
            serde_json::from_str::<UiCommand>(r#"{"cmd":"import_scan"}"#).unwrap(),
            UiCommand::ImportScan
        ));
    }

    #[test]
    fn catalog_commands_parse() {
        let install: UiCommand =
            serde_json::from_str(r#"{"cmd":"install_catalog","id":"x"}"#).unwrap();
        assert!(matches!(install, UiCommand::InstallCatalog { id } if id == "x"));
        let remove: UiCommand =
            serde_json::from_str(r#"{"cmd":"remove_catalog","id":"x"}"#).unwrap();
        assert!(matches!(remove, UiCommand::RemoveCatalog { id } if id == "x"));
    }

    #[test]
    fn easel_commands_parse_and_pages_cannot_send_them() {
        let c: UiCommand = serde_json::from_str(r#"{"cmd":"easel_add_note"}"#).unwrap();
        assert!(matches!(c, UiCommand::EaselAddNote { x: None, y: None }));
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"easel_move","item":2,"x":1.5,"y":-3}"#).unwrap();
        assert!(matches!(c, UiCommand::EaselMove { item: 2, .. }));
        assert!(matches!(
            serde_json::from_str::<UiCommand>(r#"{"cmd":"capture_to_easel"}"#).unwrap(),
            UiCommand::CaptureToEasel
        ));
        assert!(matches!(
            serde_json::from_str::<UiCommand>(r#"{"cmd":"open_easel_tab","id":4}"#).unwrap(),
            UiCommand::OpenEaselTab { id: 4 }
        ));
        assert!(serde_json::from_str::<PageEvent>(r#"{"cmd":"open_easel_tab","id":4}"#).is_err());
        let notes = r#"{"cmd":"meeting_notes","url":"https://meet.google.com/abc-defg-hij","start":1700000000}"#;
        assert!(matches!(
            serde_json::from_str::<UiCommand>(notes).unwrap(),
            UiCommand::MeetingNotes { start: 1_700_000_000, .. }
        ));
        assert!(serde_json::from_str::<PageEvent>(notes).is_err());
        assert!(serde_json::from_str::<PageEvent>(r#"{"cmd":"capture_to_easel"}"#).is_err());
        assert!(
            serde_json::from_str::<PageEvent>(r#"{"cmd":"easel_open_link","item":1}"#).is_err()
        );
    }

    #[test]
    fn live_commands_parse_and_pages_cannot_drive_the_easel() {
        let c: UiCommand = serde_json::from_str(
            r#"{"cmd":"easel_layout","slots":[{"item":3,"x":1,"y":2,"w":3,"h":4,"visible":true}]}"#,
        )
        .unwrap();
        assert!(
            matches!(c, UiCommand::EaselLayout { slots, embedded: false } if slots.len() == 1 && slots[0].visible)
        );
        let c: UiCommand =
            serde_json::from_str(r#"{"cmd":"capture_live","selector":"h1"}"#).unwrap();
        assert!(matches!(c, UiCommand::CaptureLive { w, .. } if w == 0.0));
        let e: PageEvent =
            serde_json::from_str(r#"{"cmd":"live_pick","selector":"h1","w":9,"h":4}"#).unwrap();
        assert!(matches!(e, PageEvent::LivePick { .. }));
        assert!(
            serde_json::from_str::<PageEvent>(r#"{"cmd":"capture_live","selector":"h1"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<PageEvent>(r#"{"cmd":"easel_live_play","item":1}"#).is_err()
        );
    }

    #[test]
    fn link_events_parse_and_pages_cannot_open_windows_themselves() {
        let open: PageEvent =
            serde_json::from_str(r#"{"cmd":"open_link","url":"https://a.test/"}"#).unwrap();
        assert!(matches!(open, PageEvent::OpenLink { .. }));
        let menu: PageEvent =
            serde_json::from_str(r#"{"cmd":"link_menu","url":"https://a.test/","x":3,"y":4}"#)
                .unwrap();
        assert!(matches!(menu, PageEvent::LinkMenu { x, .. } if x == 3.0));
        let selected: PageEvent =
            serde_json::from_str(r#"{"cmd":"link_menu","text":"black hole","x":3,"y":4}"#)
                .unwrap();
        assert!(
            matches!(selected, PageEvent::LinkMenu { url, text, .. } if url.is_empty() && text == "black hole")
        );
        // Opening a window or picking the Easel is for the shell's own pages only: from a page,
        // `target` does not exist (the event only opens a background tab).
        let window = r#"{"cmd":"open_link","url":"https://a.test/","target":"window"}"#;
        assert!(matches!(
            serde_json::from_str::<UiCommand>(window),
            Ok(UiCommand::OpenLink { target, .. }) if target == "window"
        ));
        assert!(matches!(
            serde_json::from_str::<PageEvent>(window),
            Ok(PageEvent::OpenLink { .. })
        ));
        for raw in [
            r#"{"cmd":"open_meet"}"#,
            r#"{"cmd":"open_meet","url":"https://meet.google.com/abc-defg-hij"}"#,
            r#"{"cmd":"calendar_add","url":"https://calendar.google.com/calendar/ical/a/private-b/basic.ics"}"#,
            r#"{"cmd":"calendar_remove","index":0}"#,
            r#"{"cmd":"calendar_sync"}"#,
            r#"{"cmd":"set_meeting_option","name":"card","on":false}"#,
            r#"{"cmd":"live_capture_to","easel":null}"#,
            r#"{"cmd":"live_capture_to","easel":4}"#,
            r#"{"cmd":"open_peek","url":"https://a.test/"}"#,
            r#"{"cmd":"device_show_code"}"#,
            r#"{"cmd":"device_join","code":"ABCD-EFGH-JKMN"}"#,
            r#"{"cmd":"device_cancel_pairing"}"#,
            r#"{"cmd":"device_forget","id":"ab12"}"#,
            r#"{"cmd":"continue_on","id":3,"device":"ab12"}"#,
        ] {
            assert!(serde_json::from_str::<UiCommand>(raw).is_ok(), "{raw}");
            assert!(serde_json::from_str::<PageEvent>(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn pip_left_parses_with_or_without_extra_fields() {
        for raw in [r#"{"cmd":"pip_left"}"#, r#"{"cmd":"pip_left","paused":true}"#] {
            assert!(matches!(serde_json::from_str::<PageEvent>(raw), Ok(PageEvent::PipLeft {})), "{raw}");
        }
    }

    #[test]
    fn tab_menu_commands_parse_and_pages_cannot_send_them() {
        let cases = [
            r#"{"cmd":"reload_tab","id":1}"#,
            r#"{"cmd":"duplicate_tab","id":1}"#,
            r#"{"cmd":"move_tab_to_space","id":1,"space":2}"#,
            r#"{"cmd":"close_other_tabs","id":1}"#,
            r##"{"cmd":"set_space_color","id":1,"color":"#ff5c8a"}"##,
            r#"{"cmd":"undo_removal"}"#,
        ];
        for raw in cases {
            assert!(serde_json::from_str::<UiCommand>(raw).is_ok(), "{raw}");
            assert!(serde_json::from_str::<PageEvent>(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn edit_folder_parses_and_pages_cannot_send_it() {
        let edit = r#"{"cmd":"edit_folder","id":4}"#;
        assert!(matches!(
            serde_json::from_str::<UiCommand>(edit),
            Ok(UiCommand::EditFolder { id: 4 })
        ));
        assert!(serde_json::from_str::<PageEvent>(edit).is_err());
    }

    #[test]
    fn history_commands_parse_and_pages_cannot_send_them() {
        let forget = r#"{"cmd":"forget_history","url":"https://a.test/"}"#;
        assert!(matches!(
            serde_json::from_str::<UiCommand>(forget),
            Ok(UiCommand::ForgetHistory { url }) if url == "https://a.test/"
        ));
        assert!(serde_json::from_str::<PageEvent>(forget).is_err());
    }

    #[test]
    fn pages_cannot_send_catalog_commands() {
        assert!(
            serde_json::from_str::<PageEvent>(r#"{"cmd":"install_catalog","id":"x"}"#).is_err()
        );
    }

    #[test]
    fn password_events_parse_and_pages_cannot_drive_the_wallet() {
        use crate::passwords::PasswordAction;
        // Older pages send the query without `fresh`.
        assert!(matches!(
            serde_json::from_str::<PageEvent>(r#"{"cmd":"autofill_query"}"#),
            Ok(PageEvent::AutofillQuery { fresh: false })
        ));
        let submitted = r#"{"cmd":"login_submitted","username":"u","password":"pw-test"}"#;
        assert!(matches!(
            serde_json::from_str::<PageEvent>(submitted),
            Ok(PageEvent::LoginSubmitted { password, .. }) if password.as_str() == "pw-test"
        ));
        let reveal = r#"{"cmd":"passwords","action":{"op":"reveal","id":"a"}}"#;
        assert!(matches!(
            serde_json::from_str::<UiCommand>(reveal),
            Ok(UiCommand::Passwords { action: PasswordAction::Reveal { id } }) if id == "a"
        ));
        assert!(serde_json::from_str::<PageEvent>(reveal).is_err());
        let unlock = r#"{"cmd":"passwords","action":{"op":"unlock","password":"pw-test"}}"#;
        assert!(serde_json::from_str::<UiCommand>(unlock).is_ok());
        assert!(serde_json::from_str::<PageEvent>(unlock).is_err());
    }
}

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
    Open {
        input: String,
    },
    Home,
    ClearHistory,
    NewTab,
    SetAnimations {
        on: bool,
    },
    SetHardwareAcceleration {
        on: bool,
    },
    SetExtensionsInPrivate {
        on: bool,
    },
    SetTabSleep {
        minutes: u32,
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
    Shortcut {
        key: String,
    },
    Back,
    Forward,
    Reload,
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
    /// Controls the audio/video of a tab from the sidebar card: toggle, mute, pip or close.
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
    /// Empties the download history or the archive.
    LibraryClear {
        kind: String,
    },
    ReopenArchived {
        index: usize,
    },
    DeleteArchived {
        index: usize,
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
    /// A login field got focus: ask for the saved usernames of this site.
    AutofillQuery,
    /// The user picked the `index`-th suggestion from the last `AutofillQuery`.
    AutofillFill {
        index: usize,
    },
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
    /// The page finished loading: extensions are injected now that the DOM exists.
    Loaded(TabId, Url),
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
    fn pages_cannot_send_catalog_commands() {
        assert!(
            serde_json::from_str::<PageEvent>(r#"{"cmd":"install_catalog","id":"x"}"#).is_err()
        );
    }
}

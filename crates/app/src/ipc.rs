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
    NewFolder,
    RenameFolder {
        id: u64,
        name: String,
    },
    ToggleFolder {
        id: u64,
    },
    DeleteFolder {
        id: u64,
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
        assert!(serde_json::from_str::<PageEvent>(r#"{"cmd":"capture_to_easel"}"#).is_err());
        assert!(
            serde_json::from_str::<PageEvent>(r#"{"cmd":"easel_open_link","item":1}"#).is_err()
        );
    }

    #[test]
    fn pages_cannot_send_catalog_commands() {
        assert!(
            serde_json::from_str::<PageEvent>(r#"{"cmd":"install_catalog","id":"x"}"#).is_err()
        );
    }
}

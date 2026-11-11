//! The JSON protocol between the three sides of the Android app.
//!
//! - [`MobileCommand`]: what the shell interface (`ui/mobile.html`, trusted, our own page) asks.
//!   The desktop's `UiCommand`.
//! - [`PageEvent`]: what a tab's page may say (untrusted: any site). Closed and tiny on purpose;
//!   a page can never send a [`MobileCommand`].
//! - [`NativeEvent`]: what the Kotlin shell reports (lifecycle, WebView callbacks).
//! - [`Effect`]: what the core asks the Kotlin shell to do in return.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroize;

/// A password on its way to the WebView: never printed, wiped when dropped.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Requests of the shell interface. Unknown or malformed messages are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum MobileCommand {
    /// The interface loaded: it wants the whole state.
    Ready,
    /// Address bar or palette entry: an address or a search.
    Navigate {
        input: String,
        /// Open in a new tab instead of the current one.
        #[serde(default)]
        new_tab: bool,
        #[serde(default)]
        private: bool,
    },
    /// A link of a page (long press menu) opened in another tab.
    OpenLink {
        url: String,
        #[serde(default)]
        private: bool,
    },
    Activate {
        tab: u64,
    },
    Close {
        tab: u64,
    },
    UndoClose,
    /// Closes every private tab and forgets their data.
    ClosePrivate,
    Back,
    Forward,
    Reload,
    /// Stops loading the page on screen.
    Stop,
    /// Desktop site for the tab (a desktop user agent), or back to the mobile one.
    DesktopSite {
        tab: u64,
    },
    /// Long press menu: "Download link" / "Download image".
    DownloadUrl {
        url: String,
    },
    /// The answer to a site's request for the camera, microphone or location.
    PermissionAnswer {
        id: u64,
        allow: bool,
    },
    /// Looks for a newer release (Settings > Updates, or the automatic daily check).
    CheckUpdate {
        #[serde(default)]
        manual: bool,
    },
    /// Downloads the update found by `CheckUpdate`, checks it, then hands it to the installer.
    InstallUpdate,
    /// The start page (no tab on screen).
    Home,
    /// Adds the tab to quick access, or removes it.
    Pin {
        tab: u64,
    },
    SwitchSpace {
        space: u64,
    },
    AddSpace {
        name: String,
        color: String,
    },
    SetSpaceColor {
        space: u64,
        color: String,
    },
    /// Palette suggestions for what is typed.
    Suggest {
        text: String,
    },
    History {
        #[serde(default)]
        query: String,
    },
    ForgetHistory {
        url: String,
        /// What the history page is filtered by: the list sent back keeps that filter.
        #[serde(default)]
        query: String,
    },
    ClearHistory,
    /// Cookies, storage and cache of every profile.
    ClearSiteData,
    PrivacyReport,
    SetSetting {
        name: String,
        value: Value,
    },
    /// The menu entry of an error page: the only way to grant an exception.
    ErrorProceed {
        tab: u64,
    },
    /// A `intent:`, `mailto:`, `tel:`… link the user confirmed.
    OpenExternal {
        url: String,
    },
    /// Language of the interface: `system`, `en` or `fr`.
    SetLanguage {
        language: String,
    },
    /// The user's own title for a tab; a blank one hands the title back to the page.
    RenameTab {
        tab: u64,
        name: String,
    },
    /// Moves a tab of the space on screen to another space (it loads again on its cookies).
    MoveTabToSpace {
        tab: u64,
        space: u64,
    },
    /// Files a tab into a folder, or takes it out (`null`).
    MoveTabToFolder {
        tab: u64,
        #[serde(default)]
        folder: Option<u64>,
    },
    /// Closes the loose tabs of the space but this one.
    CloseOtherTabs {
        tab: u64,
    },
    RenameSpace {
        space: u64,
        name: String,
    },
    /// Deletes a space and its tabs (never the last one); `UndoRemoval` brings it back.
    DeleteSpace {
        space: u64,
    },
    /// Puts back the space or folder deleted a moment ago.
    UndoRemoval,
    /// A new folder in the space on screen, with `tab` filed into it when given.
    AddFolder {
        name: String,
        #[serde(default)]
        tab: Option<u64>,
    },
    RenameFolder {
        folder: u64,
        name: String,
    },
    /// Folds or unfolds a folder.
    ToggleFolder {
        folder: u64,
    },
    /// Deletes a folder and the tabs it holds; `UndoRemoval` brings them back.
    DeleteFolder {
        folder: u64,
    },
    /// Files the loose tabs into folders by site (the card of the drawer); `Untidy` undoes it.
    Tidy,
    Untidy,
    DismissTidy,
    /// The answer to a server's request for a name and password (`auth` message).
    AuthAnswer {
        id: u64,
        user: String,
        password: Secret,
    },
    /// The question was closed: the server's own 401 page shows.
    AuthCancel {
        id: u64,
    },
    /// Settings > Devices: shows a pairing code, or pairs with the other device's code.
    DeviceShowCode,
    DeviceJoin {
        code: String,
    },
    DeviceCancelPairing,
    DeviceForget {
        id: String,
    },
    /// Tab sync with a device: `ask`, `auto` or `off`.
    DeviceSync {
        id: String,
        mode: String,
    },
    /// Sends this device's tabs and asks for the other's.
    DeviceSyncNow {
        id: String,
    },
    /// The proposal to synchronize: `merge`, `always` or `later`.
    SyncAnswer {
        id: String,
        answer: String,
    },
    /// "Continue on …": the tab on screen opens on a paired device.
    ContinueOn {
        device: String,
    },
}

/// What a page (untrusted) may report.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum PageEvent {
    /// The "continue anyway" button of our error page: it only asks the shell to show its menu.
    ErrorMenu {
        #[serde(default)]
        x: f64,
        #[serde(default)]
        y: f64,
    },
}

/// What the Kotlin shell reports.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum NativeEvent {
    Started {
        tab: u64,
        url: String,
    },
    Finished {
        tab: u64,
        url: String,
        #[serde(default)]
        title: String,
    },
    Title {
        tab: u64,
        title: String,
    },
    /// A main-frame navigation the page started (link, form, script): allowed, rewritten or
    /// refused by the core (`Effect::Cancel`).
    Navigating {
        tab: u64,
        url: String,
    },
    /// `onReceivedError` of the main frame.
    Failed {
        tab: u64,
        url: String,
        code: i32,
        #[serde(default = "yes")]
        online: bool,
    },
    /// `onReceivedSslError`: answered by `Effect::Ssl`.
    SslError {
        tab: u64,
        url: String,
        primary: i32,
    },
    /// A tracker or ad server opened as the page itself.
    BlockedPage {
        tab: u64,
        url: String,
    },
    /// Requests of `tab` were refused by the Shields: its count changed (the interface gets the
    /// two numbers, not the whole state).
    Blocked {
        tab: u64,
    },
    /// The system's preferred languages (`fr-FR`, `en-US`…), most wanted first.
    Locale {
        tags: Vec<String>,
    },
    /// The renderer of the tab's view died (`onRenderProcessGone`): the view is unusable.
    Crashed {
        tab: u64,
    },
    /// The main frame was answered 401, 403 or 5xx and finished loading: `probe` is what the
    /// page looks like (JSON of `href`, `title`, `text`, `rich`, as a JSON string).
    HttpStatus {
        tab: u64,
        url: String,
        status: u16,
        #[serde(default)]
        probe: String,
    },
    /// The page of `tab` opens a window on a user gesture (`onCreateWindow`); `sized`: it asked
    /// for a size or a position. Answered by `Effect::Popup`, or by nothing (nothing opens).
    NewWindow {
        tab: u64,
        #[serde(default)]
        sized: bool,
    },
    /// The page of `tab` called `window.close()`.
    ClosedWindow {
        tab: u64,
    },
    /// A server asks for a name and password (`id` is the shell's request).
    Auth {
        id: u64,
        tab: u64,
        host: String,
        #[serde(default)]
        realm: String,
    },
    /// Back pressed with nothing left in the tab's history and no overlay open.
    BackExhausted,
    Download {
        url: String,
        #[serde(default)]
        mime: String,
        #[serde(default)]
        disposition: String,
    },
    /// An address handed by another app (`ACTION_VIEW`).
    Intent {
        url: String,
    },
    /// The activity goes to the background: save now.
    Pause,
    /// The activity is on screen again (the daily update check runs from here).
    Resume,
    /// The icon of the page shown by `tab`, as a small PNG `data:` URL.
    Favicon {
        tab: u64,
        data: String,
    },
    /// A site asks for the camera, microphone or location (`id` is the shell's request).
    Permission {
        id: u64,
        tab: u64,
        origin: String,
        kinds: Vec<String>,
    },
    /// Text shared with BlackHole by another app (`ACTION_SEND`, "Search with…").
    Shared {
        text: String,
    },
    /// Where `Effect::UpdateProbe` was redirected to (the latest release's tag page).
    UpdateLatest {
        url: String,
    },
    /// The `SHA256SUMS.txt` of release `tag`; `debug`: this app is signed with the debug key.
    UpdateSums {
        tag: String,
        sums: String,
        #[serde(default)]
        debug: bool,
    },
    /// The SHA-256 of the APK downloaded for `Effect::UpdateDownload`.
    UpdateDownloaded {
        sha256: String,
    },
    /// A step of the update failed (network, download, installer).
    UpdateFailed {
        error: String,
    },
    /// A few times a second while the app is on screen: what the network brought.
    Tick,
}

fn yes() -> bool {
    true
}

/// Requests to the Kotlin shell, run in order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Effect {
    /// Creates the tab's view if it has none, then loads `url` in it.
    Open {
        tab: u64,
        url: String,
        /// WebView profile (cookies and storage): one per space, `private` for private tabs.
        profile: String,
        private: bool,
        /// Document-start script of the page (Shields, error page bridge).
        script: String,
        /// Desktop site (desktop user agent).
        desktop: bool,
    },
    /// Creates the view of a window a page opened and hands it to the engine as that window
    /// (`window.opener` is kept); it loads what the page asked by itself.
    Popup {
        tab: u64,
        opener: u64,
        profile: String,
        script: String,
        desktop: bool,
    },
    /// Puts the tab on screen; `None`: the start page.
    Show {
        tab: Option<u64>,
    },
    Load {
        tab: u64,
        url: String,
    },
    /// Our error page in place of the failed one (`url` stays the tab's history entry).
    Html {
        tab: u64,
        html: String,
        url: String,
    },
    Nav {
        tab: u64,
        action: &'static str,
    },
    /// Destroys the tab's view (closed, or released to keep memory low).
    Drop {
        tab: u64,
    },
    /// A message for the shell interface.
    Ui {
        msg: Value,
    },
    /// The answer to `NativeEvent::SslError`.
    Ssl {
        proceed: bool,
    },
    /// The answer to `NativeEvent::Navigating`: do not load it in the WebView.
    Cancel,
    /// Deletes a profile's cookies, storage and cache (`*` for all).
    ClearProfile {
        profile: String,
    },
    Download {
        url: String,
        mime: String,
        disposition: String,
    },
    /// Hands an address to another app, after the user confirmed.
    External {
        url: String,
    },
    /// The app goes to the background (back on the start page).
    Exit,
    /// Desktop site on or off for the tab's view (then reloaded).
    DesktopMode {
        tab: u64,
        on: bool,
    },
    /// Settings every tab view follows.
    WebPrefs {
        /// Text size in percent.
        text_zoom: u32,
        pull_refresh: bool,
    },
    /// The answer to `NativeEvent::Permission`.
    Permission {
        id: u64,
        allow: bool,
    },
    /// The answer to `NativeEvent::Auth`: the credentials, or none to cancel.
    HttpAuth {
        id: u64,
        user: Option<String>,
        password: Option<Secret>,
    },
    /// Follows `url` (the releases page) and reports where it led (`NativeEvent::UpdateLatest`).
    UpdateProbe {
        url: String,
    },
    /// Fetches the checksums of release `tag` (`NativeEvent::UpdateSums`).
    UpdateSums {
        tag: String,
        url: String,
    },
    /// Downloads the APK (https, GitHub) and reports its SHA-256 (`NativeEvent::UpdateDownloaded`).
    UpdateDownload {
        url: String,
        sha256: String,
        name: String,
    },
    /// Hands the checked APK to the system installer.
    UpdateInstall,
    /// Deletes the downloaded APK (its checksum did not match).
    UpdateDiscard,
}

/// Effects as the JSON array returned over JNI.
pub fn to_json(effects: &[Effect]) -> String {
    serde_json::to_string(effects).unwrap_or_else(|_| "[]".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse_and_unknown_ones_fail() {
        let c: MobileCommand =
            serde_json::from_str(r#"{"cmd":"navigate","input":"rust"}"#).unwrap();
        assert_eq!(
            c,
            MobileCommand::Navigate {
                input: "rust".into(),
                new_tab: false,
                private: false
            }
        );
        assert!(serde_json::from_str::<MobileCommand>(r#"{"cmd":"rm_rf"}"#).is_err());
        assert!(serde_json::from_str::<MobileCommand>(r#"{"cmd":"close"}"#).is_err());
    }

    #[test]
    fn a_page_cannot_send_a_shell_command() {
        for cmd in [r#"{"cmd":"error_proceed","tab":1}"#, r#"{"cmd":"clear_site_data"}"#] {
            assert!(serde_json::from_str::<PageEvent>(cmd).is_err());
        }
        assert!(serde_json::from_str::<PageEvent>(r#"{"cmd":"error_menu","x":1,"y":2}"#).is_ok());
    }

    #[test]
    fn new_commands_and_events_parse() {
        let c: MobileCommand = serde_json::from_str(r#"{"cmd":"check_update","manual":true}"#).unwrap();
        assert_eq!(c, MobileCommand::CheckUpdate { manual: true });
        let c: MobileCommand = serde_json::from_str(r#"{"cmd":"permission_answer","id":3,"allow":false}"#).unwrap();
        assert_eq!(c, MobileCommand::PermissionAnswer { id: 3, allow: false });
        let e: NativeEvent =
            serde_json::from_str(r#"{"ev":"update_sums","tag":"v1.0.0","sums":""}"#).unwrap();
        assert_eq!(
            e,
            NativeEvent::UpdateSums {
                tag: "v1.0.0".into(),
                sums: String::new(),
                debug: false
            }
        );
        // A page can never ask for a permission answer or an update itself.
        for cmd in [r#"{"cmd":"permission_answer","id":1,"allow":true}"#, r#"{"cmd":"install_update"}"#] {
            assert!(serde_json::from_str::<PageEvent>(cmd).is_err());
        }
    }

    #[test]
    fn effects_are_tagged() {
        let json = to_json(&[Effect::Show { tab: None }, Effect::Cancel]);
        assert_eq!(json, r#"[{"do":"show","tab":null},{"do":"cancel"}]"#);
    }
}

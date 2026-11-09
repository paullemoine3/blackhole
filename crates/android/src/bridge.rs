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
    fn effects_are_tagged() {
        let json = to_json(&[Effect::Show { tab: None }, Effect::Cancel]);
        assert_eq!(json, r#"[{"do":"show","tab":null},{"do":"cancel"}]"#);
    }
}

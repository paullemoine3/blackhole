//! What a page's `window.open` (or `target=_blank` link) becomes.
//!
//! Sign-in popups (Google Identity Services, Microsoft, Apple, GitHub, Facebook…) hand their
//! result back through `window.opener.postMessage` and then call `window.close()`. Opened as a
//! plain tab they lose `window.opener`: the login completes in the popup but the page that asked
//! for it never hears back (GSI logs "Relay function on opener iframe not found"). Such requests
//! get a real popup window linked to its opener; everything else stays a background tab, as
//! before.

use url::Url;

/// How to honour a new-window request from a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewWindow {
    /// A popup window sharing the opener's engine profile and keeping `window.opener`.
    Popup,
    /// A new tab (no opener) on the given address, filtered like any navigation.
    Tab,
    /// Nothing opens.
    Deny,
}

/// Identity providers whose sign-in pages run as popups. A host matches itself and its
/// subdomains.
const SIGN_IN_HOSTS: &[&str] = &[
    "accounts.google.com",
    "login.microsoftonline.com",
    "login.live.com",
    "appleid.apple.com",
    "id.apple.com",
    "api.twitter.com",
    "x.com",
];

/// Paths that are sign-in endpoints on hosts that also serve ordinary pages.
const SIGN_IN_PATHS: &[(&str, &str)] = &[
    ("github.com", "/login/oauth/"),
    ("facebook.com", "/dialog/oauth"),
    ("gitlab.com", "/oauth/"),
    ("discord.com", "/oauth2/"),
    ("slack.com", "/oauth/"),
    ("linkedin.com", "/oauth/"),
];

fn host_is(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.'))
}

/// Whether `url` is an identity provider's sign-in page.
pub fn is_sign_in(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    SIGN_IN_HOSTS.iter().any(|d| host_is(&host, d))
        || SIGN_IN_PATHS
            .iter()
            .any(|(d, path)| host_is(&host, d) && url.path().starts_with(path))
}

/// Decides what a page's new-window request opens. `raw` is the requested address as the engine
/// reports it; `has_features` is true when the page asked for a size or a position
/// (`window.open(url, name, "width=…,height=…")`), the mark of a script popup (a `target=_blank`
/// link or a bare `window.open(url)` has none). Engines only let pages open windows on a user
/// gesture, so this does not let pages spawn windows on their own.
pub fn new_window(raw: &str, has_features: bool) -> NewWindow {
    // `window.open('', name, features)` then `popup.location = …`: the usual OAuth pattern.
    if raw.is_empty() || raw == "about:blank" {
        return NewWindow::Popup;
    }
    let Ok(url) = Url::parse(raw) else {
        return NewWindow::Deny;
    };
    match url.scheme() {
        "http" | "https" if has_features || is_sign_in(&url) => NewWindow::Popup,
        "http" | "https" => NewWindow::Tab,
        // `javascript:`, `data:`, `blob:`, `file:`… never get a window of their own. Extension
        // pages and the rest go through the tab's navigation filter, as before.
        "javascript" | "data" | "blob" | "file" => NewWindow::Deny,
        _ => NewWindow::Tab,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_identity_services_popup_keeps_its_opener() {
        // GSI button: `window.open(".../gsi/select?...", "g_a11y...", "width=500,height=600")`.
        assert_eq!(
            new_window("https://accounts.google.com/gsi/select?client_id=x", true),
            NewWindow::Popup
        );
        // Even without features, Google's sign-in host is a popup.
        assert_eq!(
            new_window("https://accounts.google.com/gsi/transform", false),
            NewWindow::Popup
        );
        assert_eq!(
            new_window("https://accounts.google.com/o/oauth2/v2/auth?x=1", false),
            NewWindow::Popup
        );
    }

    #[test]
    fn blank_popup_navigated_by_script_keeps_its_opener() {
        assert_eq!(new_window("about:blank", false), NewWindow::Popup);
        assert_eq!(new_window("", true), NewWindow::Popup);
    }

    #[test]
    fn sized_script_popup_keeps_its_opener() {
        assert_eq!(
            new_window("https://sso.example.com/login", true),
            NewWindow::Popup
        );
        assert_eq!(
            new_window("http://localhost:3000/auth", true),
            NewWindow::Popup
        );
    }

    #[test]
    fn sign_in_paths_of_general_hosts() {
        assert_eq!(
            new_window(
                "https://github.com/login/oauth/authorize?client_id=1",
                false
            ),
            NewWindow::Popup
        );
        assert_eq!(
            new_window("https://www.facebook.com/v19.0/dialog/oauth?x=1", false),
            NewWindow::Tab,
            "the path must start with the endpoint"
        );
        assert_eq!(
            new_window("https://www.facebook.com/dialog/oauth?x=1", false),
            NewWindow::Popup
        );
        assert_eq!(
            new_window("https://github.com/rust-lang/rust", false),
            NewWindow::Tab
        );
        assert_eq!(
            new_window("https://login.microsoftonline.com/common/oauth2", false),
            NewWindow::Popup
        );
    }

    #[test]
    fn ordinary_links_stay_tabs() {
        assert_eq!(
            new_window("https://example.com/article", false),
            NewWindow::Tab
        );
        assert_eq!(
            new_window("https://mail.google.com/", false),
            NewWindow::Tab
        );
        // A look-alike host is not Google's.
        assert_eq!(
            new_window("https://accounts.google.com.evil.test/", false),
            NewWindow::Tab
        );
        assert_eq!(
            new_window("https://notaccounts.google.com/", false),
            NewWindow::Tab
        );
        // Sign-in pages are https only.
        assert_eq!(
            new_window("http://accounts.google.com/", false),
            NewWindow::Tab
        );
    }

    #[test]
    fn script_and_local_schemes_never_open() {
        assert_eq!(new_window("javascript:alert(1)", true), NewWindow::Deny);
        assert_eq!(new_window("data:text/html,hi", true), NewWindow::Deny);
        assert_eq!(new_window("file:///etc/passwd", false), NewWindow::Deny);
        assert_eq!(new_window("not a url", false), NewWindow::Deny);
    }
}

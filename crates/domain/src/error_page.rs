//! Which error page a tab shows when a navigation fails, from the raw error of the engine
//! (WebView2 `COREWEBVIEW2_WEB_ERROR_STATUS`, WKWebView `NSError`) or the HTTP status.

/// A failed navigation, as the engine reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure<'a> {
    /// WebView2: `COREWEBVIEW2_WEB_ERROR_STATUS`.
    WebView2(i32),
    /// WKWebView: the `NSError` domain and code.
    NsError(&'a str, i64),
    /// Android WebView: a `WebViewClient.ERROR_*` code (main frame only).
    Android(i32),
    /// Android WebView: the primary error of an `SslError` (`SslError.SSL_*`).
    AndroidSsl(i32),
}

/// Why a server certificate was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertProblem {
    Expired,
    NotYetValid,
    /// Signed by an authority the system does not trust.
    Untrusted,
    /// Signed by itself (issuer = subject).
    SelfSigned,
    /// Issued for another name.
    NameMismatch,
    Revoked,
    Invalid,
}

impl CertProblem {
    pub fn key(self) -> &'static str {
        match self {
            Self::Expired => "expired",
            Self::NotYetValid => "not_yet_valid",
            Self::Untrusted => "untrusted",
            Self::SelfSigned => "self_signed",
            Self::NameMismatch => "name_mismatch",
            Self::Revoked => "revoked",
            Self::Invalid => "invalid",
        }
    }

    fn about_dates(self) -> bool {
        matches!(self, Self::Expired | Self::NotYetValid)
    }

    /// WebView2 says "invalid" for both an unknown authority and a self-signed certificate:
    /// the certificate itself tells them apart.
    pub fn refine(self, issuer: &str, subject: &str) -> Self {
        let signed_by_itself = !issuer.is_empty() && issuer.trim() == subject.trim();
        match self {
            Self::Invalid | Self::Untrusted if signed_by_itself => Self::SelfSigned,
            Self::Invalid if !issuer.is_empty() => Self::Untrusted,
            other => other,
        }
    }
}

/// The error page of a tab. Its `key` selects the variant of the shell template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorPage {
    /// No answer: refused, reset, timed out.
    Unreachable,
    /// The computer has no network.
    Offline,
    /// The name does not resolve (DNS).
    NotFound,
    TooManyRedirects,
    Certificate(CertProblem),
    /// A certificate looks out of date because the computer's clock is wrong.
    ClockWrong,
    /// The site was asked in https (automatic upgrade) and has no working secure version.
    HttpOnly,
    /// HTTP 401 without a page of the server's own.
    Unauthorized,
    /// HTTP 403 without a page of the server's own.
    Forbidden,
    /// HTTP 5xx without a page of the server's own.
    ServerError(u16),
    /// A tracker or ad server the Shields block, opened as a page.
    Blocked,
}

impl ErrorPage {
    pub fn key(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Offline => "offline",
            Self::NotFound => "not_found",
            Self::TooManyRedirects => "redirects",
            Self::Certificate(_) => "certificate",
            Self::ClockWrong => "clock",
            Self::HttpOnly => "http_only",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::ServerError(_) => "server",
            Self::Blocked => "blocked",
        }
    }

    /// Whether "Continue anyway (not secure)" may be offered for this page.
    pub fn can_bypass(self) -> bool {
        matches!(self, Self::Certificate(_) | Self::ClockWrong | Self::HttpOnly)
    }

    /// A failure of a site the shell upgraded from http to https: a missing or broken secure
    /// version is not the site being down.
    pub fn after_https_upgrade(self) -> Self {
        match self {
            Self::Unreachable | Self::Certificate(_) | Self::ClockWrong => Self::HttpOnly,
            other => other,
        }
    }

    /// A failure right after a redirection answer (3xx): WebView2 reports a redirect loop as
    /// an aborted connection.
    pub fn after_redirect(self, last_status: u16) -> Self {
        match self {
            Self::Unreachable if (300..400).contains(&last_status) => Self::TooManyRedirects,
            other => other,
        }
    }

    /// A certificate refused for its dates while the computer's clock is implausible.
    pub fn with_clock(self, now_unix: i64) -> Self {
        match self {
            Self::Certificate(p) if p.about_dates() && !clock_plausible(now_unix) => Self::ClockWrong,
            other => other,
        }
    }
}

/// 2026-01-01T00:00:00Z: no clock showing an earlier date is right (this build is newer).
const CLOCK_FLOOR: i64 = 1_767_225_600;
/// 2100-01-01T00:00:00Z.
const CLOCK_CEILING: i64 = 4_102_444_800;

/// Ports a browser never connects to (the Fetch standard's "bad ports", which WebKit applies).
/// WebKit refuses them without calling the navigation delegate, so the tab would stay blank:
/// the shell shows its own page instead.
pub fn is_restricted_port(port: u16) -> bool {
    const BAD: [u16; 81] = [
        0, 1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95,
        101, 102, 103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143, 161, 179,
        389, 427, 465, 512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601,
        636, 989, 990, 993, 995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566,
        6665, 6666, 6667, 6668, 6669, 10080,
    ];
    BAD.binary_search(&port).is_ok()
}

pub fn clock_plausible(now_unix: i64) -> bool {
    (CLOCK_FLOOR..CLOCK_CEILING).contains(&now_unix)
}

/// The page for a failed navigation; `None` when it is no error to show (cancelled,
/// superseded, turned into a download, waiting for credentials).
pub fn classify(failure: Failure) -> Option<ErrorPage> {
    use CertProblem::*;
    use ErrorPage::*;
    match failure {
        Failure::WebView2(status) => match status {
            1 => Some(Certificate(NameMismatch)),
            2 => Some(Certificate(Expired)),
            3 | 5 => Some(Certificate(Invalid)),
            4 => Some(Certificate(Revoked)),
            11 => Some(Offline),
            13 => Some(NotFound),
            14 | 17 | 18 => None,
            15 => Some(TooManyRedirects),
            _ => Some(Unreachable),
        },
        Failure::NsError("NSURLErrorDomain" | "kCFErrorDomainCFNetwork", code) => match code {
            -999 => None,
            -1003 | -1006 => Some(NotFound),
            -1009 | -1018 | -1020 => Some(Offline),
            -1007 => Some(TooManyRedirects),
            -1201 => Some(Certificate(Expired)),
            -1202 | -1203 => Some(Certificate(Untrusted)),
            -1204 => Some(Certificate(NotYetValid)),
            -1205 => Some(Certificate(Invalid)),
            _ => Some(Unreachable),
        },
        // 101 cannot show the URL, 102 load interrupted by a policy change (a download, a
        // navigation the shell redirected), 204 handed to a plug-in. 103 is a restricted port
        // (e.g. 9, 25): WebKit shows nothing, so the tab gets our page.
        Failure::NsError("WebKitErrorDomain", 101 | 102 | 204) => None,
        Failure::NsError(..) => Some(Unreachable),
        // -2 host lookup, -3/-4/-5 authentication (the server's own page follows), -9 redirect
        // loop, -10 unsupported scheme (handed to another app), -11 TLS handshake, -12 bad URL,
        // -16 Safe Browsing (WebView shows its own interstitial).
        Failure::Android(code) => match code {
            -2 => Some(NotFound),
            -5..=-3 | -10 | -16 => None,
            -9 => Some(TooManyRedirects),
            -11 => Some(Certificate(Invalid)),
            _ => Some(Unreachable),
        },
        // 0 not yet valid, 1 expired, 2 name mismatch, 3 untrusted, 4 date invalid, 5 invalid.
        Failure::AndroidSsl(primary) => Some(Certificate(match primary {
            0 => NotYetValid,
            1 => Expired,
            2 => NameMismatch,
            3 => Untrusted,
            _ => Invalid,
        })),
    }
}

/// The page for an HTTP status, when the server sent no page of its own.
pub fn classify_status(status: u16) -> Option<ErrorPage> {
    match status {
        401 => Some(ErrorPage::Unauthorized),
        403 => Some(ErrorPage::Forbidden),
        500..=599 => Some(ErrorPage::ServerError(status)),
        _ => None,
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        401 => "unauthorized",
        403 => "forbidden",
        500 => "internal server error",
        501 => "not implemented",
        502 => "bad gateway",
        503 => "service unavailable",
        504 => "gateway timeout",
        _ => "error",
    }
}

/// Whether the body of an error answer is no page worth showing: empty, or only the bare
/// status line ("403 Forbidden") without a title. A page with a title, links, forms, images
/// or frames (`rich`) is the server's own and is shown as sent.
pub fn is_bare_error_body(status: u16, title: &str, text: &str, rich: bool) -> bool {
    if rich || !title.trim().is_empty() {
        return false;
    }
    let text = text.trim().to_lowercase();
    text.is_empty()
        || (text.chars().count() <= 80
            && (text.contains(&status.to_string()) || text.contains(reason_phrase(status))))
}

/// The engine replaced an empty error answer with its own error page (WebView2 shows
/// `chrome-error://chromewebdata/`): the server sent nothing to show.
pub fn is_engine_error_page(href: &str) -> bool {
    href.starts_with("chrome-error:")
}

/// Whole top-level domains and sites in the HSTS preload list of the browsers: a certificate
/// error there is never bypassed. Neither engine exposes its own list, so this is a short
/// excerpt of well-known entries.
const HSTS_TLDS: &[&str] = &[
    "android", "app", "bank", "boo", "chrome", "dad", "day", "dev", "eat", "esq", "fly", "foo",
    "gle", "gmail", "google", "hangout", "ing", "insurance", "meme", "mov", "new", "nexus", "page",
    "phd", "prof", "rsvp", "search", "youtube", "zip",
];
const HSTS_SITES: &[&str] = &[
    "google.com", "gmail.com", "youtube.com", "facebook.com", "twitter.com", "github.com",
    "paypal.com", "dropbox.com", "torproject.org",
];

pub fn hsts_preloaded(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let tld = host.rsplit('.').next().unwrap_or_default();
    HSTS_TLDS.contains(&tld)
        || HSTS_SITES
            .iter()
            .any(|site| host == *site || host.ends_with(&format!(".{site}")))
}

/// Hosts the user chose to open despite an error, for this run of the app only (never saved).
#[derive(Debug, Default, Clone)]
pub struct Bypasses {
    hosts: std::collections::HashSet<String>,
}

impl Bypasses {
    /// Records the exception; refused for an HSTS-preloaded host or an empty one.
    pub fn allow(&mut self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() || hsts_preloaded(&host) {
            return false;
        }
        self.hosts.insert(host);
        true
    }

    pub fn allows(&self, host: &str) -> bool {
        self.hosts
            .contains(&host.trim_end_matches('.').to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_ports_are_the_fetch_bad_ports() {
        for port in [9, 25, 6000, 10080] {
            assert!(is_restricted_port(port), "{port}");
        }
        for port in [80, 443, 8080, 3000, 47999] {
            assert!(!is_restricted_port(port), "{port}");
        }
    }

    #[test]
    fn android_errors() {
        assert_eq!(classify(Failure::Android(-2)), Some(ErrorPage::NotFound));
        assert_eq!(classify(Failure::Android(-9)), Some(ErrorPage::TooManyRedirects));
        assert_eq!(
            classify(Failure::Android(-11)),
            Some(ErrorPage::Certificate(CertProblem::Invalid))
        );
        for code in [-1, -6, -7, -8, -15] {
            assert_eq!(classify(Failure::Android(code)), Some(ErrorPage::Unreachable));
        }
        for code in [-3, -4, -5, -10, -16] {
            assert_eq!(classify(Failure::Android(code)), None);
        }
        let ssl = |p| classify(Failure::AndroidSsl(p));
        assert_eq!(ssl(0), Some(ErrorPage::Certificate(CertProblem::NotYetValid)));
        assert_eq!(ssl(1), Some(ErrorPage::Certificate(CertProblem::Expired)));
        assert_eq!(ssl(2), Some(ErrorPage::Certificate(CertProblem::NameMismatch)));
        assert_eq!(ssl(3), Some(ErrorPage::Certificate(CertProblem::Untrusted)));
        assert_eq!(ssl(5), Some(ErrorPage::Certificate(CertProblem::Invalid)));
        // A wrong clock still turns a date problem into the clock page.
        assert_eq!(ssl(1).map(|p| p.with_clock(0)), Some(ErrorPage::ClockWrong));
    }

    #[test]
    fn webview2_statuses() {
        assert_eq!(classify(Failure::WebView2(13)), Some(ErrorPage::NotFound));
        assert_eq!(classify(Failure::WebView2(11)), Some(ErrorPage::Offline));
        assert_eq!(classify(Failure::WebView2(15)), Some(ErrorPage::TooManyRedirects));
        assert_eq!(
            classify(Failure::WebView2(2)),
            Some(ErrorPage::Certificate(CertProblem::Expired))
        );
        assert_eq!(
            classify(Failure::WebView2(1)),
            Some(ErrorPage::Certificate(CertProblem::NameMismatch))
        );
        for status in [6, 7, 9, 10, 12, 0, 16] {
            assert_eq!(classify(Failure::WebView2(status)), Some(ErrorPage::Unreachable));
        }
        // Cancelled, or waiting for credentials (the shell asks for them).
        for status in [14, 17, 18] {
            assert_eq!(classify(Failure::WebView2(status)), None);
        }
    }

    #[test]
    fn webkit_errors() {
        let ns = |code| classify(Failure::NsError("NSURLErrorDomain", code));
        assert_eq!(ns(-999), None);
        assert_eq!(ns(-1003), Some(ErrorPage::NotFound));
        assert_eq!(ns(-1009), Some(ErrorPage::Offline));
        assert_eq!(ns(-1007), Some(ErrorPage::TooManyRedirects));
        assert_eq!(ns(-1004), Some(ErrorPage::Unreachable));
        assert_eq!(ns(-1001), Some(ErrorPage::Unreachable));
        assert_eq!(ns(-1202), Some(ErrorPage::Certificate(CertProblem::Untrusted)));
        assert_eq!(ns(-1201), Some(ErrorPage::Certificate(CertProblem::Expired)));
        assert_eq!(ns(-1204), Some(ErrorPage::Certificate(CertProblem::NotYetValid)));
        assert_eq!(classify(Failure::NsError("WebKitErrorDomain", 102)), None);
        assert_eq!(
            classify(Failure::NsError("WebKitErrorDomain", 103)),
            Some(ErrorPage::Unreachable),
            "restricted port: the tab would stay blank"
        );
        assert_eq!(
            classify(Failure::NsError("WebKitErrorDomain", 300)),
            Some(ErrorPage::Unreachable)
        );
    }

    #[test]
    fn certificate_refined_by_its_names() {
        let p = CertProblem::Invalid;
        assert_eq!(p.refine("CN=127.0.0.1", "CN=127.0.0.1"), CertProblem::SelfSigned);
        assert_eq!(p.refine("CN=Corp CA", "CN=intranet"), CertProblem::Untrusted);
        assert_eq!(p.refine("", ""), CertProblem::Invalid);
        assert_eq!(CertProblem::Expired.refine("a", "a"), CertProblem::Expired);
        assert_eq!(CertProblem::Untrusted.refine("a", "a"), CertProblem::SelfSigned);
    }

    #[test]
    fn wrong_clock_explains_date_errors() {
        let expired = ErrorPage::Certificate(CertProblem::Expired);
        assert_eq!(expired.with_clock(1_000_000_000), ErrorPage::ClockWrong);
        assert_eq!(expired.with_clock(1_800_000_000), expired);
        let other = ErrorPage::Certificate(CertProblem::NameMismatch);
        assert_eq!(other.with_clock(0), other);
        assert!(!clock_plausible(5_000_000_000));
    }

    #[test]
    fn upgrade_failure_means_no_secure_version() {
        assert_eq!(ErrorPage::Unreachable.after_https_upgrade(), ErrorPage::HttpOnly);
        assert_eq!(
            ErrorPage::Certificate(CertProblem::NameMismatch).after_https_upgrade(),
            ErrorPage::HttpOnly
        );
        assert_eq!(ErrorPage::NotFound.after_https_upgrade(), ErrorPage::NotFound);
        assert_eq!(ErrorPage::Offline.after_https_upgrade(), ErrorPage::Offline);
    }

    #[test]
    fn statuses_without_a_page() {
        assert_eq!(classify_status(401), Some(ErrorPage::Unauthorized));
        assert_eq!(classify_status(403), Some(ErrorPage::Forbidden));
        assert_eq!(classify_status(502), Some(ErrorPage::ServerError(502)));
        assert_eq!(classify_status(404), None);
        assert_eq!(classify_status(200), None);
        assert!(is_bare_error_body(500, "", "  \n", false));
        assert!(is_bare_error_body(403, "", "403 Forbidden\nnginx", false));
        assert!(is_bare_error_body(503, "", "Service Unavailable", false));
        assert!(!is_bare_error_body(403, "", "", true));
        // A page with a title is the server's own (default nginx pages included).
        assert!(!is_bare_error_body(403, "bh-403", "403 Forbidden", false));
        assert!(!is_bare_error_body(
            500,
            "",
            "Our servers are having a moment. Our engineers have been told and are looking into it; \
             in the meantime you can browse the archive or read the status page for updates.",
            false
        ));
        assert!(!is_bare_error_body(403, "", "Members only: please sign in", false));
    }

    #[test]
    fn redirect_loops_and_engine_pages() {
        assert_eq!(ErrorPage::Unreachable.after_redirect(302), ErrorPage::TooManyRedirects);
        assert_eq!(ErrorPage::Unreachable.after_redirect(0), ErrorPage::Unreachable);
        assert_eq!(ErrorPage::NotFound.after_redirect(301), ErrorPage::NotFound);
        assert!(is_engine_error_page("chrome-error://chromewebdata/"));
        assert!(!is_engine_error_page("http://127.0.0.1/"));
    }

    #[test]
    fn hsts_preloaded_hosts_cannot_be_bypassed() {
        assert!(hsts_preloaded("mail.google.com"));
        assert!(hsts_preloaded("example.dev"));
        assert!(hsts_preloaded("GITHUB.COM."));
        assert!(!hsts_preloaded("notgoogle.com"));
        assert!(!hsts_preloaded("127.0.0.1"));
        let mut b = Bypasses::default();
        assert!(!b.allow("accounts.google.com"));
        assert!(!b.allow(""));
        assert!(b.allow("NAS.local"));
        assert!(b.allows("nas.local"));
        assert!(!b.allows("other.local"));
    }

    #[test]
    fn bypass_offered_only_for_security_pages() {
        assert!(ErrorPage::Certificate(CertProblem::SelfSigned).can_bypass());
        assert!(ErrorPage::HttpOnly.can_bypass());
        assert!(ErrorPage::ClockWrong.can_bypass());
        assert!(!ErrorPage::Forbidden.can_bypass());
        assert!(!ErrorPage::Blocked.can_bypass());
    }
}

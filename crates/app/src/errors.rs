//! Error pages of the tabs (`ui/error.html`): which page a failed navigation shows (rules in
//! `domain::error_page`), and the exceptions the user grants from the shell's own menu —
//! a refused certificate, a site without https, a blocked site — for this run only.

use crate::i18n::{t, tf};
use crate::{views, Running, PEEK_TAB};
use blackhole_domain::error_page::{
    self, classify, classify_status, is_bare_error_body, is_engine_error_page, Back, Bypasses, ErrorPage, Failure,
    Retry, Trail,
};
use blackhole_domain::{PrivacyDecision, TabId};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{LazyLock, Mutex};
use url::Url;

/// Exceptions granted in the shell's menu. Kept in memory only (never written to disk) and
/// read by the handlers of every view.
#[derive(Default)]
struct Exceptions {
    certs: Bypasses,
    http: Bypasses,
    unblocked: HashSet<String>,
}

static EXCEPTIONS: LazyLock<Mutex<Exceptions>> = LazyLock::new(Mutex::default);

fn exceptions<T>(f: impl FnOnce(&mut Exceptions) -> T) -> T {
    let mut guard = EXCEPTIONS.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// Whether the user chose to go on despite the certificate of `host`.
pub fn cert_allowed(host: &str) -> bool {
    exceptions(|e| e.certs.allows(host))
}

/// The navigation decision once the user's exceptions apply: a site let through the Shields,
/// or one opened in http (no automatic upgrade).
pub fn with_exceptions(decision: PrivacyDecision, target: &Url) -> PrivacyDecision {
    let host = target.host_str().unwrap_or_default();
    match decision {
        PrivacyDecision::Block(why)
            if why != "unsupported scheme" && exceptions(|e| e.unblocked.contains(host)) =>
        {
            PrivacyDecision::Allow
        }
        PrivacyDecision::Rewrite(url)
            if target.scheme() == "http"
                && url.scheme() == "https"
                && exceptions(|e| e.http.allows(host)) =>
        {
            let mut plain = url;
            let _ = plain.set_scheme("http");
            let _ = plain.set_port(target.port());
            if &plain == target {
                PrivacyDecision::Allow
            } else {
                PrivacyDecision::Rewrite(plain)
            }
        }
        other => other,
    }
}

/// The page is open through an exception: a certificate accepted by hand, or http by choice.
pub fn insecure(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    match url.scheme() {
        "https" => cert_allowed(host),
        "http" => exceptions(|e| e.http.allows(host)),
        _ => false,
    }
}

/// What a tab's error page is about, while it is shown.
struct Shown {
    page: ErrorPage,
    url: Url,
    /// The http address, for a site without a secure version.
    plain: Option<Url>,
    /// Where the page sits in the tab's history: how far "back" is, how to retry.
    trail: Trail,
}

/// Body of an HTTP error answer: whether the server sent a page of its own.
const PROBE_JS: &str = "(()=>{const b=document.body;return JSON.stringify({href:location.href,title:document.title||'',\
text:b?b.innerText.slice(0,400):'',rich:!!document.querySelector('a[href],form,img,video,iframe,\
canvas,svg,object,embed,input,button')})})()";

/// How long a "Back to safety" may still be on its way (a load without answer gives up after
/// `CONNECT_MAX`, 20 s).
const ESCAPE_FOR: std::time::Duration = std::time::Duration::from_secs(30);

pub struct Errors {
    failures: HashMap<TabId, wry::LoadFailure>,
    http: HashMap<TabId, (u16, String)>,
    certs: HashMap<TabId, wry::ServerTrustChallenge>,
    upgraded: HashMap<TabId, Url>,
    shown: HashMap<TabId, Shown>,
    /// Tabs whose server asked for credentials: their 401 is the prompt's business.
    auth: HashSet<TabId>,
    /// Tabs whose refused certificate already got its page: the `Failed` that ends that same
    /// (cancelled) navigation must not replace it with "Site unreachable".
    settled: HashSet<TabId>,
    /// The last page each tab showed that was no error: where "Back to safety" lands when
    /// the history has nothing before the failure.
    good: HashMap<TabId, Url>,
    /// "Try again" went back to the entry of the failed address: once the engine has shown
    /// its cached copy, the address is really loaded again.
    reloading: HashMap<TabId, Url>,
    /// "Back to safety" is on its way from this address: failing on it again means the
    /// history had one more entry than expected, so the tab goes back once more.
    escaping: HashMap<TabId, (Url, std::time::Instant)>,
    /// "Try again" asked the address in place of the error page that had this trail.
    retried: HashMap<TabId, (Url, Trail)>,
    tx: Sender<(TabId, u16, String, String)>,
    rx: Receiver<(TabId, u16, String, String)>,
}

impl Default for Errors {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            failures: HashMap::new(),
            http: HashMap::new(),
            certs: HashMap::new(),
            upgraded: HashMap::new(),
            shown: HashMap::new(),
            auth: HashSet::new(),
            settled: HashSet::new(),
            good: HashMap::new(),
            reloading: HashMap::new(),
            escaping: HashMap::new(),
            retried: HashMap::new(),
            tx,
            rx,
        }
    }
}

fn failure_of(f: &wry::LoadFailure) -> Failure<'_> {
    if f.domain == "webview2" {
        Failure::WebView2(i32::try_from(f.code).unwrap_or(0))
    } else {
        Failure::NsError(&f.domain, f.code)
    }
}

fn same_page(a: &str, b: &str) -> bool {
    let strip = |u: &str| u.split('#').next().unwrap_or_default().trim_end_matches('/').to_owned();
    strip(a) == strip(b)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Running {
    /// A new navigation started in the tab: its error page (if any) is gone.
    /// Whether `tab` shows one of BlackHole's error pages rather than a page of the web.
    pub(crate) fn error_shown(&self, tab: TabId) -> bool {
        self.errors.shown.contains_key(&tab)
    }

    pub(crate) fn errors_load_start(&mut self, tab: TabId) {
        self.errors.failures.remove(&tab);
        self.errors.settled.remove(&tab);
        self.errors.shown.remove(&tab);
    }

    pub(crate) fn on_load_failure(&mut self, tab: TabId, failure: wry::LoadFailure) {
        if failure.http_status > 0 {
            self.errors.http.insert(tab, (failure.http_status, failure.url));
            return;
        }
        // A certificate WebView2 was told to refuse: the navigation ends cancelled or aborted,
        // often on `about:blank` (a new view), so no `Failed` reaches the shell: shown now.
        let host = Url::parse(&failure.url).ok().and_then(|u| u.host_str().map(str::to_owned));
        let cert = self.errors.certs.get(&tab).filter(|c| {
            c.error != 0
                && (Some(&c.host) == host.as_ref() || failure.url == "about:blank")
        });
        if let Some(c) = cert {
            let problem = match classify(Failure::WebView2(i32::try_from(c.error).unwrap_or(5))) {
                Some(ErrorPage::Certificate(p)) => p,
                _ => error_page::CertProblem::Invalid,
            };
            if let Ok(url) = Url::parse(&c.url) {
                self.show_error(tab, url, ErrorPage::Certificate(problem));
            }
            // Said once: the challenge must not turn a later failure of this host (the load
            // our own page interrupts) into a certificate page again. In a tab that showed a
            // page, WebView2 still ends the cancelled navigation with a `Failed`.
            self.errors.certs.remove(&tab);
            if failure.url != "about:blank" {
                self.errors.settled.insert(tab);
            }
            return;
        }
        // Kept even when it calls for no page (a load interrupted because it became a download,
        // after a redirect of the page shown): `on_failed` must know the `Failed` that follows
        // is that one, not a failure without a reason ("Site unreachable").
        self.errors.failures.insert(tab, failure);
    }

    /// The failure just reported for `tab` is WebView2's "connection aborted" (9): also how
    /// it ends a navigation that turns into a download.
    pub(crate) fn failure_is_abort(&self, tab: TabId) -> bool {
        self.errors
            .failures
            .get(&tab)
            .is_some_and(|f| f.domain == "webview2" && f.code == 9)
    }

    pub(crate) fn forget_failure(&mut self, tab: TabId) {
        self.errors.failures.remove(&tab);
    }

    pub(crate) fn errors_auth_asked(&mut self, tab: TabId) {
        self.errors.auth.insert(tab);
    }

    pub(crate) fn on_server_trust(&mut self, tab: TabId, challenge: wry::ServerTrustChallenge) {
        self.errors.certs.insert(tab, challenge);
    }

    pub(crate) fn on_upgraded(&mut self, tab: TabId, plain: Url) {
        self.errors.upgraded.insert(tab, plain);
    }

    /// The load failed (`PageLoadEvent::Failed`): the page for the reason given just before.
    pub(crate) fn on_failed(&mut self, tab: TabId, url: Url) {
        if self.errors.settled.remove(&tab) {
            return;
        }
        // A retry that went back to the failed entry: this failure is a new attempt already.
        self.errors.reloading.remove(&tab);
        let page = match self.errors.failures.remove(&tab) {
            Some(f) => match classify(failure_of(&f)) {
                Some(page) => page,
                None => return,
            },
            None => ErrorPage::Unreachable,
        };
        let last = self.errors.http.remove(&tab).map_or(0, |(status, _)| status);
        self.show_error(tab, url, page.after_redirect(last));
    }

    /// The page loaded: an HTTP error answer without a page of the server's own gets ours.
    pub(crate) fn errors_loaded(&mut self, tab: TabId, url: &Url) {
        if !self.errors.upgraded.get(&tab).is_some_and(|u| u.host_str() != url.host_str()) {
            self.errors.upgraded.remove(&tab);
        }
        self.errors.certs.remove(&tab);
        let asked = self.errors.auth.remove(&tab);
        let again = self.errors.reloading.remove(&tab).is_some_and(|u| same_page(u.as_str(), url.as_str()));
        let Some((status, raw)) = self.errors.http.remove(&tab) else {
            self.loaded_fine(tab, url);
            return;
        };
        // "Try again" went back to this entry and the engine showed its cached error answer:
        // now the server is really asked again.
        if again && status >= 400 {
            if let Some(view) = self.tabs.get(&tab) {
                let _ = view.reload();
            }
            return;
        }
        // WebView2 ends each hop of a redirect loop as "finished": the 3xx stays known for the
        // failure that ends the loop.
        if (300..400).contains(&status) {
            self.errors.http.insert(tab, (status, raw));
            return;
        }
        if (asked && status == 401) || tab == PEEK_TAB || !same_page(&raw, url.as_str()) || classify_status(status).is_none() {
            self.loaded_fine(tab, url);
            return;
        }
        let Some(view) = self.tabs.get(&tab) else { return };
        let tx = self.errors.tx.clone();
        let page = url.to_string();
        let _ = view.evaluate_script_with_callback(PROBE_JS, move |raw| {
            let _ = tx.send((tab, status, page.clone(), raw));
        });
    }

    /// `tab` shows a page of the web (the server's own error page included).
    fn loaded_fine(&mut self, tab: TabId, url: &Url) {
        self.errors.escaping.remove(&tab);
        self.errors.retried.remove(&tab);
        if tab != PEEK_TAB && matches!(url.scheme(), "http" | "https") {
            self.errors.good.insert(tab, url.clone());
        }
    }

    /// Answers of the body probes.
    pub(crate) fn service_errors(&mut self) {
        while let Ok((tab, status, raw_url, raw)) = self.errors.rx.try_recv() {
            let body = serde_json::from_str::<String>(&raw)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
            let (Some(body), Ok(url), Some(page)) =
                (body, Url::parse(&raw_url), classify_status(status))
            else {
                continue;
            };
            let still_there = self
                .svc
                .workspace()
                .spaces()
                .iter()
                .flat_map(|s| s.tabs.iter())
                .any(|t| t.id == tab && same_page(t.url.as_str(), &raw_url));
            let text = |k: &str| body[k].as_str().unwrap_or_default().to_owned();
            let rich = body["rich"].as_bool().unwrap_or(true);
            let bare = is_engine_error_page(&text("href"))
                || is_bare_error_body(status, &text("title"), &text("text"), rich);
            if still_there && bare {
                let page = match page {
                    ErrorPage::ServerError(_) => ErrorPage::ServerError(status),
                    other => other,
                };
                self.show_error(tab, url, page);
            } else if still_there {
                self.loaded_fine(tab, &url);
            }
        }
        if self.errors.shown.len() + self.errors.upgraded.len() + self.errors.certs.len() + self.errors.good.len() > 64 {
            let live: HashSet<TabId> = self.tabs.keys().copied().collect();
            self.errors.shown.retain(|t, _| live.contains(t));
            self.errors.upgraded.retain(|t, _| live.contains(t));
            self.errors.certs.retain(|t, _| live.contains(t));
            self.errors.good.retain(|t, _| live.contains(t));
            self.errors.reloading.retain(|t, _| live.contains(t));
            self.errors.escaping.retain(|t, _| live.contains(t));
            self.errors.retried.retain(|t, _| live.contains(t));
        }
    }

    /// The tab was sent to a tracker or an ad server that the Shields block.
    pub(crate) fn on_blocked_page(&mut self, tab: TabId, url: Url, why: &'static str) {
        if tab == PEEK_TAB || !self.tabs.contains_key(&tab) {
            return;
        }
        self.show_error_with(tab, url, ErrorPage::Blocked, json!({ "why": why }));
    }

    /// A restricted port (e.g. 9, 25): WebKit refuses it without telling the delegate, so the
    /// tab would stay blank. Shows "Site injoignable" at once; true when it did.
    pub(crate) fn refuse_restricted_port(&mut self, tab: TabId, url: &Url) -> bool {
        if !url.port().is_some_and(error_page::is_restricted_port) {
            return false;
        }
        self.show_error(tab, url.clone(), ErrorPage::Unreachable);
        true
    }

    pub(crate) fn show_error(&mut self, tab: TabId, url: Url, page: ErrorPage) {
        self.show_error_with(tab, url, page, json!({}));
    }

    fn show_error_with(&mut self, tab: TabId, url: Url, page: ErrorPage, mut extra: serde_json::Value) {
        // "Back to safety" landed on the failed address again (one entry more than expected
        // in the history): the tab goes on to the page before, it does not show the error twice.
        if self.errors.escaping.remove(&tab).is_some_and(|(u, at)| at.elapsed() < ESCAPE_FOR && same_page(u.as_str(), url.as_str())) {
            let further = self.tabs.get(&tab).is_some_and(|v| v.can_go_back().unwrap_or(false));
            match (further, self.tabs.get(&tab)) {
                (true, Some(view)) => views::go_back(view),
                _ => self.error_leave(tab, &url),
            }
            return;
        }
        // What the engine shows now tells whether the failed address became a history entry:
        // it did when it got an answer or an engine error page, not when it was cancelled.
        let failed_entry = self
            .tabs
            .get(&tab)
            .and_then(|v| v.url().ok())
            .is_some_and(|shown| same_page(&shown, url.as_str()));
        // WebView2 adds an entry for the HTML it is given; WKWebView does not.
        let trail = match self.errors.retried.remove(&tab) {
            Some((asked, before)) if same_page(asked.as_str(), url.as_str()) => before.after_retry(failed_entry),
            _ => Trail { own_entry: cfg!(windows), failed_entry, stale: 0 },
        };
        let host = url.host_str().unwrap_or_default().to_owned();
        let plain = self
            .errors
            .upgraded
            .get(&tab)
            .filter(|u| u.host_str() == Some(host.as_str()) && url.scheme() == "https")
            .cloned();
        let mut page = page.with_clock(now_unix());
        if plain.is_some() {
            page = page.after_https_upgrade();
        }
        let mut config = json!({
            "kind": page.key(),
            "url": url.as_str(),
            "host": host,
            "animations": self.svc.settings().animations,
            "hsts": page.can_bypass() && error_page::hsts_preloaded(&host),
            // The shell answers the page's "back" and "retry" (see `error_back`).
            "shell": true,
            "steps": trail.steps(),
        });
        if let ErrorPage::ServerError(status) = page {
            config["status"] = json!(status);
        }
        if matches!(page, ErrorPage::Certificate(_) | ErrorPage::ClockWrong) {
            let problem = match page {
                ErrorPage::Certificate(p) => p,
                _ => error_page::CertProblem::Expired,
            };
            let cert = self.errors.certs.get(&tab).filter(|c| c.host == host);
            let problem = cert.map_or(problem, |c| problem.refine(&c.issuer, &c.subject));
            config["reason"] = json!(problem.key());
            if let Some(c) = cert {
                config["issuer"] = json!(c.issuer);
                config["subject"] = json!(c.subject);
                config["from"] = json!(c.valid_from);
                config["to"] = json!(c.valid_to);
            }
        }
        if let (Some(map), serde_json::Value::Object(more)) = (config.as_object_mut(), extra.take()) {
            map.extend(more);
        }
        self.awaiting.remove(&tab);
        self.pending.remove(&tab);
        self.loading.remove(&tab);
        self.splash.remove(&tab);
        if let Some(view) = self.tabs.get(&tab) {
            let _ = view.load_html(&views::error_page(&config));
        }
        let plain = if page == ErrorPage::HttpOnly { plain } else { None };
        self.errors.shown.insert(tab, Shown { page, url: url.clone(), plain, trail });
        self.page_url(tab, url);
        self.sync();
    }

    /// "Back to safety" (the page's button, the shell's Back): to the page seen before the
    /// failure. `history.back()` alone returns to the address that failed, which fails again.
    /// Nothing happens unless this tab shows our error page.
    pub(crate) fn error_back(&mut self, tab: TabId, length: u32) {
        let Some(shown) = self.errors.shown.get(&tab) else { return };
        let url = shown.url.clone();
        match shown.trail.back(length) {
            Back::Go(steps) => {
                self.errors.escaping.insert(tab, (url, std::time::Instant::now()));
                if let Some(view) = self.tabs.get(&tab) {
                    let _ = view.evaluate_script(&format!("history.go(-{steps})"));
                }
            }
            Back::Leave => self.error_leave(tab, &url),
        }
    }

    /// Nothing before the failure in the history: the last page of the tab that worked, or
    /// the tab closes (a quick access tab is put to sleep).
    fn error_leave(&mut self, tab: TabId, failed: &Url) {
        let good = self.errors.good.get(&tab).filter(|g| !same_page(g.as_str(), failed.as_str())).cloned();
        match (good, self.tabs.get(&tab)) {
            (Some(good), Some(view)) => {
                let _ = view.load_url(good.as_str());
            }
            _ => {
                self.errors.shown.remove(&tab);
                self.handle_ui(crate::ipc::UiCommand::Close { id: tab.0 });
            }
        }
    }

    /// "Try again" (the page's button, the shell's Reload): the address that failed is asked
    /// again, without adding to the history.
    pub(crate) fn error_retry(&mut self, tab: TabId) {
        let (Some(shown), Some(view)) = (self.errors.shown.get(&tab), self.tabs.get(&tab)) else { return };
        match shown.trail.retry() {
            Retry::ReloadEntry => {
                self.errors.reloading.insert(tab, shown.url.clone());
                views::go_back(view);
            }
            Retry::Replace => {
                self.errors.retried.insert(tab, (shown.url.clone(), shown.trail));
                let _ = view.evaluate_script(&format!("location.replace({})", json!(shown.url.as_str())));
            }
            Retry::Push => {
                let _ = view.load_url(shown.url.as_str());
            }
        }
    }

    /// The risky button of an error page: the shell's menu asks, so a page cannot grant an
    /// exception by itself (and nothing happens unless this tab shows our error page).
    pub(crate) fn error_menu(&mut self, tab: TabId, x: f64, y: f64) {
        let Some(shown) = self.errors.shown.get(&tab) else { return };
        let host = shown.url.host_str().unwrap_or_default().to_owned();
        let run = json!({"cmd": "error_proceed", "tab": tab.0});
        let items = match shown.page {
            ErrorPage::Blocked => vec![
                json!({"head": t("menu.shields")}),
                json!({"icon": "shield", "label": tf("menu.allow_until_quit", &[("host", &host)]), "run": run}),
            ],
            page if page.can_bypass() && !error_page::hsts_preloaded(&host) => {
                let label = if page == ErrorPage::HttpOnly {
                    tf("menu.open_over_http", &[("host", &host)])
                } else {
                    tf("menu.continue_insecure", &[("host", &host)])
                };
                vec![
                    json!({"head": t("menu.until_quit")}),
                    json!({"icon": "close", "label": label, "run": run}),
                ]
            }
            _ => return,
        };
        let (ox, oy) = self.view_origin(tab);
        self.show_menu((ox + x, oy + y), json!(items));
    }

    /// The menu entry was picked: the exception is granted and the page loads again.
    pub(crate) fn error_proceed(&mut self, tab: TabId) {
        let Some(shown) = self.errors.shown.remove(&tab) else { return };
        let host = shown.url.host_str().unwrap_or_default().to_owned();
        let target = match shown.page {
            ErrorPage::Blocked => {
                exceptions(|e| e.unblocked.insert(host.to_ascii_lowercase()));
                shown.url
            }
            ErrorPage::HttpOnly => {
                if !exceptions(|e| e.http.allow(&host)) {
                    return;
                }
                self.errors.upgraded.remove(&tab);
                shown.plain.unwrap_or_else(|| {
                    let mut plain = shown.url.clone();
                    let _ = plain.set_scheme("http");
                    plain
                })
            }
            ErrorPage::Certificate(_) | ErrorPage::ClockWrong => {
                if !exceptions(|e| e.certs.allow(&host)) {
                    self.toast(&t("toast.hsts"));
                    return;
                }
                shown.url
            }
            _ => return,
        };
        if let Some(view) = self.tabs.get(&tab) {
            let _ = view.load_url(target.as_str());
        }
        self.ui_bar();
    }
}

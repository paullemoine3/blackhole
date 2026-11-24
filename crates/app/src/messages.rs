//! Messages from the engine and from background threads (`Msg`), handled on the UI thread.

use std::time::{Duration, Instant};

use serde_json::json;
use url::Url;

use crate::views;
use crate::i18n::{t, tf};
use crate::ipc::{Msg, PageEvent};
use crate::platform::trace;
use crate::shell::notices;
use crate::shell::peek::PEEK_TAB;
use crate::state::Running;

#[cfg(target_os = "macos")]
use crate::shell::layout::BAR_HEIGHT;

/// How long a failed navigation waits before its error page: for the download that would
/// explain a "connection aborted", or the next page a redirect is already loading.
pub(crate) const ABORT_GRACE: Duration = Duration::from_millis(600);

impl Running {
    pub(crate) fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Ui(command) => self.handle_ui(command),
            Msg::Page(tab, PageEvent::Shortcut { key }) => {
                let active = self.svc.workspace().active_tab().map(|t| t.id);
                if Some(tab) == active || Some(tab) == self.split_tab() || tab == PEEK_TAB {
                    self.shortcut(&key);
                }
            }
            Msg::Page(_, PageEvent::Peek { url }) => {
                if let Ok(url) = Url::parse(&url) {
                    self.open_peek(&url);
                }
            }
            Msg::Page(tab, PageEvent::LivePick { selector, w, h, vw }) => {
                if self.live_pick.take() == Some(tab) {
                    self.live_picked(tab, selector, (w, h, vw));
                }
            }
            Msg::Page(tab, PageEvent::OpenLink { url }) => {
                if let Ok(target) = Url::parse(&url) {
                    self.opened_from(tab, &target);
                }
                self.open_in_background(&url);
                self.referrer_next = None;
            }
            Msg::Page(tab, PageEvent::PilotLink { url }) => self.pilot_link(tab, &url),
            Msg::Page(tab, PageEvent::LinkMenu { url, text, mark, image, x, y }) => {
                self.link_menu(tab, &url, &text, mark, &image, (x, y))
            }
            Msg::Page(tab, PageEvent::DebugLog { msg }) => {
                if cfg!(debug_assertions) {
                    eprintln!("[page {}] {}", tab.0, msg.chars().take(600).collect::<String>());
                }
            }
            Msg::Page(tab, PageEvent::PipLeft { paused, stream }) => self.pip_left(tab, paused, stream),
            Msg::Page(tab, PageEvent::AutofillQuery { fresh }) => self.autofill_query(tab, fresh),
            Msg::Page(tab, PageEvent::AutofillFill { index, token }) => {
                self.autofill_fill(tab, index, &token)
            }
            Msg::Page(tab, PageEvent::AutofillUnlock { token }) => self.autofill_unlock(tab, &token),
            Msg::Page(tab, PageEvent::AutofillGenerate { token }) => {
                self.autofill_generate(tab, &token)
            }
            Msg::Page(tab, PageEvent::LoginSubmitted { username, password }) => {
                self.login_submitted(tab, &username, &password)
            }
            Msg::Page(tab, PageEvent::Location { url }) => {
                self.recall_seen(tab);
                self.marks_push(tab, false);
                self.pilot_mark(tab);
                self.media_hinted(tab);
                self.page_moved(tab, &url)
            }
            Msg::Page(tab, PageEvent::Favicon { url }) => {
                let page = self.tabs.get(&tab).and_then(|v| v.url().ok());
                if let Some(page) = page.and_then(|raw| Url::parse(&raw).ok()) {
                    if self.favicons.set(&page, &url) {
                        self.icons_for_ui.set(true);
                        self.icons_for_home.set(true);
                        self.ui_state();
                    }
                }
            }
            Msg::Page(tab, PageEvent::Blocked { host }) => {
                let site = self.site_of(tab);
                let space = self.space_of_tab(tab);
                self.on_blocked("", &host, &site, space);
            }
            Msg::Page(tab, PageEvent::Requests { list }) => self.tracker_requests(tab, list),
            Msg::Page(tab, PageEvent::PasskeyRefused { .. }) => {
                let site = self.site_of(tab);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                if self.passkey_refusals.should_tell(&site, now) {
                    self.notify_action(
                        notices::Notice::Problem,
                        &tf("toast.passkey_refused", &[("host", &site)]),
                        &t("toast.passkey_extensions"),
                        json!({"cmd": "open_settings", "tab": "extensions"}),
                    );
                }
            }
            Msg::Url(tab, url) => {
                self.pilot_mark(tab);
                self.page_url(tab, url);
            }
            Msg::LoadStart(tab) => {
                self.aborted.remove(&tab);
                self.downloads.navs.remove(&tab);
                self.errors_load_start(tab);
                self.awaiting.remove(&tab);
                self.pending.remove(&tab);
                self.loading.insert(tab, Instant::now());
                self.page_progress(tab, "tint");
                self.ui_bar();
            }
            Msg::Loaded(tab, url) => {
                // A page loaded after a failure: that failure was a hop nobody waits for.
                if self.aborted.remove(&tab).is_some() {
                    self.forget_failure(tab);
                }
                self.loaded_again(&url);
                self.recall_seen(tab);
                self.marks_push(tab, true);
                self.pilot_mark(tab);
                self.media_hinted(tab);
                self.errors_loaded(tab, &url);
                self.auth.prompts.loaded(tab);
                self.loading.remove(&tab);
                self.page_progress(tab, "done");
                self.pending.remove(&tab);
                if self.splash.remove(&tab).is_some() {
                    self.sync();
                } else {
                    self.ui_bar();
                }
                if tab != PEEK_TAB && url.scheme() != "about" {
                    let injection = self.svc.injection_for(&url);
                    if let (false, Some(view)) = (injection.is_empty(), self.tabs.get(&tab)) {
                        views::apply_injection(view, &injection.css, &injection.js);
                    }
                }
            }
            Msg::Failed(tab, url) => {
                trace(|| format!("failed tab={} {url}", tab.0));
                self.page_progress(tab, "done");
                if self.download_took(tab) {
                    self.ui_bar();
                } else {
                    // Never at once: WebView2 ends a navigation that becomes a download as
                    // "connection aborted" just before it says a download starts, and a
                    // redirect made by a script ends the page it leaves as failed while the
                    // next one loads. Both settle within `ABORT_GRACE`.
                    let engine_at = self.tabs.get(&tab).and_then(|v| v.url().ok());
                    self.aborted.insert(tab, (Instant::now(), url, engine_at));
                }
            }
            Msg::Crashed(tab) => self.on_crashed(tab),
            Msg::Page(tab, PageEvent::Media { .. }) => self.media_hinted(tab),
            Msg::LoadFailure(tab, failure) => {
                trace(|| format!("load failure tab={} {} {} {}", tab.0, failure.domain, failure.code, failure.url));
                if failure.http_status > 0 || !self.download_took(tab) {
                    self.on_load_failure(tab, failure);
                }
            }
            Msg::ServerTrust(tab, challenge) => self.on_server_trust(tab, challenge),
            Msg::Upgraded(tab, plain) => self.on_upgraded(tab, plain),
            Msg::Hop(tab) => self.errors_hop(tab),
            Msg::BlockedPage(tab, url, why) => self.on_blocked_page(tab, url, why),
            Msg::LinkCleaned => self.links_cleaned = self.links_cleaned.saturating_add(1),
            Msg::Page(tab, PageEvent::ErrorMenu { x, y }) => self.error_menu(tab, x, y),
            Msg::Page(tab, PageEvent::ErrorBack { length }) => self.error_back(tab, length),
            Msg::Page(tab, PageEvent::ErrorRetry) => self.error_retry(tab),
            Msg::Page(tab, PageEvent::ErrorClearCookies) => self.error_clear_cookies(tab),
            Msg::Auth(tab, challenge, reply) => {
                self.errors_auth_asked(tab);
                self.on_auth(tab, &challenge, reply);
            }
            Msg::Title(tab, _) if tab == PEEK_TAB => {}
            Msg::Title(tab, title) => {
                self.svc.page_changed(tab, None, Some(title));
                self.ui_stale = true;
            }
            // An extension asking for a window (Bitwarden's unlock) gets a popup, not a tab.
            #[cfg(windows)]
            Msg::OpenTab(url) if blackhole_domain::is_extension_popout(&url) => {
                self.close_ext_popup();
                let rect = self.ext_anchor(&url);
                self.show_ext_popup(url.as_str(), rect);
            }
            #[cfg(target_os = "macos")]
            Msg::OpenTab(url)
                if blackhole_domain::is_extension_popout(&url)
                    && self.show_native_popup(
                        url.host_str().unwrap_or_default(),
                        (self.window_size().width, BAR_HEIGHT, 0.0, 0.0),
                    ) => {}
            Msg::OpenTab(url) if self.welcome.drops(false, Instant::now()) => {
                trace(|| format!("welcome page dropped {url}"));
            }
            Msg::OpenTab(url) => {
                trace(|| format!("open tab {url}"));
                self.open(url.as_str());
            }
            // No tab behind it (a view not showing a tab yet, never the Peek page).
            Msg::OpenTabFrom(opener, url)
                if self
                    .welcome
                    .drops(opener != PEEK_TAB || self.peek.is_some(), Instant::now()) =>
            {
                trace(|| format!("welcome page dropped {url}"));
            }
            Msg::OpenTabFrom(opener, url) => {
                trace(|| format!("open tab {url} from tab {}", opener.0));
                self.opened_from(opener, &url);
                self.open(url.as_str());
                self.referrer_next = None;
            }
            Msg::External(url) if crate::shell::windows::private_mode() => {
                crate::shell::windows::hand_external(&url);
            }
            Msg::External(url) => {
                self.window.set_minimized(false);
                self.window.focus_window();
                self.open(url.as_str());
            }
            Msg::Redirect(tab, url) => {
                if self.refuse_restricted_port(tab, &url) {
                    // Nothing to load: WebKit would refuse it without a word.
                } else if let Some(view) = self.view_of(tab) {
                    let _ = view.load_url(url.as_str());
                }
            }
            Msg::DownloadStarted { tab, url, path } => self.download_started(tab, url, path),
            Msg::DownloadDone { url, path, ok } => self.download_done(url, path, ok),
            // A navigation the user started: the space shown.
            Msg::Blocked { why, host } => {
                let space = self.svc.workspace().active_space().id.0;
                self.on_blocked(why, &host, "", space);
            }
        }
    }
}

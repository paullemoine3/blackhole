//! HTTP authentication prompts of the tabs.

use std::collections::HashMap;

use blackhole_domain::{http_auth, TabId};
use url::Url;

use crate::shell::peek::PEEK_TAB;
use crate::state::Running;

/// HTTP authentication prompts waiting for the user, their engine answers, and the one shown.
#[derive(Default)]
pub(crate) struct Auth {
    pub(crate) prompts: http_auth::AuthPrompts,
    pub(crate) replies: HashMap<u64, wry::HttpAuthResponder>,
    pub(crate) shown: Option<u64>,
    /// The exact origin of each prompt (read from the engine), for the logins saved for it.
    pub(crate) origins: HashMap<u64, String>,
}

impl Running {
    /// A server asks for a user name and password. The tab is not unreachable: it waits for
    /// the prompt, shown once the tab is on screen.
    pub(crate) fn on_auth(&mut self, tab: TabId, challenge: &wry::HttpAuthChallenge, reply: wry::HttpAuthResponder) {
        if !self.auth_tab_alive(tab) {
            return; // `reply` dropped: cancelled.
        }
        self.awaiting.remove(&tab);
        self.pending.remove(&tab);
        let realm = http_auth::realm_of(&challenge.challenge);
        let id = self
            .auth.prompts
            .push(tab, &challenge.host, &realm, challenge.secure, challenge.retry);
        self.auth.replies.insert(id, reply);
        // The logins saved for exactly this origin (https or localhost) can answer it.
        let origin = Url::parse(&challenge.url)
            .ok()
            .and_then(|u| blackhole_domain::passwords::origin_of(&u));
        if let Some(origin) = origin {
            self.auth.origins.insert(id, origin);
        }
        if self.splash.remove(&tab).is_some() {
            self.sync();
        }
        self.service_auth();
    }

    /// Peek has no prompt (the palette would close it): its page gets the 401, opening it as
    /// a tab asks.
    fn auth_tab_alive(&self, tab: TabId) -> bool {
        tab.0 != 0 && tab != PEEK_TAB && self.tabs.contains_key(&tab)
    }

    /// Tabs whose page is on screen: the active one and the other pane of a split.
    fn auth_tabs_shown(&self) -> Vec<TabId> {
        let mut shown = Vec::new();
        if !self.home_shown() {
            shown.extend(self.svc.workspace().active_tab().map(|t| t.id));
        }
        shown.extend(self.split_tab());
        shown
    }

    /// Shows the oldest prompt of a tab on screen, unless one is up; drops those of closed tabs.
    pub(crate) fn service_auth(&mut self) {
        if self.auth.prompts.is_empty() {
            return;
        }
        let gone: Vec<TabId> = self
            .auth.replies
            .keys()
            .filter_map(|id| self.auth.prompts.get(*id).map(|c| c.tab))
            .filter(|tab| !self.auth_tab_alive(*tab))
            .collect();
        for tab in gone {
            for id in self.auth.prompts.drop_tab(tab) {
                self.auth.replies.remove(&id);
                if self.auth.shown == Some(id) {
                    self.auth.shown = None;
                    if self.palette.mode == "auth" {
                        self.close_palette();
                    }
                }
            }
        }
        if self.auth.shown.is_some() {
            return;
        }
        let Some((id, _)) = self.auth.prompts.next_for(&self.auth_tabs_shown()) else {
            return;
        };
        self.auth.shown = Some(id);
        self.open_palette("auth");
    }

    /// Sends the answer to the engine (credentials are never kept nor logged), for every
    /// challenge of the same site, then shows the next prompt if any.
    pub(crate) fn answer_auth(&mut self, id: u64, credentials: Option<(String, String)>) {
        if self.auth.shown == Some(id) {
            self.auth.shown = None;
            if self.palette.mode == "auth" {
                self.close_palette();
            }
        }
        self.reply_auth(id, credentials);
        self.service_auth();
    }

    pub(crate) fn reply_auth(&mut self, id: u64, credentials: Option<(String, String)>) {
        for each in self.auth.prompts.answer(id, credentials.is_some()) {
            self.auth.origins.remove(&each);
            if let Some(reply) = self.auth.replies.remove(&each) {
                reply.respond(credentials.clone());
            }
        }
    }
}

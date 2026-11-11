//! HTTP authentication (Basic, Digest) asked by a server, with the desktop's rules
//! (`domain::http_auth`): the question goes to the interface for the tab on screen only, one
//! answer serves every challenge of the same tab, host and realm, and credentials are handed
//! to the WebView once, never stored nor logged.

use crate::bridge::{Effect, Secret};
use crate::core::Core;
use blackhole_domain::http_auth::{realm_of, AuthPrompts};
use blackhole_domain::TabId;
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub(crate) struct Auth {
    prompts: AuthPrompts,
    /// The domain's challenge id → the shell's request id (its `HttpAuthHandler`).
    shell: HashMap<u64, u64>,
    /// The challenge the interface is asking about.
    asked: Option<u64>,
}

impl Core {
    /// `onReceivedHttpAuthRequest`: `id` is the shell's request.
    pub(crate) fn auth_asked(&mut self, id: u64, tab: u64, host: &str, realm: &str) -> Vec<Effect> {
        if !self.exists(tab) || host.is_empty() {
            return vec![Self::auth_cancel(id)];
        }
        let secure = self
            .pages
            .get(&tab)
            .map(|p| p.0.clone())
            .or_else(|| self.tab_url(tab))
            .is_some_and(|u| u.scheme() == "https");
        let challenge = self.auth.prompts.push(TabId(tab), host, &realm_of(realm), secure, false);
        self.auth.shell.insert(challenge, id);
        self.auth_prompt().into_iter().collect()
    }

    fn auth_cancel(id: u64) -> Effect {
        Effect::HttpAuth {
            id,
            user: None,
            password: None,
        }
    }

    /// The question for the tab on screen, unless one is already shown.
    pub(crate) fn auth_prompt(&mut self) -> Option<Effect> {
        if self.auth.asked.is_some_and(|id| self.auth.prompts.get(id).is_some()) {
            return None;
        }
        self.auth.asked = None;
        let shown: Vec<TabId> = self.current.map(TabId).into_iter().collect();
        let (id, c) = self.auth.prompts.next_for(&shown)?;
        self.auth.asked = Some(id);
        Some(Effect::Ui {
            msg: json!({
                "type": "auth", "id": id, "host": c.host, "realm": c.realm,
                "secure": c.secure, "retry": c.retry,
            }),
        })
    }

    /// The interface's answer; `None`: cancelled (the server's own 401 page shows).
    pub(crate) fn auth_answer(&mut self, id: u64, credentials: Option<(String, Secret)>) -> Vec<Effect> {
        let mut out: Vec<Effect> = self
            .auth
            .prompts
            .answer(id, credentials.is_some())
            .into_iter()
            .filter_map(|challenge| self.auth.shell.remove(&challenge))
            .map(|shell| match &credentials {
                Some((user, password)) => Effect::HttpAuth {
                    id: shell,
                    user: Some(user.clone()),
                    password: Some(password.clone()),
                },
                None => Self::auth_cancel(shell),
            })
            .collect();
        out.extend(self.auth_prompt());
        out
    }

    /// The tab loaded a page: the credentials given before were accepted.
    pub(crate) fn auth_loaded(&mut self, tab: u64) {
        self.auth.prompts.loaded(TabId(tab));
    }

    /// A closed (or dead) tab's challenges are cancelled.
    pub(crate) fn auth_of_closed(&mut self, tab: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        for challenge in self.auth.prompts.drop_tab(TabId(tab)) {
            if self.auth.asked == Some(challenge) {
                self.auth.asked = None;
                out.push(Effect::Ui {
                    msg: json!({"type": "auth_gone", "id": challenge}),
                });
            }
            if let Some(shell) = self.auth.shell.remove(&challenge) {
                out.push(Self::auth_cancel(shell));
            }
        }
        out
    }
}

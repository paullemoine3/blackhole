//! What pages ask for (camera, microphone, location, notifications, clipboard): the prompt, the
//! answers kept for each site, and where they are changed. Rules in `domain::permissions`.

use std::collections::HashMap;

use blackhole_domain::permissions::{origin_of, Ask, Asks, Permission, Verdict};
use blackhole_domain::TabId;
use serde_json::{json, Value};
use url::Url;

use crate::i18n::t;
use crate::ipc::UiCommand;
use crate::state::Running;

/// Requests waiting for the user, their engine answers, and the prompt shown.
#[derive(Default)]
pub(crate) struct Permits {
    asks: Asks,
    replies: HashMap<u64, wry::SitePermissionResponder>,
    /// The requests the prompt answers, with what it says; `seq` ties an answer to its prompt.
    shown: Option<(Vec<u64>, Ask)>,
    seq: u64,
}

fn wanted(kinds: &[wry::PermissionKind]) -> Option<Vec<Permission>> {
    kinds
        .iter()
        .map(|k| {
            Some(match k {
                wry::PermissionKind::Camera => Permission::Camera,
                wry::PermissionKind::Microphone => Permission::Microphone,
                wry::PermissionKind::Geolocation => Permission::Location,
                wry::PermissionKind::Notifications => Permission::Notifications,
                wry::PermissionKind::ClipboardRead => Permission::Clipboard,
                _ => return None,
            })
        })
        .collect()
}

/// What the engine of this system asks the shell about.
#[cfg(windows)]
const OFFERED: &[Permission] = &Permission::ALL;
#[cfg(target_os = "macos")]
const OFFERED: &[Permission] = &[Permission::Camera, Permission::Microphone];
#[cfg(not(any(windows, target_os = "macos")))]
const OFFERED: &[Permission] =
    &[Permission::Camera, Permission::Microphone, Permission::Location, Permission::Notifications];

fn label(permission: Permission) -> String {
    t(match permission {
        Permission::Camera => "permission.camera",
        Permission::Microphone => "permission.microphone",
        Permission::Location => "permission.location",
        Permission::Notifications => "permission.notifications",
        Permission::Clipboard => "permission.clipboard",
    })
}

fn icon(permission: Permission) -> &'static str {
    match permission {
        Permission::Camera => "camera",
        Permission::Microphone => "mic",
        Permission::Location => "globe",
        Permission::Notifications => "bell",
        Permission::Clipboard => "copy",
    }
}

impl Running {
    /// A page asks for something: settled by what was decided for its site, or asked once its
    /// tab is on screen. Dropping `reply` denies (Peek, a tab that is gone, an unknown page).
    pub(crate) fn on_permission(
        &mut self,
        tab: TabId,
        request: &wry::SitePermissionRequest,
        reply: wry::SitePermissionResponder,
    ) {
        let origin = |raw: &str| Url::parse(raw).ok().and_then(|u| origin_of(&u));
        let (Some(page), Some(asker), Some(wanted)) =
            (origin(&request.page), origin(&request.origin), wanted(&request.kinds))
        else {
            return;
        };
        if !self.prompt_tab_alive(tab) {
            return;
        }
        let ask = Ask { tab, frame: (asker != page).then_some(asker), origin: page, wanted };
        // A black hole tab remembers nothing, and gets nothing of what other tabs were allowed.
        let stored = &self.svc.settings().site_permissions;
        let verdict = if self.svc.workspace().is_burner(tab) {
            self.permits.asks.verdict(&ask, &stored.for_private())
        } else {
            self.permits.asks.verdict(&ask, stored)
        };
        match verdict {
            Verdict::Allow => reply.respond(true),
            Verdict::Deny => reply.respond(false),
            Verdict::Ask => {
                let id = self.permits.asks.push(ask);
                self.permits.replies.insert(id, reply);
            }
        }
    }

    /// The page of a tab is being left: what it asked goes with it.
    pub(crate) fn permissions_left(&mut self, tab: TabId) {
        for id in self.permits.asks.drop_tab(tab) {
            self.permits.replies.remove(&id);
        }
        if self.permits.shown.as_ref().is_some_and(|(_, a)| a.tab == tab) {
            self.permits.shown = None;
            if self.palette.mode == "permission" {
                self.close_palette();
            }
        }
    }

    /// Shows the oldest request of a tab on screen, unless the command bar or Peek is in use: a
    /// page never takes the keyboard from something the user is typing in.
    pub(crate) fn service_permissions(&mut self) {
        if self.permits.asks.is_empty() {
            return;
        }
        for tab in self.permits.asks.tabs() {
            if !self.prompt_tab_alive(tab) {
                self.permissions_left(tab);
            }
        }
        if self.permits.shown.is_some()
            || self.palette.open
            || self.peek.is_some()
            || self.settings_shown
            || self.library
            || self.easel_tab
        {
            return;
        }
        let Some(next) = self.permits.asks.next_for(&self.tabs_on_screen()) else { return };
        self.permits.shown = Some(next);
        self.permits.seq += 1;
        self.open_palette("permission");
    }

    /// The prompt as the palette draws it. `keep`: the answer may be remembered.
    pub(crate) fn permission_prompt(&self) -> Value {
        let Some((_, ask)) = &self.permits.shown else { return Value::Null };
        let host = blackhole_domain::passwords::display_host;
        json!({
            "seq": self.permits.seq,
            "host": host(&ask.origin),
            "frame": ask.frame.as_deref().map(host),
            "insecure": ask.origin.starts_with("http://"),
            "keep": self.keeps(ask),
            "wanted": ask.wanted.iter().map(|p| json!({"icon": icon(*p), "label": label(*p)})).collect::<Vec<_>>(),
        })
    }

    fn keeps(&self, ask: &Ask) -> bool {
        ask.frame.is_none() && !self.svc.workspace().is_burner(ask.tab)
    }

    /// The prompt was left without an answer (Esc, a click outside, another use of the palette).
    pub(crate) fn dismiss_permission(&mut self) {
        if let Some((ids, ask)) = self.permits.shown.take() {
            self.permits.asks.dismiss(&ask);
            self.reply_permissions(&ids, false);
        }
    }

    fn reply_permissions(&mut self, ids: &[u64], allow: bool) {
        self.permits.asks.take(ids);
        for id in ids {
            if let Some(reply) = self.permits.replies.remove(id) {
                reply.respond(allow);
            }
        }
    }

    pub(crate) fn handle_permissions(&mut self, command: UiCommand) {
        match command {
            UiCommand::PermissionAnswer { seq, allow, keep } => {
                if seq != self.permits.seq {
                    return;
                }
                let Some((ids, ask)) = self.permits.shown.take() else { return };
                if keep && self.keeps(&ask) {
                    for p in &ask.wanted {
                        self.svc.set_site_permission(&ask.origin, *p, Some(allow));
                    }
                } else if !allow {
                    self.permits.asks.dismiss(&ask);
                }
                self.close_palette();
                self.reply_permissions(&ids, allow);
                self.ui_bar();
                self.ui_settings();
            }
            UiCommand::SitePanel { x, y } => self.site_panel((x, y)),
            UiCommand::PermissionSet { origin, permission, allowed } => {
                if let Some(p) = Permission::from_id(&permission) {
                    self.svc.set_site_permission(&origin, p, allowed);
                    self.permits.asks.forgive(&origin);
                    // The panel says it itself.
                    if !self.palette.open {
                        self.toast(&t("toast.permission_changed"));
                    }
                    self.ui_settings();
                    self.ui_bar();
                }
            }
            UiCommand::PermissionForget { origin } => {
                self.svc.forget_site_permissions(origin.as_deref());
                match &origin {
                    Some(origin) => self.permits.asks.forgive(origin),
                    None => self.permits.asks = Asks::default(),
                }
                // A request waiting when everything is reset is refused rather than lost.
                if origin.is_none() {
                    self.permits.replies.clear();
                    self.permits.shown = None;
                }
                self.ui_settings();
            }
            _ => {}
        }
    }

    /// The origin of the page shown, when it is a web page.
    fn origin_shown(&self) -> Option<(TabId, String)> {
        let tab = self.svc.workspace().active_tab().filter(|_| !self.home_shown())?;
        Some((tab.id, origin_of(&tab.url)?))
    }

    /// Whether something was decided for the site shown (the dot of the address's button).
    pub(crate) fn site_has_permissions(&self) -> bool {
        self.origin_shown().is_some_and(|(tab, origin)| {
            !self.svc.workspace().is_burner(tab) && !self.svc.settings().site_permissions.of(&origin).is_empty()
        })
    }

    /// The panel under the address's button: every permission of the site shown, with a choice.
    fn site_panel(&mut self, at: (f64, f64)) {
        let Some((tab, origin)) = self.origin_shown() else { return };
        let mut items = vec![
            json!({"head": blackhole_domain::passwords::display_host(&origin)}),
            json!({"note": t(if origin.starts_with("https://") { "permission.secure" } else { "permission.insecure" })}),
            json!({"sep": true}),
        ];
        if self.svc.workspace().is_burner(tab) {
            items.push(json!({"note": t("permission.burner")}));
        } else {
            let stored = &self.svc.settings().site_permissions;
            items.extend(OFFERED.iter().map(|p| {
                let now = stored.get(&origin, *p);
                let choice = |text: &str, value: Option<bool>| {
                    json!({"label": t(text), "on": now == value,
                        "run": {"cmd": "permission_set", "origin": origin, "permission": p.id(), "allowed": value}})
                };
                json!({"icon": icon(*p), "label": label(*p), "choices": [
                    choice("permission.choice_ask", None),
                    choice("permission.choice_allow", Some(true)),
                    choice("permission.choice_block", Some(false)),
                ]})
            }));
            items.push(json!({"note": t("permission.next_time")}));
        }
        items.push(json!({"sep": true}));
        items.push(json!({"icon": "settings", "label": t("permission.every_site"), "run": {"cmd": "open_settings", "tab": "privacy"}}));
        self.show_menu(at, json!(items));
    }

    /// The shield's menu: what the site shown may use, each entry turning into its contrary.
    pub(crate) fn permission_menu_items(&self) -> Vec<Value> {
        let origin = self.svc.workspace().active_tab().and_then(|t| origin_of(&t.url));
        let Some(origin) = origin.filter(|_| !self.home_shown()) else { return Vec::new() };
        let stored = self.svc.settings().site_permissions.of(&origin);
        if stored.is_empty() {
            return Vec::new();
        }
        let mut items = vec![json!({"sep": true}), json!({"head": t("permission.title")})];
        items.extend(stored.iter().map(|(p, allowed)| {
            let state = t(if *allowed { "permission.allowed" } else { "permission.blocked" });
            json!({"icon": icon(*p), "label": format!("{} · {state}", label(*p)),
                "run": {"cmd": "permission_set", "origin": origin, "permission": p.id(), "allowed": !allowed}})
        }));
        items.push(json!({"icon": "reload", "label": t("permission.ask_again"),
            "run": {"cmd": "permission_forget", "origin": origin}}));
        items
    }

    /// Settings > Privacy > Site permissions.
    pub(crate) fn permissions_json(&self) -> Value {
        let mut sites: Vec<(String, Vec<Value>)> = Vec::new();
        for (origin, p, allowed) in self.svc.settings().site_permissions.iter() {
            let entry = json!({"id": p.id(), "icon": icon(p), "label": label(p), "allowed": allowed});
            match sites.last_mut() {
                Some((o, list)) if o == origin => list.push(entry),
                _ => sites.push((origin.to_owned(), vec![entry])),
            }
        }
        sites
            .into_iter()
            .map(|(origin, list)| {
                json!({"origin": origin, "host": blackhole_domain::passwords::display_host(&origin), "list": list})
            })
            .collect()
    }
}

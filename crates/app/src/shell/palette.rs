//! The command bar: its state, its suggestions and the actions it runs.

use blackhole_domain::{SpaceId, suggest, TabId};
use blackhole_domain::verbs::Verb;
use serde_json::json;
use wry::WebView;

use crate::views;
use crate::i18n::{t, tf};
use crate::ipc::UiCommand;
use crate::platform::default_browser_unavailable;
use crate::shell::layout::palette_rect;
use crate::shell::render::SPACE_COLORS;
use crate::shell::windows::private_mode;
use crate::state::Running;

/// The command bar overlay and what it shows: its mode, a context menu, the emoji picker.
#[derive(Default)]
pub(crate) struct Palette {
    /// The overlay's page; rebuilt whenever a view is added so it stays on top.
    pub(crate) view: Option<WebView>,
    /// Context menu being shown by the overlay (position and items).
    pub(crate) menu: Option<serde_json::Value>,
    pub(crate) ready: bool,
    pub(crate) open: bool,
    pub(crate) mode: String,
    /// Space or folder whose emoji the picker is editing.
    pub(crate) icon_target: Option<(String, u64)>,
}

impl Running {
    pub(crate) fn open_palette(&mut self, mode: &str) {
        // Another use of the palette replaces the sign-in prompt: it is cancelled.
        if mode != "auth" {
            if let Some(id) = self.auth.shown.take() {
                self.reply_auth(id, None);
            }
        }
        if mode != "permission" {
            self.dismiss_permission();
        }
        self.close_peek();
        self.flush_raise(true);
        self.palette.mode = mode.to_owned();
        self.palette.open = true;
        self.apply_slots();
        if let Some(palette) = &self.palette.view {
            let _ = palette.set_bounds(palette_rect(true, &self.window_size()));
            let _ = palette.set_visible(true);
            let _ = palette.focus();
        }
        if self.palette.ready {
            self.push_palette();
        }
    }

    pub(crate) fn push_palette(&self) {
        let state = self.palette_state();
        if matches!(self.palette.mode.as_str(), "new" | "url") {
            self.suggest_prepare(state["actions"].as_array().map_or(&[][..], Vec::as_slice));
        }
        if let Some(palette) = &self.palette.view {
            let _ = palette.evaluate_script(&format!("window.bh&&bh.open({state})"));
        }
    }

    pub(crate) fn close_palette(&mut self) {
        self.burner_next = false;
        if !self.palette.open {
            return;
        }
        self.palette.open = false;
        self.sidebar.hold_for_menu(false);
        self.apply_slots();
        if let Some(palette) = &self.palette.view {
            let _ = palette.evaluate_script("window.bh&&bh.hide()");
            let _ = palette.set_visible(false);
            let _ = palette.set_bounds(palette_rect(false, &self.window_size()));
        }
        self.apply_focus();
        // Closing the sign-in prompt any other way (Esc, click outside, another palette)
        // cancels it: the server's own 401 page shows.
        if self.palette.mode == "auth" {
            if let Some(id) = self.auth.shown.take() {
                self.answer_auth(id, None);
            }
        }
        if self.palette.mode == "permission" {
            self.dismiss_permission();
        }
    }

    // ---- HTTP authentication --------------------------------------------------------------

    fn palette_state(&self) -> serde_json::Value {
        let ws = self.svc.workspace();
        let space = ws.active_space();
        let active = if self.home_shown() {
            None
        } else {
            ws.active_tab()
        };
        let site = self.active_site();
        let easel_actions: Vec<_> = self
            .easels
            .all()
            .iter()
            .map(|e| json!({"id": format!("easel:{}", e.id), "label": tf("palette.open_easel_named", &[("name", &e.name)]), "hint": "Easel"}))
            .collect();
        // Labels: sentence case, the words of the menus. `keys` is a shortcut the palette spells
        // the way the system does (see ui/keys.js); `confirm` asks for a second Enter.
        let mut actions = vec![
            json!({"id": "new_space", "label": t("palette.new_space")}),
            json!({"id": "new_folder", "label": t("palette.new_folder")}),
            json!({"id": "icon_space", "label": tf("palette.space_emoji", &[("name", &space.name)]), "hint": t("palette.hint_emoji")}),
            json!({"id": "toggle_sidebar", "label": t("palette.toggle_sidebar"), "keys": "mod+b"}),
            json!({"id": "new_easel", "label": t("menu.new_easel"), "hint": "Easel"}),
            json!({"id": "open_easel", "label": t("palette.open_easel"), "keys": "mod+shift+e"}),
            json!({"id": "open_library", "label": t("palette.open_library"), "hint": t("palette.hint_library_all")}),
            json!({"id": "settings", "label": t("palette.open_settings"), "keys": "mod+,"}),
            json!({"id": "default_browser", "label": t("palette.default_browser"), "hint": t("palette.hint_system")}),
            json!({"id": "clear_history", "label": t("palette.clear_history"), "confirm": t("palette.clear_history_confirm")}),
            json!({"id": "open_history", "label": t("palette.open_history"), "hint": t("palette.hint_library")}),
            json!({"id": "recall", "label": t("palette.recall"), "keys": "mod+shift+f"}),
            json!({"id": "tidy", "label": t("palette.tidy"), "hint": t("palette.hint_tabs")}),
            json!({"id": "open_watches", "label": t("palette.watched_areas"), "hint": t("palette.hint_library")}),
            json!({"id": "open_highlights", "label": t("palette.highlights"), "hint": t("palette.hint_library")}),
            json!({"id": "compare", "label": t("palette.compare"), "hint": t("palette.hint_tabs")}),
            json!({"id": "key:shift+t", "label": t("palette.reopen_closed"), "keys": "mod+shift+t"}),
            json!({"id": "key:n", "label": t("palette.new_window"), "keys": "mod+n"}),
            json!({"id": "key:shift+n", "label": t("palette.new_private_window"), "keys": "mod+shift+n"}),
            json!({"id": "burner_tab", "label": t("palette.new_burner_tab"), "hint": t("palette.hint_burner")}),
        ];
        if self.pulsars_on() {
            actions.push(json!({"id": "new_pulsar", "label": t("palette.new_pulsar"), "hint": "Pulsar"}));
        }
        // From a private window Mod+N opens a private one too: one entry is enough.
        if private_mode() {
            actions.retain(|a| a["id"] != "key:n");
        }
        actions.extend(easel_actions);
        actions.extend(self.meeting_actions());
        actions.extend(self.continue_actions());
        actions.extend(self.beam_actions());
        for folder in &space.folders {
            actions.push(json!({
                "id": format!("icon_folder:{}", folder.id.0),
                "label": tf("palette.folder_emoji", &[("name", &folder.name)]),
                "hint": t("palette.hint_emoji"),
            }));
        }
        if self.split_tab().is_some() {
            actions
                .push(json!({"id": "close_split", "label": t("palette.close_split"), "keys": "mod+\\"}));
            let label = if self.pilot_panes().is_some() { "palette.pilot_off" } else { "palette.pilot_on" };
            actions.push(json!({"id": "pilot", "label": t(label), "hint": t("palette.hint_split")}));
        }
        if let Some(current) = active {
            for tab in space.tabs.iter().filter(|t| t.id != current.id) {
                let name = if tab.title.is_empty() {
                    tab.url.as_str()
                } else {
                    &tab.title
                };
                actions.push(json!({
                    "id": format!("split:{}", tab.id.0),
                    "label": tf("palette.split_with", &[("name", name)]),
                    "hint": t("palette.hint_split"),
                }));
            }
        }
        if let Some(current_tab) = active.map(|t| t.id) {
            actions.extend([
                json!({"id": "key:f", "label": t("palette.find"), "keys": "mod+f"}),
                json!({"id": "key:p", "label": t("palette.print"), "keys": "mod+p"}),
                json!({"id": "key:zoom_in", "label": t("palette.zoom_in"), "keys": "mod+plus"}),
                json!({"id": "key:zoom_out", "label": t("palette.zoom_out"), "keys": "mod+minus"}),
                json!({"id": "key:zoom_reset", "label": t("palette.zoom_reset"), "keys": "mod+0"}),
                json!({"id": "key:shift+r", "label": t("palette.hard_reload"), "keys": "mod+shift+r"}),
                json!({
                    "id": "key:devtools", "label": t("palette.devtools"),
                    "keys": if cfg!(target_os = "macos") { "mod+alt+i" } else { "F12" },
                }),
            ]);
            actions.push(json!({"id": "pip", "label": t("palette.pip"), "hint": t("palette.hint_video")}));
            if self.can_mark(current_tab) {
                actions.push(json!({"id": "highlight", "label": t("palette.highlight"), "hint": t("palette.hint_page")}));
            }
            // Only what the page menu would offer for it (`domain::verbs`): nothing is kept of
            // a black hole tab.
            if self.front_page_allows(Verb::Keep) {
                actions.push(
                    json!({"id": "capture_easel", "label": t("palette.capture_easel"), "keys": "mod+shift+s"}),
                );
            }
            if self.front_page_allows(Verb::LiveArea) {
                actions.push(
                    json!({"id": "capture_live", "label": t("palette.capture_live"), "keys": "mod+shift+l"}),
                );
            }
        }
        if let Some(site) = &site {
            if self.recall_on() && !blackhole_domain::recall::excluded(site, &self.svc.settings().recall_exclude) {
                actions.push(json!({"id": "recall_exclude", "label": tf("palette.recall_exclude", &[("site", site)]), "hint": "Recall"}));
            }
            actions.push(json!({"id": "boost", "label": tf("palette.boost_site", &[("site", site)]), "hint": "Boost"}));
            let routed = self.svc.route_for(site);
            for other in ws.spaces().iter().filter(|s| Some(s.id) != routed) {
                actions.push(json!({
                    "id": format!("route:{}", other.id.0),
                    "label": tf("palette.route_to", &[("site", site), ("space", &other.name)]),
                    "hint": "Air Traffic",
                }));
            }
            if routed.is_some() {
                actions.push(json!({"id": "route_clear", "label": tf("palette.route_clear", &[("site", site)]), "hint": "Air Traffic"}));
            }
        }
        let url = match (self.palette.mode.as_str(), active) {
            ("url", Some(tab)) => tab.url.to_string(),
            _ => String::new(),
        };
        let target = self.palette.icon_target.as_ref().and_then(|(kind, id)| {
            let (name, icon) = if kind == "space" {
                let s = ws.spaces().iter().find(|s| s.id.0 == *id)?;
                (&s.name, &s.icon)
            } else {
                let f = space.folders.iter().find(|f| f.id.0 == *id)?;
                (&f.name, &f.icon)
            };
            Some(json!({"kind": kind, "id": id, "name": name, "icon": icon}))
        });
        json!({
            "target": target,
            "mode": self.palette.mode,
            "burner": self.burner_next,
            "recall": self.recall_state(),
            "animations": self.svc.settings().animations,
            "depth": self.svc.settings().depth,
            "url": url,
            "engine": self.svc.settings().search_engine.label(),
            "color": space.color,
            "menu": self.palette.menu,
            "compare": self.comparing.result,
            "permission": self.permission_prompt(),
            "pulsar": self.pulsar_form(),
            "auth": self.auth.shown.and_then(|id| self.auth.prompts.get(id)).map(|c| json!({
                "id": self.auth.shown,
                "host": c.host,
                "realm": c.realm,
                "secure": c.secure,
                "retry": c.retry,
                // User names only: the password goes from the wallet to the engine.
                "saved": self.auth.shown.map(|id| self.auth_saved_users(id)).unwrap_or_default(),
            })),
            // The highlights, watched areas, archive and downloads the search also looks in.
            "found": if matches!(self.palette.mode.as_str(), "new" | "url") { self.found_json() } else { json!(null) },
            "boostHost": site,
            "boost": site.as_deref().and_then(|h| self.svc.boost_for(h)),
            // Every row carries its `key` (domain::suggest): the same page met twice is offered once.
            "tabs": space.tabs.iter().map(|t| json!({"id": t.id.0, "title": t.title, "url": t.url.as_str(), "key": suggest::key(&t.url)})).collect::<Vec<_>>(),
            "favorites": self.svc.favorites().iter().map(|f| json!({"name": f.name, "url": f.url.as_str(), "key": suggest::key(&f.url)})).collect::<Vec<_>>(),
            // What is typed is ranked by the shell (`UiCommand::Suggest`, suggesting.rs): the
            // palette only lists the tabs and quick access before the first letter.
            "actions": actions,
        })
    }

    pub(crate) fn run_action(&mut self, id: &str) {
        let site = self.active_site();
        if id == "boost" {
            self.palette.mode = "boost".to_owned();
            self.push_palette();
            return;
        }
        if id == "recall" {
            self.palette.mode = "recall".to_owned();
            self.push_palette();
            return;
        }
        self.close_palette();
        match id {
            "new_space" => {
                let n = self.svc.workspace().spaces().len();
                self.handle_ui(UiCommand::NewSpace {
                    name: tf("sidebar.default_space", &[("n", &(n + 1).to_string())]),
                    color: SPACE_COLORS[n % SPACE_COLORS.len()].to_owned(),
                });
            }
            "new_folder" => self.handle_ui(UiCommand::NewFolder { parent: None }),
            "new_pulsar" => self.handle_pulsars(UiCommand::PulsarForm),
            "pip" => {
                let _ = self.active_view().evaluate_script(views::PIP_TOGGLE_JS);
                if let Some(id) = self.shown {
                    self.pip_toggled(id);
                }
            }
            "toggle_sidebar" => self.handle_ui(UiCommand::Lock),
            "settings" => self.settings_wanted = true,
            "new_easel" => {
                self.handle_ui(UiCommand::NewEasel {
                    name: String::new(),
                });
                if let Some(id) = self.easel_current {
                    self.open_easel_tab(id);
                }
            }
            "open_easel" => self.handle_ui(UiCommand::OpenEasel { id: None }),
            "open_library" => self.handle_ui(UiCommand::OpenLibrary { section: None }),
            "new_meet" => self.open_meet(None),
            "tidy" => self.handle_ui(UiCommand::Tidy { skip: Vec::new() }),
            "pilot" => self.toggle_pilot(None),
            "burner_tab" => {
                self.new_tab();
                self.burner_next = true;
                self.push_palette();
            }
            "compare" => self.compare_start(),
            other if other.starts_with("meet:") => self.open_meet(Some(other["meet:".len()..].to_owned())),
            other if other.starts_with("beam_clip:") => {
                self.handle_devices(UiCommand::BeamClipboard { device: other["beam_clip:".len()..].to_owned() });
            }
            other if other.starts_with("beam_file:") => {
                self.handle_devices(UiCommand::BeamFile { device: other["beam_file:".len()..].to_owned() });
            }
            other if other.starts_with("continue:") => {
                let tab = self.svc.workspace().active_tab().map(|t| t.id.0);
                if let Some(tab) = tab {
                    self.continue_on(tab, other["continue:".len()..].to_owned());
                }
            }
            "open_history" => {
                self.handle_ui(UiCommand::OpenLibrary { section: Some("history".into()) });
            }
            "open_watches" => {
                self.handle_ui(UiCommand::OpenLibrary { section: Some("watches".into()) });
            }
            "open_highlights" => {
                self.handle_ui(UiCommand::OpenLibrary { section: Some("highlights".into()) });
            }
            "highlight" => self.mark_selection(None),
            "recall_exclude" => {
                if let Some(site) = site {
                    self.handle_recall(UiCommand::RecallExclude { site, on: true });
                }
            }
            // The commands that have a shortcut run exactly as the shortcut does.
            other if other.starts_with("key:") => self.shortcut(&other["key:".len()..]),
            other if other.starts_with("easel:") => {
                if let Ok(id) = other["easel:".len()..].parse() {
                    self.handle_ui(UiCommand::OpenEaselTab { id });
                }
            }
            "capture_easel" => self.handle_ui(UiCommand::CaptureToEasel),
            "capture_live" => self.handle_ui(UiCommand::LivePick),
            "clear_history" => {
                self.svc.clear_history();
                self.recall_forget(None);
                self.toast(&t("toast.history_cleared"));
            }
            "default_browser" => {
                let message = if !blackhole_macos_url::set_default_browser() {
                    default_browser_unavailable()
                } else if cfg!(windows) {
                    t("toast.default_browser_settings")
                } else {
                    t("toast.default_browser_confirm")
                };
                self.toast(&message);
            }
            "icon_space" => {
                let id = self.svc.workspace().active_space().id.0;
                self.handle_ui(UiCommand::PickIcon {
                    kind: "space".to_owned(),
                    id,
                });
            }
            "close_split" => {
                self.svc.clear_split();
                self.sync();
            }
            "route_clear" => {
                if let Some(site) = site {
                    self.svc.set_route(&site, None);
                    self.toast(&tf("toast.route_cleared", &[("site", &site)]));
                }
            }
            other => {
                if let Some(n) = other
                    .strip_prefix("icon_folder:")
                    .and_then(|n| n.parse().ok())
                {
                    self.handle_ui(UiCommand::PickIcon {
                        kind: "folder".to_owned(),
                        id: n,
                    });
                } else if let Some(n) = other.strip_prefix("split:").and_then(|n| n.parse().ok()) {
                    self.svc.set_split(TabId(n));
                    self.sync();
                } else if let (Some(n), Some(site)) = (
                    other.strip_prefix("route:").and_then(|n| n.parse().ok()),
                    site,
                ) {
                    self.svc.set_route(&site, Some(SpaceId(n)));
                    self.toast(&tf("toast.route_set", &[("site", &site)]));
                }
            }
        }
    }
}

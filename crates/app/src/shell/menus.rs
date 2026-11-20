//! Context menus of pages and links, drawn by the command bar overlay.

use blackhole_domain::{TabId, verbs};
use blackhole_domain::verbs::{Target, Verb};
use serde_json::json;
use url::Url;
use wry::WebView;

use crate::i18n::{t, tf};
use crate::state::Running;

impl Running {
    /// Shows a menu (drawn by the command bar overlay) at a point of the window.
    pub(crate) fn show_menu(&mut self, (x, y): (f64, f64), items: serde_json::Value) {
        self.palette.menu = Some(json!({"x": x, "y": y, "items": items}));
        self.open_palette("menu");
    }

    /// Right click on a link of a page: open it in a new tab, a new window, a private window,
    /// preview it, or copy it. On selected text (`text`, untrusted): search it with the engine
    /// of the Settings, or open it when it is a web address, or copy it.
    pub(crate) fn link_menu(&mut self, tab: TabId, raw: &str, text: &str, mark: u64, image: &str, (x, y): (f64, f64)) {
        use blackhole_domain::selection::{
            clean_selection, excerpt, selection_action, SelectionAction,
        };
        let link = Url::parse(raw)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        let selection = clean_selection(text);
        // Copy keeps the selection as it is (lines included), not the one-line search text.
        let copied = blackhole_domain::selection::copy_text(text);
        let mut items = Vec::new();
        if let Some(text) = &selection {
            let short = excerpt(text, 30);
            let action = selection_action(text, self.svc.settings().search_engine);
            let (icon, label) = match &action {
                SelectionAction::Open(_) => ("open", tf("menu.open_address", &[("text", &short)])),
                SelectionAction::Search(_) => ("search", tf("menu.search_for", &[("text", &short)])),
            };
            let url = action.url().as_str();
            // Ctrl/Cmd+click or middle click on the entry: in the background, like a link.
            items.push(json!({"icon": icon, "label": label,
                "run": {"cmd": "open_link", "url": url, "target": "front"},
                "background": {"cmd": "open_link", "url": url, "target": "tab"}}));
            items.push(json!({"icon": "copy", "label": t("menu.copy"), "copy": copied.as_deref().unwrap_or(text)}));
            let can = verbs::verbs(Target::Selection, self.verb_context(tab));
            if can.contains(&Verb::Send) {
                items.extend(self.beam_menu_items(text));
            }
            // The selection is read now, while it is there: the entry only confirms it.
            if can.contains(&Verb::Highlight) && self.can_mark(tab) {
                self.mark_capture(tab, false);
                items.push(json!({"icon": "marker", "label": t("menu.highlight"), "run": {"cmd": "highlight_selection", "tab": tab.0}}));
            }
        }
        let on_mark = if mark == 0 { Vec::new() } else { self.mark_menu_items(tab, mark) };
        if !on_mark.is_empty() {
            if !items.is_empty() {
                items.push(json!({"sep": true}));
            }
            items.extend(on_mark);
        }
        // "Copy link" copies the clean address: what is pasted elsewhere tracks nobody.
        let clean = link
            .as_ref()
            .filter(|_| self.svc.settings().shields.clean_links)
            .and_then(blackhole_domain::clean_link::clean);
        if let Some(url) = &link {
            let link = url.as_str();
            let copied = clean.as_ref().map_or(link, Url::as_str);
            if !items.is_empty() {
                items.push(json!({"sep": true}));
            }
            items.extend([
                json!({"icon": "plus", "label": t("menu.link_new_tab"), "run": {"cmd": "open_link", "url": link, "target": "tab"}}),
                json!({"icon": "window", "label": t("menu.link_new_window"), "run": {"cmd": "open_link", "url": link, "target": "window"}}),
                json!({"icon": "sleep", "label": t("menu.link_private_window"), "run": {"cmd": "open_link", "url": link, "target": "private"}}),
                json!({"icon": "hole", "label": t("menu.link_burner"), "run": {"cmd": "open_link", "url": link, "target": "burner"}}),
                json!({"sep": true}),
                json!({"icon": "open", "label": t("menu.link_peek"), "run": {"cmd": "open_peek", "url": link}}),
                json!({"icon": "link", "label": t(if clean.is_some() { "menu.copy_clean_link" } else { "menu.copy_link" }), "copy": copied}),
            ]);
            // Keep it, send it: the same rule as every other menu (`domain::verbs`).
            let ctx = verbs::Context { web: true, ..self.verb_context(tab) };
            for verb in verbs::verbs(Target::Link, ctx) {
                match verb {
                    Verb::Keep => items.push(json!({"icon": "easel", "label": t("menu.link_to_easel"),
                        "run": {"cmd": "link_to_easel", "url": copied}})),
                    Verb::Send => items.extend(self.send_items("menu.link_send_to", |d| json!({"cmd": "send_link", "device": d, "url": copied}))),
                    _ => {}
                }
            }
        }
        let image = Url::parse(image)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        if let Some(image) = &image {
            let image = image.as_str();
            if !items.is_empty() {
                items.push(json!({"sep": true}));
            }
            items.extend([
                json!({"icon": "image", "label": t("menu.image_new_tab"),
                    "run": {"cmd": "open_link", "url": image, "target": "front"},
                    "background": {"cmd": "open_link", "url": image, "target": "tab"}}),
                json!({"icon": "copy", "label": t("menu.copy_image"), "run": {"cmd": "copy_image", "url": image, "tab": tab.0}}),
                json!({"icon": "link", "label": t("menu.copy_image_address"), "copy": image}),
            ]);
        }
        // Off a link and off a selection: the page's own entries. On one, BlackHole's tools
        // (Easel, live area, watched area) still follow, so the menu offers them wherever the
        // click lands.
        let more = if link.is_none() && selection.is_none() {
            self.page_menu_items(tab)
        } else {
            self.page_tool_items(tab)
        };
        if !items.is_empty() && !more.is_empty() {
            items.push(json!({"sep": true}));
        }
        items.extend(more);
        if items.is_empty() {
            return;
        }
        let (ox, oy) = self.view_origin(tab);
        self.show_menu((ox + x, oy + y), json!(items));
    }

    /// BlackHole's own tools for the page in front: clip it to the Easel, capture a live area,
    /// watch an area. None for another pane, a Peek, an error page, or a black hole tab
    /// (nothing of it is kept).
    fn page_tool_items(&self, tab: TabId) -> Vec<serde_json::Value> {
        verbs::verbs(Target::Page, self.verb_context(tab))
            .into_iter()
            .filter_map(|verb| match verb {
                Verb::Keep => Some(json!({"icon": "easel", "label": t("menu.page_to_easel"), "run": {"cmd": "capture_to_easel"}})),
                Verb::LiveArea => Some(json!({"icon": "eye", "label": t("menu.live_capture"), "run": {"cmd": "live_pick"}})),
                Verb::Watch => Some(json!({"icon": "watch", "label": t("menu.watch_pick"), "run": {"cmd": "watch_pick"}})),
                _ => None,
            })
            .collect()
    }

    /// "Send this page to …": the tab goes to a paired device (`domain::verbs`, Send).
    fn page_send_items(&self, tab: TabId) -> Vec<serde_json::Value> {
        if !verbs::allows(Target::Page, self.verb_context(tab), Verb::Send) {
            return Vec::new();
        }
        self.send_items("menu.page_send_to", |d| json!({"cmd": "continue_on", "id": tab.0, "device": d}))
    }

    /// The entries of a right click on the page itself: history, the Easel, a live area, a
    /// watched area, the address, find, print, the inspector. Those that act on "the page in
    /// front" are only offered for it (not for the other pane of a split, nor for a Peek).
    fn page_menu_items(&self, tab: TabId) -> Vec<serde_json::Value> {
        let ws = self.svc.workspace();
        let front = ws.active_tab().is_some_and(|t| t.id == tab) && !self.home_shown();
        let address = self
            .view_of(tab)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok())
            .filter(|u| matches!(u.scheme(), "http" | "https"));
        let mut items = Vec::new();
        if front {
            let can = |f: fn(&WebView) -> wry::Result<bool>| {
                self.tabs.get(&tab).is_some_and(|v| f(v).unwrap_or(false))
            };
            if can(WebView::can_go_back) {
                items.push(json!({"icon": "back", "label": t("menu.page_back"), "run": {"cmd": "back"}}));
            }
            if can(WebView::can_go_forward) {
                items.push(json!({"icon": "forward", "label": t("menu.page_forward"), "run": {"cmd": "forward"}}));
            }
            items.push(json!({"icon": "reload", "label": t("menu.page_reload"), "run": {"cmd": "reload"}}));
            items.push(json!({"icon": "reload", "label": t("menu.page_hard_reload"), "run": {"cmd": "hard_reload"}}));
            let tools = self.page_tool_items(tab);
            if !tools.is_empty() {
                items.push(json!({"sep": true}));
                items.extend(tools);
            }
            items.push(json!({"sep": true}));
        }
        if let Some(url) = &address {
            let clean = self
                .svc
                .settings()
                .shields
                .clean_links
                .then(|| blackhole_domain::clean_link::clean(url))
                .flatten();
            items.push(json!({"icon": "link", "label": t("menu.copy_page_link"),
                "copy": clean.as_ref().map_or(url.as_str(), Url::as_str)}));
            items.extend(self.page_send_items(tab));
        }
        if front {
            items.extend([
                json!({"icon": "search", "label": t("menu.find_in_page"), "run": {"cmd": "shortcut", "key": "f"}}),
                json!({"icon": "print", "label": t("menu.print"), "run": {"cmd": "shortcut", "key": "p"}}),
                json!({"sep": true}),
                json!({"icon": "code", "label": t("menu.inspect"), "run": {"cmd": "shortcut", "key": "devtools"}}),
            ]);
        }
        while items.last().is_some_and(|i| i["sep"] == json!(true)) {
            items.pop();
        }
        items
    }
}

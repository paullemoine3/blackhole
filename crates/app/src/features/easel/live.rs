//! Live areas of an Easel: page views cropped to the area picked, playing in their card.

use blackhole_domain::TabId;
use blackhole_domain::verbs::Verb;
use serde_json::json;
use url::Url;

use crate::views;
use crate::i18n::t;
use crate::shell::notices;
use crate::state::Running;

impl Running {
    /// Drops the live views whose card is gone (removed, other easel, no longer live).
    pub(crate) fn sync_live(&mut self) {
        let current = self.easel_current.and_then(|id| self.easels.get(id));
        let keep: Vec<u64> = current
            .map(|e| {
                e.items
                    .iter()
                    .filter(|i| i.kind == blackhole_domain::EaselItemKind::Live)
                    .map(|i| i.id)
                    .collect()
            })
            .unwrap_or_default();
        self.live.retain(|id, _| keep.contains(id));
        self.live_slots.retain(|id, _| keep.contains(id));
    }

    /// The card's page, cropped to its region, in the card's slot of the easel window.
    pub(crate) fn live_play(&mut self, item: u64) {
        if self.live.contains_key(&item) {
            return;
        }
        let card = self
            .easel_current
            .and_then(|id| self.easels.get(id))
            .and_then(|e| e.items.iter().find(|i| i.id == item))
            .filter(|i| i.kind == blackhole_domain::EaselItemKind::Live)
            .and_then(|i| Some((Url::parse(i.url.as_deref()?).ok()?, i.selector.clone())));
        let host = if self.easel_tab || self.easel_side() {
            Some(&*self.window)
        } else {
            self.easel.as_ref().map(|w| &w.window)
        };
        let (Some((url, selector)), Some(host)) = (card, host) else {
            return;
        };
        match views::live_view(
            host,
            &mut self.web_context,
            &url,
            &selector,
            self.svc.policy(),
            &self.mailbox,
        ) {
            Ok(view) => {
                self.live_embedded = self.easel_tab || self.easel_side();
                self.live.insert(item, view);
                self.apply_slot(item);
            }
            Err(err) => {
                tracing::error!("cannot create live view: {err}");
                self.notify(crate::shell::notices::Notice::Problem, &t("toast.live_failed"), serde_json::Value::Null);
            }
        }
        self.ui_easel();
    }

    /// The Easel that held live views in the main window is no longer shown (another tab, the
    /// start page, the Library…): its live views go with it, whatever path left it.
    pub(crate) fn drop_stray_live(&mut self) {
        if self.live_embedded && !(self.easel_tab || self.easel_side()) {
            self.live.clear();
            self.live_slots.clear();
        }
        if self.live.is_empty() {
            self.live_embedded = false;
        }
    }

    fn apply_slot(&self, item: u64) {
        let (Some(view), Some(slot)) = (self.live.get(&item), self.live_slots.get(&item)) else {
            return;
        };
        // Slots are relative to the Easel page: the content area when it is a tab, the right
        // pane when it is beside a tab.
        let (ox, oy) = if self.easel_tab || self.easel_side() {
            self.easel_origin()
        } else {
            (0.0, 0.0)
        };
        let _ = view.set_bounds(views::rect(ox + slot.x, oy + slot.y, slot.w, slot.h));
        // The page is laid out at the width it had when the area was picked, whatever the
        // width of the card: the area keeps its shape and fills the slot.
        let viewport = self
            .easel_current
            .and_then(|id| self.easels.get(id))
            .and_then(|e| e.items.iter().find(|i| i.id == item))
            .map_or(0.0, |i| i.viewport);
        if viewport > 0.0 && slot.w >= 2.0 {
            let _ = view.zoom(blackhole_domain::easel::live_zoom(viewport, slot.w));
        }
        // The palette and the peek only cover the main window: an Easel window of its own keeps
        // its live views.
        let embedded = self.easel_tab || self.easel_side();
        let covered = embedded && (self.palette.open || self.peek.is_some());
        let _ = view.set_visible(slot.visible && !covered && slot.w >= 2.0 && slot.h >= 2.0);
    }

    /// Re-applies every live view's place and visibility (an overlay opened or closed).
    pub(crate) fn apply_slots(&self) {
        for item in self.live.keys() {
            self.apply_slot(*item);
        }
    }

    /// Arms the picker on the page in front.
    pub(crate) fn live_pick_start(&mut self) {
        let tab = self.svc.workspace().active_tab().map(|t| t.id);
        let (Some(tab), false) = (tab, self.home_shown()) else {
            self.pick_watches = false;
            self.toast(&t("toast.open_page_for_area"));
            return;
        };
        // A live area and a watched area keep a trace of the page: never for a black hole
        // tab (`domain::verbs`, the same rule as the menus).
        if !self.front_page_allows(Verb::LiveArea) {
            self.pick_watches = false;
            let key = if self.svc.workspace().is_burner(tab) { "toast.burner_keeps_nothing" } else { "toast.open_page_for_area" };
            self.toast(&t(key));
            return;
        }
        let Some(view) = self.tabs.get(&tab) else {
            return;
        };
        if view.evaluate_script(&views::live_picker_js()).is_ok() {
            self.live_pick = Some(tab);
        } else {
            self.pick_watches = false;
        }
    }

    /// An area was picked: ask which Easel of the space it goes to (or a new one), or whether
    /// to watch it for changes.
    pub(crate) fn live_picked(&mut self, tab: TabId, selector: String, size: (f64, f64, f64)) {
        // Picked from "Watch an area…": nothing more to ask.
        if std::mem::take(&mut self.pick_watches) {
            self.watch_area(tab, &selector);
            return;
        }
        let space = self.svc.workspace().active_space().id.0;
        let easels: Vec<(u64, String)> = self
            .easels
            .all()
            .iter()
            .filter(|e| self.easel_space(e) == space)
            .map(|e| (e.id, e.name.clone()))
            .collect();
        self.live_pending = Some((tab, selector, size));
        let current = self.easel_current;
        let mut items = vec![json!({"head": t("menu.live_add_to")})];
        items.extend(easels.iter().map(|(id, name)| {
            json!({
                "icon": if Some(*id) == current { "check" } else { "easel" },
                "label": name, "run": {"cmd": "live_capture_to", "easel": id},
            })
        }));
        items.push(json!({"sep": true}));
        items.push(json!({"icon": "plus", "label": t("menu.new_easel"), "run": {"cmd": "live_capture_to", "easel": null}}));
        items.push(json!({"sep": true}));
        items.push(json!({"icon": "watch", "label": t("menu.watch_area"), "run": {"cmd": "watch_area"}}));
        // In the middle of the page, where the area was just picked.
        let (x, y) = self.view_origin(tab);
        let size = self.window_size();
        let at = (x + (size.width - x) / 2.0 - 120.0, y + 120.0);
        self.show_menu(at, json!(items));
    }

    /// Adds a live card for `selector` of the page of `tab` to `easel` (`None`: a new Easel);
    /// the address comes from the engine. `size`: the area's width and height, then the width
    /// of the page it was measured in.
    pub(crate) fn capture_live(&mut self, tab: TabId, selector: &str, size: (f64, f64, f64), easel: Option<u64>) {
        let page = self.tabs.get(&tab).and_then(|v| v.url().ok());
        let title = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .find(|t| t.id == tab)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let easel = match easel.filter(|id| self.easels.get(*id).is_some()) {
            Some(id) => {
                self.easel_current = Some(id);
                Some(id)
            }
            None => {
                self.easel_current = self.create_easel("");
                self.tag_easel_space();
                self.easel_current
            }
        };
        let (Some(url), Some(easel)) = (page, easel) else {
            self.notify(crate::shell::notices::Notice::Problem, &t("toast.cannot_capture"), serde_json::Value::Null);
            return;
        };
        match self
            .easels
            .capture_live(easel, &url, &title, selector, (size.0, size.1))
        {
            Ok(item) => {
                self.easels.set_viewport(easel, item, size.2);
                self.easel_changed(true);
                self.notify(notices::Notice::Easel, &t("toast.live_added"), serde_json::Value::Null);
            }
            Err(why) => self.toast(&t(why)),
        }
    }
}

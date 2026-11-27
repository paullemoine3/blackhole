//! A tab dragged out of the sidebar stays in sight: above the page (where it would go: the
//! favorites of the start page, a half of the split view) and, outside the window, in a small
//! window of its own that follows the pointer (a window of the same kind takes it, the other
//! kind refuses it, nowhere makes it a new window). The sidebar keeps the gesture: neither view
//! ever takes the mouse.

use std::time::{Duration, Instant};

use blackhole_domain::TabId;
use blackhole_infrastructure::peers;
use serde_json::json;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};
use wry::WebView;

use crate::i18n::t;
use crate::shell::layout::{card_rect, BAR_HEIGHT};
use crate::shell::pages::DRAG_HTML;
use crate::shell::windows::{peers_dir, private_mode};
use crate::state::Running;
use crate::views;

/// The window that follows the pointer outside: the tab's card and what a drop would do.
const FLOAT_SIZE: (f64, f64) = (232.0, 70.0);
/// Where it sits from the pointer.
const FLOAT_OFFSET: (f64, f64) = (14.0, 8.0);
/// Out of sight while the pointer is back over the window (never hidden: on macOS showing a
/// window again makes it the key window, which would end the drag).
const AWAY: f64 = -20_000.0;
/// The other windows' places are read again this often during a drag.
const PEERS_AGAIN: Duration = Duration::from_millis(700);

#[derive(Default)]
pub(crate) struct TabDragging {
    now: Option<Moving>,
    /// Above the page; kept hidden between drags.
    overlay: Option<WebView>,
    overlay_ready: bool,
    float: Option<Float>,
    float_wanted: bool,
    peers: Vec<peers::Peer>,
    peers_at: Option<Instant>,
    /// The Favorites section of the start page, in the window (asked to home.html at each drag).
    favorites: Option<(f64, f64, f64, f64)>,
    /// Dropped: drag.html plays where the tab went, then says `tab_drag_gone`.
    landing: bool,
}

struct Moving {
    id: TabId,
    card: serde_json::Value,
    /// The pointer, in screen coordinates.
    at: (f64, f64),
}

/// Fields in this order: the page goes before its window.
struct Float {
    view: WebView,
    window: Window,
    ready: bool,
}

/// What a drop at the pointer would do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Zone {
    Nothing,
    Favorite(Fav),
    /// A half of the page; `pane` in the window's coordinates; `split` when two panes show.
    Split { left: bool, pane: (f64, f64, f64, f64), split: bool },
    Out(peers::Drop),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Fav {
    Add,
    Already,
    Burner,
}

impl Zone {
    /// Its name for drag.html, its words, and whether it is a refusal.
    fn said(&self) -> (&'static str, String, bool) {
        match self {
            Zone::Nothing => ("none", String::new(), false),
            Zone::Favorite(Fav::Add) => ("fav", t("sidebar.drag_fav"), false),
            Zone::Favorite(Fav::Already) => ("fav", t("sidebar.drag_fav_already"), true),
            Zone::Favorite(Fav::Burner) => ("fav", t("sidebar.drag_fav_burner"), true),
            Zone::Split { left, split, .. } => (
                if *left { "left" } else { "right" },
                t(if *split { "sidebar.drag_pane" } else if *left { "sidebar.drag_left" } else { "sidebar.drag_right" }),
                false,
            ),
            Zone::Out(peers::Drop::Into(_)) => ("out", t("sidebar.drag_into"), false),
            Zone::Out(peers::Drop::Refused) => (
                "out",
                t(if private_mode() { "sidebar.drag_refused_private" } else { "sidebar.drag_refused_normal" }),
                true,
            ),
            Zone::Out(peers::Drop::Nowhere) => {
                ("out", t(if private_mode() { "sidebar.drag_new_private" } else { "sidebar.drag_new" }), false)
            }
        }
    }
}

/// The half of the content card under `x`, and that half (full card height, below the bar).
pub(crate) fn half_at(x: f64, card: (f64, f64, f64, f64)) -> (bool, (f64, f64, f64, f64)) {
    let (cx, cy, w, h) = card;
    let half = (w / 2.0).max(1.0);
    let left = x < cx + half;
    (left, (if left { cx } else { cx + half }, cy, half, h))
}

#[cfg(feature = "devtools")]
impl TabDragging {
    /// The overlay above the page, and the page of the window outside (devtools `eval drag|dragfloat`).
    pub(crate) fn views(&self) -> (Option<&WebView>, Option<&WebView>) {
        (self.overlay.as_ref(), self.float.as_ref().map(|f| &f.view))
    }

    /// The pointer of the drag going on (screen points).
    pub(crate) fn at(&self) -> Option<(f64, f64)> {
        self.now.as_ref().map(|m| m.at)
    }

    /// The window outside: where it is (screen points), if there is one.
    pub(crate) fn float_at(&self) -> Option<(f64, f64)> {
        let f = self.float.as_ref()?;
        let p = f.window.outer_position().ok()?.to_logical::<f64>(f.window.scale_factor());
        Some((p.x, p.y))
    }
}

impl Running {
    /// The sidebar's tab is out of its view, at this point of the screen. `card` (title, icon,
    /// letter) comes with the first move.
    pub(crate) fn tab_drag(&mut self, id: TabId, x: f64, y: f64, card: Option<serde_json::Value>) {
        let fresh = self.tab_drag.now.as_ref().is_none_or(|m| m.id != id);
        if fresh {
            if self.tab_drag.landing {
                self.drag_clear();
            }
            let card = card.unwrap_or_else(|| json!({}));
            self.tab_drag.now = Some(Moving { id, card, at: (x, y) });
            self.tab_drag.peers_at = None;
            self.tab_drag.favorites = None;
            if self.start_page_front() {
                let _ = self.home.evaluate_script("window.bh&&bh.favoritesArea&&bh.favoritesArea()");
            }
            self.drag_overlay_up();
        }
        if let Some(m) = &mut self.tab_drag.now {
            m.at = (x, y);
        }
        self.drag_show();
    }

    /// The drag ended (dropped, cancelled, or back over the sidebar).
    pub(crate) fn tab_drag_end(&mut self) {
        if self.tab_drag.now.take().is_none() && !self.tab_drag.landing {
            return;
        }
        self.drag_clear();
    }

    /// Released at this point of the screen: drag.html shows where the tab goes (drawn into its
    /// pane, taken by the favorites, off to another window, refused) before it is hidden. Called
    /// before the drop changes anything.
    pub(crate) fn tab_drag_land(&mut self, id: TabId, x: f64, y: f64) {
        if self.tab_drag.now.as_ref().is_none_or(|m| m.id != id) {
            self.tab_drag_end();
            return;
        }
        let kind = match self.drag_zone(id, x, y) {
            Zone::Split { .. } => "pane",
            Zone::Favorite(Fav::Add) => "fav",
            Zone::Out(peers::Drop::Refused) => "refused",
            Zone::Out(_) => "away",
            _ => "",
        };
        let out = kind == "away" || kind == "refused";
        let shown = if out { self.tab_drag.float.as_ref().filter(|f| f.ready).map(|f| &f.view) } else { self.tab_drag.overlay.as_ref().filter(|_| self.tab_drag.overlay_ready) };
        let Some(view) = shown.filter(|_| !kind.is_empty() && self.svc.settings().animations) else {
            self.tab_drag_end();
            return;
        };
        let _ = view.evaluate_script(&format!("window.bhDrag&&bhDrag.drop('{kind}')"));
        self.tab_drag.now = None;
        self.tab_drag.landing = true;
        self.tab_drag.float_wanted = false;
        if out {
            if let Some(view) = &self.tab_drag.overlay {
                let _ = view.evaluate_script("window.bhDrag&&bhDrag.stop()");
                let _ = view.set_visible(false);
            }
        } else if let Some(Float { view, window, .. }) = self.tab_drag.float.take() {
            drop(view);
            self.retire_window(window);
        }
    }

    /// drag.html played the drop: nothing is left drawn.
    pub(crate) fn tab_drag_gone(&mut self) {
        if self.tab_drag.now.is_none() && self.tab_drag.landing {
            self.drag_clear();
        }
    }

    fn drag_clear(&mut self) {
        self.tab_drag.landing = false;
        if let Some(view) = &self.tab_drag.overlay {
            let _ = view.evaluate_script("window.bhDrag&&bhDrag.stop()");
            let _ = view.set_visible(false);
        }
        self.tab_drag.float_wanted = false;
        if let Some(Float { view, window, .. }) = self.tab_drag.float.take() {
            drop(view);
            self.retire_window(window);
        }
    }

    /// home.html says where its Favorites section is (its own pixels); none when it is hidden.
    pub(crate) fn tab_drag_area(&mut self, area: Option<(f64, f64, f64, f64)>) {
        // Where the start page sits, as laid out here (WKWebView's `bounds()` counts y from the bottom).
        let (cx, cy, _, _) = card_rect(&self.sidebar, &self.window_size());
        self.tab_drag.favorites = area.map(|(x, y, w, h)| (cx + x, cy + y, w, h));
        self.drag_show();
    }

    /// drag.html loaded (above the page or in the window outside): it gets the tab again.
    pub(crate) fn tab_drag_ready(&mut self) {
        self.tab_drag.overlay_ready = self.tab_drag.overlay.is_some();
        if let Some(f) = &mut self.tab_drag.float {
            f.ready = true;
        }
        self.drag_started();
        self.drag_show();
    }

    /// Called from the event loop, which alone creates windows.
    pub(crate) fn service_tab_drag(&mut self, event_loop: &ActiveEventLoop) {
        if !std::mem::take(&mut self.tab_drag.float_wanted) || self.tab_drag.float.is_some() {
            return;
        }
        let Some((x, y)) = self.tab_drag.now.as_ref().map(|m| m.at) else { return };
        let (w, h) = FLOAT_SIZE;
        // Transparent where the system draws it well; WebView2 gets a plain card instead.
        let clear = cfg!(target_os = "macos");
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title("BlackHole")
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(clear)
            .with_active(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(w, h))
            .with_position(LogicalPosition::new(x + FLOAT_OFFSET.0, y + FLOAT_OFFSET.1));
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_skip_taskbar(true).with_undecorated_shadow(false);
        }
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs.with_has_shadow(false);
        }
        let Ok(window) = event_loop.create_window(attrs) else { return };
        let _ = window.set_cursor_hittest(false);
        match views::ui_view(
            &window,
            &mut self.web_context,
            DRAG_HTML,
            views::rect(0.0, 0.0, w, h),
            true,
            clear,
            &self.mailbox,
        ) {
            Ok(view) => self.tab_drag.float = Some(Float { view, window, ready: false }),
            Err(error) => {
                tracing::error!("cannot create the dragged tab's window: {error}");
                self.retire_window(window);
            }
        }
    }

    /// The tab released over this window, outside the sidebar: the favorites or a pane.
    pub(crate) fn drop_tab_on_page(&mut self, id: TabId, x: f64, y: f64) {
        match self.drag_zone(id, x, y) {
            Zone::Favorite(Fav::Add) => {
                self.svc.toggle_pin(id);
                self.sync();
            }
            Zone::Split { left, .. } => {
                if self.svc.drop_into_split(id, left) {
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                    self.settings_shown = false;
                }
                self.sync();
            }
            _ => {}
        }
    }

    /// The point of the screen in this window's content, if it is inside.
    pub(crate) fn window_point(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let scale = self.window.scale_factor();
        let at = self.window.inner_position().ok()?.to_logical::<f64>(scale);
        let size = self.window_size();
        let (lx, ly) = (x - at.x, y - at.y);
        (lx >= 0.0 && ly >= 0.0 && lx < size.width && ly < size.height).then_some((lx, ly))
    }

    fn drag_zone(&mut self, id: TabId, x: f64, y: f64) -> Zone {
        let Some((lx, ly)) = self.window_point(x, y) else {
            if self.tab_drag.peers_at.is_none_or(|at| at.elapsed() >= PEERS_AGAIN) {
                self.tab_drag.peers = peers::peers(&peers_dir(), std::process::id());
                self.tab_drag.peers_at = Some(Instant::now());
            }
            return Zone::Out(peers::drop_target(&self.tab_drag.peers, x, y, private_mode()));
        };
        if lx < self.sidebar.width() {
            return Zone::Nothing;
        }
        let ws = self.svc.workspace();
        if self.start_page_front() {
            // Only over the Favorites section.
            let over = self.tab_drag.favorites.is_some_and(|(fx, fy, fw, fh)| lx >= fx && lx < fx + fw && ly >= fy && ly < fy + fh);
            if !over {
                return Zone::Nothing;
            }
            let tab = ws.active_space().tabs.iter().find(|t| t.id == id);
            return match tab {
                None => Zone::Nothing,
                Some(_) if ws.is_burner(id) => Zone::Favorite(Fav::Burner),
                Some(t) if t.pinned => Zone::Favorite(Fav::Already),
                Some(_) => Zone::Favorite(Fav::Add),
            };
        }
        // Library, Settings, an Easel: no pane to share.
        if self.home_shown() || self.easel_side() {
            return Zone::Nothing;
        }
        let (cx, _, w, h) = card_rect(&self.sidebar, &self.window_size());
        let (left, half) = half_at(lx, (cx, BAR_HEIGHT, w, h));
        if ws.split_drop(id, left).is_none() {
            return Zone::Nothing;
        }
        let split = self.split_tab().is_some();
        let pane = match self.content_rects() {
            (a, Some(b)) => {
                let r = if left { a } else { b };
                let (p, s) = (r.position.to_logical::<f64>(1.0), r.size.to_logical::<f64>(1.0));
                (p.x, p.y, s.width, s.height)
            }
            _ => half,
        };
        Zone::Split { left, pane, split }
    }

    /// Shows the overlay above everything, over the page beside the sidebar.
    fn drag_overlay_up(&mut self) {
        let bounds = self.drag_overlay_rect();
        if let Some(view) = &self.tab_drag.overlay {
            let _ = view.set_bounds(bounds);
            views::raise_views(&[view]);
            let _ = view.set_visible(true);
            self.drag_started();
            return;
        }
        match views::ui_view(&self.window, &mut self.web_context, DRAG_HTML, bounds, true, true, &self.mailbox) {
            Ok(view) => {
                self.tab_drag.overlay = Some(view);
                self.tab_drag.overlay_ready = false;
            }
            Err(error) => tracing::error!("cannot create the drag overlay: {error}"),
        }
    }

    fn drag_overlay_rect(&self) -> wry::Rect {
        let size = self.window_size();
        let x = self.sidebar.width();
        views::rect(x, 0.0, (size.width - x).max(1.0), size.height)
    }

    /// The tab's card, to the views that are ready.
    fn drag_started(&self) {
        let Some(m) = &self.tab_drag.now else { return };
        let still = !self.svc.settings().animations;
        if let Some(view) = self.tab_drag.overlay.as_ref().filter(|_| self.tab_drag.overlay_ready) {
            let mut card = m.card.clone();
            card["still"] = json!(still);
            let _ = view.evaluate_script(&format!("window.bhDrag&&bhDrag.start({card})"));
        }
        if let Some(f) = self.tab_drag.float.as_ref().filter(|f| f.ready) {
            let mut card = m.card.clone();
            card["still"] = json!(still);
            card["float"] = json!(true);
            card["solid"] = json!(!cfg!(target_os = "macos"));
            let _ = f.view.evaluate_script(&format!("window.bhDrag&&bhDrag.start({card})"));
        }
    }

    /// Where the pointer is: the overlay inside the window, the small window outside.
    fn drag_show(&mut self) {
        let Some((id, (x, y))) = self.tab_drag.now.as_ref().map(|m| (m.id, m.at)) else { return };
        let zone = self.drag_zone(id, x, y);
        let (name, label, no) = zone.said();
        let inside = self.window_point(x, y);
        if let (Some(view), true) = (&self.tab_drag.overlay, self.tab_drag.overlay_ready) {
            match inside {
                Some((lx, ly)) => {
                    let dx = self.sidebar.width();
                    let pane = match (&zone, self.tab_drag.favorites) {
                        (Zone::Split { pane: (px, py, pw, ph), .. }, _) => json!({"x": px - dx, "y": py, "w": pw, "h": ph}),
                        (Zone::Favorite(_), Some((px, py, pw, ph))) => json!({"x": px - dx, "y": py, "w": pw, "h": ph}),
                        _ => json!(null),
                    };
                    let state = json!({"x": lx - dx, "y": ly, "sx": x, "zone": name, "label": label, "no": no, "pane": pane});
                    let _ = view.evaluate_script(&format!("window.bhDrag&&bhDrag.at({state})"));
                }
                None => {
                    let _ = view.evaluate_script("window.bhDrag&&bhDrag.stop()");
                }
            }
        }
        match (&self.tab_drag.float, inside) {
            (None, None) => self.tab_drag.float_wanted = true,
            (Some(f), None) => {
                f.window.set_outer_position(LogicalPosition::new(x + FLOAT_OFFSET.0, y + FLOAT_OFFSET.1));
                if f.ready {
                    let state = json!({"x": 0, "y": 0, "sx": x, "zone": name, "label": label, "no": no});
                    let _ = f.view.evaluate_script(&format!("window.bhDrag&&bhDrag.at({state})"));
                }
            }
            (Some(f), Some(_)) => f.window.set_outer_position(LogicalPosition::new(AWAY, AWAY)),
            (None, Some(_)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_half_under_the_pointer_takes_the_tab() {
        let card = (200.0, 38.0, 800.0, 600.0);
        assert_eq!(half_at(250.0, card), (true, (200.0, 38.0, 400.0, 600.0)));
        assert!(half_at(599.9, card).0);
        assert_eq!(half_at(600.0, card), (false, (600.0, 38.0, 400.0, 600.0)));
    }

    #[test]
    fn a_refusal_is_said_as_one() {
        assert!(Zone::Out(peers::Drop::Refused).said().2);
        assert!(Zone::Favorite(Fav::Burner).said().2);
        assert!(!Zone::Out(peers::Drop::Nowhere).said().2);
        assert_eq!(Zone::Split { left: false, pane: (0.0, 0.0, 1.0, 1.0), split: false }.said().0, "right");
        assert_eq!(Zone::Nothing.said(), ("none", String::new(), false));
    }
}

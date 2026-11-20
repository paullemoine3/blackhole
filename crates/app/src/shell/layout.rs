//! Where each view sits in the window: sidebar, page card, split panes, and their motion.

use std::time::{Duration, Instant};

use blackhole_domain::TabId;

use crate::views;
use crate::i18n::t;
use crate::ipc::UiCommand;
use crate::shell::pane_motion;
use crate::shell::sidebar::Sidebar;
use crate::state::Running;

/// Gap between the two panes of a split view.
pub(crate) const SPLIT_GAP: f64 = 8.0;

/// Height of the strip above a split view that carries each pane's title and close button.
pub(crate) const SPLIT_HEADER: f64 = 30.0;

/// Logical height of the top bar that holds the window controls and the address field.
pub(crate) const BAR_HEIGHT: f64 = 38.0;

/// Margin between the page card and the window frame (the card floats above the tinted frame).
const CARD_GAP: f64 = 8.0;

/// The sidebar keeps its full width and slides out to the left as `width` shrinks to zero.
/// The view spans the rest of the window so its tint also paints the frame around the card.
pub(crate) fn chrome_rect(sidebar: &Sidebar, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    let x = sidebar.width() - sidebar.full();
    views::rect(
        x,
        BAR_HEIGHT,
        (window.width - x).max(1.0),
        (window.height - BAR_HEIGHT).max(1.0),
    )
}

/// Invisible view over the page while the sidebar is hidden. Page views swallow mouse events, so
/// it is the one that notices the cursor: a thin strip on the left edge reveals the sidebar, and
/// once the flyout is out it covers the rest of the page to hide it when the cursor leaves.
const HOTZONE_WIDTH: f64 = 12.0;

pub(crate) fn hotzone_rect(sidebar: &Sidebar, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    // Shown by the shell (a download): the strip stays thin, the page keeps its clicks.
    let (x, width) = if sidebar.flyout_shown() && !sidebar.held() {
        let x = sidebar.full();
        (x, (window.width - x).max(1.0))
    } else {
        (0.0, HOTZONE_WIDTH)
    };
    views::rect(x, BAR_HEIGHT, width, (window.height - BAR_HEIGHT).max(1.0))
}

/// The floating sidebar: full width, sliding in from the left edge below the top bar.
pub(crate) fn flyout_rect(sidebar: &Sidebar, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    views::rect(
        sidebar.flyout_width() - sidebar.full(),
        BAR_HEIGHT,
        sidebar.full(),
        (window.height - BAR_HEIGHT).max(1.0),
    )
}

/// Where the page lives: a card inset from the sidebar and the window edges.
/// The gap on its left narrows while a sidebar is beside it.
pub(crate) fn card_rect(sidebar: &Sidebar, window: &winit::dpi::LogicalSize<f64>) -> (f64, f64, f64, f64) {
    let open = (sidebar.width() / sidebar.full()).clamp(0.0, 1.0);
    let x = sidebar.width() + CARD_GAP * (1.0 - open / 2.0);
    (
        x,
        BAR_HEIGHT,
        (window.width - x - CARD_GAP).max(1.0),
        (window.height - BAR_HEIGHT - CARD_GAP).max(1.0),
    )
}

/// The page while the sidebar opens or hides: it keeps the width it has with the sidebar hidden
/// and only its left edge moves (the window clips the rest), so the site is not laid out again
/// on every frame (what made the motion stutter); its final size is set once, at the end.
fn sliding(main: wry::Rect, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    let scale = 1.0;
    let at = main.position.to_logical::<f64>(scale);
    let size = main.size.to_logical::<f64>(scale);
    views::rect(at.x, at.y, (window.width - 2.0 * CARD_GAP).max(1.0), size.height)
}

/// A pane of the split on its way into place: moved, never resized.
fn shifted(pane: wry::Rect, by: pane_motion::Shift) -> wry::Rect {
    if by == pane_motion::Shift::default() {
        return pane;
    }
    let at = pane.position.to_logical::<f64>(1.0);
    let size = pane.size.to_logical::<f64>(1.0);
    views::rect(at.x + by.x, at.y + by.y, size.width, size.height)
}

/// Where the command bar lives: the whole window while open, a parked 1x1 view otherwise. A hidden
/// full-window view still wins the drag destination lookup and would swallow in-page drags.
pub(crate) fn palette_rect(open: bool, window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    if open {
        views::rect(0.0, 0.0, window.width, window.height)
    } else {
        parked()
    }
}

/// Bounds of a hidden view that must not catch drags (see `palette_rect`).
fn parked() -> wry::Rect {
    views::rect(0.0, 0.0, 1.0, 1.0)
}

/// The Library covers the whole window under the command bar.
fn library_rect(window: &winit::dpi::LogicalSize<f64>) -> wry::Rect {
    views::rect(
        0.0,
        BAR_HEIGHT,
        window.width,
        (window.height - BAR_HEIGHT).max(1.0),
    )
}

/// One frame of the sidebar's motion (~120 per second): paced with a precise sleep, since the
/// system timer behind `WaitUntil` (15.6 ms on Windows) made the frames uneven.
pub(crate) const FRAME: Duration = Duration::from_millis(8);

impl Running {
    fn scale(&self) -> f64 {
        self.window.scale_factor()
    }

    pub(crate) fn window_size(&self) -> winit::dpi::LogicalSize<f64> {
        self.window.inner_size().to_logical::<f64>(self.scale())
    }

    /// The pane of the active tab, and the second pane when a split view is showing.
    pub(crate) fn content_rects(&self) -> (wry::Rect, Option<wry::Rect>) {
        let size = self.window_size();
        let (offset, _, width, height) = card_rect(&self.sidebar, &size);
        if self.split_tab().is_none() && !self.easel_side() {
            return (views::rect(offset, BAR_HEIGHT, width, height), None);
        }
        let half = ((width - SPLIT_GAP) / 2.0).max(1.0);
        let (top, height) = (BAR_HEIGHT + SPLIT_HEADER, (height - SPLIT_HEADER).max(1.0));
        (
            views::rect(offset, top, half, height),
            Some(views::rect(offset + half + SPLIT_GAP, top, half, height)),
        )
    }

    pub(crate) fn full_content_bounds(&self) -> wry::Rect {
        let (x, y, w, h) = card_rect(&self.sidebar, &self.window_size());
        views::rect(x, y, w, h)
    }

    pub(crate) fn peek_bounds(&self) -> wry::Rect {
        let size = self.window_size();
        views::rect(
            size.width * 0.1,
            size.height * 0.07,
            size.width * 0.8,
            size.height * 0.84,
        )
    }

    /// Positions the views; `all` also resizes background tabs (window resize, tab switch).
    pub(crate) fn layout(&self, all: bool) {
        self.sync_bar_notice();
        let size = self.window_size();
        let _ = self
            .bar
            .set_bounds(views::rect(0.0, 0.0, size.width, BAR_HEIGHT));
        let _ = self.chrome.set_bounds(chrome_rect(&self.sidebar, &size));
        if let Some(flyout) = &self.flyout {
            let _ = flyout.set_bounds(flyout_rect(&self.sidebar, &size));
            let _ = flyout.set_visible(self.sidebar.flyout_shown() && !self.library);
        }
        if let Some(hotzone) = &self.hotzone {
            let _ = hotzone.set_bounds(hotzone_rect(&self.sidebar, &size));
            let _ = hotzone.set_visible(!self.sidebar.is_open() && !self.library);
        }
        if let Some(palette) = &self.palette.view {
            let _ = palette.set_bounds(palette_rect(self.palette.open, &size));
        }
        if let Some(peek) = &self.peek {
            let _ = peek
                .scrim
                .set_bounds(views::rect(0.0, 0.0, size.width, size.height));
            let _ = peek.page.set_bounds(self.peek_bounds());
        }
        if all {
            let _ = self.home.set_bounds(self.full_content_bounds());
            for view in self.tabs.values() {
                let _ = view.set_bounds(self.full_content_bounds());
            }
        }
        let (cx, cy, cw, _) = card_rect(&self.sidebar, &size);
        if let Some(bar) = &self.splitbar.view {
            let _ = bar.set_bounds(views::rect(cx, cy, cw, SPLIT_HEADER));
        }
        let (main, side) = self.content_rects();
        let main = if self.sidebar.moving() && side.is_none() {
            sliding(main, &size)
        } else {
            main
        };
        // Only the two tabs of a split move (`pane_motion`): an Easel beside a tab stays put,
        // its live views are placed from its own layout.
        let split = self.split_tab();
        let main = if split.is_some() {
            shifted(main, self.panes.shift(pane_motion::Side::Left))
        } else {
            main
        };
        let _ = self.active_view().set_bounds(main);
        self.place_panels();
        self.apply_slots();
        if let (Some(id), Some(rect)) = (split, side) {
            if let Some(view) = self.tabs.get(&id) {
                let _ = view.set_bounds(shifted(rect, self.panes.shift(pane_motion::Side::Right)));
            }
        }
        self.round_cards(all);
        self.place_card_shadow();
    }

    /// Sizes the Library and Easel panels, or parks them while hidden: they stay alive a minute
    /// after closing, and a hidden full-size view would still swallow the drags of the page
    /// under it (e.g. moving a Jira ticket), like the command bar did.
    pub(crate) fn place_panels(&self) {
        if let Some(panel) = &self.easel_panel.view {
            let rect = match self.content_rects() {
                (main, _) if self.easel_tab => main,
                (_, Some(side)) if self.easel_side() => side,
                _ => parked(),
            };
            let _ = panel.set_bounds(rect);
        }
        if let Some(panel) = &self.library_panel.view {
            let rect = if self.library { library_rect(&self.window_size()) } else { parked() };
            let _ = panel.set_bounds(rect);
        }
        if let Some(panel) = &self.settings_panel.view {
            let rect = if self.settings_shown { self.content_rects().0 } else { parked() };
            let _ = panel.set_bounds(rect);
        }
    }

    /// Rounds the page views that fill the card (the others are rounded when they are shown).
    pub(crate) fn round_cards(&self, all: bool) {
        let scale = self.scale();
        let mut cards = vec![&self.home, self.active_view()];
        cards.extend(self.easel_panel.view.as_ref());
        cards.extend(self.settings_panel.view.as_ref());
        if let Some(side) = self.split_tab().and_then(|id| self.tabs.get(&id)) {
            cards.push(side);
        }
        if all {
            cards.extend(self.tabs.values());
        }
        for view in cards {
            views::round_corners(view, scale);
        }
    }

    /// Tells the sidebar page where the card starts so the shadow under it follows the card.
    fn place_card_shadow(&self) {
        let size = self.window_size();
        let card_x = card_rect(&self.sidebar, &size).0;
        let moved = self.bar_left.get().is_nan() || (self.bar_left.get() - card_x).abs() >= 0.5;
        if moved {
            self.bar_left.set(card_x);
            let _ = self.bar.evaluate_script(&format!(
                "document.documentElement.style.setProperty('--cl','{card_x}px')"
            ));
        }
        let left = card_x - (self.sidebar.width() - self.sidebar.full());
        if (self.card_left.get() - left).abs() < 0.5 {
            return;
        }
        self.card_left.set(left);
        let origin = self.sidebar.full() - self.sidebar.width();
        let _ = self.chrome.evaluate_script(&format!(
            "{{const s=document.documentElement.style;s.setProperty('--cl','{left}px');s.setProperty('--ox','{origin}px')}}"
        ));
    }

    pub(crate) fn set_sidebar_width(&mut self, width: u32) {
        self.svc.set_sidebar_width(width);
        self.sidebar
            .set_full(f64::from(self.svc.settings().sidebar_width));
        self.layout(true);
    }

    /// The sidebar is either hidden or fully open, like Arc's.
    pub(crate) fn toggle_sidebar(&mut self) {
        self.sidebar.toggle();
        self.svc.set_sidebar_open(self.sidebar.is_open());
    }

    /// A hidden sidebar slides out by itself for a moment: what just happened shows in it.
    pub(crate) fn reveal_sidebar(&mut self, time: Duration) {
        if !self.library && self.sidebar.reveal(Instant::now(), time) {
            self.layout(false);
        }
    }

    /// One frame of what moves in the window: the sidebar, the panes of a split.
    pub(crate) fn tick_motion(&mut self) {
        let sidebar = self.sidebar.moving();
        if sidebar {
            self.sidebar.tick();
        }
        let panes = self.panes.moving();
        if panes {
            self.panes.tick();
        }
        if sidebar || panes {
            self.layout(false);
        }
    }

    /// A split that shows up (a tab split just now, or a tab shown again with its other pane)
    /// slides its right pane in; swapping the focus between its two panes moves nothing.
    pub(crate) fn split_shown(&mut self, active: Option<TabId>, split: Option<TabId>) {
        let pair = active.zip(split).map(|(a, b)| if a.0 <= b.0 { (a, b) } else { (b, a) });
        if pair == self.split_seen {
            return;
        }
        self.split_seen = pair;
        self.panes.stop();
        if pair.is_none() || !self.svc.settings().animations {
            return;
        }
        if let (_, Some(side)) = self.content_rects() {
            self.panes.open(side.size.to_logical::<f64>(1.0).width + SPLIT_GAP);
            self.split_opened = self.split_opened.wrapping_add(1);
        }
    }

    // ---- state → UI -------------------------------------------------------------------

    /// Top-left corner of a page view in the window (logical pixels).
    pub(crate) fn view_origin(&self, tab: TabId) -> (f64, f64) {
        let scale = self.scale();
        self.view_of(tab)
            .and_then(|v| v.bounds().ok())
            .map(|b| {
                let p = b.position.to_logical::<f64>(scale);
                (p.x, p.y)
            })
            .unwrap_or((card_rect(&self.sidebar, &self.window_size()).0, BAR_HEIGHT))
    }

    /// This window's rectangle on the screen, in logical pixels.
    pub(crate) fn screen_rect(&self) -> Option<(f64, f64, f64, f64)> {
        let scale = self.scale();
        let pos = self.window.outer_position().ok()?.to_logical::<f64>(scale);
        let size = self.window.outer_size().to_logical::<f64>(scale);
        Some((pos.x, pos.y, size.width, size.height))
    }

    /// Split view with the first other tab of the space, or closes the current split.
    pub(crate) fn toggle_split(&mut self) {
        if self.home_shown() {
            return;
        }
        if self.split_tab().is_some() {
            self.handle_ui(UiCommand::CloseSplit);
            return;
        }
        let ws = self.svc.workspace();
        let active = ws.active_tab().map(|t| t.id);
        let other = ws
            .active_space()
            .tabs
            .iter()
            .find(|t| Some(t.id) != active && !t.pinned)
            .map(|t| t.id);
        match other {
            Some(id) => self.handle_ui(UiCommand::Split { id: id.0 }),
            None => self.toast(&t("toast.split_needs_tab")),
        }
    }
}

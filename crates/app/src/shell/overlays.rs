//! The overlays (hotzone, flyout, command bar) kept above the page views.

use std::time::{Duration, Instant};

use crate::views;
use crate::shell::layout::{flyout_rect, hotzone_rect, palette_rect};
use crate::shell::pages::{CHROME_HTML, HOTZONE_HTML, PALETTE_HTML};
use crate::state::Running;

/// Quiet time after a tab creation before the overlays (palette, flyout) are rebuilt on top.
pub(crate) const RAISE_DELAY: Duration = Duration::from_millis(400);

impl Running {
    /// Native views stack in creation order, so the overlays are recreated above new tabs. Burst
    /// of tab creations (session restore) share a single recreation.
    pub(crate) fn raise_palette(&mut self) {
        self.raise_due = Some(Instant::now() + RAISE_DELAY);
    }

    /// Recreates the overlays now if a tab was created since; true while still waiting.
    pub(crate) fn flush_raise(&mut self, force: bool) -> bool {
        let Some(due) = self.raise_due else {
            return false;
        };
        if self.palette.open || (!force && Instant::now() < due) {
            return !self.palette.open;
        }
        self.raise_due = None;
        self.raise_overlays();
        false
    }

    fn raise_overlays(&mut self) {
        // Restacking the existing overlays: rebuilding them made the sidebar flash.
        if let (Some(hotzone), Some(flyout), Some(palette)) =
            (&self.hotzone, &self.flyout, &self.palette.view)
        {
            if views::raise_views(&[hotzone, flyout, palette]) {
                self.apply_focus();
                return;
            }
        }
        let size = self.window_size();
        match views::ui_view(
            &self.window,
            &mut self.web_context,
            HOTZONE_HTML,
            hotzone_rect(&self.sidebar, &size),
            !self.sidebar.is_open(),
            true,
            &self.mailbox,
        ) {
            Ok(view) => self.hotzone = Some(view),
            Err(error) => tracing::error!("cannot create sidebar hotzone: {error}"),
        }
        match views::ui_view(
            &self.window,
            &mut self.web_context,
            CHROME_HTML,
            flyout_rect(&self.sidebar, &size),
            self.sidebar.flyout_shown(),
            false,
            &self.mailbox,
        ) {
            Ok(view) => self.flyout = Some(view),
            Err(error) => tracing::error!("cannot create sidebar flyout: {error}"),
        }
        let bounds = palette_rect(false, &size);
        match views::ui_view(
            &self.window,
            &mut self.web_context,
            PALETTE_HTML,
            bounds,
            false,
            true,
            &self.mailbox,
        ) {
            Ok(view) => {
                self.palette.view = Some(view);
                self.palette.ready = false;
                self.palette.open = false;
            }
            Err(error) => tracing::error!("cannot create command bar: {error}"),
        }
    }
}

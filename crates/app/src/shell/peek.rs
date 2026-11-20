//! Peek: a link previewed in a floating card over the page.

use blackhole_application::NavigationOutcome;
use blackhole_domain::TabId;
use url::Url;
use wry::WebView;

use crate::views;
use crate::shell::pages::PEEK_HTML;
use crate::state::Running;

/// Reserved id of the peek page; real tabs start at 1.
pub(crate) const PEEK_TAB: TabId = TabId(0);

/// Floating preview of a link (Arc's "Peek"): a dimming scrim plus the page itself.
pub(crate) struct Peek {
    pub(crate) scrim: WebView,
    pub(crate) page: WebView,
}

impl Running {
    pub(crate) fn open_peek(&mut self, url: &Url) {
        let NavigationOutcome::Go(url) = self.svc.evaluate(url) else {
            return;
        };
        self.close_palette();
        self.close_peek();
        let size = self.window_size();
        let scrim_bounds = views::rect(0.0, 0.0, size.width, size.height);
        let scrim = views::ui_view(
            &self.window,
            &mut self.web_context,
            PEEK_HTML,
            scrim_bounds,
            true,
            true,
            &self.mailbox,
        );
        let peek_bounds = self.peek_bounds();
        let peek_store = self.store_of_space(self.svc.workspace().active_space().id);
        let page = views::tab_view(
            &self.window,
            &mut self.web_context,
            PEEK_TAB,
            &url,
            peek_bounds,
            self.svc.policy(),
            &self.mailbox,
            self.webext.as_ref(),
            peek_store,
        );
        match (scrim, page) {
            (Ok(scrim), Ok(page)) => {
                let _ = page.set_visible(true);
                let _ = page.focus();
                self.peek = Some(Peek { scrim, page });
                self.apply_slots();
            }
            (scrim, page) => {
                tracing::error!("cannot create peek: {:?} {:?}", scrim.err(), page.err());
            }
        }
    }

    pub(crate) fn close_peek(&mut self) {
        if self.peek.take().is_some() {
            self.apply_slots();
            self.apply_focus();
        }
    }

    pub(crate) fn peek_to_tab(&mut self) {
        let url = self.peek.as_ref().and_then(|p| p.page.url().ok());
        self.close_peek();
        if let Some(url) = url {
            self.open(&url);
        }
    }
}

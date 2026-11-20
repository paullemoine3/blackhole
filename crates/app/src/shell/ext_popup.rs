//! Extension toolbar popups: native popover on macOS, a sized view on Windows.

use crate::state::Running;

#[cfg(windows)]
use crate::views;
#[cfg(windows)]
use crate::shell::layout::BAR_HEIGHT;
#[cfg(windows)]
use crate::shell::pages::EXT_POPUP_HTML;
#[cfg(windows)]
use std::time::{Duration, Instant};
#[cfg(windows)]
use url::Url;
#[cfg(windows)]
use wry::WebView;

/// An extension's toolbar popup, shown under its button; a click anywhere else closes it.
#[cfg(windows)]
#[allow(dead_code)] // held only to keep the views alive
pub(crate) struct ExtPopup {
    pub(crate) scrim: WebView,
    pub(crate) page: WebView,
    /// Right edge of the popup: it grows leftwards from its toolbar button.
    pub(crate) right: f64,
    pub(crate) size: (f64, f64),
    pub(crate) probes_left: u8,
    /// After the first fit the popup only grows: shrinking toggles scrollbars and makes it flicker.
    pub(crate) fitted: bool,
    pub(crate) next_probe: Instant,
    pub(crate) measured: std::sync::mpsc::Receiver<String>,
    pub(crate) measure: std::sync::mpsc::Sender<String>,
}

/// Popup limits: a narrow popup keeps its own width (uBlock Origin Lite's is 273 px wide: a wider
/// view showed an empty strip beside it), and none gets larger than Chromium's maximum.
#[cfg(windows)]
const POPUP_MIN: (f64, f64) = (200.0, 80.0);

/// Corner radius of an extension popup, in logical pixels.
#[cfg(windows)]
const POPUP_RADIUS: f64 = 10.0;

#[cfg(windows)]
const POPUP_MAX: (f64, f64) = (800.0, 600.0);

impl Running {
    /// Shows an extension's native popover under `rect`; false when it has none to show.
    #[cfg(target_os = "macos")]
    pub(crate) fn show_native_popup(&self, id: &str, rect: (f64, f64, f64, f64)) -> bool {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let (Some(host), Ok(handle)) = (&self.webext, self.window.window_handle()) else {
            return false;
        };
        match handle.as_raw() {
            RawWindowHandle::AppKit(view) => host.show_popup(id, view.ns_view, rect),
            _ => false,
        }
    }

    /// Opens the popup of an extension under its toolbar button (`rect` is the button).
    #[cfg(windows)]
    pub(crate) fn open_ext_popup(&mut self, url: &str, rect: (f64, f64, f64, f64)) {
        let was_open = self.ext_popup.is_some();
        self.close_ext_popup();
        if !was_open {
            self.show_ext_popup(url, rect);
        }
    }

    #[cfg(windows)]
    pub(crate) fn show_ext_popup(&mut self, url: &str, rect: (f64, f64, f64, f64)) {
        let size = self.window_size();
        let (width, height) = (
            380.0_f64.min(size.width - 16.0),
            600.0_f64.min(size.height - BAR_HEIGHT - 16.0),
        );
        let x = (rect.0 + rect.2 - width).clamp(8.0, (size.width - width - 8.0).max(8.0));
        let scrim = views::ui_view(
            &self.window,
            &mut self.web_context,
            EXT_POPUP_HTML,
            views::rect(0.0, 0.0, size.width, size.height),
            true,
            true,
            &self.mailbox,
        );
        // The page in front, which the popup's "active tab" stands for (`ui/ext_tabs.js`).
        let front = (!self.home_shown())
            .then(|| self.svc.workspace().active_tab().map(|t| t.url.clone()))
            .flatten();
        let page = views::extension_popup(
            &self.window,
            &mut self.web_context,
            url,
            front.as_ref(),
            views::rect(x, BAR_HEIGHT - 4.0, width, height),
            &self.mailbox,
        );
        let (measure, measured) = std::sync::mpsc::channel();
        match (scrim, page) {
            (Ok(scrim), Ok(page)) => {
                let _ = page.focus();
                self.round_ext_popup(&page);
                self.ext_popup = Some(ExtPopup {
                    scrim,
                    page,
                    right: x + width,
                    size: (width, height),
                    probes_left: 8,
                    fitted: false,
                    next_probe: Instant::now() + Duration::from_millis(300),
                    measured,
                    measure,
                });
            }
            (scrim, page) => {
                tracing::error!(
                    "cannot open extension popup: {:?} {:?}",
                    scrim.err(),
                    page.err()
                );
            }
        }
    }

    /// Where the window an extension asked for by itself goes: under that extension's button
    /// in the command bar, or at the right edge of the window when the bar has not drawn it.
    #[cfg(windows)]
    pub(crate) fn ext_anchor(&self, page: &Url) -> (f64, f64, f64, f64) {
        let button = self.webext.as_ref().and_then(|host| {
            host.actions().into_iter().find_map(|a| {
                let popup = Url::parse(&host.popup_url(&a.id)?).ok()?;
                (popup.host_str() == page.host_str())
                    .then(|| self.ext_anchors.get(&a.id).copied())
                    .flatten()
            })
        });
        match button {
            Some((x, w)) => (x, 0.0, w, BAR_HEIGHT),
            None => (self.window_size().width, BAR_HEIGHT, 0.0, 0.0),
        }
    }

    /// Asks the popup how big its content is and applies the answers that came back.
    #[cfg(windows)]
    pub(crate) fn probe_ext_popup(&mut self) {
        let Some(popup) = &mut self.ext_popup else {
            return;
        };
        let answers: Vec<String> = popup.measured.try_iter().collect();
        for raw in answers {
            self.fit_ext_popup(&raw);
        }
        let Some(popup) = &mut self.ext_popup else {
            return;
        };
        if popup.probes_left == 0 || Instant::now() < popup.next_probe {
            return;
        }
        popup.probes_left -= 1;
        popup.next_probe = Instant::now() + Duration::from_millis(250);
        let measure = popup.measure.clone();
        let _ = popup.page.evaluate_script_with_callback(
            "(()=>{const b=document.body,d=document.documentElement;if(!b)return '';const w=Math.max(b.scrollWidth,d.scrollWidth>innerWidth?d.scrollWidth:0),h=Math.max(b.scrollHeight,d.scrollHeight>innerHeight?d.scrollHeight:0);return Math.ceil(w+innerWidth-d.clientWidth)+'x'+Math.ceil(h+innerHeight-d.clientHeight)})()",
            move |raw| {
                let _ = measure.send(raw);
            },
        );
    }

    /// Resizes the popup to its content (within the limits), keeping its right edge.
    #[cfg(windows)]
    fn fit_ext_popup(&mut self, raw: &str) {
        let max_h = (self.window_size().height - BAR_HEIGHT - 16.0).min(POPUP_MAX.1);
        let Some(popup) = &mut self.ext_popup else {
            return;
        };
        let Some((w, h)) = raw.trim_matches('"').split_once('x') else {
            return;
        };
        let (Ok(w), Ok(h)) = (w.parse::<f64>(), h.parse::<f64>()) else {
            return;
        };
        let w = w.clamp(POPUP_MIN.0, POPUP_MAX.0);
        let h = h.clamp(POPUP_MIN.1, max_h.max(POPUP_MIN.1));
        let (w, h) = if popup.fitted {
            (w.max(popup.size.0), h.max(popup.size.1))
        } else {
            (w, h)
        };
        if (w - popup.size.0).abs() < 4.0 && (h - popup.size.1).abs() < 4.0 {
            return;
        }
        popup.fitted = true;
        popup.size = (w, h);
        let x = (popup.right - w).max(8.0);
        let _ = popup
            .page
            .set_bounds(views::rect(x, BAR_HEIGHT - 4.0, w, h));
        if let Some(popup) = &self.ext_popup {
            self.round_ext_popup(&popup.page);
        }
    }

    /// Rounded corners for the popup, like the menus; the region has the size of the view, so
    /// this follows every resize.
    #[cfg(windows)]
    fn round_ext_popup(&self, page: &WebView) {
        let radius = (POPUP_RADIUS * self.window.scale_factor()).round() as i32;
        blackhole_macos_webext::round_view(page, radius);
    }

    #[cfg(windows)]
    pub(crate) fn close_ext_popup(&mut self) {
        if self.ext_popup.take().is_some() {
            self.apply_focus();
        }
    }
}

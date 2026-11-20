//! Rarely used shell pages created on demand, and the secondary windows that host some.

use std::time::{Duration, Instant};

use winit::dpi::LogicalSize;
use winit::window::Window;
use wry::WebView;

use crate::views;
use crate::platform::drag_window;
use crate::shell::layout::{BAR_HEIGHT, card_rect, SPLIT_HEADER};
use crate::shell::pages::{EASEL_HTML, LIBRARY_HTML, SETTINGS_HTML, SPLITBAR_HTML};
use crate::state::Running;

/// How long a rarely used page of the shell (library, easel tab, split bar) stays loaded once hidden.
const PANEL_KEEP: Duration = Duration::from_secs(60);

/// A page of the shell created on first use and released after `PANEL_KEEP` hidden: each one
/// is a renderer process, and most sessions never open them.
pub(crate) struct LazyView {
    pub(crate) view: Option<WebView>,
    pub(crate) shown: Instant,
}

impl LazyView {
    pub(crate) fn new() -> Self {
        Self {
            view: None,
            shown: Instant::now(),
        }
    }
}

impl Running {
    /// Hides a secondary window now (AppKit hands the key window over while it is still alive)
    /// and drops it on the next pass of the event loop.
    pub(crate) fn retire_window(&mut self, window: Window) {
        window.set_visible(false);
        self.closed_windows.push(window);
    }

    /// A secondary window (Settings, Easel) dressed like the main one: its page draws the top
    /// bar (title, and on Windows the minimize / maximize / close buttons). Owned by the main
    /// window on Windows so it always stays above it.
    pub(crate) fn panel_window_attributes(
        &self,
        title: &str,
        (width, height): (f64, f64),
    ) -> winit::window::WindowAttributes {
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(title)
            .with_theme(Some(winit::window::Theme::Dark))
            .with_inner_size(LogicalSize::new(width, height))
            .with_min_inner_size(LogicalSize::new(640.0, 440.0));
        #[cfg(windows)]
        {
            use winit::{
                platform::windows::{IconExtWindows, WindowAttributesExtWindows},
                raw_window_handle::{HasWindowHandle, RawWindowHandle},
            };
            attrs = attrs.with_decorations(false).with_undecorated_shadow(true);
            if let Ok(icon) = winit::window::Icon::from_resource(1, None) {
                attrs = attrs.with_window_icon(Some(icon));
            }
            if let Ok(handle) = self.window.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    attrs = attrs.with_owner_window(h.hwnd.get());
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true);
        }
        attrs
    }

    /// The buttons and drag area of a secondary window's top bar. The Settings have no window
    /// any more: only their close (Escape) is left.
    pub(crate) fn panel_window_action(&mut self, which: &str, action: &str) {
        if which == "settings" {
            if action == "close" {
                self.close_settings();
            }
            return;
        }
        let window = match which {
            "easel" => self.easel.as_ref().map(|w| &w.window),
            _ => None,
        };
        let Some(window) = window else { return };
        match action {
            "drag" => drag_window(window),
            "minimize" => window.set_minimized(true),
            "maximize" => {
                window.set_maximized(!window.is_maximized());
            }
            "close" => self.close_easel(),
            _ => {}
        }
    }

    // ---- easels ---------------------------------------------------------------------------

    /// Creates the shell pages that are about to be needed and releases the ones left unused.
    pub(crate) fn ensure_panels(&mut self) {
        let now = Instant::now();
        let side = self.easel_side();
        let split = self.split_tab().is_some() || side;
        let size = self.window_size();
        let (cx, cy, cw, ch) = card_rect(&self.sidebar, &size);
        let mut created = false;
        for (needed, which) in [(self.library, 0), (self.easel_tab || side, 1), (split, 2), (self.settings_shown, 3)] {
            let (panel, html, bounds) = match which {
                0 => (
                    &mut self.library_panel,
                    LIBRARY_HTML,
                    views::rect(
                        0.0,
                        BAR_HEIGHT,
                        size.width,
                        (size.height - BAR_HEIGHT).max(1.0),
                    ),
                ),
                1 => (
                    &mut self.easel_panel,
                    EASEL_HTML,
                    views::rect(cx, cy, cw, ch),
                ),
                3 => (
                    &mut self.settings_panel,
                    SETTINGS_HTML,
                    views::rect(cx, cy, cw, ch),
                ),
                _ => (
                    &mut self.splitbar,
                    SPLITBAR_HTML,
                    views::rect(cx, cy, cw, SPLIT_HEADER),
                ),
            };
            if needed {
                panel.shown = now;
                if panel.view.is_none() {
                    match views::ui_view(
                        &self.window,
                        &mut self.web_context,
                        html,
                        bounds,
                        false,
                        false,
                        &self.mailbox,
                    ) {
                        Ok(view) => {
                            panel.view = Some(view);
                            created = true;
                        }
                        Err(error) => tracing::error!("cannot create panel: {error}"),
                    }
                }
            } else if panel.view.is_some() && now.duration_since(panel.shown) >= PANEL_KEEP {
                panel.view = None;
            }
        }
        if created {
            self.raise_palette();
        }
    }
}

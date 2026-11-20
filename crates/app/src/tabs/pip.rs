//! Automatic Picture in Picture: a video floats when its tab or the window is left.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use blackhole_domain::TabId;

use crate::views;
use crate::ipc::UiCommand;
use crate::state::Running;

pub(crate) const BLUR_DELAY: Duration = Duration::from_millis(350);

/// How often an unfocused but still visible window is checked for being covered.
const PIP_RECHECK: Duration = Duration::from_millis(1000);

/// A closed media card stays closed this long whatever the probes say (one may have left before
/// the pause): longer than the probe interval.
pub(crate) const DISMISS_GRACE: Duration = Duration::from_millis(2000);

/// How long after the shell brings a floating video back its "left PiP" is taken as an echo.
const PIP_SHELL_EXIT: Duration = Duration::from_millis(1500);

/// Videos the shell floated in Picture in Picture, and why.
#[derive(Default)]
pub(crate) struct Pip {
    /// Tab floated because the user left it for another tab (its "back to tab" button returns).
    pub(crate) tab: Option<TabId>,
    /// The probe has seen the floating video of `tab`: its disappearance means it came back.
    pub(crate) seen: bool,
    /// `tab` when the probe stopped seeing its floating video, and when. WebKit changes
    /// the video's mode as soon as the window's "back to tab" button is pressed but only tells
    /// the page once the window has shrunk back: the page's `pip_left` arrives after the probe
    /// and must still count as a video the shell floated (`pip::LEFT_EVENT_GRACE`).
    pub(crate) faded: Option<(TabId, Instant)>,
    /// Tab floated because the window lost focus; it returns when the window gets focus back.
    pub(crate) on_blur: Option<TabId>,
    /// When a focus loss of the window becomes a real blur, unless focus came back before.
    pub(crate) blur_due: Option<Instant>,
    /// Tabs whose floating video the shell itself just brought back (their "left PiP" is no
    /// request from the user).
    pub(crate) shell_exits: HashMap<u64, Instant>,
}

impl Running {
    /// A video left its floating window. The window's close button pauses it: nothing changes;
    /// its "back to tab" button leaves it playing: its tab is shown, as in Safari. WebView2
    /// cannot pause a live stream on close (`stream`): for those nothing is switched on
    /// Windows. Only a video the shell floated counts, and not when the shell itself brought
    /// it back.
    pub(crate) fn pip_left(&mut self, tab: TabId, paused: bool, stream: bool) {
        let now = Instant::now();
        self.pip.shell_exits.retain(|_, at| now.duration_since(*at) < PIP_SHELL_EXIT);
        // The probe may have seen the video leave before the page said so (WebKit tells the
        // page only once its window has shrunk back): it was still floated by the shell.
        let faded = self.pip.faded.take().is_some_and(|(id, at)| {
            id == tab && now.duration_since(at) < blackhole_domain::pip::LEFT_EVENT_GRACE
        });
        let left = blackhole_domain::pip::PipLeft {
            floated_by_shell: self.pip.tab == Some(tab) || self.pip.on_blur == Some(tab) || faded,
            left_by_shell: self.pip.shell_exits.contains_key(&tab.0),
            paused,
            tab_shown: self.shown == Some(tab) && !self.home_shown(),
            close_pauses: cfg!(target_os = "macos") || !stream,
        };
        self.pip_returned(tab);
        if self.pip.on_blur == Some(tab) {
            self.pip.on_blur = None;
        }
        if blackhole_domain::pip::back_to_tab(left) && self.tabs.contains_key(&tab) {
            self.window.set_minimized(false);
            self.window.focus_window();
            self.handle_ui(UiCommand::Activate { id: tab.0 });
        }
    }

    /// The shell's PiP button (card, command bar) toggled the video of `tab`: floating, it comes
    /// back by the shell's hand (no request to show the tab); in its page, the shell floats it.
    pub(crate) fn pip_toggled(&mut self, tab: TabId) {
        let floating = self
            .media.probes
            .lock()
            .is_ok_and(|probes| probes.get(&tab.0).is_some_and(|p| p.pip));
        if floating {
            self.pip.shell_exits.insert(tab.0, Instant::now());
        } else {
            self.pip.tab = Some(tab);
            self.pip.seen = false;
        }
    }

    /// The floating video of `tab` is gone (left, or no longer seen by the probe): the shell
    /// stops tracking it.
    pub(crate) fn pip_returned(&mut self, tab: TabId) {
        if self.pip.tab == Some(tab) {
            self.pip.tab = None;
            self.pip.seen = false;
        }
    }

    /// The window lost focus (another app, a dialog): a playing video floats so it stays visible.
    /// Moving the window or clicking into a page view also drops the window's own focus, so the
    /// loss is only acted on when it lasts and the whole application is no longer frontmost.
    pub(crate) fn pip_on_blur(&mut self) {
        let Some(due) = self.pip.blur_due else { return };
        if Instant::now() < due {
            return;
        }
        // Still the frontmost app: focus only moved inside it. Keep watching, since switching
        // to another app later raises no further event.
        #[cfg(any(target_os = "macos", windows))]
        if blackhole_macos_webext::app_is_active() {
            self.pip.blur_due = Some(Instant::now() + BLUR_DELAY);
            return;
        }
        // The browser is still on screen (e.g. the user works in another window on a second
        // monitor): nothing to rescue. Look again later in case that window is moved over it.
        if !self.window_is_covered() {
            self.pip.blur_due = Some(Instant::now() + PIP_RECHECK);
            return;
        }
        self.pip.blur_due = None;
        let Some(id) = self.shown else { return };
        // A call is left alone: sharing a screen is precisely working in another window.
        if self.in_call(id) {
            return;
        }
        if let Some(view) = self.tabs.get(&id) {
            let _ = view.evaluate_script(views::PIP_ENTER_JS);
            self.pip.on_blur = Some(id);
        }
    }

    /// Whether most of the browser window is hidden: minimised, or under the window of another
    /// application that now has focus.
    fn window_is_covered(&self) -> bool {
        if self.window.is_minimized().unwrap_or(false) || !self.window.is_visible().unwrap_or(true)
        {
            return true;
        }
        #[cfg(any(target_os = "macos", windows))]
        {
            let (Ok(pos), size) = (self.window.outer_position(), self.window.outer_size()) else {
                return true;
            };
            let Some(front) = blackhole_macos_webext::frontmost_foreign_window() else {
                return false;
            };
            // winit reports pixels; macOS window lists use points.
            let scale = if cfg!(target_os = "macos") { self.window.scale_factor() } else { 1.0 };
            let own = (
                f64::from(pos.x) / scale,
                f64::from(pos.y) / scale,
                f64::from(size.width) / scale,
                f64::from(size.height) / scale,
            );
            return blackhole_domain::pip::window_covered(own, front);
        }
        #[allow(unreachable_code)]
        true
    }

    /// Focus is back: the video floated by `pip_on_blur` returns into its page.
    pub(crate) fn pip_back_on_focus(&mut self) {
        self.pip.blur_due = None;
        if let Some(id) = self.pip.on_blur.take() {
            if let Some(view) = self.tabs.get(&id) {
                let _ = view.evaluate_script(views::PIP_EXIT_JS);
                self.pip.shell_exits.insert(id.0, Instant::now());
            }
        }
    }
}

//! The event loop: what the window system asks of the app, and its pacing.

use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::ipc::{Mailbox, Wake};
use crate::platform::launch_url;
use crate::shell::layout::FRAME;
use crate::shell::overlays::RAISE_DELAY;
use crate::shell::windows::PEERS_EVERY;
use crate::state::Running;
use crate::tabs::downloads::DOWNLOAD_POLL;
use crate::tabs::pip::BLUR_DELAY;

pub(crate) enum App {
    Initial(Mailbox),
    Running(Box<Running>),
}

impl ApplicationHandler<Wake> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Self::Initial(mailbox) = self {
            let mut running = Running::new(event_loop, mailbox.clone());
            running.pump();
            if let Some(url) = launch_url() {
                running.open(&url);
            }
            *self = Self::Running(Box::new(running));
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[allow(unused_mut)]
        let mut wake = PEERS_EVERY;
        // WebKitGTK only paints, loads and answers IPC while GTK's main context runs: give it
        // its events every turn, and come back often enough for pages to stay live.
        #[cfg(target_os = "linux")]
        {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            wake = wake.min(GTK_PUMP);
        }
        let mut animating = false;
        if let Self::Running(app) = self {
            // Closed last pass: no longer the key window, safe to destroy.
            app.closed_windows.clear();
            app.service_windows(event_loop);
            if std::mem::take(&mut app.ui_stale) {
                app.ui_state();
            }
            if app.last_flush.elapsed() >= Duration::from_secs(1) {
                app.svc.flush();
                app.trackers.flush();
                app.last_flush = Instant::now();
            }
            if app.quit {
                event_loop.exit();
                return;
            }
            app.poll_webext();
            // WebKit can reset the layer of a page view when it resizes or reappears.
            #[cfg(target_os = "macos")]
            if app.cards_rounded.elapsed() >= Duration::from_millis(300) {
                app.cards_rounded = Instant::now();
                // Only the cards on screen: the hidden tabs are rounded when they are shown.
                app.round_cards(false);
            }
            app.wake_anchored();
            app.discard_idle_tabs();
            if app.flush_raise(false) {
                wake = wake.min(RAISE_DELAY);
            }
            app.refill_spares();
            if app.service_loading() {
                wake = wake.min(Duration::from_millis(250));
            }
            app.service_peers();
            app.service_media();
            app.service_stats();
            app.service_calendar();
            app.service_auth();
            app.service_nearby();
            app.service_watches();
            app.service_signals();
            app.service_marks();
            if let Some(soon) = app.service_recall() {
                wake = wake.min(soon.max(Duration::from_millis(20)));
            }
            app.service_compare();
            app.service_errors();
            if app.blur_due.is_some() {
                app.pip_on_blur();
                wake = wake.min(BLUR_DELAY);
            }
            if !app.dl_live.is_empty() {
                wake = wake.min(DOWNLOAD_POLL);
            }
            #[cfg(windows)]
            if app.ext_popup.is_some() {
                app.probe_ext_popup();
                wake = wake.min(Duration::from_millis(250));
            }
            if app.sidebar.held() {
                if app.sidebar.release_due(Instant::now()) {
                    app.layout(false);
                }
                wake = wake.min(Duration::from_millis(200));
            }
            if app.sidebar.moving() || app.panes.moving() {
                let since = app.last_frame.elapsed();
                if since < FRAME {
                    std::thread::sleep(FRAME - since);
                }
                app.last_frame = Instant::now();
                app.tick_motion();
                animating = true;
            }
            #[cfg(feature = "devtools")]
            {
                app.dev_tick(event_loop);
                if app.dev_active() {
                    wake = wake.min(Duration::from_millis(30));
                }
            }
        }
        // While the sidebar or a pane moves, the loop runs frame after frame (paced above).
        event_loop.set_control_flow(if animating {
            ControlFlow::Poll
        } else {
            ControlFlow::WaitUntil(Instant::now() + wake)
        });
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: Wake) {
        if let Self::Running(app) = self {
            app.pump();
            app.service_windows(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Self::Running(app) = self else { return };
        app.pump();
        app.service_windows(event_loop);
        if app.easel_window_id() == Some(id) {
            match event {
                WindowEvent::CloseRequested => app.close_easel(),
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    app.easel_resized();
                    let maxed = app.easel.as_ref().is_some_and(|w| w.window.is_maximized());
                    if maxed != app.panels_maxed.1 {
                        app.panels_maxed.1 = maxed;
                        app.ui_easel();
                    }
                }
                _ => {}
            }
            return;
        }
        // A Settings / Easel window being retired (or already gone): its last events (focus
        // lost, close, destroyed) must not be read as the main window's.
        if id != app.window.id() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                #[cfg(windows)]
                if let Some(backdrop) = &mut app.backdrop {
                    backdrop.paint();
                }
                app.layout(true);
                #[cfg(windows)]
                app.close_ext_popup();
                if app.window.is_maximized() != app.maximized {
                    app.maximized = !app.maximized;
                    app.ui_bar();
                }
            }
            #[cfg(windows)]
            WindowEvent::RedrawRequested => {
                if let Some(backdrop) = &mut app.backdrop {
                    backdrop.paint();
                }
            }
            WindowEvent::Focused(true) => {
                app.apply_focus();
                app.pip_back_on_focus();
                // Back from the system's dialog or Settings app: the tour reads the choice again.
                if app.default_asked && app.onboarding.is_some() && app.start_page_front() {
                    app.ui_home();
                }
            }
            WindowEvent::Focused(false) => app.blur_due = Some(Instant::now() + BLUR_DELAY),
            _ => {}
        }
    }
}

/// How often the loop hands GTK its events on Linux when nothing else wakes it.
#[cfg(target_os = "linux")]
const GTK_PUMP: Duration = Duration::from_millis(8);

impl Running {
    fn service_windows(&mut self, event_loop: &ActiveEventLoop) {
        if std::mem::take(&mut self.settings_wanted) {
            self.open_settings();
            self.ui_state();
        }
        if std::mem::take(&mut self.easel_wanted) {
            self.open_easel(event_loop);
            self.ui_easel();
        }
    }

    fn pump(&mut self) {
        while let Some(msg) = self.mailbox.pop() {
            self.handle_msg(msg);
        }
    }
}

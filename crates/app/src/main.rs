//! BlackHole composition root: window, native web views and message routing.
//! Business rules live in the inner crates; this crate only wires them to the platform web engine
//! (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux) through `wry`.
//!
//! - `state`: a window's state (`Running`); `boot` builds it, `app` runs the event loop.
//! - `commands` / `messages`: what the shell's pages and the engine ask, routed to the modules below.
//! - `shell`: the browser's own pages and where they sit; `tabs`: page views and their life;
//!   `features`: one module per feature (easels, extensions, sync, passwords…).
//! - `views` builds the engine views, `ipc` defines the messages, `i18n` the interface strings.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::time::Instant;

use blackhole_infrastructure::peers;
use url::Url;
use winit::event_loop::EventLoop;

use crate::app::App;
use crate::ipc::{Mailbox, Msg};
use crate::platform::LAUNCHED;
use crate::shell::windows::{erase_private, peers_dir};

mod app;
#[cfg(windows)]
mod backdrop;
mod boot;
mod commands;
#[cfg(feature = "devtools")]
mod devtools;
mod features;
mod i18n;
mod ipc;
mod messages;
mod platform;
mod shell;
mod state;
mod tabs;
mod views;

fn main() {
    LAUNCHED.get_or_init(Instant::now);
    #[cfg(target_os = "linux")]
    let event_loop = {
        use winit::platform::x11::EventLoopBuilderExtX11;
        // wry embeds WebKitGTK in an X11 window only (XWayland on a Wayland desktop): winit and
        // GDK must both talk to the X server.
        std::env::set_var("GDK_BACKEND", "x11");
        gtk::init().expect("GTK");
        EventLoop::with_user_event().with_x11().build().expect("event loop")
    };
    #[cfg(not(target_os = "linux"))]
    let event_loop = EventLoop::with_user_event().build().expect("event loop");
    let mailbox = Mailbox::new(event_loop.create_proxy());
    // Registered before the loop starts so a link that launches the app is not lost.
    let links = mailbox.clone();
    blackhole_macos_url::register_url_handler(move |link| {
        if let Some(url) = Url::parse(&link)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"))
        {
            links.push(Msg::External(url));
        }
    });
    let mut app = App::Initial(mailbox);
    event_loop.run_app(&mut app).expect("run");
    drop(app);
    blackhole_infrastructure::flush_writes();
    peers::withdraw(&peers_dir(), std::process::id());
    erase_private();
}

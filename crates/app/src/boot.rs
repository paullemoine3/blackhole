//! Building a window: its native window, menus and the shell's first views.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use blackhole_application::{BrowserService, WorkspaceRepository};
use blackhole_domain::BlockLog;
use blackhole_infrastructure::app_extensions;
use blackhole_infrastructure::{
    catalog,
    default_privacy_policy,
    extensions_dir,
    JsonWorkspaceRepository,
    load_extensions,
};
use blackhole_infrastructure::store::{BookmarkStore, EaselStore};
use serde_json::json;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;
use wry::WebContext;

use crate::{i18n, views};
use crate::features::{
    aliases,
    compare,
    extensions,
    github_ext,
    handoff,
    highlights,
    meet,
    passwords,
    recall,
    stats,
    suggesting,
    trackers,
    watching,
};
use crate::features::imports::{file_bookmarks, Imports};
use crate::features::market::Market;
use crate::features::updates::Updates;
use crate::ipc::Mailbox;
use crate::platform::trace_startup;
use crate::shell::pane_motion;
use crate::shell::home::Onboarding;
use crate::shell::layout::{BAR_HEIGHT, card_rect, chrome_rect};
use crate::shell::pages::{BAR_HTML, CHROME_HTML, HOME_HTML};
use crate::shell::palette::Palette;
use crate::shell::panels::LazyView;
use crate::shell::sidebar::Sidebar;
use crate::shell::windows::{
    is_new_window,
    PEERS_PUBLISH_EVERY,
    private_data_dir,
    private_mode,
    window_tasks,
};
use crate::state::Running;
use crate::tabs::{errors, favicons};
use crate::tabs::auth::Auth;
use crate::tabs::downloads::Downloads;
use crate::tabs::lifecycle::{ANCHORS_DELAY, SPARES_DELAY};
use crate::tabs::media::MediaWatch;
use crate::tabs::pip::Pip;

#[cfg(feature = "devtools")]
use crate::devtools;
#[cfg(windows)]
use crate::backdrop;

/// The main window's title (shown by the system: taskbar, window list).
pub(crate) fn main_title() -> String {
    if private_mode() {
        i18n::t("window.private")
    } else {
        "BlackHole".to_owned()
    }
}

/// The Edit menu is what routes Cmd+C/V/X/A/Z to the focused web view on macOS; the Window
/// menu gives Cmd+M and the system's list of open windows.
#[cfg(target_os = "macos")]
pub(crate) fn install_menu() -> muda::Menu {
    use i18n::t;
    use muda::{Menu, PredefinedMenuItem as Item, Submenu};
    let menu = Menu::new();
    let app = Submenu::new("BlackHole", true);
    let edit = Submenu::new(t("native.edit"), true);
    let _ = app.append_items(&[
        &Item::about(Some(&t("native.about")), None),
        &Item::separator(),
        &Item::hide(Some(&t("native.hide"))),
        &Item::hide_others(Some(&t("native.hide_others"))),
        &Item::separator(),
        &Item::quit(Some(&t("native.quit"))),
    ]);
    let _ = edit.append_items(&[
        &Item::undo(Some(&t("native.undo"))),
        &Item::redo(Some(&t("native.redo"))),
        &Item::separator(),
        &Item::cut(Some(&t("native.cut"))),
        &Item::copy(Some(&t("native.copy"))),
        &Item::paste(Some(&t("native.paste"))),
        &Item::select_all(Some(&t("native.select_all"))),
    ]);
    let window = Submenu::new(t("native.window"), true);
    let _ = window.append_items(&[
        &Item::minimize(Some(&t("native.minimize"))),
        &Item::maximize(Some(&t("native.zoom"))),
        &Item::separator(),
        &Item::bring_all_to_front(Some(&t("native.bring_all_to_front"))),
    ]);
    let _ = menu.append_items(&[&app, &edit, &window]);
    menu.init_for_nsapp();
    window.set_as_windows_menu_for_nsapp();
    menu
}

impl Running {
    pub(crate) fn new(event_loop: &ActiveEventLoop, mailbox: Mailbox) -> Self {
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(main_title())
            .with_theme(Some(winit::window::Theme::Dark))
            .with_maximized(true)
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(720.0, 480.0));
        // The top bar draws its own minimize / maximize / close buttons.
        #[cfg(windows)]
        {
            use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
            attrs = attrs.with_decorations(false).with_undecorated_shadow(true);
            // Resource 1 is the icon embedded by build.rs.
            if let Ok(icon) = winit::window::Icon::from_resource(1, None) {
                attrs = attrs
                    .with_window_icon(Some(icon.clone()))
                    .with_taskbar_icon(Some(icon));
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
        trace_startup("event loop running");
        let window = Rc::new(event_loop.create_window(attrs).expect("window"));
        trace_startup("window created");
        #[cfg(windows)]
        let backdrop = backdrop::Backdrop::new(&window);

        // Dev builds use a scratch profile so scripted runs never touch the user's data.
        #[cfg(feature = "devtools")]
        let profile = devtools::scratch().join("bh_data");
        #[cfg(not(feature = "devtools"))]
        let profile = blackhole_infrastructure::data_dir();
        let data = if private_mode() {
            private_data_dir()
        } else {
            profile.clone()
        };
        crate::shell::windows::sweep_private();
        if private_mode() {
            blackhole_infrastructure::private::prepare(&profile, &data);
        }
        // A private window with extensions starts from a copy of what they keep in the main
        // profile (settings, first run done), before its engine opens the folder.
        #[cfg(windows)]
        if private_mode() {
            let shared = JsonWorkspaceRepository::new(profile.clone())
                .load_settings()
                .ok()
                .flatten()
                .is_some_and(|s| s.extensions_in_private && s.private_extension_settings);
            if shared {
                blackhole_infrastructure::seed_private_extensions(
                    &profile.join("webview"),
                    &data.join("webview"),
                );
            }
        }
        let mut web_context = WebContext::new(Some(data.join("webview")));
        // Profiles of the black hole tabs of earlier runs (WebView2 keeps a folder per profile).
        #[cfg(windows)]
        {
            let (engine, keep) = (data.join("webview"), views::burner_prefix());
            std::thread::spawn(move || {
                blackhole_infrastructure::remove_burner_profiles(&engine, &keep);
            });
        }
        // Session, icons and extensions live in separate files: load them side by side.
        let (mut svc, favicons, preloaded_ext) = std::thread::scope(|scope| {
            let icons = scope.spawn(|| favicons::Favicons::load(&data));
            let exts = (!private_mode()).then(|| {
                scope.spawn(|| {
                    let dir = extensions_dir(&data);
                    extensions::refresh_sample(&dir);
                    load_extensions(&dir)
                })
            });
            let svc = BrowserService::start(
                JsonWorkspaceRepository::new(data.clone()),
                default_privacy_policy(),
            );
            (
                svc,
                icons.join().expect("favicons"),
                exts.map(|h| h.join().unwrap_or_default()),
            )
        });
        trace_startup("session, icons and extensions read");
        // Learned trackers are blocked from the first page; a private window reads the main
        // profile's and writes nothing.
        let learning = trackers::Learning::new(&profile, private_mode());
        let choices = suggesting::Choices::new(&profile);
        svc.policy().set_learned(
            learning.data.learned().iter().cloned(),
            learning.data.allowed().iter().cloned(),
        );
        views::announce_browser();
        views::prepare_blocking(svc.policy());
        trace_startup("blocking lists ready");
        // A private window has no extensions unless the main profile's settings allow them.
        let ext_src = if private_mode() {
            let shared = JsonWorkspaceRepository::new(profile.clone())
                .load_settings()
                .ok()
                .flatten()
                .filter(|s| s.extensions_in_private);
            match shared {
                Some(s) => {
                    for id in &s.disabled_extensions {
                        svc.set_enabled_flag(id, false);
                    }
                    Some(profile.clone())
                }
                None => None,
            }
        } else {
            Some(data.clone())
        };
        let ext_src_dir = ext_src.clone().unwrap_or_else(|| data.clone());
        if let Some(src) = &ext_src {
            svc.set_extensions(
                preloaded_ext.unwrap_or_else(|| load_extensions(&extensions_dir(src))),
            );
        }
        views::set_hardware_acceleration(svc.settings().hardware_acceleration);
        views::set_encrypted_dns(
            svc.settings()
                .encrypted_dns
                .then(|| blackhole_domain::dns::doh_template(&svc.settings().dns_resolver))
                .flatten()
                .as_deref(),
        );
        // Before any page is created: each one receives the catalog of this language.
        i18n::apply(&svc.settings().language);
        window_tasks();
        window.set_title(&main_title());
        let sidebar = Sidebar::starting(
            f64::from(svc.settings().sidebar_width),
            svc.settings().sidebar_open,
        );

        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        let chrome_bounds = chrome_rect(&sidebar, &size);
        let bar_bounds = views::rect(0.0, 0.0, size.width, BAR_HEIGHT);
        let (cx, cy, cw, ch) = card_rect(&sidebar, &size);
        let home_bounds = views::rect(cx, cy, cw, ch);
        let chrome = views::ui_view(
            &window,
            &mut web_context,
            CHROME_HTML,
            chrome_bounds,
            true,
            false,
            &mailbox,
        )
        .expect("sidebar view");
        trace_startup("sidebar view");
        let bar = views::ui_view(
            &window,
            &mut web_context,
            BAR_HTML,
            bar_bounds,
            true,
            false,
            &mailbox,
        )
        .expect("top bar view");
        let home = views::ui_view(
            &window,
            &mut web_context,
            HOME_HTML,
            home_bounds,
            true,
            false,
            &mailbox,
        )
        .expect("start page view");
        trace_startup("interface views");

        let (install_tx, install_rx) = std::sync::mpsc::channel();
        let (capture_tx, capture_rx) = std::sync::mpsc::channel();
        let market = Market::new(&ext_src_dir);
        let github = github_ext::Sideload::new(&ext_src_dir);
        let easel_store = EaselStore::new(&data);
        let easels = easel_store.load();
        let webext = blackhole_macos_webext::ExtensionHost::new(
            // Extensions write nothing down from a private window, unless the user lets them use
            // the settings they keep in normal windows (macOS; Windows copies those instead).
            private_mode()
                && !(svc.settings().extensions_in_private && svc.settings().private_extension_settings),
        );
        let app_ext = blackhole_macos_webext::discover_app_extensions();
        #[cfg(windows)]
        if let Some(host) = &webext {
            host.attach(&chrome);
        }
        if let (Some(host), Some(src)) = (&webext, &ext_src) {
            for e in app_ext.iter().filter(|e| app_extensions::is_enabled(src, &e.id)) {
                host.load(&e.id, &e.path);
            }
            // The market's, then those added from GitHub (pinned to the package accepted).
            let ids = market
                .index
                .for_this_system()
                .map(|e| e.id.clone())
                .chain(github.pinned.iter().map(|p| p.id.clone()));
            for id in ids {
                if catalog::is_installed(src, &id) && svc.extension_enabled(&id) {
                    host.load(&id, &catalog::package_path(src, &id));
                }
            }
        }

        // Bookmarks saved by older versions become folders and tabs once.
        let filed = data.join("bookmarks.filed");
        if !filed.exists() {
            file_bookmarks(&mut svc, BookmarkStore::new(&data).load());
            let _ = std::fs::write(&filed, b"1");
        }

        trace_startup("extensions asked for");
        let release_notes: Vec<serde_json::Value> = svc
            .take_release_notes(env!("CARGO_PKG_VERSION"))
            .into_iter()
            .map(|(version, items)| json!({"version": version, "items": items}))
            .collect();
        // The UI scripts of the devtools harness start on fresh profiles: the tour stays out of
        // their way unless a script asks for it (`BH_ONBOARDING=1`).
        let harness = cfg!(feature = "devtools") && std::env::var_os("BH_ONBOARDING").is_none();
        let onboarding = (!harness
            && blackhole_domain::onboarding::offered(svc.first_launch(), private_mode(), is_new_window()))
        .then_some(blackhole_domain::onboarding::Mode::Welcome);

        Self {
            release_notes,
            chrome,
            flyout: None,
            hotzone: None,
            bar,
            home,
            tabs: HashMap::new(),
            spares: Vec::new(),
            spares_due: Some(Instant::now() + SPARES_DELAY),
            anchors_due: Some(Instant::now() + ANCHORS_DELAY),
            last_active: HashMap::new(),
            zoom: HashMap::new(),
            zoomed: HashSet::new(),
            // The overlays come once the first views are up (see `flyout`).
            raise_due: Some(Instant::now()),
            shown: None,
            pip: Pip::default(),
            card_left: std::cell::Cell::new(f64::NAN),
            bar_left: std::cell::Cell::new(f64::NAN),
            bar_notice: std::cell::Cell::new((true, 0)),
            sampler: stats::Sampler::start(),
            usage: None,
            stats_tabs: 0,
            peers_due: Instant::now(),
            peers_published: Instant::now() - PEERS_PUBLISH_EVERY,
            loading: HashMap::new(),
            splash: HashMap::new(),
            awaiting: HashMap::new(),
            pending: HashMap::new(),
            errors: errors::Errors::default(),
            auth: Auth::default(),
            signals: Default::default(),
            received: None,
            #[cfg(target_os = "macos")]
            cards_rounded: Instant::now(),
            media: MediaWatch::new(),
            crashed: HashMap::new(),
            burner_next: false,
            pilot: None,
            panes: pane_motion::PaneMotion::default(),
            split_seen: None,
            split_opened: 0,
            pilot_sent: (0, pane_motion::Side::Right),
            links_cleaned: 0,
            shields_dirty: false,
            shields_sent: Instant::now(),
            undo: None,
            trashed: None,
            library_jump: (0, ""),
            panels_maxed: (false, false),
            last_frame: Instant::now(),
            live_pending: None,
            referrer_next: None,
            pick_watches: false,
            calendar: meet::Calendar::new(&data),
            nearby: handoff::Nearby::new(&data, private_mode()),
            ext_sync: Default::default(),
            watching: watching::Watching::new(&data),
            recall: recall::Recall::new(&data, private_mode()),
            trackers: learning,
            choices,
            next_visit: None,
            marks: highlights::Marks::new(&data, private_mode()),
            comparing: compare::Comparing::new(),
            tidied: None,
            tidy_dismissed: false,
            svc,
            mailbox,
            sidebar,
            web_context,
            blocked: BlockLog::default(),
            settings_tab: None,
            imports: Imports::new(&data),
            favicons,
            icons_for_ui: std::cell::Cell::new(true),
            icons_for_home: std::cell::Cell::new(true),
            ui_stale: false,
            last_flush: Instant::now(),
            app_ext,
            bar_exts: json!([]),
            bar_exts_at: Instant::now(),
            passwords: passwords::Passwords::new(&profile, private_mode()),
            aliases: aliases::Aliases::new(),
            settings_seq: 0,
            home_visible: true,
            splitbar: LazyView::new(),
            palette: Palette::default(),
            peek: None,
            passkey_refusals: Default::default(),
            #[cfg(windows)]
            ext_popup: None,
            #[cfg(windows)]
            ext_anchors: HashMap::new(),
            webext,
            installing: Default::default(),
            welcome: if private_mode() && ext_src.is_some() {
                blackhole_domain::welcome::WelcomePages::private(Instant::now())
            } else {
                blackhole_domain::welcome::WelcomePages::off()
            },
            ext_loaded: 0,
            intro: !is_new_window(),
            onboarding: Onboarding::new(onboarding),
            market,
            github,
            updates: Updates::new(),
            clips: crate::features::clipboard::Clips::new(),
            quit: false,
            maximized: false,
            install_errors: HashMap::new(),
            install_tx,
            install_rx,
            webext_sig: String::new(),
            downloads: Downloads::new(&data),
            aborted: HashMap::new(),
            archive: blackhole_infrastructure::store::ArchiveStore::new(&data).load(),
            data,
            ext_src: ext_src_dir,
            settings_panel: LazyView::new(),
            settings_shown: false,
            settings_opened: 0,
            settings_wanted: false,
            easel_current: easels.all().first().map(|e| e.id),
            easels,
            easel_store,
            live: HashMap::new(),
            live_slots: HashMap::new(),
            live_embedded: false,
            live_pick: None,
            easel_panel: LazyView::new(),
            easel_tab: false,
            easel_beside: None,
            library_panel: LazyView::new(),
            library: false,
            thumbs: std::cell::RefCell::default(),
            easel: None,
            easel_wanted: false,
            closed_windows: Vec::new(),
            capture_tx,
            capture_rx,
            #[cfg(target_os = "macos")]
            _menu: install_menu(),
            #[cfg(feature = "devtools")]
            dev: devtools::Dev::load(),
            #[cfg(windows)]
            backdrop,
            window,
        }
    }
}

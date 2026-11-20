//! The Settings page, shown in the page card like a tab.

use blackhole_domain::{SearchEngine, Settings};
use blackhole_infrastructure::extensions_dir;
use serde_json::json;
use wry::WebView;

use crate::i18n;
use crate::features::extensions::app_extension_enabled;
use crate::state::Running;

impl Running {
    pub(crate) fn ui_settings(&self) {
        let Some(view) = self.settings_panel.view.as_ref() else { return };
        let settings = self.svc.settings();
        let mut state = json!({
            "os": std::env::consts::OS,
            // Shown in the page card of the main window (no top bar of its own), tinted by the
            // space's color.
            "embedded": true,
            "color": self.svc.workspace().active_space().color,
            "version": env!("CARGO_PKG_VERSION"),
            "engine": settings.search_engine.id(),
            "engines": SearchEngine::ALL.iter().map(|e| json!({"id": e.id(), "label": e.label()})).collect::<Vec<_>>(),
            "sidebarWidth": settings.sidebar_width,
            "sidebarWidths": Settings::SIDEBAR_WIDTH_CHOICES,
            "animations": settings.animations,
            "hardwareAcceleration": settings.hardware_acceleration,
            "extensionsInPrivate": settings.extensions_in_private,
            "trackerBlocking": settings.tracker_blocking,
            "calendar": self.calendar_json(),
            "devices": self.devices_json(),
            "adBlocking": settings.ad_blocking,
            "shields": {
                "hideAds": settings.shields.hide_ads,
                "videoAds": settings.shields.video_ads,
                "cookieBanners": settings.shields.cookie_banners,
                "cleanLinks": settings.shields.clean_links,
            },
            "tabSleep": settings.tab_sleep_minutes,
            "tabSleepChoices": Settings::SLEEP_CHOICES,
            "maxLiveTabs": settings.max_live_tabs,
            "maxLiveTabsChoices": Settings::LIVE_TABS_CHOICES,
            "spareViews": settings.spare_views,
            "spareChoices": Settings::SPARE_CHOICES,
            "blocked": self.blocked.total(),
            "blockedLog": self.blocked.entries().iter().take(100).map(|b| json!({"host": b.host, "count": b.count, "site": b.site})).collect::<Vec<_>>(),
            "tab": self.settings_tab.clone(),
            "tabSeq": self.settings_seq,
            "catalog": self.catalog_state(),
            "catalogChecked": self.catalog_checked,
            "appUpdate": self.app_update_json(),
            "appExtensions": self.app_ext.iter().map(|e| {
                let on = app_extension_enabled(&self.data, &e.id);
                let state = self.webext.as_ref().and_then(|h| h.state(&e.id));
                json!({
                    "id": e.id, "name": e.name, "enabled": on, "supported": self.webext.is_some(),
                    "status": match state {
                        Some(blackhole_macos_webext::LoadState::Loading) => "loading",
                        Some(blackhole_macos_webext::LoadState::Loaded) => "active",
                        Some(blackhole_macos_webext::LoadState::Failed(_)) => "failed",
                        None => "off",
                    },
                    "error": match state { Some(blackhole_macos_webext::LoadState::Failed(w)) => Some(w), _ => None },
                })
            }).collect::<Vec<_>>(),
            "import": json!({
                "sources": self.import_sources.iter().map(|s| json!({"id": s.id, "browser": s.browser, "profile": s.profile})).collect::<Vec<_>>(),
                "message": i18n::render(&self.import_msg),
                "bookmarks": self.bookmarks.load().len(),
                "logins": self.passwords.count(),
            }),
            "passwords": self.passwords.state_json(settings),
            "extensionsDir": extensions_dir(&self.data).display().to_string(),
            "extensions": self.svc.extensions().iter().map(|e| json!({
                "id": e.id, "name": e.name, "version": e.version, "description": e.description,
                "enabled": self.svc.extension_enabled(&e.id),
                "sites": e.scripts.iter().flat_map(|s| s.matches.iter().cloned()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        // Apart: the macro above is at serde_json's recursion limit.
        state["tileSize"] = json!(settings.quick_access_size);
        state["tileSizes"] = json!(blackhole_domain::QUICK_ACCESS_SIZES);
        state["homeSections"] = blackhole_domain::home_order(&settings.home_order)
            .iter()
            .map(|s| json!({"id": s, "shown": !settings.hidden_home_sections.contains(*s)}))
            .collect();
        // General > Language: the setting, the languages in their own words, and what "System"
        // gives on this computer.
        state["recall"] = self.recall_state();
        state["trackers"] = self.trackers_json();
        state["language"] = json!(settings.language);
        state["depth"] = json!(settings.depth);
        state["languages"] = json!(blackhole_domain::i18n::LANGUAGES
            .iter()
            .map(|(id, label)| json!({"id": id, "label": label}))
            .collect::<Vec<_>>());
        state["market"] = self.market_json();
        state["github"] = self.github_json();
        state["systemLanguage"] = json!(blackhole_domain::i18n::resolve(
            blackhole_domain::i18n::SYSTEM,
            &i18n::system_languages()
        ));
        let _ = view.evaluate_script(&format!("window.bh&&bh.render({state})"));
    }

    /// The Settings page, when it exists (shown or kept hidden for a while).
    pub(crate) fn settings_view(&self) -> Option<&WebView> {
        self.settings_panel.view.as_ref()
    }

    /// Shows the Settings in the page card, like a tab (no window of their own any more): the
    /// tab, the Library or the Easel shown before goes back when they close.
    pub(crate) fn open_settings(&mut self) {
        self.close_palette();
        self.close_peek();
        if self.easel_tab {
            self.live.clear();
            self.live_slots.clear();
            self.easel_tab = false;
        }
        self.library = false;
        self.settings_shown = true;
        self.sync();
        self.ui_settings();
    }

    pub(crate) fn close_settings(&mut self) {
        if std::mem::take(&mut self.settings_shown) {
            self.sync();
        }
    }
}

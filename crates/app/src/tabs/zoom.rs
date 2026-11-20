//! Page zoom, remembered per site.

use blackhole_domain::TabId;
use url::Url;

use crate::i18n::tf;
use crate::state::Running;

impl Running {
    /// The zoom of `host`'s site for `tab`, in percent: remembered in the settings, or for the
    /// session only in a black hole tab (it keeps nothing).
    pub(crate) fn zoom_of(&self, tab: TabId, host: &str) -> u32 {
        if self.svc.workspace().is_burner(tab) {
            self.zoom.get(&blackhole_domain::site_key(host)).copied().unwrap_or(100)
        } else {
            self.svc.settings().zoom_of(host)
        }
    }

    /// Zooms the active page like other browsers do: the level belongs to the site (`www.`
    /// included) and is remembered between launches.
    pub(crate) fn zoom_active(&mut self, action: &str) {
        let Some(tab) = self.svc.workspace().active_tab().map(|t| t.id) else {
            return;
        };
        let host = self
            .tabs
            .get(&tab)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok())
            .and_then(|u| u.host_str().map(str::to_owned));
        let Some(host) = host else { return };
        let current = self.zoom_of(tab, &host);
        let next = match action {
            "zoom_in" => blackhole_domain::next_zoom(current, true),
            "zoom_out" => blackhole_domain::next_zoom(current, false),
            _ => 100,
        };
        if self.svc.workspace().is_burner(tab) {
            let site = blackhole_domain::site_key(&host);
            if next == 100 {
                self.zoom.remove(&site);
            } else {
                self.zoom.insert(site, next);
            }
        } else {
            self.svc.set_site_zoom(&host, next);
        }
        // Every tab of this window showing the site follows, as in other browsers.
        let ids: Vec<TabId> = self.tabs.keys().copied().collect();
        for id in ids {
            self.apply_zoom(id);
        }
        self.ui_bar();
        self.toast(&tf("toast.zoom", &[("n", &next.to_string())]));
    }

    /// Gives a tab the zoom of the site it shows (100% when the site has none).
    pub(crate) fn apply_zoom(&mut self, tab: TabId) {
        let Some(view) = self.tabs.get(&tab) else { return };
        let host = view
            .url()
            .ok()
            .and_then(|raw| Url::parse(&raw).ok())
            .and_then(|u| u.host_str().map(str::to_owned));
        let wanted = host.map_or(100, |h| self.zoom_of(tab, &h));
        let zoomed = self.zoomed.contains(&tab);
        if wanted == 100 && !zoomed {
            return;
        }
        let _ = view.zoom(f64::from(wanted) / 100.0);
        if wanted == 100 {
            self.zoomed.remove(&tab);
        } else {
            self.zoomed.insert(tab);
        }
    }
}

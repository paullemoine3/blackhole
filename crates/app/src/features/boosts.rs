//! Boosts: custom CSS per site.

use blackhole_domain::TabId;

use crate::views;
use crate::state::Running;

impl Running {
    pub(crate) fn save_boost(&mut self, css: &str) {
        let Some(site) = self.active_site() else {
            return;
        };
        self.svc.set_boost(&site, css);
        self.close_palette();
        let key = blackhole_domain::site_key(&site);
        let matching: Vec<TabId> = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .filter(|t| {
                t.url
                    .host_str()
                    .is_some_and(|h| blackhole_domain::site_key(h) == key)
            })
            .map(|t| t.id)
            .collect();
        for id in matching {
            if let Some(view) = self.tabs.get(&id) {
                views::apply_boost(view, css);
            }
        }
    }

    // ---- peek ---------------------------------------------------------------------------
}

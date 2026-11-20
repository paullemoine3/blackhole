//! Extension sync between paired devices (0.8.25), beside the tab and history syncs: one switch
//! (`Settings::extension_sync`, Settings > Devices), asked of each online device with the history
//! (`HISTORY_EVERY`, when a device comes online, "Sync now"). What the other device has
//! installed is compared with what is here (`domain::ext_sync::plan`): market extensions
//! (reviewed and signed) are installed at once; those added from GitHub or a link are proposed in
//! a notice, and installed only on "Add". Additions only: nothing is ever removed by a sync.
//! Wire format: `blackhole_infrastructure::ext_sync`.

use std::collections::{HashMap, HashSet};

use blackhole_domain::ext_sync::{self, Offer, Source};
use blackhole_infrastructure::catalog;
use blackhole_infrastructure::ext_sync::{self as wire, Incoming};
use serde_json::{json, Value};

use crate::i18n::{t, tf, tn};
use crate::shell::notices::Notice;
use crate::state::Running;

#[derive(Default)]
pub struct ExtSync {
    /// The last exchange (device name, seconds) for Settings > Devices.
    pub last: Option<(String, u64)>,
    /// Proposals waiting for an answer, by device.
    proposals: HashMap<String, Vec<Offer>>,
    /// Extensions proposed and answered in this run: not proposed again until the next launch.
    declined: HashSet<String>,
}

impl Running {
    /// What is installed here, as the other device reads it.
    fn installed_offers(&self) -> Vec<Offer> {
        let market = self
            .market
            .index
            .for_this_system()
            .filter(|e| catalog::is_installed(&self.data, &e.id))
            .map(|e| Offer { id: e.id.clone(), name: e.name.clone(), source: Source::Market });
        let sideloaded = self.github.pinned.iter().map(|p| Offer {
            id: p.id.clone(),
            name: p.name.clone(),
            source: if p.direct {
                Source::Link { url: p.url.clone() }
            } else {
                Source::Github { owner: p.owner.clone(), repo: p.repo.clone() }
            },
        });
        market.chain(sideloaded).collect()
    }

    /// Asks `device` what it has installed.
    pub(crate) fn ask_extensions(&self, device: &str) {
        if self.svc.settings().extension_sync && self.webext.is_some() {
            self.nearby.send_snapshot(device, wire::pull());
        }
    }

    /// An extension message of a paired device (`None`: not one).
    pub(crate) fn on_extensions(&mut self, device: &str, name: &str, json: &str) -> bool {
        let Some(incoming) = wire::parse(json) else {
            return false;
        };
        if !self.svc.settings().extension_sync || !self.nearby.is_paired(device) {
            return true;
        }
        match incoming {
            Incoming::Pull => self.nearby.send_snapshot(device, wire::list(&self.installed_offers())),
            Incoming::List(offers) => self.on_offers(device, name, &offers),
        }
        true
    }

    fn on_offers(&mut self, device: &str, name: &str, offers: &[Offer]) {
        // Nothing to install with: Linux, Android, a private window.
        if self.webext.is_none() {
            return;
        }
        let mut here: HashSet<String> = self.installed_offers().into_iter().map(|o| o.id).collect();
        here.extend(self.installing.iter().cloned());
        let market = &self.market.index;
        let plan = ext_sync::plan(offers, &here, |id| market.for_this_system().any(|e| e.id == id), &self.ext_sync.declined);
        for id in &plan.install {
            self.install_catalog(id);
        }
        if !plan.install.is_empty() {
            self.notify(Notice::Device, &tn("toast.ext_sync_installing", plan.install.len(), &[("name", name)]), Value::Null);
        }
        self.ext_sync.last = Some((name.to_owned(), crate::platform::now_secs()));
        if !plan.propose.is_empty() && self.ext_sync.proposals.get(device) != Some(&plan.propose) {
            let names: Vec<&str> = plan.propose.iter().take(3).map(|o| o.name.as_str()).collect();
            let mut list = names.join(", ");
            if plan.propose.len() > names.len() {
                list.push_str(", …");
            }
            let message = tf("toast.ext_sync_ask", &[("name", name), ("list", &list)]);
            let run = |accept: bool| json!({"cmd": "ext_sync_answer", "id": device, "accept": accept});
            let actions = json!([
                {"label": t("toast.ext_sync_add"), "run": run(true)},
                {"label": t("toast.sync_later"), "run": run(false)},
            ]);
            self.ext_sync.proposals.insert(device.to_owned(), plan.propose);
            self.notify(Notice::Device, &message, actions);
        }
        self.ui_settings();
    }

    /// The answer to a proposal: add the extensions (checked like any other), or not this time.
    pub(crate) fn ext_sync_answer(&mut self, device: &str, accept: bool) {
        let Some(offers) = self.ext_sync.proposals.remove(device) else {
            return;
        };
        // Answered either way: not proposed again in this run (an install that fails does not
        // come back every few minutes; Settings > Extensions can still add it).
        for offer in &offers {
            self.ext_sync.declined.insert(offer.id.clone());
            if accept {
                self.sync_install(offer);
            }
        }
        if accept {
            self.toast(&tn("toast.ext_sync_adding", offers.len(), &[]));
            self.ui_settings();
        }
    }
}

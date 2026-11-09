//! "Continue on another device": Settings > Devices pairs this BlackHole with another one on the
//! local network; the tab menu and the command bar send a tab there, which opens it. The network,
//! the keys and the message format live in `blackhole_infrastructure::handoff`; this side only
//! reads the video position of the tab and shows the results. Not started in private windows.
//!
//! Tab synchronization with the paired devices (quick access, open tabs, spaces) rides on the
//! same service: the rules are `blackhole_domain::sync`, the ledger `blackhole_infrastructure::
//! sync`. A device coming online gets this device's ledger; an `auto` device also gets it a few
//! seconds after each change; a received ledger is merged at once (`auto`) or proposed (`ask`).

use crate::i18n::{message, t, tf, tn};
use crate::{ipc::UiCommand, Running, TabId};
use blackhole_domain::handoff::{can_continue, has_position, resume_url};
use blackhole_infrastructure::handoff::{self, DeviceInfo, Event, Handle, Request};
use blackhole_infrastructure::sync::{decide, Decision, Received, SyncEngine};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use url::Url;

/// How often the workspace is compared with the synced ledger.
const OBSERVE_EVERY: Duration = Duration::from_secs(2);
/// After a change, `auto` devices get the ledger once things have settled this long.
const SEND_AFTER: Duration = Duration::from_secs(3);

/// Where the video of the page is (the longest one that has started), in seconds.
const POSITION_JS: &str = "(()=>{const v=[...document.querySelectorAll('video')].filter(v=>v.currentTime>0&&!v.ended).sort((a,b)=>(b.duration||0)-(a.duration||0))[0];return v?v.currentTime:null})()";

/// A tab waiting for its page to say where its video is: device, address, title, position.
type Pending = (String, String, String, String);

pub struct Nearby {
    handle: Option<Handle>,
    devices: Vec<DeviceInfo>,
    /// The pairing code on show.
    code: Option<String>,
    status: String,
    tx: Sender<Pending>,
    rx: Receiver<Pending>,
    sync: SyncEngine,
    /// Devices seen online at the last list (a device coming online gets the ledger).
    online: HashSet<String>,
    /// Ledgers waiting for the user's answer, by device: its name and the ledger.
    proposals: HashMap<String, (String, Received)>,
    /// Devices whose proposal was put off ("Later") for this run.
    later: HashSet<String>,
    observe_at: Instant,
    changed_at: Option<Instant>,
}

impl Nearby {
    pub fn new(data: &std::path::Path, private: bool) -> Self {
        let (tx, rx) = channel();
        // Development builds stay on this machine: no firewall prompt while testing.
        let config = if cfg!(feature = "devtools") {
            let mut config = handoff::Config::loopback(data, &device_name());
            // scripts/test-sync.ps1: the Android emulator is reached through `adb emu redir`,
            // which needs other ports than the default one on this machine.
            let port = |name| std::env::var(name).ok().and_then(|p| p.parse::<u16>().ok());
            if let Some(p) = port("BH_HANDOFF_PORT") {
                config.port = p;
            }
            if let Some(p) = port("BH_HANDOFF_PEER_PORT") {
                config.peer_port = p;
            }
            config
        } else {
            handoff::Config::lan(data, &device_name())
        };
        Self {
            handle: (!private).then(|| handoff::spawn(config)),
            devices: Vec::new(),
            code: None,
            status: String::new(),
            tx,
            rx,
            sync: SyncEngine::open(data),
            online: HashSet::new(),
            proposals: HashMap::new(),
            later: HashSet::new(),
            observe_at: Instant::now(),
            changed_at: None,
        }
    }

    fn request(&self, request: Request) {
        if let Some(handle) = &self.handle {
            handle.request(request);
        }
    }

    /// The paired devices, for the tab menu (`state.devices`).
    pub fn menu_json(&self) -> Value {
        json!(self
            .devices
            .iter()
            .map(|d| json!({"id": d.id, "name": d.name, "online": d.online, "sync": d.sync}))
            .collect::<Vec<_>>())
    }
}

/// The name the other device shows ("Continue on …").
#[cfg(windows)]
fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Windows PC".to_owned())
}

#[cfg(target_os = "macos")]
fn device_name() -> String {
    sysinfo::System::host_name()
        .map(|h| h.trim_end_matches(".local").to_owned())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Mac".to_owned())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn device_name() -> String {
    "BlackHole".to_owned()
}

impl Running {
    /// Command bar actions: "Continue on …" for the tab on screen.
    pub(crate) fn continue_actions(&self) -> Vec<Value> {
        let web = self
            .svc
            .workspace()
            .active_tab()
            .is_some_and(|t| can_continue(&t.url));
        if !web || self.home_shown() {
            return Vec::new();
        }
        self.nearby
            .devices
            .iter()
            .map(|d| json!({"id": format!("continue:{}", d.id), "label": tf("palette.continue_on", &[("name", &d.name)]), "hint": t("palette.hint_devices")}))
            .collect()
    }

    /// Sends a tab to a paired device, with the position of its video when it has one.
    pub(crate) fn continue_on(&mut self, tab: u64, device: String) {
        let found = self
            .svc
            .workspace()
            .spaces()
            .iter()
            .flat_map(|s| s.tabs.iter())
            .find(|t| t.id.0 == tab)
            .map(|t| (t.url.clone(), t.title.clone()));
        let Some((url, title)) = found.filter(|(u, _)| can_continue(u)) else {
            self.toast(&t("msg.only_web_continue"));
            return;
        };
        match self.tabs.get(&TabId(tab)).filter(|_| has_position(&url)) {
            Some(view) => {
                let tx = self.nearby.tx.clone();
                let (u, t) = (url.to_string(), title);
                let _ = view.evaluate_script_with_callback(POSITION_JS, move |raw| {
                    let _ = tx.send((device.clone(), u.clone(), t.clone(), raw));
                });
            }
            None => self.nearby.request(Request::Send {
                device,
                url: url.to_string(),
                title,
                position: None,
            }),
        }
    }

    /// Called every turn of the event loop: positions read, then what the network brought.
    pub(crate) fn service_nearby(&mut self) {
        while let Ok((device, url, title, raw)) = self.nearby.rx.try_recv() {
            let position = serde_json::from_str::<Option<f64>>(&raw).ok().flatten();
            self.nearby.request(Request::Send {
                device,
                url,
                title,
                position,
            });
        }
        let mut events = Vec::new();
        if let Some(handle) = &self.nearby.handle {
            while let Some(event) = handle.next_event() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                Event::Devices(list) => {
                    let came: Vec<String> = list
                        .iter()
                        .filter(|d| d.online && d.sync != "off" && !self.nearby.online.contains(&d.id))
                        .map(|d| d.id.clone())
                        .collect();
                    self.nearby.online = list.iter().filter(|d| d.online).map(|d| d.id.clone()).collect();
                    self.nearby.devices = list;
                    for device in came {
                        self.send_ledger(&device, false, false);
                    }
                    self.ui_settings();
                    self.ui_state();
                }
                Event::Snapshot { device, name, json } => self.on_ledger(device, name, &json),
                Event::Code(code) => {
                    self.nearby.code = code;
                    if self.nearby.code.is_some() {
                        self.nearby.status.clear();
                    }
                    self.ui_settings();
                }
                Event::Paired(name) => {
                    self.nearby.status = message("settings.devices_paired_with", &[("name", &name)]);
                    self.toast(&tf("toast.paired", &[("name", &name)]));
                    self.ui_settings();
                }
                Event::Received {
                    from,
                    url,
                    position,
                    ..
                } => {
                    if let Ok(url) = Url::parse(&url) {
                        self.open(resume_url(&url, position).as_str());
                        self.toast(&tf("toast.continued_from", &[("name", &from)]));
                    }
                }
                Event::Delivered(name) => self.toast(&tf("toast.opened_on", &[("name", &name)])),
                Event::Status(status) => {
                    if self.settings.is_none() {
                        self.toast(&status);
                    }
                    self.nearby.status = status;
                    self.ui_settings();
                }
            }
        }
        self.sync_tick();
    }

    /// Tab synchronization, every turn of the event loop: the workspace is compared with the
    /// ledger now and then, and `auto` devices get it once a change has settled.
    fn sync_tick(&mut self) {
        if self.nearby.handle.is_none() || self.nearby.devices.is_empty() {
            return;
        }
        let now = Instant::now();
        if now >= self.nearby.observe_at {
            self.nearby.observe_at = now + OBSERVE_EVERY;
            if self.nearby.sync.observe(&self.svc) {
                self.nearby.changed_at = Some(now);
            }
        }
        if self.nearby.changed_at.is_some_and(|at| at.elapsed() >= SEND_AFTER) {
            self.nearby.changed_at = None;
            let auto: Vec<String> = self
                .nearby
                .devices
                .iter()
                .filter(|d| d.online && d.sync == "auto")
                .map(|d| d.id.clone())
                .collect();
            for device in auto {
                self.send_ledger(&device, false, false);
            }
        }
    }

    fn send_ledger(&mut self, device: &str, reply: bool, pull: bool) {
        self.nearby.sync.observe(&self.svc);
        let json = self.nearby.sync.snapshot(reply, pull);
        self.nearby.request(Request::Snapshot {
            device: device.to_owned(),
            json,
        });
    }

    /// A paired device's ledger arrived: merge it, propose it, or answer with this one.
    fn on_ledger(&mut self, device: String, name: String, json: &str) {
        let Some(received) = SyncEngine::parse(json) else {
            return;
        };
        let mode = self
            .nearby
            .devices
            .iter()
            .find(|d| d.id == device)
            .map_or_else(|| "ask".to_owned(), |d| d.sync.clone());
        let merge = self.nearby.sync.preview(&self.svc, &received);
        match decide(&mode, &received, &merge) {
            Decision::Nothing => {}
            Decision::SendBack => self.send_ledger(&device, true, false),
            Decision::Apply => {
                let quiet = received.reply;
                self.apply_ledger(&device, &received);
                if !quiet {
                    self.toast(&tf("toast.synced_with", &[("name", &name)]));
                }
            }
            Decision::Ask { opens, closes, changes } => {
                if self.nearby.later.contains(&device) {
                    return;
                }
                let mut what = Vec::new();
                if opens > 0 {
                    what.push(tn("toast.sync_opens", opens, &[]));
                }
                if closes > 0 {
                    what.push(tn("toast.sync_closes", closes, &[]));
                }
                if changes > 0 {
                    what.push(tn("toast.sync_changes", changes, &[]));
                }
                let message = tf("toast.sync_ask", &[("name", &name), ("what", &what.join(", "))]);
                self.nearby.proposals.insert(device.clone(), (name, received));
                let run = |answer: &str| json!({"cmd": "sync_answer", "id": device, "answer": answer});
                let actions = json!([
                    {"label": t("toast.sync_merge"), "run": run("merge")},
                    {"label": t("toast.sync_always"), "run": run("always")},
                    {"label": t("toast.sync_later"), "run": run("later")},
                ]);
                self.ui(format!("window.bh&&bh.toast({},{actions})", json!(message)));
            }
        }
    }

    fn apply_ledger(&mut self, device: &str, received: &Received) {
        let (removed, behind) = self.nearby.sync.accept(&mut self.svc, received);
        for id in removed {
            self.drop_view(id);
        }
        self.sync();
        if behind || received.pull {
            self.send_ledger(device, true, false);
        }
    }

    /// Settings > Devices.
    pub(crate) fn devices_json(&self) -> Value {
        json!({
            "available": self.nearby.handle.is_some(),
            "list": self.nearby.menu_json(),
            "code": self.nearby.code,
            "status": crate::i18n::render(&self.nearby.status),
        })
    }

    pub(crate) fn handle_devices(&mut self, command: UiCommand) {
        match command {
            UiCommand::DeviceShowCode => self.nearby.request(Request::ShowCode),
            UiCommand::DeviceJoin { code } => self.nearby.request(Request::Join(code)),
            UiCommand::DeviceCancelPairing => self.nearby.request(Request::CancelPairing),
            UiCommand::DeviceForget { id } => {
                self.nearby.status.clear();
                self.nearby.request(Request::Forget(id));
            }
            UiCommand::ContinueOn { id, device } => self.continue_on(id, device),
            UiCommand::DeviceSync { id, mode } => {
                self.nearby.later.remove(&id);
                self.nearby.request(Request::SetSync { device: id, mode });
            }
            UiCommand::DeviceSyncNow { id } => {
                self.nearby.later.remove(&id);
                self.send_ledger(&id, false, true);
                self.toast(&t("toast.synchronizing"));
            }
            UiCommand::SyncAnswer { id, answer } => {
                let Some((name, received)) = self.nearby.proposals.remove(&id) else {
                    return;
                };
                match answer.as_str() {
                    "merge" | "always" => {
                        if answer == "always" {
                            self.nearby.request(Request::SetSync {
                                device: id.clone(),
                                mode: "auto".into(),
                            });
                        }
                        self.apply_ledger(&id, &received);
                        self.toast(&tf("toast.synced_with", &[("name", &name)]));
                    }
                    _ => {
                        self.nearby.later.insert(id);
                    }
                }
            }
            _ => {}
        }
    }
}

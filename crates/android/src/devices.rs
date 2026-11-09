//! Paired devices on Android: pairing, "Continue on …", tabs received, and tab sync — the
//! same service and rules as the desktop (`blackhole_infrastructure::handoff` and `::sync`,
//! `blackhole_domain::sync`). The pairing keys are sealed with a key the shell keeps in the
//! Android Keystore (`keyring` has no store on Android). Private tabs never travel.

use crate::bridge::{Effect, MobileCommand};
use crate::core::Core;
use blackhole_domain::handoff::{can_continue, resume_url};
use blackhole_domain::TabId;
use blackhole_infrastructure::handoff::{self, DeviceInfo, DeviceStore, Event, Handle, Request};
use blackhole_infrastructure::sync::{decide, Decision, Received, SyncEngine};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};
use url::Url;

const OBSERVE_EVERY: Duration = Duration::from_secs(2);
const SEND_AFTER: Duration = Duration::from_secs(3);

pub(crate) struct Nearby {
    handle: Handle,
    devices: Vec<DeviceInfo>,
    code: Option<String>,
    status: String,
    sync: SyncEngine,
    online: HashSet<String>,
    proposals: HashMap<String, (String, Received)>,
    later: HashSet<String>,
    observe_at: Instant,
    changed_at: Option<Instant>,
}

/// Where the service talks: the local network, or (tests, emulator) a given address.
#[derive(Debug, Clone, Copy)]
pub enum Network {
    Lan,
    /// Listen on `bind`, send to `peer` (loopback tests; the emulator reaching its host).
    Direct { bind: SocketAddr, peer: SocketAddr },
}

impl Nearby {
    fn start(data: &Path, key: [u8; 32], name: &str, network: Network) -> Self {
        let mut config = handoff::Config::lan_with(DeviceStore::with_vault_key(data, key), name);
        if let Network::Direct { bind, peer } = network {
            config.bind = bind.ip();
            config.port = bind.port();
            config.broadcast = peer.ip();
            config.peer_port = peer.port();
        }
        Self {
            handle: handoff::spawn(config),
            devices: Vec::new(),
            code: None,
            status: String::new(),
            sync: SyncEngine::open(data),
            online: HashSet::new(),
            proposals: HashMap::new(),
            later: HashSet::new(),
            observe_at: Instant::now(),
            changed_at: None,
        }
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

impl Core {
    /// Starts the paired-devices service. `key` seals the pairing keys (Android Keystore).
    pub fn enable_devices(&mut self, key: [u8; 32], name: &str, network: Network) {
        if self.nearby.is_none() {
            self.nearby = Some(Nearby::start(&self.data, key, name, network));
        }
    }

    fn request(&self, request: Request) {
        if let Some(n) = &self.nearby {
            n.handle.request(request);
        }
    }

    /// Settings > Devices, in the state of the interface.
    pub(crate) fn devices_json(&self) -> Value {
        match &self.nearby {
            None => json!({"available": false, "list": []}),
            Some(n) => json!({
                "available": true,
                "list": n.devices.iter().map(|d| json!({"id": d.id, "name": d.name, "online": d.online, "sync": d.sync})).collect::<Vec<_>>(),
                "code": n.code,
                "status": n.status,
            }),
        }
    }

    pub(crate) fn device_command(&mut self, command: MobileCommand) -> Vec<Effect> {
        use MobileCommand as C;
        if self.nearby.is_none() {
            return vec![Self::toast("Devices are not available", false)];
        }
        match command {
            C::DeviceShowCode => self.request(Request::ShowCode),
            C::DeviceJoin { code } => self.request(Request::Join(code)),
            C::DeviceCancelPairing => self.request(Request::CancelPairing),
            C::DeviceForget { id } => {
                if let Some(n) = &mut self.nearby {
                    n.status.clear();
                }
                self.request(Request::Forget(id));
            }
            C::DeviceSync { id, mode } => {
                if let Some(n) = &mut self.nearby {
                    n.later.remove(&id);
                }
                self.request(Request::SetSync { device: id, mode });
            }
            C::DeviceSyncNow { id } => {
                if let Some(n) = &mut self.nearby {
                    n.later.remove(&id);
                }
                self.send_ledger(&id, false, true);
                return vec![Self::toast("Synchronizing…", false)];
            }
            C::SyncAnswer { id, answer } => return self.sync_answer(&id, &answer),
            C::ContinueOn { device } => return self.continue_on(device),
            _ => {}
        }
        vec![self.state()]
    }

    fn continue_on(&mut self, device: String) -> Vec<Effect> {
        let Some(tab) = self.current.filter(|t| !Self::is_private(*t)) else {
            return vec![Self::toast("Private tabs stay on this device", false)];
        };
        let Some(url) = self.current_page().filter(can_continue) else {
            return vec![Self::toast("Only web pages can continue on another device", false)];
        };
        let title = self.pages.get(&tab).map(|p| p.1.clone()).unwrap_or_default();
        self.request(Request::Send {
            device,
            url: url.to_string(),
            title,
            position: None,
        });
        Vec::new()
    }

    fn sync_answer(&mut self, id: &str, answer: &str) -> Vec<Effect> {
        let Some(n) = &mut self.nearby else { return Vec::new() };
        let Some((name, received)) = n.proposals.remove(id) else {
            return Vec::new();
        };
        if answer == "later" {
            n.later.insert(id.to_owned());
            return Vec::new();
        }
        if answer == "always" {
            self.request(Request::SetSync {
                device: id.to_owned(),
                mode: "auto".into(),
            });
        }
        let mut out = self.apply_ledger(id, &received);
        out.push(Self::toast(&format!("Tabs synchronized with {name}"), false));
        out
    }

    fn send_ledger(&mut self, device: &str, reply: bool, pull: bool) {
        let Some(n) = &mut self.nearby else { return };
        n.sync.observe(&self.svc);
        let json = n.sync.snapshot(reply, pull);
        n.handle.request(Request::Snapshot {
            device: device.to_owned(),
            json,
        });
    }

    /// Merges a received ledger; the views of tabs closed by it go, and the screen moves on
    /// when the shown tab was one of them.
    fn apply_ledger(&mut self, device: &str, received: &Received) -> Vec<Effect> {
        let Some(n) = &mut self.nearby else { return Vec::new() };
        let (removed, behind) = n.sync.accept(&mut self.svc, received);
        let mut out = Vec::new();
        for TabId(tab) in removed {
            self.live.remove(tab);
            self.pages.remove(&tab);
            self.errors.remove(&tab);
            out.push(Effect::Drop { tab });
            if self.current == Some(tab) {
                let next = self.svc.workspace().active_tab().map(|t| t.id.0);
                out.extend(match next {
                    Some(next) => self.show(next, None),
                    None => self.home(),
                });
            }
        }
        if behind || received.pull {
            self.send_ledger(device, true, false);
        }
        out.push(self.state());
        out
    }

    fn on_ledger(&mut self, device: String, name: String, json: &str) -> Vec<Effect> {
        let Some(received) = SyncEngine::parse(json) else {
            return Vec::new();
        };
        let Some(n) = &mut self.nearby else { return Vec::new() };
        let mode = n
            .devices
            .iter()
            .find(|d| d.id == device)
            .map_or_else(|| "ask".to_owned(), |d| d.sync.clone());
        let merge = n.sync.preview(&self.svc, &received);
        match decide(&mode, &received, &merge) {
            Decision::Nothing => Vec::new(),
            Decision::SendBack => {
                self.send_ledger(&device, true, false);
                Vec::new()
            }
            Decision::Apply => {
                let quiet = received.reply;
                let mut out = self.apply_ledger(&device, &received);
                if !quiet {
                    out.push(Self::toast(&format!("Tabs synchronized with {name}"), false));
                }
                out
            }
            Decision::Ask { opens, closes, changes } => {
                if n.later.contains(&device) {
                    return Vec::new();
                }
                let mut what = Vec::new();
                if opens > 0 {
                    what.push(format!("{} to open", plural(opens, "tab")));
                }
                if closes > 0 {
                    what.push(format!("{closes} to close"));
                }
                if changes > 0 {
                    what.push(plural(changes, "change"));
                }
                n.proposals.insert(device.clone(), (name.clone(), received));
                vec![Effect::Ui {
                    msg: json!({"type": "sync_offer", "id": device, "name": name, "text": what.join(", ")}),
                }]
            }
        }
    }

    /// What the network brought since the last call, and the sync of local changes.
    pub(crate) fn tick(&mut self) -> Vec<Effect> {
        let Some(n) = &mut self.nearby else { return Vec::new() };
        let mut events = Vec::new();
        while let Some(event) = n.handle.next_event() {
            events.push(event);
        }
        let mut out = Vec::new();
        let mut state = false;
        for event in events {
            match event {
                Event::Devices(list) => {
                    let Some(n) = &mut self.nearby else { break };
                    let came: Vec<String> = list
                        .iter()
                        .filter(|d| d.online && d.sync != "off" && !n.online.contains(&d.id))
                        .map(|d| d.id.clone())
                        .collect();
                    n.online = list.iter().filter(|d| d.online).map(|d| d.id.clone()).collect();
                    n.devices = list;
                    for device in came {
                        self.send_ledger(&device, false, false);
                    }
                    state = true;
                }
                Event::Code(code) => {
                    if let Some(n) = &mut self.nearby {
                        if code.is_some() {
                            n.status.clear();
                        }
                        n.code = code;
                    }
                    state = true;
                }
                Event::Paired(name) => {
                    if let Some(n) = &mut self.nearby {
                        n.status = format!("Paired with {name}");
                    }
                    out.push(Self::toast(&format!("Paired with {name}"), false));
                    state = true;
                }
                Event::Received { from, url, position, .. } => {
                    if let Ok(url) = Url::parse(&url).map(|u| resume_url(&u, position)) {
                        out.extend(self.navigate(url.as_str(), true, false));
                        out.push(Self::toast(&format!("Continued from {from}"), false));
                    }
                }
                Event::Delivered(name) => out.push(Self::toast(&format!("Opened on {name}"), false)),
                Event::Snapshot { device, name, json } => out.extend(self.on_ledger(device, name, &json)),
                Event::Status(status) => {
                    out.push(Self::toast(&status, false));
                    if let Some(n) = &mut self.nearby {
                        n.status = status;
                    }
                    state = true;
                }
            }
        }
        // Local changes: observed now and then, sent to `auto` devices once settled.
        if let Some(n) = &mut self.nearby {
            let now = Instant::now();
            if !n.devices.is_empty() && now >= n.observe_at {
                n.observe_at = now + OBSERVE_EVERY;
                if n.sync.observe(&self.svc) {
                    n.changed_at = Some(now);
                }
            }
            if n.changed_at.is_some_and(|at| at.elapsed() >= SEND_AFTER) {
                n.changed_at = None;
                let auto: Vec<String> = n
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
        if state {
            out.push(self.state());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::NativeEvent;
    use blackhole_domain::PrivacyPolicy;
    use std::net::UdpSocket;

    fn port() -> u16 {
        UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    fn core(dir: &Path, name: &str, me: u16, peer: u16) -> Core {
        let mut c = Core::with_policy(dir, PrivacyPolicy::default());
        let addr = |p| SocketAddr::from(([127, 0, 0, 1], p));
        c.enable_devices([7; 32], name, Network::Direct { bind: addr(me), peer: addr(peer) });
        c
    }

    /// Ticks both cores until `wanted` matches an effect of `on` (a UI message, mostly).
    fn until(a: &mut Core, b: &mut Core, on_b: bool, mut wanted: impl FnMut(&Effect) -> bool) -> Vec<Effect> {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let ea = a.event(NativeEvent::Tick);
            let eb = b.event(NativeEvent::Tick);
            let mine = if on_b { eb } else { ea };
            if mine.iter().any(&mut wanted) {
                return mine;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("not reached");
    }

    /// Ticks both cores until `done` holds for the first one.
    fn settle(a: &mut Core, b: &mut Core, done: impl Fn(&Core) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(a) {
            assert!(Instant::now() < deadline, "not settled");
            a.event(NativeEvent::Tick);
            b.event(NativeEvent::Tick);
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn ui_type(e: &Effect, kind: &str) -> bool {
        matches!(e, Effect::Ui { msg } if msg["type"] == kind)
    }

    #[test]
    fn a_phone_pairs_continues_a_tab_and_syncs_after_asking() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (pa, pb) = (port(), port());
        let mut phone = core(da.path(), "Pixel", pa, pb);
        let mut pc = core(db.path(), "Desk", pb, pa);
        phone.command(MobileCommand::Navigate { input: "https://phone.example/".into(), new_tab: true, private: false });
        pc.command(MobileCommand::Navigate { input: "https://desk.example/".into(), new_tab: true, private: false });

        pc.command(MobileCommand::DeviceShowCode);
        let out = until(&mut phone, &mut pc, true, |e| matches!(e, Effect::Ui { msg } if msg["devices"]["code"].is_string()));
        let code = out.iter().find_map(|e| match e {
            Effect::Ui { msg } if msg["devices"]["code"].is_string() => msg["devices"]["code"].as_str().map(str::to_owned),
            _ => None,
        }).unwrap();
        phone.command(MobileCommand::DeviceJoin { code });

        // Paired and online: each side gets the other's tabs, and asks (default mode).
        let offer = until(&mut phone, &mut pc, false, |e| ui_type(e, "sync_offer"));
        let Some(Effect::Ui { msg }) = offer.iter().find(|e| ui_type(e, "sync_offer")) else { unreachable!() };
        assert_eq!(msg["name"], "Desk");
        assert_eq!(msg["text"], "1 tab to open");
        let id = msg["id"].as_str().unwrap().to_owned();

        let merged = phone.command(MobileCommand::SyncAnswer { id: id.clone(), answer: "always".into() });
        let Some(Effect::Ui { msg: state }) = merged.iter().find(|e| ui_type(e, "state")) else { unreachable!() };
        let urls: Vec<&str> = state["tabs"].as_array().unwrap().iter().map(|t| t["url"].as_str().unwrap()).collect();
        assert!(urls.contains(&"https://desk.example/") && urls.contains(&"https://phone.example/"));

        // The answer travels back: the desk applies it without asking (the user agreed).
        until(&mut phone, &mut pc, true, |e| {
            matches!(e, Effect::Ui { msg } if msg["type"] == "state" && msg["tabs"].as_array().is_some_and(|t| t.len() == 2))
        });
        settle(&mut phone, &mut pc, |p| p.devices_json()["list"][0]["sync"] == "auto");

        // Continue on: the shown tab opens on the desk.
        settle(&mut phone, &mut pc, |p| p.devices_json()["list"][0]["online"] == true);
        phone.command(MobileCommand::Navigate { input: "https://read.example/later".into(), new_tab: true, private: false });
        phone.command(MobileCommand::ContinueOn { device: id });
        let got = until(&mut phone, &mut pc, true, |e| matches!(e, Effect::Ui { msg } if msg["type"] == "toast" && msg["text"] == "Continued from Pixel"));
        assert!(got.iter().any(|e| matches!(e, Effect::Open { url, .. } if url == "https://read.example/later")));
    }

    #[test]
    fn private_tabs_do_not_continue_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = core(dir.path(), "Pixel", port(), port());
        c.command(MobileCommand::Navigate { input: "https://secret.example/".into(), new_tab: true, private: true });
        let out = c.command(MobileCommand::ContinueOn { device: "x".into() });
        assert!(matches!(&out[..], [Effect::Ui { msg }] if msg["text"] == "Private tabs stay on this device"));
    }
}

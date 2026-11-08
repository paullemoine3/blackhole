//! "Continue on another device": Settings > Devices pairs this BlackHole with another one on the
//! local network; the tab menu and the command bar send a tab there, which opens it. The network,
//! the keys and the message format live in `blackhole_infrastructure::handoff`; this side only
//! reads the video position of the tab and shows the results. Not started in private windows.

use crate::{ipc::UiCommand, Running, TabId};
use blackhole_domain::handoff::{can_continue, has_position, resume_url};
use blackhole_infrastructure::handoff::{self, DeviceInfo, Event, Handle, Request};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver, Sender};
use url::Url;

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
}

impl Nearby {
    pub fn new(data: &std::path::Path, private: bool) -> Self {
        let (tx, rx) = channel();
        // Development builds stay on this machine: no firewall prompt while testing.
        let config = if cfg!(feature = "devtools") {
            handoff::Config::loopback(data, &device_name())
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
            .map(|d| json!({"id": d.id, "name": d.name, "online": d.online}))
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
            .map(|d| json!({"id": format!("continue:{}", d.id), "label": format!("Continue on {}", d.name), "hint": "Devices"}))
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
            self.toast("Only web pages can continue on another device");
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
                    self.nearby.devices = list;
                    self.ui_settings();
                    self.ui_state();
                }
                Event::Code(code) => {
                    self.nearby.code = code;
                    if self.nearby.code.is_some() {
                        self.nearby.status.clear();
                    }
                    self.ui_settings();
                }
                Event::Paired(name) => {
                    self.nearby.status = format!("Paired with {name}");
                    self.toast(&format!("Paired with {name}: tabs can now continue on it"));
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
                        self.toast(&format!("Continued from {from}"));
                    }
                }
                Event::Delivered(name) => self.toast(&format!("Opened on {name}")),
                Event::Status(status) => {
                    if self.settings.is_none() {
                        self.toast(&status);
                    }
                    self.nearby.status = status;
                    self.ui_settings();
                }
            }
        }
    }

    /// Settings > Devices.
    pub(crate) fn devices_json(&self) -> Value {
        json!({
            "available": self.nearby.handle.is_some(),
            "list": self.nearby.menu_json(),
            "code": self.nearby.code,
            "status": self.nearby.status,
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
            _ => {}
        }
    }
}

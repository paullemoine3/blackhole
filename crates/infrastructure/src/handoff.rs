//! "Continue on another device" over the local network, without a server or an account.
//!
//! Pairing: one device shows a 60-bit code (see `blackhole_domain::handoff`), the user types it
//! on the other. Both stretch it into a temporary key, exchange a random half each under it,
//! and keep `SHA-256(code key, both halves)` as the key of the pair, sealed in `devices.bin`
//! (same vault key as the passwords). The code itself is never stored nor sent.
//!
//! Afterwards every datagram is ChaCha20-Poly1305 sealed with the key of the pair and carries
//! its sender, its time and an identifier (stale or replayed copies are dropped). Devices find
//! each other with an encrypted hello broadcast on the local network, then talk directly.
//! Nothing listens while no device is paired and no pairing is in progress: the port (and the
//! firewall prompt of Windows and macOS) only comes with the first pairing.
//!
//! The network runs on a thread of its own; the app sends [`Request`]s and reads [`Event`]s.

use blackhole_domain::i18n::message;
use crate::store::Vault;
use crate::wormhole::{Done, Inbound, Outbound};
use blackhole_domain::handoff::{self, Seen};
use blackhole_domain::wormhole::{self as beam_rules, Kind};
use chacha20poly1305::{
    aead::{rand_core::RngCore, Aead, AeadCore, KeyInit, OsRng, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The UDP port of BlackHole on the local network (both devices use the same).
pub const PORT: u16 = 47_807;
/// Encrypted list of the paired devices and their keys.
const FILE: &str = "devices.bin";
const MAGIC: &[u8; 3] = b"BH1";
/// Second byte after the magic: sealed with the key of a pair, or with a pairing code.
const PAIRED: u8 = 0;
const PAIRING: u8 = 1;
const MAX_PACKET: usize = 16 * 1024;
const MAX_URL: usize = 8 * 1024;
const MAX_TITLE: usize = 300;
const MAX_NAME: usize = 64;
/// A synced ledger travels in parts of this many bytes (a packet stays under `MAX_PACKET`
/// even if every character needed escaping), at most `MAX_PARTS` of them.
const PART_BYTES: usize = 6_000;
const MAX_PARTS: usize = 100;
/// Parts of a ledger not completed within this time are dropped.
const PARTS_FOR: Duration = Duration::from_secs(60);
/// Rounds of SHA-256 turning a pairing code into its key: guessing the 60-bit code from a
/// recorded pairing costs as much again for every guess.
const STRETCH: u32 = 100_000;
/// How long a shown code stays valid, and how long the other device tries it.
const SHOW_FOR: Duration = Duration::from_secs(5 * 60);
const JOIN_FOR: Duration = Duration::from_secs(20);
const JOIN_EVERY: Duration = Duration::from_secs(1);
/// A tab is sent again until the other device confirms, this many times.
const SEND_TRIES: u32 = 6;
const SEND_EVERY: Duration = Duration::from_millis(600);
const HELLO_EVERY: Duration = Duration::from_secs(30);
/// A device not heard of for this long is shown as away.
const ONLINE_FOR: Duration = Duration::from_secs(95);
const TICK: Duration = Duration::from_millis(100);
/// Wormhole: parts of a text or a file on their way at once (each is confirmed), how long an
/// offer waits for its answer, and how long a transfer may stall.
const BEAM_WINDOW: usize = 16;
const BEAM_OFFER_FOR: Duration = Duration::from_secs(120);
const BEAM_STALL: Duration = Duration::from_secs(60);

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0; N];
    OsRng.fill_bytes(&mut out);
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != 2 * N {
        return None;
    }
    let mut out = [0; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

fn bounded(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The key of a pairing code (the same on both devices).
#[must_use]
pub fn code_key(code: u64) -> [u8; 32] {
    let seed = Sha256::new()
        .chain_update(b"BlackHole pairing v1")
        .chain_update(code.to_be_bytes())
        .finalize();
    let mut key: [u8; 32] = seed.into();
    for _ in 0..STRETCH {
        key = Sha256::new()
            .chain_update(key)
            .chain_update(code.to_be_bytes())
            .finalize()
            .into();
    }
    key
}

/// The key a pair keeps: both random halves under the code's key.
fn pair_key(code: &[u8; 32], shower: &[u8; 32], joiner: &[u8; 32]) -> [u8; 32] {
    Sha256::new()
        .chain_update(b"BlackHole device v1")
        .chain_update(code)
        .chain_update(shower)
        .chain_update(joiner)
        .finalize()
        .into()
}

/// A paired device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    key: String,
    #[serde(default)]
    pub paired_at: i64,
    /// Synchronization with this device: `ask` (default), `auto` or `off`.
    #[serde(default = "ask")]
    pub sync: String,
}

fn ask() -> String {
    "ask".into()
}

/// The synchronization modes a device can have.
pub const SYNC_MODES: [&str; 3] = ["ask", "auto", "off"];

impl Device {
    fn key(&self) -> Option<[u8; 32]> {
        unhex(&self.key)
    }
}

/// What `devices.bin` holds: this device's identifier and the devices paired with it.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Book {
    #[serde(default)]
    me: String,
    #[serde(default)]
    devices: Vec<Device>,
}

pub struct DeviceStore(Vault);

impl DeviceStore {
    pub fn new(data: &std::path::Path) -> Self {
        Self(Vault::named(data, FILE))
    }

    /// Sealed with a key given by the platform (Android: kept by the Android Keystore), where
    /// the system keychain is not available.
    pub fn with_vault_key(data: &std::path::Path, key: [u8; 32]) -> Self {
        Self(Vault::named_with_key(data, FILE, key))
    }

    #[cfg(test)]
    fn with_key(data: &std::path::Path) -> Self {
        Self::with_vault_key(data, [9; 32])
    }

    fn load(&self) -> Result<Book, String> {
        self.0.read_json()
    }

    fn save(&self, book: &Book) -> Result<(), String> {
        self.0.write_json(book)
    }
}

/// What travels, sealed. `from` is the sender's identifier, `at` its clock (Unix seconds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Envelope {
    from: String,
    at: i64,
    #[serde(flatten)]
    message: Message,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Message {
    /// "I am here" (and, with `reply`, "where are you?").
    Hello {
        reply: bool,
    },
    /// A tab to open: its address only, and the playback position of its video.
    Tab {
        id: u64,
        url: String,
        title: String,
        #[serde(default)]
        position: Option<f64>,
    },
    Ack {
        id: u64,
    },
    /// A part of a synced ledger (`snap` identifies the ledger, `id` this part for its `Ack`).
    Part {
        snap: u64,
        id: u64,
        part: u16,
        parts: u16,
        data: String,
    },
    /// Wormhole: announces a text or a file (`kind`) of `size` bytes in `parts` parts, with
    /// its SHA-256. `id` names the transfer.
    Beam {
        id: u64,
        kind: String,
        name: String,
        size: u64,
        parts: u32,
        sha: String,
    },
    /// The answer to a `Beam`: send it, or do not.
    BeamReply {
        id: u64,
        beam: u64,
        ok: bool,
    },
    /// A part of the transfer `beam` (hexadecimal).
    Chunk {
        id: u64,
        beam: u64,
        part: u32,
        data: String,
    },
    /// The sender forgot this device: forget it too.
    Unpair,
    /// Pairing, sealed with the code's key: the device typing the code, then the one showing it.
    Offer {
        name: String,
        half: String,
    },
    Accept {
        name: String,
        half: String,
    },
}

fn seal(key: &[u8; 32], kind: u8, envelope: &Envelope) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let header = [MAGIC[0], MAGIC[1], MAGIC[2], kind];
    let plain = serde_json::to_vec(envelope).unwrap_or_default();
    let mut packet = header.to_vec();
    packet.extend_from_slice(&nonce);
    if let Ok(sealed) = cipher.encrypt(
        &nonce,
        Payload {
            msg: &plain,
            aad: &header,
        },
    ) {
        packet.extend(sealed);
    }
    packet
}

/// The kind of a packet (`PAIRED` or `PAIRING`), when it looks like one of ours.
fn kind_of(packet: &[u8]) -> Option<u8> {
    (packet.len() > 16 && packet[..3] == MAGIC[..]).then(|| packet[3])
}

fn open(key: &[u8; 32], packet: &[u8]) -> Option<Envelope> {
    kind_of(packet)?;
    let (header, rest) = packet.split_at(4);
    let (nonce, sealed) = rest.split_at(12);
    let plain = ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: sealed,
                aad: header,
            },
        )
        .ok()?;
    serde_json::from_slice(&plain).ok()
}

/// Where the service listens and whom it talks to.
pub struct Config {
    pub store: DeviceStore,
    /// This device's name, as the other one shows it ("Continue on …").
    pub name: String,
    pub bind: IpAddr,
    pub port: u16,
    pub peer_port: u16,
    pub broadcast: IpAddr,
    /// Where received files are saved.
    pub downloads: std::path::PathBuf,
}

impl Config {
    /// The local network: listen on every interface, broadcast to the whole subnet.
    pub fn lan(data: &std::path::Path, name: &str) -> Self {
        Self::on(
            DeviceStore::new(data),
            name,
            Ipv4Addr::UNSPECIFIED.into(),
            Ipv4Addr::BROADCAST.into(),
        )
    }

    /// Only this machine (development builds: no firewall prompt, nothing leaves it).
    pub fn loopback(data: &std::path::Path, name: &str) -> Self {
        Self::on(
            DeviceStore::new(data),
            name,
            Ipv4Addr::LOCALHOST.into(),
            Ipv4Addr::LOCALHOST.into(),
        )
    }

    /// The local network, with devices sealed by `store` (see `DeviceStore::with_vault_key`).
    pub fn lan_with(store: DeviceStore, name: &str) -> Self {
        Self::on(store, name, Ipv4Addr::UNSPECIFIED.into(), Ipv4Addr::BROADCAST.into())
    }

    fn on(store: DeviceStore, name: &str, bind: IpAddr, broadcast: IpAddr) -> Self {
        Self {
            store,
            name: bounded(name, MAX_NAME),
            bind,
            port: PORT,
            peer_port: PORT,
            broadcast,
            downloads: crate::downloads_dir(),
        }
    }
}

pub enum Request {
    /// Shows a pairing code (and listens for the device that types it).
    ShowCode,
    /// The code shown by the other device.
    Join(String),
    CancelPairing,
    Send {
        device: String,
        url: String,
        title: String,
        position: Option<f64>,
    },
    Forget(String),
    /// `ask`, `auto` or `off` for a device.
    SetSync {
        device: String,
        mode: String,
    },
    /// This device's synced ledger (JSON), for a paired device.
    Snapshot {
        device: String,
        json: String,
    },
    /// Wormhole: sends a text or a file to a paired device.
    Beam {
        device: String,
        payload: Cargo,
    },
    /// Wormhole: the user's answer to a file offered by a device (`Event::BeamOffer`).
    BeamAnswer {
        device: String,
        beam: u64,
        accept: bool,
    },
}

/// What a Wormhole carries.
pub enum Cargo {
    Text(String),
    File(std::path::PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub sync: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Devices(Vec<DeviceInfo>),
    /// The code to show (`None` once the pairing is over).
    Code(Option<String>),
    Paired(String),
    Received {
        from: String,
        url: String,
        title: String,
        position: Option<f64>,
    },
    Delivered(String),
    /// The synced ledger of a paired device (JSON, untrusted: parse it with care).
    Snapshot {
        device: String,
        name: String,
        json: String,
    },
    /// Wormhole: a device offers a file; answer with `Request::BeamAnswer`.
    BeamOffer {
        device: String,
        from: String,
        beam: u64,
        name: String,
        size: u64,
    },
    /// Wormhole: a text arrived (cleaned and bounded; still untrusted content).
    BeamText {
        from: String,
        text: String,
    },
    /// Wormhole: a file arrived, checked, and was saved at `path`.
    BeamFile {
        from: String,
        name: String,
        path: std::path::PathBuf,
    },
    /// Wormhole: the other device has all of what was sent (`name` is empty for a text).
    BeamSent {
        to: String,
        name: String,
    },
    /// Something to tell the user (an error, mostly).
    Status(String),
}

/// The app's side of the service.
pub struct Handle {
    tx: Sender<Request>,
    rx: Receiver<Event>,
}

impl Handle {
    pub fn request(&self, request: Request) {
        let _ = self.tx.send(request);
    }

    pub fn next_event(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}

/// Starts the service (its thread ends with the [`Handle`]).
pub fn spawn(config: Config) -> Handle {
    let (tx, requests) = channel();
    let (events, rx) = channel();
    std::thread::Builder::new()
        .name("handoff".into())
        .spawn(move || Worker::new(config, events).run(&requests))
        .ok();
    Handle { tx, rx }
}

enum Pairing {
    Showing {
        key: [u8; 32],
        until: Instant,
        /// Answers already given, by the other half: sent again if the offer comes again.
        answered: Vec<(String, Vec<u8>)>,
    },
    Joining {
        key: [u8; 32],
        half: [u8; 32],
        packet: Vec<u8>,
        next: Instant,
        until: Instant,
    },
}

struct Outgoing {
    id: u64,
    device: String,
    packet: Vec<u8>,
    tries: u32,
    next: Instant,
    /// A part of a ledger: no "delivered" nor "did not answer" for it.
    quiet: bool,
    /// What it carries of a Wormhole transfer, if anything.
    tag: Tag,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    None,
    /// The announcement of a transfer, or the answer to one.
    Beam(u64),
    /// A part of a transfer.
    Chunk(u64),
}

/// A file a device offered, waiting for the user's answer.
struct Offer {
    device: String,
    beam: u64,
    name: String,
    size: u64,
    sha: String,
    since: Instant,
}

/// A ledger arriving in parts.
struct Incoming {
    parts: Vec<Option<String>>,
    since: Instant,
}

struct Worker {
    store: DeviceStore,
    name: String,
    bind: IpAddr,
    port: u16,
    peer_port: u16,
    broadcast: IpAddr,
    book: Book,
    socket: Option<UdpSocket>,
    pairing: Option<Pairing>,
    outgoing: Vec<Outgoing>,
    seen: Seen,
    /// Where each device was last heard from, and when.
    heard: HashMap<String, (IpAddr, Instant)>,
    hello_due: Instant,
    /// After a failed bind, when to try again (the error is told once).
    bind_retry: Option<Instant>,
    shown: Option<Vec<DeviceInfo>>,
    incoming: HashMap<(String, u64), Incoming>,
    /// Wormhole: what is being sent, what is being received, the files offered.
    beams_out: Vec<Outbound>,
    beams_in: Vec<Inbound>,
    offers: Vec<Offer>,
    downloads: std::path::PathBuf,
    events: Sender<Event>,
}

impl Worker {
    fn new(config: Config, events: Sender<Event>) -> Self {
        Self {
            store: config.store,
            name: config.name,
            bind: config.bind,
            port: config.port,
            peer_port: config.peer_port,
            broadcast: config.broadcast,
            book: Book::default(),
            socket: None,
            pairing: None,
            outgoing: Vec::new(),
            seen: Seen::default(),
            heard: HashMap::new(),
            hello_due: Instant::now(),
            bind_retry: None,
            shown: None,
            incoming: HashMap::new(),
            beams_out: Vec::new(),
            beams_in: Vec::new(),
            offers: Vec::new(),
            downloads: config.downloads,
            events,
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn run(mut self, requests: &Receiver<Request>) {
        match self.store.load() {
            Ok(book) => self.book = book,
            Err(why) => self.emit(Event::Status(why)),
        }
        self.devices_changed();
        loop {
            self.update_socket();
            let request = if self.socket.is_some() {
                match requests.try_recv() {
                    Ok(r) => Some(r),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match requests.recv_timeout(Duration::from_secs(1)) {
                    Ok(r) => Some(r),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            };
            if let Some(request) = request {
                self.handle(request);
                continue;
            }
            if let Some(socket) = &self.socket {
                let mut buf = vec![0; MAX_PACKET];
                if let Ok((n, from)) = socket.recv_from(&mut buf) {
                    buf.truncate(n);
                    self.receive(&buf, from);
                }
            }
            self.tick();
        }
    }

    /// Opens the port when something needs it, closes it when nothing does any more.
    fn update_socket(&mut self) {
        let needed = !self.book.devices.is_empty() || self.pairing.is_some();
        if !needed {
            self.socket = None;
            return;
        }
        if self.socket.is_some() || self.bind_retry.is_some_and(|at| Instant::now() < at) {
            return;
        }
        // Another BlackHole window may own the port: it receives; this one can still send
        // (the answers come back to its own port).
        let socket =
            UdpSocket::bind((self.bind, self.port)).or_else(|_| UdpSocket::bind((self.bind, 0)));
        match socket {
            Ok(socket) => {
                let _ = socket.set_broadcast(true);
                let _ = socket.set_read_timeout(Some(TICK));
                self.socket = Some(socket);
                self.bind_retry = None;
                self.hello_due = Instant::now() + HELLO_EVERY;
                self.hello(true);
            }
            Err(why) => {
                if self.bind_retry.is_none() {
                    self.emit(Event::Status(message(
                        "msg.no_local_network",
                        &[("why", &why.to_string())],
                    )));
                }
                self.bind_retry = Some(Instant::now() + HELLO_EVERY);
                self.pairing = None;
            }
        }
    }

    fn listening(&self) -> bool {
        self.socket
            .as_ref()
            .and_then(|s| s.local_addr().ok())
            .is_some_and(|a| a.port() == self.port)
    }

    fn send(&self, packet: &[u8], to: SocketAddr) {
        if let Some(socket) = &self.socket {
            let _ = socket.send_to(packet, to);
        }
    }

    /// To where the device was last heard from, or to the whole network.
    fn send_to_device(&self, packet: &[u8], device: &str) {
        let ip = self.heard.get(device).map_or(self.broadcast, |(ip, _)| *ip);
        self.send(packet, SocketAddr::new(ip, self.peer_port));
    }

    fn envelope(&self, message: Message) -> Envelope {
        Envelope {
            from: self.book.me.clone(),
            at: now_secs(),
            message,
        }
    }

    fn hello(&self, reply: bool) {
        for device in &self.book.devices {
            if let Some(key) = device.key() {
                let packet = seal(&key, PAIRED, &self.envelope(Message::Hello { reply }));
                self.send(&packet, SocketAddr::new(self.broadcast, self.peer_port));
            }
        }
    }

    fn save(&mut self) {
        if let Err(why) = self.store.save(&self.book) {
            self.emit(Event::Status(why));
        }
    }

    fn devices_changed(&mut self) {
        let list: Vec<DeviceInfo> = self
            .book
            .devices
            .iter()
            .map(|d| DeviceInfo {
                id: d.id.clone(),
                name: d.name.clone(),
                online: self
                    .heard
                    .get(&d.id)
                    .is_some_and(|(_, at)| at.elapsed() < ONLINE_FOR),
                sync: d.sync.clone(),
            })
            .collect();
        if self.shown.as_ref() != Some(&list) {
            self.shown = Some(list.clone());
            self.emit(Event::Devices(list));
        }
    }

    fn remember(&mut self, id: &str, name: &str, key: [u8; 32]) {
        self.book.devices.retain(|d| d.id != id);
        self.book.devices.push(Device {
            id: id.to_owned(),
            name: bounded(name, MAX_NAME),
            key: hex(&key),
            paired_at: now_secs(),
            sync: ask(),
        });
        self.save();
        self.emit(Event::Paired(bounded(name, MAX_NAME)));
        self.devices_changed();
    }

    fn handle(&mut self, request: Request) {
        if self.book.me.is_empty() && matches!(request, Request::ShowCode | Request::Join(_)) {
            self.book.me = hex(&random_bytes::<16>());
        }
        match request {
            Request::ShowCode => {
                let code = handoff::code_from_random(u64::from_le_bytes(random_bytes()));
                self.pairing = Some(Pairing::Showing {
                    key: code_key(code),
                    until: Instant::now() + SHOW_FOR,
                    answered: Vec::new(),
                });
                self.update_socket();
                if self.pairing.is_none() {
                    return;
                }
                if !self.listening() {
                    self.pairing = None;
                    self.emit(Event::Status(message("msg.other_window_pairs", &[])));
                    return;
                }
                self.emit(Event::Code(Some(handoff::format_code(code))));
            }
            Request::Join(typed) => {
                let Some(code) = handoff::parse_code(&typed) else {
                    self.emit(Event::Status(message("msg.type_code", &[])));
                    return;
                };
                let key = code_key(code);
                let half = random_bytes::<32>();
                let offer = Message::Offer {
                    name: self.name.clone(),
                    half: hex(&half),
                };
                let packet = seal(&key, PAIRING, &self.envelope(offer));
                self.pairing = Some(Pairing::Joining {
                    key,
                    half,
                    packet,
                    next: Instant::now(),
                    until: Instant::now() + JOIN_FOR,
                });
                self.emit(Event::Code(None));
                self.emit(Event::Status(message("msg.looking_for_device", &[])));
            }
            Request::CancelPairing => {
                self.pairing = None;
                self.emit(Event::Code(None));
            }
            Request::Send {
                device,
                url,
                title,
                position,
            } => {
                let Some(target) = self.book.devices.iter().find(|d| d.id == device).cloned()
                else {
                    self.emit(Event::Status(message("msg.device_unpaired", &[])));
                    return;
                };
                let can = url::Url::parse(&url).is_ok_and(|u| handoff::can_continue(&u));
                let Some(key) = target.key().filter(|_| can && url.len() <= MAX_URL) else {
                    self.emit(Event::Status(message("msg.only_web_continue", &[])));
                    return;
                };
                let id = u64::from_le_bytes(random_bytes());
                let tab = Message::Tab {
                    id,
                    url,
                    title: bounded(&title, MAX_TITLE),
                    position: position.filter(|p| p.is_finite() && *p >= 0.0),
                };
                let packet = seal(&key, PAIRED, &self.envelope(tab));
                self.outgoing.push(Outgoing {
                    id,
                    device,
                    packet,
                    tries: 0,
                    next: Instant::now(),
                    quiet: false,
                    tag: Tag::None,
                });
            }
            Request::Beam { device, payload } => {
                let Some(key) = self.key_of(&device) else {
                    self.emit(Event::Status(message("msg.device_unpaired", &[])));
                    return;
                };
                let id = u64::from_le_bytes(random_bytes());
                let made = match &payload {
                    Cargo::Text(text) => Outbound::text(id, &device, text),
                    Cargo::File(path) => Outbound::file(id, &device, path),
                };
                let out = match made {
                    Ok(out) => out,
                    Err(why) => {
                        self.emit(Event::Status(why));
                        return;
                    }
                };
                let announce = Message::Beam {
                    id,
                    kind: out.kind.id().to_owned(),
                    name: out.name.clone(),
                    size: out.size,
                    parts: out.parts,
                    sha: out.sha.clone(),
                };
                let packet = seal(&key, PAIRED, &self.envelope(announce));
                self.push_quiet(id, &device, packet, Tag::Beam(id));
                self.beams_out.push(out);
            }
            Request::BeamAnswer { device, beam, accept } => {
                let Some(at) = self.offers.iter().position(|o| o.device == device && o.beam == beam) else {
                    return;
                };
                let offer = self.offers.remove(at);
                let ok = accept
                    && match Inbound::file(beam, &device, &offer.name, offer.size, &offer.sha, &self.downloads) {
                        Ok(inbound) => {
                            self.beams_in.push(inbound);
                            true
                        }
                        Err(why) => {
                            self.emit(Event::Status(why));
                            false
                        }
                    };
                self.beam_reply(&device, beam, ok);
            }
            Request::SetSync { device, mode } => {
                if !SYNC_MODES.contains(&mode.as_str()) {
                    return;
                }
                if let Some(d) = self.book.devices.iter_mut().find(|d| d.id == device) {
                    d.sync = mode;
                    self.save();
                    self.devices_changed();
                }
            }
            Request::Snapshot { device, json } => {
                let Some(key) = self
                    .book
                    .devices
                    .iter()
                    .find(|d| d.id == device)
                    .and_then(Device::key)
                else {
                    return;
                };
                let chunks = split(&json, PART_BYTES);
                if chunks.len() > MAX_PARTS {
                    self.emit(Event::Status(message("msg.too_many_to_sync", &[])));
                    return;
                }
                // A newer ledger replaces the parts of an older one still on their way.
                self.outgoing.retain(|o| !(o.quiet && o.device == device));
                let snap = u64::from_le_bytes(random_bytes());
                let parts = u16::try_from(chunks.len()).unwrap_or(u16::MAX);
                for (i, data) in chunks.into_iter().enumerate() {
                    let id = u64::from_le_bytes(random_bytes());
                    let part = Message::Part {
                        snap,
                        id,
                        part: u16::try_from(i).unwrap_or(u16::MAX),
                        parts,
                        data,
                    };
                    let packet = seal(&key, PAIRED, &self.envelope(part));
                    self.outgoing.push(Outgoing {
                        id,
                        device: device.clone(),
                        packet,
                        tries: 0,
                        next: Instant::now(),
                        quiet: true,
                        tag: Tag::None,
                    });
                }
            }
            Request::Forget(id) => {
                let Some(device) = self.book.devices.iter().find(|d| d.id == id).cloned() else {
                    return;
                };
                // Forget (and save) here first, then tell the other device: once it has heard, this
                // device is already gone from disk, and a failed send still leaves it forgotten.
                let unpair = device.key().map(|key| {
                    let ip = self.heard.get(&id).map_or(self.broadcast, |(ip, _)| *ip);
                    (seal(&key, PAIRED, &self.envelope(Message::Unpair)), ip)
                });
                self.forget(&id);
                if let Some((packet, ip)) = unpair {
                    self.send(&packet, SocketAddr::new(ip, self.peer_port));
                }
            }
        }
    }

    fn key_of(&self, device: &str) -> Option<[u8; 32]> {
        self.book.devices.iter().find(|d| d.id == device).and_then(Device::key)
    }

    /// Queues a packet that is sent until confirmed, without a word to the user about it.
    fn push_quiet(&mut self, id: u64, device: &str, packet: Vec<u8>, tag: Tag) {
        self.outgoing.push(Outgoing {
            id,
            device: device.to_owned(),
            packet,
            tries: 0,
            next: Instant::now(),
            quiet: true,
            tag,
        });
    }

    /// Wormhole: tells a device whether its transfer `beam` is wanted.
    fn beam_reply(&mut self, device: &str, beam: u64, ok: bool) {
        let Some(key) = self.key_of(device) else { return };
        let id = u64::from_le_bytes(random_bytes());
        let packet = seal(&key, PAIRED, &self.envelope(Message::BeamReply { id, beam, ok }));
        self.push_quiet(id, device, packet, Tag::Beam(beam));
    }

    /// Wormhole: a transfer to a device is over before its end (no answer, a part lost for
    /// good, a file that changed): everything of it is dropped and the user is told `why`.
    fn beam_failed(&mut self, beam: u64, why: &str) {
        let before = self.beams_out.len();
        self.beams_out.retain(|b| b.id != beam);
        self.outgoing.retain(|o| o.tag != Tag::Beam(beam) && o.tag != Tag::Chunk(beam));
        if self.beams_out.len() < before {
            self.emit(Event::Status(message(why, &[])));
        }
    }

    /// Wormhole, every tick: the next parts of what is being sent (a window of them at once),
    /// and the transfers and offers that waited too long.
    fn tick_beams(&mut self) {
        let mut fresh = Vec::new();
        let mut broken = Vec::new();
        for out in self.beams_out.iter_mut().filter(|b| b.accepted) {
            let flying = self.outgoing.iter().filter(|o| o.tag == Tag::Chunk(out.id)).count();
            for _ in flying..BEAM_WINDOW {
                if out.next >= out.parts {
                    break;
                }
                match out.part(out.next) {
                    Some(data) => {
                        fresh.push((out.id, out.device.clone(), out.next, data));
                        out.next += 1;
                    }
                    None => {
                        broken.push(out.id);
                        break;
                    }
                }
            }
        }
        for (beam, device, part, data) in fresh {
            let Some(key) = self.key_of(&device) else { continue };
            let id = u64::from_le_bytes(random_bytes());
            let packet = seal(&key, PAIRED, &self.envelope(Message::Chunk { id, beam, part, data }));
            self.push_quiet(id, &device, packet, Tag::Chunk(beam));
        }
        for beam in broken {
            self.beam_failed(beam, "msg.beam_unreadable");
        }
        let unanswered: Vec<u64> = self
            .beams_out
            .iter()
            .filter(|b| !b.accepted && b.since.elapsed() >= BEAM_OFFER_FOR)
            .map(|b| b.id)
            .collect();
        for beam in unanswered {
            self.beam_failed(beam, "msg.beam_no_answer");
        }
        self.offers.retain(|o| o.since.elapsed() < BEAM_OFFER_FOR);
        let mut stalled = false;
        let mut at = 0;
        while at < self.beams_in.len() {
            if self.beams_in[at].last.elapsed() >= BEAM_STALL {
                self.beams_in.remove(at).abort();
                stalled = true;
            } else {
                at += 1;
            }
        }
        if stalled {
            self.emit(Event::Status(message("msg.beam_interrupted", &[])));
        }
    }

    fn forget(&mut self, id: &str) {
        self.book.devices.retain(|d| d.id != id);
        self.beams_out.retain(|b| b.device != id);
        self.offers.retain(|o| o.device != id);
        let mut at = 0;
        while at < self.beams_in.len() {
            if self.beams_in[at].device == id {
                self.beams_in.remove(at).abort();
            } else {
                at += 1;
            }
        }
        self.outgoing.retain(|o| o.device != id);
        self.heard.remove(id);
        self.save();
        self.devices_changed();
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if now >= self.hello_due && !self.book.devices.is_empty() {
            self.hello_due = now + HELLO_EVERY;
            self.hello(false);
        }
        match &mut self.pairing {
            Some(Pairing::Showing { until, .. }) if now >= *until => {
                self.pairing = None;
                self.emit(Event::Code(None));
            }
            Some(Pairing::Joining { until, .. }) if now >= *until => {
                self.pairing = None;
                self.emit(Event::Status(message("msg.no_device_answered", &[])));
            }
            Some(Pairing::Joining { packet, next, .. }) if now >= *next => {
                *next = now + JOIN_EVERY;
                let packet = packet.clone();
                self.send(&packet, SocketAddr::new(self.broadcast, self.peer_port));
            }
            _ => {}
        }
        self.tick_beams();
        let mut failed = Vec::new();
        for i in 0..self.outgoing.len() {
            if now < self.outgoing[i].next {
                continue;
            }
            if self.outgoing[i].tries == SEND_TRIES {
                failed.push(i);
                continue;
            }
            self.outgoing[i].tries += 1;
            self.outgoing[i].next = now + SEND_EVERY;
            self.send_to_device(&self.outgoing[i].packet, &self.outgoing[i].device);
        }
        for i in failed.into_iter().rev() {
            let gone = self.outgoing.remove(i);
            // A lost part or announcement of a Wormhole transfer ends that transfer.
            if let Tag::Beam(beam) | Tag::Chunk(beam) = gone.tag {
                self.beam_failed(beam, "msg.beam_interrupted");
            }
            if gone.quiet {
                continue;
            }
            let name = self.name_of(&gone.device);
            self.emit(Event::Status(message("msg.device_silent", &[("name", &name)])));
        }
        self.incoming.retain(|_, i| i.since.elapsed() < PARTS_FOR);
        self.devices_changed();
    }

    fn name_of(&self, id: &str) -> String {
        self.book
            .devices
            .iter()
            .find(|d| d.id == id)
            .map_or_else(|| message("msg.other_device", &[]), |d| d.name.clone())
    }

    fn receive(&mut self, packet: &[u8], from: SocketAddr) {
        match kind_of(packet) {
            Some(PAIRING) => self.receive_pairing(packet, from),
            Some(PAIRED) => self.receive_paired(packet, from),
            _ => {}
        }
    }

    fn receive_pairing(&mut self, packet: &[u8], from: SocketAddr) {
        let me = self.book.me.clone();
        let fresh = |e: &Envelope| e.from != me && handoff::is_fresh(e.at, now_secs());
        match &mut self.pairing {
            Some(Pairing::Showing {
                key,
                until,
                answered,
            }) => {
                let key = *key;
                let Some(envelope) = open(&key, packet).filter(fresh) else {
                    return;
                };
                let Message::Offer { name, half } = envelope.message else {
                    return;
                };
                if let Some((_, reply)) = answered.iter().find(|(h, _)| *h == half) {
                    let reply = reply.clone();
                    self.send(&reply, from);
                    return;
                }
                let Some(theirs) = unhex::<32>(&half) else {
                    return;
                };
                let mine = random_bytes::<32>();
                let accept = Message::Accept {
                    name: self.name.clone(),
                    half: hex(&mine),
                };
                let reply = seal(
                    &key,
                    PAIRING,
                    &Envelope {
                        from: me,
                        at: now_secs(),
                        message: accept,
                    },
                );
                // Kept a little longer to answer the offer again if this answer is lost.
                *until = (*until).min(Instant::now() + JOIN_FOR);
                answered.push((half, reply.clone()));
                self.send(&reply, from);
                self.heard
                    .insert(envelope.from.clone(), (from.ip(), Instant::now()));
                self.remember(&envelope.from, &name, pair_key(&key, &mine, &theirs));
                self.emit(Event::Code(None));
            }
            Some(Pairing::Joining { key, half, .. }) => {
                let (key, half) = (*key, *half);
                let Some(envelope) = open(&key, packet).filter(fresh) else {
                    return;
                };
                let Message::Accept { name, half: theirs } = envelope.message else {
                    return;
                };
                let Some(theirs) = unhex::<32>(&theirs) else {
                    return;
                };
                self.pairing = None;
                self.heard
                    .insert(envelope.from.clone(), (from.ip(), Instant::now()));
                self.remember(&envelope.from, &name, pair_key(&key, &theirs, &half));
            }
            None => {}
        }
    }

    fn receive_paired(&mut self, packet: &[u8], from: SocketAddr) {
        let Some((device, envelope)) = self.book.devices.iter().find_map(|d| {
            let envelope = open(&d.key()?, packet)?;
            (envelope.from == d.id).then(|| (d.clone(), envelope))
        }) else {
            return;
        };
        if !handoff::is_fresh(envelope.at, now_secs()) {
            return;
        }
        self.heard
            .insert(device.id.clone(), (from.ip(), Instant::now()));
        let Some(key) = device.key() else { return };
        match envelope.message {
            Message::Hello { reply } => {
                if reply {
                    let packet = seal(
                        &key,
                        PAIRED,
                        &self.envelope(Message::Hello { reply: false }),
                    );
                    self.send(&packet, SocketAddr::new(from.ip(), self.peer_port));
                }
            }
            Message::Tab {
                id,
                url,
                title,
                position,
            } => {
                let ack = seal(&key, PAIRED, &self.envelope(Message::Ack { id }));
                self.send(&ack, from);
                let valid = url.len() <= MAX_URL
                    && url::Url::parse(&url).is_ok_and(|u| handoff::can_continue(&u));
                if valid && self.seen.first_time(id) {
                    self.emit(Event::Received {
                        from: device.name.clone(),
                        url,
                        title: bounded(&title, MAX_TITLE),
                        position: position.filter(|p| p.is_finite() && *p >= 0.0),
                    });
                }
            }
            Message::Ack { id } => {
                let found = self.outgoing.iter().position(|o| o.id == id && o.device == device.id);
                if let Some(at) = found {
                    let done = self.outgoing.remove(at);
                    if let Tag::Chunk(beam) = done.tag {
                        // A part of a Wormhole transfer got there; the last one ends it.
                        if let Some(at) = self.beams_out.iter().position(|b| b.id == beam) {
                            self.beams_out[at].acked += 1;
                            if self.beams_out[at].done() {
                                let sent = self.beams_out.remove(at);
                                self.emit(Event::BeamSent { to: device.name.clone(), name: sent.name });
                            }
                        }
                    } else if !done.quiet {
                        self.emit(Event::Delivered(device.name.clone()));
                    }
                }
            }
            Message::Beam { id, kind, name, size, parts, sha } => {
                let ack = seal(&key, PAIRED, &self.envelope(Message::Ack { id }));
                self.send(&ack, from);
                if self.seen.first_time(id) {
                    // One transfer at a time from a device: no pile of offers, no pile of files.
                    let busy = self.beams_in.iter().any(|b| b.device == device.id)
                        || self.offers.iter().any(|o| o.device == device.id);
                    match beam_rules::acceptable(&kind, size, parts, &sha).filter(|_| !busy) {
                        Some(Kind::Text) => {
                            self.beams_in.push(Inbound::text(id, &device.id, size, &sha));
                            self.beam_reply(&device.id, id, true);
                        }
                        Some(Kind::File) => {
                            let name = beam_rules::safe_name(&name);
                            self.offers.push(Offer {
                                device: device.id.clone(),
                                beam: id,
                                name: name.clone(),
                                size,
                                sha,
                                since: Instant::now(),
                            });
                            self.emit(Event::BeamOffer {
                                device: device.id.clone(),
                                from: device.name.clone(),
                                beam: id,
                                name,
                                size,
                            });
                        }
                        None => self.beam_reply(&device.id, id, false),
                    }
                }
            }
            Message::BeamReply { id, beam, ok } => {
                let ack = seal(&key, PAIRED, &self.envelope(Message::Ack { id }));
                self.send(&ack, from);
                if self.seen.first_time(id) {
                    let mine = self.beams_out.iter().position(|b| b.id == beam && b.device == device.id);
                    match mine {
                        Some(at) if ok => self.beams_out[at].accepted = true,
                        Some(_) => self.beam_failed(beam, "msg.beam_declined"),
                        None => {}
                    }
                }
            }
            Message::Chunk { id, beam, part, data } => {
                let ack = seal(&key, PAIRED, &self.envelope(Message::Ack { id }));
                self.send(&ack, from);
                let mine = self.beams_in.iter().position(|b| b.id == beam && b.device == device.id);
                if let Some(at) = mine {
                    match self.beams_in[at].put(part, &data) {
                        Ok(false) => {}
                        Ok(true) => match self.beams_in.remove(at).finish() {
                            Ok(Done::Text(text)) => self.emit(Event::BeamText { from: device.name.clone(), text }),
                            Ok(Done::File { name, path }) => {
                                self.emit(Event::BeamFile { from: device.name.clone(), name, path });
                            }
                            Err(why) => self.emit(Event::Status(why)),
                        },
                        Err(why) => {
                            self.beams_in.remove(at).abort();
                            self.emit(Event::Status(why));
                        }
                    }
                }
            }
            Message::Part {
                snap,
                id,
                part,
                parts,
                data,
            } => {
                let ack = seal(&key, PAIRED, &self.envelope(Message::Ack { id }));
                self.send(&ack, from);
                let (part, parts) = (usize::from(part), usize::from(parts));
                if parts == 0 || parts > MAX_PARTS || part >= parts || data.len() > PART_BYTES {
                    return;
                }
                let slot = self
                    .incoming
                    .entry((device.id.clone(), snap))
                    .or_insert_with(|| Incoming {
                        parts: vec![None; parts],
                        since: Instant::now(),
                    });
                if slot.parts.len() != parts {
                    return;
                }
                slot.parts[part] = Some(data);
                if slot.parts.iter().all(Option::is_some) {
                    let done = self.incoming.remove(&(device.id.clone(), snap));
                    let json: String = done
                        .map(|i| i.parts.into_iter().flatten().collect())
                        .unwrap_or_default();
                    self.emit(Event::Snapshot {
                        device: device.id.clone(),
                        name: device.name.clone(),
                        json,
                    });
                }
            }
            Message::Unpair => self.forget(&device.id),
            Message::Offer { .. } | Message::Accept { .. } => {}
        }
        self.devices_changed();
    }
}

/// Cuts `text` in pieces of at most `max` bytes, on character boundaries.
fn split(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if !current.is_empty() && current.len() + c.len_utf8() > max {
            out.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(message: Message) -> Envelope {
        Envelope {
            from: "a".into(),
            at: 100,
            message,
        }
    }

    #[test]
    fn a_sealed_packet_opens_only_with_its_key_and_untouched() {
        let tab = envelope(Message::Tab {
            id: 7,
            url: "https://example.com/".into(),
            title: "Example".into(),
            position: Some(12.5),
        });
        let packet = seal(&[1; 32], PAIRED, &tab);
        assert_eq!(kind_of(&packet), Some(PAIRED));
        assert_eq!(open(&[1; 32], &packet), Some(tab));
        assert_eq!(open(&[2; 32], &packet), None, "another pair's key");
        let mut tampered = packet.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert_eq!(open(&[1; 32], &tampered), None);
        let mut relabelled = packet.clone();
        relabelled[3] = PAIRING;
        assert_eq!(
            open(&[1; 32], &relabelled),
            None,
            "the header is authenticated"
        );
        assert!(!String::from_utf8_lossy(&packet).contains("example.com"));
        assert_eq!(kind_of(b"hello"), None);
    }

    #[test]
    fn both_ends_derive_the_same_keys_from_the_code() {
        let code = handoff::parse_code("ABCD-EFGH-JKMN").unwrap();
        assert_eq!(code_key(code), code_key(code));
        assert_ne!(code_key(code), code_key(code + 1));
        let (s, j) = ([3; 32], [4; 32]);
        assert_eq!(
            pair_key(&code_key(code), &s, &j),
            pair_key(&code_key(code), &s, &j)
        );
        assert_ne!(
            pair_key(&code_key(code), &s, &j),
            pair_key(&code_key(code), &j, &s)
        );
        assert_eq!(unhex::<2>(&hex(&[0xab, 0x01])), Some([0xab, 0x01]));
        assert_eq!(unhex::<2>("zz00"), None);
    }

    #[test]
    fn the_message_format_is_stable() {
        let json = serde_json::to_string(&envelope(Message::Ack { id: 3 })).unwrap();
        assert_eq!(json, r#"{"from":"a","at":100,"t":"ack","id":3}"#);
        let old: Envelope = serde_json::from_str(
            r#"{"from":"a","at":1,"t":"tab","id":1,"url":"https://a.test/","title":""}"#,
        )
        .unwrap();
        assert!(matches!(old.message, Message::Tab { position: None, .. }));
        let chunk = Message::Chunk { id: 2, beam: 9, part: 0, data: "0a".into() };
        assert_eq!(
            serde_json::to_string(&envelope(chunk)).unwrap(),
            r#"{"from":"a","at":100,"t":"chunk","id":2,"beam":9,"part":0,"data":"0a"}"#
        );
        // A message this version does not know (a newer device) is not read at all.
        assert!(serde_json::from_str::<Envelope>(r#"{"from":"a","at":1,"t":"teleport","id":1}"#).is_err());
    }

    #[test]
    fn the_paired_devices_are_stored_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let store = DeviceStore::with_key(dir.path());
        assert!(store.load().unwrap().devices.is_empty());
        let book = Book {
            me: "me".into(),
            devices: vec![Device {
                id: "d".into(),
                name: "Laptop".into(),
                key: hex(&[5; 32]),
                paired_at: 1,
                sync: ask(),
            }],
        };
        store.save(&book).unwrap();
        let raw = std::fs::read(dir.path().join(FILE)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("Laptop"));
        assert_eq!(store.load().unwrap().devices, book.devices);
        assert_eq!(store.load().unwrap().devices[0].key(), Some([5; 32]));
    }

    fn free_port() -> u16 {
        UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn service(dir: &std::path::Path, name: &str, port: u16, peer_port: u16) -> Handle {
        let mut config = Config::on(
            DeviceStore::with_key(dir),
            name,
            Ipv4Addr::LOCALHOST.into(),
            Ipv4Addr::LOCALHOST.into(),
        );
        config.port = port;
        config.peer_port = peer_port;
        config.downloads = dir.join("received");
        spawn(config)
    }

    fn wait_for(handle: &Handle, mut wanted: impl FnMut(&Event) -> bool) -> Event {
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until {
            match handle.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(event) if wanted(&event) => return event,
                _ => {}
            }
        }
        panic!("event not received");
    }

    #[test]
    fn split_keeps_characters_whole() {
        let parts = split("aé€😀b", 3);
        assert!(parts.iter().all(|p| p.len() <= 4));
        assert_eq!(parts.concat(), "aé€😀b");
        assert_eq!(split("", 3), vec![String::new()]);
    }

    #[test]
    fn two_devices_pair_send_a_tab_and_forget_each_other() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (pa, pb) = (free_port(), free_port());
        let a = service(da.path(), "Desk", pa, pb);
        let b = service(db.path(), "Laptop", pb, pa);
        assert!(
            matches!(wait_for(&a, |e| matches!(e, Event::Devices(_))), Event::Devices(l) if l.is_empty())
        );

        a.request(Request::Join("0000-0000-000".into()));
        assert!(
            matches!(wait_for(&a, |e| matches!(e, Event::Status(_))), Event::Status(s) if s.contains("msg.type_code"))
        );

        a.request(Request::ShowCode);
        let Event::Code(Some(code)) = wait_for(&a, |e| matches!(e, Event::Code(Some(_)))) else {
            unreachable!()
        };
        b.request(Request::Join(code.to_lowercase()));
        assert_eq!(
            wait_for(&a, |e| matches!(e, Event::Paired(_))),
            Event::Paired("Laptop".into())
        );
        assert_eq!(
            wait_for(&b, |e| matches!(e, Event::Paired(_))),
            Event::Paired("Desk".into())
        );
        let Event::Devices(list) =
            wait_for(&b, |e| matches!(e, Event::Devices(l) if !l.is_empty()))
        else {
            unreachable!()
        };
        assert_eq!(list[0].name, "Desk");

        b.request(Request::Send {
            device: list[0].id.clone(),
            url: "https://www.youtube.com/watch?v=abc".into(),
            title: "A video".into(),
            position: Some(42.0),
        });
        assert_eq!(
            wait_for(&a, |e| matches!(e, Event::Received { .. })),
            Event::Received {
                from: "Laptop".into(),
                url: "https://www.youtube.com/watch?v=abc".into(),
                title: "A video".into(),
                position: Some(42.0),
            }
        );
        assert_eq!(
            wait_for(&b, |e| matches!(e, Event::Delivered(_))),
            Event::Delivered("Desk".into())
        );

        // Wormhole: a text arrives by itself; a file waits for the answer, then arrives whole.
        let text = "wormhole é😀\n".repeat(700);
        b.request(Request::Beam { device: list[0].id.clone(), payload: Cargo::Text(text.clone()) });
        assert_eq!(
            wait_for(&a, |e| matches!(e, Event::BeamText { .. })),
            Event::BeamText { from: "Laptop".into(), text }
        );
        assert_eq!(
            wait_for(&b, |e| matches!(e, Event::BeamSent { .. })),
            Event::BeamSent { to: "Desk".into(), name: String::new() }
        );
        let content: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
        let source = db.path().join("holiday photo.bin");
        std::fs::write(&source, &content).unwrap();
        let beam_to = |file: &std::path::Path| Request::Beam {
            device: list[0].id.clone(),
            payload: Cargo::File(file.to_path_buf()),
        };
        b.request(beam_to(&source));
        let Event::BeamOffer { device, from, beam, name, size } =
            wait_for(&a, |e| matches!(e, Event::BeamOffer { .. }))
        else {
            unreachable!()
        };
        assert_eq!((from.as_str(), name.as_str(), size), ("Laptop", "holiday photo.bin", 200_000));
        a.request(Request::BeamAnswer { device: device.clone(), beam, accept: false });
        assert!(
            matches!(wait_for(&b, |e| matches!(e, Event::Status(_))), Event::Status(s) if s.contains("msg.beam_declined"))
        );
        b.request(beam_to(&source));
        let Event::BeamOffer { beam, .. } = wait_for(&a, |e| matches!(e, Event::BeamOffer { .. })) else {
            unreachable!()
        };
        a.request(Request::BeamAnswer { device, beam, accept: true });
        let Event::BeamFile { from, name, path } = wait_for(&a, |e| matches!(e, Event::BeamFile { .. })) else {
            unreachable!()
        };
        assert_eq!((from.as_str(), name.as_str()), ("Laptop", "holiday photo.bin"));
        assert_eq!(std::fs::read(&path).unwrap(), content);
        assert!(path.starts_with(da.path().join("received")));
        assert_eq!(
            wait_for(&b, |e| matches!(e, Event::BeamSent { .. })),
            Event::BeamSent { to: "Desk".into(), name: "holiday photo.bin".into() }
        );
        b.request(beam_to(&db.path().join("missing.bin")));
        assert!(
            matches!(wait_for(&b, |e| matches!(e, Event::Status(_))), Event::Status(s) if s.contains("msg.beam_unreadable"))
        );

        // A synced ledger bigger than a packet travels in parts and arrives whole.
        let big: String = (0..2500).map(|i| format!("{{\"é\":{i}}}")).collect();
        b.request(Request::SetSync {
            device: list[0].id.clone(),
            mode: "auto".into(),
        });
        wait_for(&b, |e| matches!(e, Event::Devices(l) if l.first().is_some_and(|d| d.sync == "auto")));
        b.request(Request::Snapshot {
            device: list[0].id.clone(),
            json: big.clone(),
        });
        let Event::Snapshot { name, json, .. } =
            wait_for(&a, |e| matches!(e, Event::Snapshot { .. }))
        else {
            unreachable!()
        };
        assert_eq!((name.as_str(), json.len()), ("Laptop", big.len()));
        assert_eq!(json, big);

        b.request(Request::Send {
            device: list[0].id.clone(),
            url: "file:///etc/passwd".into(),
            title: String::new(),
            position: None,
        });
        assert!(
            matches!(wait_for(&b, |e| matches!(e, Event::Status(_))), Event::Status(s) if s.contains("msg.only_web_continue"))
        );

        b.request(Request::Forget(list[0].id.clone()));
        wait_for(&a, |e| matches!(e, Event::Devices(l) if l.is_empty()));
        assert!(DeviceStore::with_key(da.path())
            .load()
            .unwrap()
            .devices
            .is_empty());
        assert!(DeviceStore::with_key(db.path())
            .load()
            .unwrap()
            .devices
            .is_empty());
    }
}

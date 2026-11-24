//! Keeps the sidebar's media card steady. Players often report "nothing" for a moment (an ad
//! starts, Twitch or YouTube rebuilds its player, a page navigates inside itself): without a
//! grace period the card disappears and slides back in a probe later, which reads as flicker.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use blackhole_domain::TabId;
use serde_json::json;

use crate::views;
use crate::state::Running;
use crate::tabs::pip::DISMISS_GRACE;

/// How long a tab keeps its card after its page last reported media.
pub const LINGER: Duration = Duration::from_secs(5);

pub struct Linger<T: Copy> {
    kept: HashMap<u64, (Instant, T)>,
    hold: Duration,
}

impl<T: Copy> Linger<T> {
    pub fn new(hold: Duration) -> Self {
        Self { kept: HashMap::new(), hold }
    }

    /// Records what a tab reports at `at`: `Some` when it has media to show.
    pub fn note(&mut self, id: u64, at: Instant, shown: Option<T>) {
        if let Some(value) = shown {
            self.kept.insert(id, (at, value));
        }
    }

    /// What to show for a tab at `at`: its last report with media, while recent enough.
    pub fn shown(&self, id: u64, at: Instant) -> Option<T> {
        self.kept
            .get(&id)
            .filter(|(seen, _)| at.saturating_duration_since(*seen) <= self.hold)
            .map(|(_, value)| *value)
    }

    /// Forgets a tab at once (closed, or its card dismissed).
    pub fn forget(&mut self, id: u64) {
        self.kept.remove(&id);
    }

    /// Drops the tabs that are gone and the reports past their grace period.
    pub fn prune(&mut self, at: Instant, alive: impl Fn(u64) -> bool) {
        let hold = self.hold;
        self.kept
            .retain(|id, (seen, _)| alive(*id) && at.saturating_duration_since(*seen) <= hold);
    }
}

/// What a page says about its audio and video (see `views::MEDIA_PROBE_JS`).
#[derive(Clone, Copy, Default, serde::Deserialize)]
pub(crate) struct MediaProbe {
    pub(crate) playing: bool,
    pub(crate) has: bool,
    pub(crate) muted: bool,
    /// A long or live video/audio is playing, muted or not (not an ambient looping clip).
    #[serde(default)]
    pub(crate) watching: bool,
    /// A video of the page is floating in Picture in Picture.
    #[serde(default)]
    pub(crate) pip: bool,
    /// Camera / microphone opened by a call, and whether each is currently on.
    #[serde(default)]
    pub(crate) cam: bool,
    #[serde(default)]
    pub(crate) mic: bool,
    /// A live WebRTC connection (a call), even when no camera or microphone is open.
    #[serde(default)]
    pub(crate) call: bool,
    #[serde(default, rename = "camOn")]
    pub(crate) cam_on: bool,
    #[serde(default, rename = "micOn")]
    pub(crate) mic_on: bool,
}

impl MediaProbe {
    /// The page is in a call: a camera or microphone open, or a live WebRTC connection. A
    /// listener of a large meeting opens neither and is still in the call.
    pub(crate) fn in_call(&self) -> bool {
        self.cam || self.mic || self.call
    }

    /// The page must keep its full resources while hidden: a call, or media playing.
    pub(crate) fn keeps_page(&self) -> bool {
        self.playing || self.watching || self.in_call()
    }
}

/// How often background tabs are asked whether they play audio or video.
const MEDIA_EVERY: Duration = Duration::from_millis(1500);

/// How long a tab that was in a call or playing media is still treated as such.
pub(crate) const BUSY_GRACE: Duration = Duration::from_secs(90);

/// How long a tab is probed after it said its media may have changed.
const MEDIA_HINT_FOR: Duration = Duration::from_secs(5);

/// What the tabs play: the probes' reports, the sidebar's media card, the tabs kept busy.
pub(crate) struct MediaWatch {
    /// Latest report of each tab's probe (written by the views' handlers).
    pub(crate) probes: Arc<Mutex<HashMap<u64, MediaProbe>>>,
    /// When the probes run next: every `MEDIA_EVERY`, or more often while a video floats.
    pub(crate) due: Instant,
    /// When the tabs other than the floating one are next asked: a floating video needs a
    /// quick probe of its own tab only, not of the call or the music playing elsewhere.
    pub(crate) all_due: Instant,
    /// When each tab last said its media may have changed (see `PageEvent::Media`).
    pub(crate) hints: HashMap<u64, Instant>,
    /// The media card as last sent to the sidebar.
    pub(crate) seen: serde_json::Value,
    /// Last report with media per tab: a short gap in the probes keeps the card on screen.
    pub(crate) hold: Linger<MediaProbe>,
    /// Tabs whose media card was closed, and when; they come back when the tab plays again.
    pub(crate) dismissed: HashMap<u64, Instant>,
    /// Last time a tab was seen in a call or playing media: it keeps its page for a while after.
    pub(crate) busy_seen: HashMap<TabId, Instant>,
    /// Tabs in a call whose page runs at full speed while hidden (macOS, `service_calls`).
    #[cfg(target_os = "macos")]
    pub(crate) awake: std::collections::HashSet<u64>,
}

impl MediaWatch {
    pub(crate) fn new() -> Self {
        Self {
            probes: Arc::default(),
            due: Instant::now(),
            all_due: Instant::now(),
            hints: HashMap::new(),
            seen: serde_json::Value::Null,
            hold: Linger::new(LINGER),
            dismissed: HashMap::new(),
            busy_seen: HashMap::new(),
            #[cfg(target_os = "macos")]
            awake: Default::default(),
        }
    }
}

impl Running {
    /// Audio/video state of the active space's tabs, for the speaker icon and the media card.
    pub(crate) fn media_json(&self) -> serde_json::Value {
        let now = Instant::now();
        let tabs = &self.svc.workspace().active_space().tabs;
        let list: Vec<_> = tabs
            .iter()
            .filter(|t| !self.media.dismissed.contains_key(&t.id.0))
            .filter_map(|t| {
                let p = self.media.hold.shown(t.id.0, now)?;
                Some(json!({
                    "id": t.id.0, "playing": p.playing, "muted": p.muted,
                    "cam": p.cam, "mic": p.mic, "camOn": p.cam_on, "micOn": p.mic_on,
                    "call": p.in_call(),
                }))
            })
            .collect();
        json!(list)
    }

    /// Asks every loaded tab about its media, and refreshes the sidebar when something changed.
    pub(crate) fn service_media(&mut self) {
        if Instant::now() >= self.media.due {
            let everyone = Instant::now() >= self.media.all_due;
            if everyone {
                self.media.all_due = Instant::now() + MEDIA_EVERY;
            }
            self.media.due = Instant::now()
                + if self.pip.seen {
                    Duration::from_millis(500)
                } else {
                    MEDIA_EVERY
                };
            if let Ok(mut probes) = self.media.probes.lock() {
                probes.retain(|id, _| self.tabs.contains_key(&TabId(*id)));
            }
            // Only the tabs with something going on are asked: a script run in every live tab
            // every second and a half kept the idle ones (documents, sheets) awake for nothing.
            let now = Instant::now();
            self.media.hints
                .retain(|id, at| self.tabs.contains_key(&TabId(*id)) && now.duration_since(*at) < MEDIA_HINT_FOR);
            let active: Vec<u64> = self.media.probes.lock().map_or_else(
                |_| Vec::new(),
                |probes| {
                    probes
                        .iter()
                        .filter(|(_, p)| p.playing || p.pip || p.cam || p.mic || p.call)
                        .map(|(id, _)| *id)
                        .collect()
                },
            );
            let floating = self.pip.tab.map(|t| t.0);
            for (id, view) in &self.tabs {
                let sink = Arc::clone(&self.media.probes);
                let id = id.0;
                if !self.media.hints.contains_key(&id) && !active.contains(&id) && floating != Some(id) {
                    continue;
                }
                if !everyone && floating != Some(id) {
                    continue;
                }
                let _ = view.evaluate_script_with_callback(views::MEDIA_PROBE_JS, move |raw| {
                    if let (Ok(probe), Ok(mut probes)) =
                        (serde_json::from_str::<MediaProbe>(&raw), sink.lock())
                    {
                        probes.insert(id, probe);
                    }
                });
            }
        }
        self.service_calls();
        let now = Instant::now();
        if let Ok(probes) = self.media.probes.lock() {
            // A probe sent before the card was closed still says "playing": only a later one
            // can bring the card back.
            self.media.dismissed.retain(|id, at| {
                at.elapsed() < DISMISS_GRACE || !probes.get(id).is_some_and(|p| p.playing)
            });
            for (id, p) in probes.iter() {
                let shown = p.playing || p.has || p.cam || p.mic;
                self.media.hold.note(*id, now, shown.then_some(*p));
            }
        }
        self.media.hold
            .prune(now, |id| self.tabs.contains_key(&TabId(id)));
        // Fallback for engines that do not tell the page when its floating window is left.
        let floating = self
            .pip.tab
            .and_then(|tab| self.media.probes.lock().ok()?.get(&tab.0).copied().map(|p| (tab, p)));
        if let Some((tab, probe)) = floating {
            if probe.pip {
                self.pip.seen = true;
            } else if self.pip.seen {
                self.pip.faded = Some((tab, Instant::now()));
                self.pip_returned(tab);
            }
        }
        let now = self.media_json();
        if now != self.media.seen {
            self.media.seen = now;
            self.ui_state();
        }
    }

    /// WebKit slows the scripts of a hidden page down to a crawl: in a call, what the page draws
    /// and sends (a blurred background, effects) falls with it. Chromium leaves such a page alone
    /// (WebView2 needs nothing); here the page in a call is told to run, and only for that time.
    fn service_calls(&mut self) {
        #[cfg(target_os = "macos")]
        {
            let calls: std::collections::HashSet<u64> = self.media.probes.lock().map_or_else(
                |_| Default::default(),
                |probes| probes.iter().filter(|(_, p)| p.in_call()).map(|(id, _)| *id).collect(),
            );
            // A tab gone from `tabs` and back has a new view: it is told again.
            self.media.awake.retain(|id| self.tabs.contains_key(&TabId(*id)));
            let changed: Vec<u64> = calls.symmetric_difference(&self.media.awake).copied().collect();
            for id in changed {
                let on = calls.contains(&id);
                if let Some(ptr) = self.tabs.get(&TabId(id)).and_then(crate::platform::web_view_ptr) {
                    blackhole_macos_webext::run_while_hidden(ptr, on);
                }
                if on {
                    self.media.awake.insert(id);
                } else {
                    self.media.awake.remove(&id);
                }
            }
        }
    }

    pub(crate) fn media_ctl(&mut self, id: TabId, action: &str) {
        let Some(view) = self.tabs.get(&id) else {
            return;
        };
        let rtc;
        let js = match action {
            "cam" | "mic" => {
                rtc = views::rtc_toggle_js(if action == "cam" { "video" } else { "audio" });
                rtc.as_str()
            }
            "toggle" => views::MEDIA_TOGGLE_JS,
            "mute" => views::MEDIA_MUTE_JS,
            "pip" => views::PIP_TOGGLE_JS,
            // Closing the player also closes its floating window.
            "close" => {
                self.media.dismissed.insert(id.0, Instant::now());
                if self.pip.tab == Some(id) {
                    self.pip.tab = None;
                    self.pip.seen = false;
                }
                views::MEDIA_CLOSE_JS
            }
            _ => return,
        };
        // The card of a call (camera, microphone or a live connection): closing it leaves the
        // call too, instead of only pausing its tiles.
        if action == "close"
            && self
                .media.probes
                .lock()
                .ok()
                .and_then(|probes| probes.get(&id.0).copied())
                .is_some_and(|p| p.in_call())
        {
            let _ = view.evaluate_script(views::CALL_HANG_UP_JS);
        }
        let _ = view.evaluate_script(js);
        self.media.hints.insert(id.0, Instant::now());
        match action {
            "pip" => self.pip_toggled(id),
            "close" => {
                self.pip.shell_exits.insert(id.0, Instant::now());
            }
            _ => {}
        }
        self.media.due = Instant::now() + Duration::from_millis(250);
        if action == "close" {
            self.ui_state();
        }
    }

    /// The page says its media may have changed: it is probed soon, and for a few seconds.
    pub(crate) fn media_hinted(&mut self, tab: TabId) {
        if !self.tabs.contains_key(&tab) {
            return;
        }
        let now = Instant::now();
        self.media.hints.insert(tab.0, now);
        self.media.due = self.media.due.min(now + Duration::from_millis(250));
    }

    /// Whether the last probe of a tab heard it play something.
    /// Whether a hidden tab must keep its full resources (Windows: no "low" memory target): a
    /// call, media playing, its video floating, or seen busy a moment ago.
    pub(crate) fn keeps_running(&self, tab: TabId) -> bool {
        self.pip.tab == Some(tab)
            || self.media.busy_seen.contains_key(&tab)
            || self.media.probes
                .lock()
                .is_ok_and(|probes| probes.get(&tab.0).is_some_and(MediaProbe::keeps_page))
    }

    pub(crate) fn is_playing(&self, tab: TabId) -> bool {
        self.media.probes
            .lock()
            .is_ok_and(|probes| probes.get(&tab.0).is_some_and(|p| p.playing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listener_without_camera_or_microphone_is_in_the_call() {
        let listener = MediaProbe { call: true, ..MediaProbe::default() };
        assert!(listener.in_call());
        assert!(listener.keeps_page());
        let speaker = MediaProbe { mic: true, ..MediaProbe::default() };
        assert!(speaker.in_call());
        let music = MediaProbe { playing: true, ..MediaProbe::default() };
        assert!(!music.in_call());
        assert!(music.keeps_page());
        assert!(!MediaProbe::default().keeps_page());
    }

    #[test]
    fn a_short_gap_in_the_reports_keeps_the_card() {
        let t0 = Instant::now();
        let mut hold = Linger::new(Duration::from_secs(5));
        hold.note(1, t0, Some('▶'));
        hold.note(1, t0 + Duration::from_secs(2), None);
        assert_eq!(hold.shown(1, t0 + Duration::from_secs(2)), Some('▶'));
    }

    #[test]
    fn the_card_goes_once_the_media_has_been_gone_long_enough() {
        let t0 = Instant::now();
        let mut hold = Linger::new(Duration::from_secs(5));
        hold.note(1, t0, Some('▶'));
        assert_eq!(hold.shown(1, t0 + Duration::from_secs(6)), None);
        hold.prune(t0 + Duration::from_secs(6), |_| true);
        assert_eq!(hold.shown(1, t0), None);
    }

    #[test]
    fn the_newest_report_wins_and_closed_tabs_are_dropped() {
        let t0 = Instant::now();
        let mut hold = Linger::new(Duration::from_secs(5));
        hold.note(1, t0, Some('▶'));
        hold.note(1, t0 + Duration::from_secs(1), Some('⏸'));
        assert_eq!(hold.shown(1, t0 + Duration::from_secs(1)), Some('⏸'));
        hold.note(2, t0, Some('▶'));
        hold.prune(t0, |id| id == 1);
        assert_eq!(hold.shown(2, t0), None);
        hold.forget(1);
        assert_eq!(hold.shown(1, t0), None);
    }
}

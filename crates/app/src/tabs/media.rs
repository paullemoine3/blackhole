//! Keeps the sidebar's media card steady. Players often report "nothing" for a moment (an ad
//! starts, Twitch or YouTube rebuilds its player, a page navigates inside itself): without a
//! grace period the card disappears and slides back in a probe later, which reads as flicker.

use std::collections::HashMap;
use std::sync::Arc;
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

/// How often background tabs are asked whether they play audio or video.
const MEDIA_EVERY: Duration = Duration::from_millis(1500);

/// How long a tab that was in a call or playing media is still treated as such.
pub(crate) const BUSY_GRACE: Duration = Duration::from_secs(90);

/// How long a tab is probed after it said its media may have changed.
const MEDIA_HINT_FOR: Duration = Duration::from_secs(5);

impl Running {
    /// Audio/video state of the active space's tabs, for the speaker icon and the media card.
    pub(crate) fn media_json(&self) -> serde_json::Value {
        let now = Instant::now();
        let tabs = &self.svc.workspace().active_space().tabs;
        let list: Vec<_> = tabs
            .iter()
            .filter(|t| !self.media_dismissed.contains_key(&t.id.0))
            .filter_map(|t| {
                let p = self.media_hold.shown(t.id.0, now)?;
                Some(json!({
                    "id": t.id.0, "playing": p.playing, "muted": p.muted,
                    "cam": p.cam, "mic": p.mic, "camOn": p.cam_on, "micOn": p.mic_on,
                    "call": p.cam || p.mic || p.call,
                }))
            })
            .collect();
        json!(list)
    }

    /// Asks every loaded tab about its media, and refreshes the sidebar when something changed.
    pub(crate) fn service_media(&mut self) {
        if Instant::now() >= self.media_due {
            self.media_due = Instant::now()
                + if self.pip_seen {
                    Duration::from_millis(500)
                } else {
                    MEDIA_EVERY
                };
            if let Ok(mut probes) = self.media.lock() {
                probes.retain(|id, _| self.tabs.contains_key(&TabId(*id)));
            }
            // Only the tabs with something going on are asked: a script run in every live tab
            // every second and a half kept the idle ones (documents, sheets) awake for nothing.
            let now = Instant::now();
            self.media_hint
                .retain(|id, at| self.tabs.contains_key(&TabId(*id)) && now.duration_since(*at) < MEDIA_HINT_FOR);
            let active: Vec<u64> = self.media.lock().map_or_else(
                |_| Vec::new(),
                |probes| {
                    probes
                        .iter()
                        .filter(|(_, p)| p.playing || p.pip || p.cam || p.mic || p.call)
                        .map(|(id, _)| *id)
                        .collect()
                },
            );
            let floating = self.pip_tab.map(|t| t.0);
            for (id, view) in &self.tabs {
                let sink = Arc::clone(&self.media);
                let id = id.0;
                if !self.media_hint.contains_key(&id) && !active.contains(&id) && floating != Some(id) {
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
        let now = Instant::now();
        if let Ok(probes) = self.media.lock() {
            // A probe sent before the card was closed still says "playing": only a later one
            // can bring the card back.
            self.media_dismissed.retain(|id, at| {
                at.elapsed() < DISMISS_GRACE || !probes.get(id).is_some_and(|p| p.playing)
            });
            for (id, p) in probes.iter() {
                let shown = p.playing || p.has || p.cam || p.mic;
                self.media_hold.note(*id, now, shown.then_some(*p));
            }
        }
        self.media_hold
            .prune(now, |id| self.tabs.contains_key(&TabId(id)));
        // Fallback for engines that do not tell the page when its floating window is left.
        let floating = self
            .pip_tab
            .and_then(|tab| self.media.lock().ok()?.get(&tab.0).copied().map(|p| (tab, p)));
        if let Some((tab, probe)) = floating {
            if probe.pip {
                self.pip_seen = true;
            } else if self.pip_seen {
                self.pip_faded = Some((tab, Instant::now()));
                self.pip_returned(tab);
            }
        }
        let now = self.media_json();
        if now != self.media_seen {
            self.media_seen = now;
            self.ui_state();
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
                self.media_dismissed.insert(id.0, Instant::now());
                if self.pip_tab == Some(id) {
                    self.pip_tab = None;
                    self.pip_seen = false;
                }
                views::MEDIA_CLOSE_JS
            }
            _ => return,
        };
        // The card of a call (camera, microphone or a live connection): closing it leaves the
        // call too, instead of only pausing its tiles.
        if action == "close"
            && self
                .media
                .lock()
                .ok()
                .and_then(|probes| probes.get(&id.0).copied())
                .is_some_and(|p| p.cam || p.mic || p.call)
        {
            let _ = view.evaluate_script(views::CALL_HANG_UP_JS);
        }
        let _ = view.evaluate_script(js);
        self.media_hint.insert(id.0, Instant::now());
        match action {
            "pip" => self.pip_toggled(id),
            "close" => {
                self.pip_shell_exits.insert(id.0, Instant::now());
            }
            _ => {}
        }
        self.media_due = Instant::now() + Duration::from_millis(250);
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
        self.media_hint.insert(tab.0, now);
        self.media_due = self.media_due.min(now + Duration::from_millis(250));
    }

    /// Whether the last probe of a tab heard it play something.
    pub(crate) fn is_playing(&self, tab: TabId) -> bool {
        self.media
            .lock()
            .is_ok_and(|probes| probes.get(&tab.0).is_some_and(|p| p.playing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

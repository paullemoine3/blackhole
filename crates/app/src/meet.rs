//! Google Meet and the calendar sync: start or join a meeting (reusing the Meet tab of quick
//! access or of a folder), the next meeting in the sidebar, a reminder when one starts. The
//! calendars are fetched on a thread; the UI thread only reads the results.

use crate::{ipc::UiCommand, Running, TabId};
use blackhole_domain::meetings::{self, Meeting};
use blackhole_infrastructure::calendar::{self, CalendarSources};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How often the calendars are read again.
const SYNC_EVERY: Duration = Duration::from_secs(10 * 60);
/// Meetings kept: those not over 12 hours ago, up to a week ahead.
const KEEP_BEFORE: i64 = 12 * 3600;
const KEEP_AFTER: i64 = 7 * 24 * 3600;
/// The sidebar card shows a meeting from this long before it starts.
const CARD_LEAD: i64 = 15 * 60;
/// The command bar offers to join the meetings of the next hours.
const JOIN_AHEAD: i64 = 12 * 3600;

type SyncResult = (Vec<Meeting>, Vec<String>);

pub struct Calendar {
    sources: CalendarSources,
    meetings: Vec<Meeting>,
    status: String,
    busy: bool,
    due: Instant,
    tx: Sender<SyncResult>,
    rx: Receiver<SyncResult>,
    /// Reminders are given for meetings starting after this instant.
    remind_since: i64,
    /// The sidebar card as last sent, to refresh it only when it changes.
    card: Value,
}

impl Calendar {
    pub fn new(data: &std::path::Path) -> Self {
        let (tx, rx) = channel();
        Self {
            sources: CalendarSources::new(data),
            meetings: Vec::new(),
            status: String::new(),
            busy: false,
            due: Instant::now(),
            tx,
            rx,
            remind_since: now(),
            card: Value::Null,
        }
    }

    /// The sidebar card (`null` when there is none).
    pub fn card(&self) -> &Value {
        &self.card
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Running {
    /// Reads every calendar again, on a thread.
    pub(crate) fn calendar_sync(&mut self) {
        self.calendar.due = Instant::now() + SYNC_EVERY;
        if self.calendar.busy {
            return;
        }
        let sources = self.calendar.sources.load();
        if sources.is_empty() {
            self.calendar.meetings.clear();
            self.calendar.status.clear();
            return;
        }
        self.calendar.busy = true;
        self.calendar.status = "Syncing…".to_owned();
        let tx = self.calendar.tx.clone();
        std::thread::spawn(move || {
            let at = now();
            let (mut all, mut errors) = (Vec::new(), Vec::new());
            for source in sources {
                match calendar::fetch(&source) {
                    Ok(text) => all.extend(calendar::parse(&text, at - KEEP_BEFORE, at + KEEP_AFTER)),
                    Err(why) => errors.push(format!("{}: {why}", calendar::source_label(&source))),
                }
            }
            let _ = tx.send((all, errors));
        });
    }

    /// Called every turn of the event loop: results of a sync, the next sync, reminders and
    /// the sidebar card.
    pub(crate) fn service_calendar(&mut self) {
        let mut changed = false;
        while let Ok((meetings, errors)) = self.calendar.rx.try_recv() {
            self.calendar.busy = false;
            self.calendar.meetings = meetings;
            let week = meetings::upcoming(&self.calendar.meetings, now(), KEEP_AFTER).len();
            self.calendar.status = if errors.is_empty() {
                format!(
                    "Synced at {} · {week} meeting{} with Google Meet this week",
                    calendar::local_time(now()),
                    if week == 1 { "" } else { "s" }
                )
            } else {
                errors.join(" · ")
            };
            changed = true;
        }
        if changed {
            self.ui_settings();
        }
        if Instant::now() >= self.calendar.due {
            self.calendar_sync();
        }
        let at = now();
        let since = std::mem::replace(&mut self.calendar.remind_since, at);
        if self.svc.settings().meeting_reminders {
            for m in meetings::starting(&self.calendar.meetings, since, at) {
                self.toast_action(
                    &format!("{} starts now", m.title),
                    "Join",
                    json!({"cmd": "open_meet", "url": m.url}),
                );
            }
        }
        let card = if self.svc.settings().meeting_card {
            meetings::next_soon(&self.calendar.meetings, at, CARD_LEAD)
                .map_or(Value::Null, |m| card_json(&m, at))
        } else {
            Value::Null
        };
        if card != self.calendar.card {
            self.calendar.card = card;
            self.ui_state();
        }
    }

    /// Opens Google Meet on `url` (a room) or on a new meeting. The Meet tab of quick access
    /// or of a folder is reused, unless it is in a call: then a tab of its own opens.
    pub(crate) fn open_meet(&mut self, url: Option<String>) {
        if self.svc.settings().meeting_notes {
            if let Some(meeting) = url.as_deref().and_then(|u| self.meeting_at(u)) {
                self.open_meeting_notes(&meeting);
            }
        }
        let target = url
            .filter(|u| meetings::is_meet_room(u))
            .unwrap_or_else(|| meetings::NEW_MEET_URL.to_owned());
        let kept = self
            .svc
            .workspace()
            .active_space()
            .tabs
            .iter()
            .filter(|t| t.url.host_str() == Some(meetings::MEET_HOST) && (t.pinned || t.folder.is_some()))
            .max_by_key(|t| t.pinned)
            .map(|t| (t.id, t.url.to_string()));
        let Some((id, current)) = kept else {
            self.open(&target);
            return;
        };
        if self.in_call(id) {
            self.open(&target);
            return;
        }
        self.handle_ui(UiCommand::Activate { id: id.0 });
        let same_room = meetings::room_code(&current).is_some()
            && meetings::room_code(&current) == meetings::room_code(&target);
        if !same_room {
            if let Some(view) = self.tabs.get(&id) {
                let _ = view.load_url(&target);
            }
        }
    }

    /// The calendar meeting of a room closest to now (a room can be reused week after week).
    fn meeting_at(&self, url: &str) -> Option<Meeting> {
        let at = now();
        self.calendar
            .meetings
            .iter()
            .filter(|m| m.url == url)
            .min_by_key(|m| (m.start - at).abs())
            .cloned()
    }

    /// The meeting copilot: the meeting's notes in an Easel of their own ("Notes · title ·
    /// day"), made from its agenda and attendees the first time, shown in the Easel window
    /// beside the call.
    fn open_meeting_notes(&mut self, meeting: &Meeting) {
        let name = format!("Notes · {} · {}", meeting.title, calendar::local_day(meeting.start));
        let existing = self.easels.all().iter().find(|e| e.name == name).map(|e| e.id);
        let id = match existing {
            Some(id) => id,
            None => {
                let Some(id) = self.easels.create(&name) else {
                    self.toast("Too many easels for the meeting notes");
                    return;
                };
                let when = format!(
                    "{} · {}–{}",
                    calendar::local_day(meeting.start),
                    calendar::local_time(meeting.start),
                    calendar::local_time(meeting.end)
                );
                if let Some(item) = self.easels.add_note(id, Some((24.0, 24.0))) {
                    self.easels.set_text(id, item, &meeting.notes_template(&when));
                    self.easels.resize_item(id, item, 460.0, 520.0);
                }
                id
            }
        };
        self.easel_current = Some(id);
        self.tag_easel_space();
        self.easel_changed(true);
        self.easel_wanted = true;
    }

    fn in_call(&self, tab: TabId) -> bool {
        self.media
            .lock()
            .ok()
            .and_then(|probes| probes.get(&tab.0).copied())
            .is_some_and(|p| p.cam || p.mic)
    }

    /// Command bar actions: a new meeting, and joining the meetings of the next hours.
    pub(crate) fn meeting_actions(&self) -> Vec<Value> {
        let mut out = vec![json!({"id": "new_meet", "label": "New Google Meet", "hint": "Meet"})];
        out.extend(
            meetings::upcoming(&self.calendar.meetings, now(), JOIN_AHEAD)
                .into_iter()
                .map(|m| {
                    json!({
                        "id": format!("meet:{}", m.url),
                        "label": format!("Join {} · {}", m.title, calendar::local_time(m.start)),
                        "hint": "Meet",
                    })
                }),
        );
        out
    }

    /// Settings > Calendar.
    pub(crate) fn calendar_json(&self) -> Value {
        let settings = self.svc.settings();
        json!({
            "sources": self.calendar.sources.load().iter().map(|s| calendar::source_label(s)).collect::<Vec<_>>(),
            "status": self.calendar.status,
            "busy": self.calendar.busy,
            "reminders": settings.meeting_reminders,
            "card": settings.meeting_card,
            "notes": settings.meeting_notes,
            "upcoming": meetings::upcoming(&self.calendar.meetings, now(), KEEP_AFTER)
                .iter()
                .take(8)
                .map(|m| json!({"title": m.title, "at": calendar::local_time(m.start), "start": m.start, "url": m.url}))
                .collect::<Vec<_>>(),
        })
    }

    /// The calendar commands of the Settings and of the meeting card.
    pub(crate) fn handle_calendar(&mut self, command: UiCommand) {
        match command {
            UiCommand::OpenMeet { url } => self.open_meet(url),
            UiCommand::CalendarAdd { url } => {
                match self.calendar.sources.add(&url) {
                    Ok(()) => {
                        self.calendar.busy = false;
                        self.calendar_sync();
                    }
                    Err(why) => self.calendar.status = why,
                }
                self.ui_settings();
            }
            UiCommand::CalendarRemove { index } => {
                if let Err(why) = self.calendar.sources.remove(index) {
                    self.calendar.status = why;
                }
                self.calendar_sync();
                self.ui_settings();
                self.ui_state();
            }
            UiCommand::CalendarSync => {
                self.calendar_sync();
                self.ui_settings();
            }
            UiCommand::SetMeetingOption { name, on } if self.svc.set_meeting_option(&name, on) => {
                self.ui_settings();
            }
            _ => {}
        }
    }
}

/// The sidebar card of a meeting: its title, when, and the room to join.
fn card_json(m: &Meeting, at: i64) -> Value {
    let when = if m.is_on(at) {
        format!("Now · until {}", calendar::local_time(m.end))
    } else {
        let minutes = (m.start - at + 59) / 60;
        format!("In {minutes} min · {}", calendar::local_time(m.start))
    };
    json!({"title": m.title, "url": m.url, "when": when, "on": m.is_on(at)})
}

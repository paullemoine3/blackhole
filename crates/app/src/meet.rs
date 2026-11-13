//! Google Meet and the calendar sync: start or join a meeting (reusing the Meet tab of quick
//! access or of a folder), the next meeting in the sidebar, a reminder when one starts. The
//! calendars are fetched on a thread; the UI thread only reads the results.

use blackhole_domain::signals::Kind;
use crate::i18n::{message, t, tf};
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
/// And until this long after its start: it does not stay for the whole meeting.
const CARD_KEEP: i64 = 5 * 60;
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
    /// Meetings (room, start) whose card the user closed; not kept across launches.
    hidden: Vec<(String, i64)>,
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
            hidden: Vec::new(),
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
        self.calendar.status = message("settings.cal_syncing", &[]);
        let tx = self.calendar.tx.clone();
        std::thread::spawn(move || {
            let at = now();
            let (mut all, mut errors) = (Vec::new(), Vec::new());
            for source in sources {
                match calendar::fetch(&source) {
                    Ok(text) => {
                        let owner = calendar::source_label(&source);
                        all.extend(calendar::parse(&text, &owner, at - KEEP_BEFORE, at + KEEP_AFTER));
                    }
                    Err(why) => errors.push(message("settings.cal_error", &[("name", &calendar::source_label(&source)), ("why", &why)])),
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
                message(
                    "settings.cal_synced",
                    &[("time", &calendar::local_time(now())), ("n", &week.to_string())],
                )
            } else {
                errors.join("\n")
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
                self.notify_action(
                    crate::notices::Notice::Meeting,
                    &tf("toast.meeting_starts", &[("title", &m.title)]),
                    &t("toast.join"),
                    json!({"cmd": "open_meet", "url": m.url}),
                );
            }
        }
        let card = if self.svc.settings().meeting_card {
            meetings::next_soon(&self.calendar.meetings, at, CARD_LEAD, CARD_KEEP, &self.calendar.hidden)
                .map_or(Value::Null, |m| card_json(&m, at))
        } else {
            Value::Null
        };
        if card != self.calendar.card {
            self.calendar.card = card;
            self.ui_state();
        }
    }

    /// Opens Google Meet on `url` (a room) or on a new meeting, then, for a meeting of the
    /// calendar, its notes beside the call: offered by a toast the first time, shown again
    /// when they already exist (setting `meeting_notes`).
    pub(crate) fn open_meet(&mut self, url: Option<String>) {
        let meeting = url.as_deref().and_then(|u| self.meeting_at(u));
        self.go_to_meet(url);
        let Some(meeting) = meeting.filter(|_| self.svc.settings().meeting_notes) else {
            return;
        };
        if self.notes_easel(&meeting).is_some() {
            self.open_meeting_notes(&meeting);
        } else {
            self.notify_action(
                crate::notices::Notice::Meeting,
                &tf("toast.take_notes_for", &[("title", &meeting.title)]),
                &t("toast.take_notes"),
                json!({"cmd": "meeting_notes", "url": meeting.url, "start": meeting.start}),
            );
            // The offer stays as a card of the sidebar while the meeting is on.
            let key = format!("{}|{}", meeting.start, meeting.url);
            self.signal(Kind::Notes, &key, &meeting.title, &t("sidebar.signal_notes"));
        }
    }

    /// The Meet tab: a tab already in that room, else the one of quick access or of a folder,
    /// unless it is in a call.
    fn go_to_meet(&mut self, url: Option<String>) {
        let target = url
            .filter(|u| meetings::is_meet_room(u))
            .unwrap_or_else(|| meetings::NEW_MEET_URL.to_owned());
        if let Some(id) = self.meet_tab(&target) {
            self.handle_ui(UiCommand::Activate { id: id.0 });
            return;
        }
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
        if !meetings::in_room(&current, &target) {
            if let Some(view) = self.tabs.get(&id) {
                let _ = view.load_url(&target);
            }
        }
    }

    /// devtools: a calendar meeting going on now, without a calendar.
    #[cfg(feature = "devtools")]
    pub(crate) fn dev_add_meeting(&mut self, url: &str, title: &str) {
        let at = now();
        self.calendar.meetings.push(Meeting {
            title: title.to_owned(),
            start: at - 60,
            end: at + 1800,
            url: url.to_owned(),
            agenda: "Release\nDemo".to_owned(),
            attendees: vec!["Ana".to_owned()],
        });
    }

    /// The toast's "Take notes": the notes of that meeting beside its call.
    pub(crate) fn take_meeting_notes(&mut self, url: &str, start: i64) {
        let meeting = self
            .calendar
            .meetings
            .iter()
            .find(|m| m.url == url && m.start == start)
            .cloned();
        self.signals_forget(Kind::Notes, &format!("{start}|{url}"));
        if let Some(meeting) = meeting {
            self.open_meeting_notes(&meeting);
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

    fn notes_name(meeting: &Meeting) -> String {
        format!("Notes · {} · {}", meeting.title, calendar::local_day(meeting.start))
    }

    /// The Easel already holding a meeting's notes.
    fn notes_easel(&self, meeting: &Meeting) -> Option<u64> {
        let name = Self::notes_name(meeting);
        self.easels.all().iter().find(|e| e.name == name).map(|e| e.id)
    }

    /// The meeting copilot: the meeting's notes in an Easel of their own ("Notes · title ·
    /// day"), made from its agenda and attendees the first time, shown beside the call (split
    /// view) in the main window, or in the Easel window when no tab is in that room.
    fn open_meeting_notes(&mut self, meeting: &Meeting) {
        let name = Self::notes_name(meeting);
        let id = match self.notes_easel(meeting) {
            Some(id) => id,
            None => {
                let Some(id) = self.easels.create(&name) else {
                    self.toast(&t("toast.too_many_easels_notes"));
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
        match self.meet_tab(&meeting.url) {
            Some(tab) => self.open_easel_side(tab, id),
            None => self.easel_wanted = true,
        }
    }

    /// The tab of the active space in a Meet room: the one shown first.
    fn meet_tab(&self, url: &str) -> Option<TabId> {
        let ws = self.svc.workspace();
        let in_room = |t: &&blackhole_domain::Tab| meetings::in_room(t.url.as_str(), url);
        ws.active_tab()
            .filter(in_room)
            .or_else(|| ws.active_space().tabs.iter().find(in_room))
            .map(|t| t.id)
    }

    /// Whether the last probe of a tab saw its camera or microphone open (a call).
    pub(crate) fn in_call(&self, tab: TabId) -> bool {
        self.media
            .lock()
            .ok()
            .and_then(|probes| probes.get(&tab.0).copied())
            .is_some_and(|p| p.cam || p.mic)
    }

    /// Command bar actions: a new meeting, and joining the meetings of the next hours.
    pub(crate) fn meeting_actions(&self) -> Vec<Value> {
        let mut out = vec![json!({"id": "new_meet", "label": t("palette.new_meet"), "hint": "Meet"})];
        out.extend(
            meetings::upcoming(&self.calendar.meetings, now(), JOIN_AHEAD)
                .into_iter()
                .map(|m| {
                    json!({
                        "id": format!("meet:{}", m.url),
                        "label": tf("palette.join_meeting", &[("title", &m.title), ("time", &calendar::local_time(m.start))]),
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
            "status": self.calendar.status.split('\n').map(crate::i18n::render).collect::<Vec<_>>().join(" · "),
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
            UiCommand::MeetingNotes { url, start } => self.take_meeting_notes(&url, start),
            // The card's close button: that meeting only (the next one shows as usual).
            UiCommand::MeetingDismiss { url, start } => {
                let at = now();
                let meetings = &self.calendar.meetings;
                self.calendar
                    .hidden
                    .retain(|(u, s)| meetings.iter().any(|m| m.url == *u && m.start == *s && m.end > at));
                if meetings.iter().any(|m| m.url == url && m.start == start) {
                    self.calendar.hidden.push((url, start));
                }
                self.service_calendar();
            }
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
        tf("sidebar.meeting_now", &[("time", &calendar::local_time(m.end))])
    } else {
        let minutes = (m.start - at + 59) / 60;
        tf("sidebar.meeting_in", &[("n", &minutes.to_string()), ("time", &calendar::local_time(m.start))])
    };
    json!({"title": m.title, "url": m.url, "start": m.start, "when": when, "on": m.is_on(at)})
}

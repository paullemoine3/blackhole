//! Google Calendar sync through each calendar's *secret address in iCal format* (Google
//! Calendar > Settings > the calendar > Integrate calendar). No account sign-in: the address is
//! the only credential, so it is kept in an encrypted file (same key as the password vault) and
//! only ever sent to `calendar.google.com` over https. The meetings kept are the ones with a
//! Google Meet room.

use crate::store::Vault;
use blackhole_domain::meetings::{self, find_meet_link, Meeting};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

/// Encrypted list of the secret addresses.
const SOURCES_FILE: &str = "calendars.bin";
/// A calendar export bigger than this is refused (a year of a busy calendar is far below).
const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Occurrences of one recurring event expanded at most (a daily meeting over the window).
const MAX_OCCURRENCES: u16 = 400;

/// A secret iCal address of Google Calendar: https on `calendar.google.com`, an `.ics` file
/// under `/calendar/ical/`, nothing else (no user name, no other port).
pub fn is_valid_source(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw.trim()) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("calendar.google.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.path().starts_with("/calendar/ical/")
        && url.path().ends_with(".ics")
}

/// What the Settings show of a source: the calendar's id (often an e-mail address), never the
/// secret part of the address.
pub fn source_label(raw: &str) -> String {
    url::Url::parse(raw)
        .ok()
        .and_then(|u| {
            u.path_segments()?
                .nth(2)
                .map(percent_decode)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "Google Calendar".to_owned())
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The secret addresses, encrypted at rest.
pub struct CalendarSources(Vault);

impl CalendarSources {
    pub fn new(data: &std::path::Path) -> Self {
        Self(Vault::named(data, SOURCES_FILE))
    }

    #[cfg(test)]
    fn with_key(data: &std::path::Path) -> Self {
        Self(Vault::named_with_key(data, SOURCES_FILE, [5; 32]))
    }

    pub fn load(&self) -> Vec<String> {
        self.0.read_json().unwrap_or_default()
    }

    /// Adds a secret address (refused when it is not one; ignored when already there).
    pub fn add(&self, raw: &str) -> Result<(), String> {
        let raw = raw.trim();
        if !is_valid_source(raw) {
            return Err("Paste the calendar's secret address in iCal format (https://calendar.google.com/calendar/ical/…/basic.ics)".into());
        }
        let mut all = self.load();
        if !all.iter().any(|s| s == raw) {
            all.push(raw.to_owned());
            self.0.write_json(&all)?;
        }
        Ok(())
    }

    pub fn remove(&self, index: usize) -> Result<(), String> {
        let mut all = self.load();
        if index < all.len() {
            all.remove(index);
            self.0.write_json(&all)?;
        }
        Ok(())
    }
}

/// Downloads one calendar (https, `calendar.google.com` only, no redirect elsewhere, size cap).
pub fn fetch(raw: &str) -> Result<String, String> {
    if !is_valid_source(raw) {
        return Err("not a Google Calendar secret address".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .redirects(0)
        .build();
    let response = agent.get(raw).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "address not found (reset or deleted?)".to_owned(),
        ureq::Error::Status(code, _) => format!("HTTP {code}"),
        ureq::Error::Transport(t) => t.to_string(),
    })?;
    let mut text = String::new();
    response
        .into_reader()
        .take(MAX_BYTES)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if !text.trim_start().starts_with("BEGIN:VCALENDAR") {
        return Err("this address did not return a calendar".into());
    }
    Ok(text)
}

/// The Google Meet meetings of an iCal file that are not over at `from` and start before `to`
/// (Unix seconds). Handles time zones, recurring events (RRULE, EXDATE), moved or cancelled
/// occurrences (RECURRENCE-ID) and skips all-day events and events without a Meet room.
pub fn parse(ics: &str, from: i64, to: i64) -> Vec<Meeting> {
    let events = events(ics);
    // Occurrences of a series that were moved or cancelled, by series.
    let mut moved: HashMap<&str, Vec<i64>> = HashMap::new();
    for e in &events {
        if let (Some(uid), Some(rid)) = (e.get("UID"), e.line("RECURRENCE-ID").and_then(instant)) {
            moved.entry(uid).or_default().push(rid);
        }
    }
    let mut out = Vec::new();
    for e in &events {
        if e.get("STATUS") == Some("CANCELLED") {
            continue;
        }
        let (Some(start_line), Some(url)) = (e.line("DTSTART"), find_meet_link(&e.raw)) else {
            continue;
        };
        let Some(start) = instant(start_line) else {
            continue; // all-day or unreadable
        };
        let length = e
            .line("DTEND")
            .and_then(instant)
            .map_or(3600, |end| (end - start).max(0));
        let title = unescape(e.get("SUMMARY").unwrap_or("Meeting"));
        let agenda = meetings::agenda_of(&unescape_lines(e.get("DESCRIPTION").unwrap_or("")));
        let attendees: Vec<String> = e
            .all("ATTENDEE")
            .filter_map(|line| {
                let (params, value) = line.split_once(':')?;
                // The calendar's own resources (rooms, equipment) are not people.
                if value.ends_with("resource.calendar.google.com")
                    || params.contains("CUTYPE=RESOURCE")
                    || params.contains("CUTYPE=ROOM")
                {
                    return None;
                }
                let name = params
                    .split(';')
                    .find_map(|p| p.strip_prefix("CN="))
                    .map(|n| n.trim_matches('"').to_owned())
                    .unwrap_or_else(|| value.trim_start_matches("mailto:").to_owned());
                (!name.is_empty()).then_some(name)
            })
            .take(30)
            .collect();
        let starts: Vec<i64> = match e.line("RRULE") {
            Some(rule) if e.line("RECURRENCE-ID").is_none() => {
                let skip = e.get("UID").and_then(|u| moved.get(u));
                occurrences(e, start_line, rule, from - length, to)
                    .into_iter()
                    .filter(|s| !skip.is_some_and(|m| m.contains(s)))
                    .collect()
            }
            _ => vec![start],
        };
        for s in starts {
            if s + length > from && s < to {
                out.push(Meeting {
                    title: title.clone(),
                    start: s,
                    end: s + length,
                    url: url.clone(),
                    agenda: agenda.clone(),
                    attendees: attendees.clone(),
                });
            }
        }
    }
    out.sort_by_key(|m| m.start);
    out
}

/// One VEVENT: its unfolded lines.
struct Event {
    lines: Vec<String>,
    raw: String,
}

impl Event {
    /// The whole line of a property (`DTSTART;TZID=…:…`).
    fn line(&self, name: &str) -> Option<&str> {
        self.lines.iter().map(String::as_str).find(|l| prop_name(l) == name)
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.lines.iter().map(String::as_str).filter(move |l| prop_name(l) == name)
    }

    /// The value of a property.
    fn get(&self, name: &str) -> Option<&str> {
        self.line(name).and_then(|l| l.split_once(':')).map(|(_, v)| v.trim())
    }
}

fn prop_name(line: &str) -> &str {
    line.split([';', ':']).next().unwrap_or("")
}

fn events(ics: &str) -> Vec<Event> {
    // Long lines are folded: a line starting with a space or a tab continues the previous one.
    let mut lines: Vec<String> = Vec::new();
    for line in ics.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match line.strip_prefix([' ', '\t']) {
            Some(rest) if !lines.is_empty() => lines.last_mut().expect("checked").push_str(rest),
            _ => lines.push(line.to_owned()),
        }
    }
    let mut out = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in lines {
        match line.as_str() {
            "BEGIN:VEVENT" => current = Some(Vec::new()),
            "END:VEVENT" => {
                if let Some(lines) = current.take() {
                    let raw = lines.join("\n");
                    out.push(Event { lines, raw });
                }
            }
            _ => {
                if let Some(lines) = current.as_mut() {
                    lines.push(line);
                }
            }
        }
    }
    out
}

/// The instant of a `DTSTART`-like line, or `None` for a date without a time (all-day).
fn instant(line: &str) -> Option<i64> {
    let (params, value) = line.split_once(':')?;
    let value = value.trim();
    if params.contains("VALUE=DATE") && !params.contains("VALUE=DATE-TIME") || value.len() == 8 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(value.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?;
    if value.ends_with('Z') {
        return Some(Utc.from_utc_datetime(&naive).timestamp());
    }
    let tzid = params
        .split(';')
        .find_map(|p| p.strip_prefix("TZID="))
        .map(|t| t.trim_matches('"'));
    match tzid.and_then(|t| t.parse::<chrono_tz::Tz>().ok()) {
        Some(tz) => tz.from_local_datetime(&naive).earliest().map(|d| d.timestamp()),
        // Floating time (or an unknown zone): the computer's own.
        None => Local.from_local_datetime(&naive).earliest().map(|d| d.timestamp()),
    }
}

/// Starts of a recurring event between `from` and `to` (Unix seconds).
fn occurrences(e: &Event, start_line: &str, rule: &str, from: i64, to: i64) -> Vec<i64> {
    let mut text = format!("{start_line}\n{rule}");
    for ex in e.all("EXDATE") {
        // One EXDATE line may list several dates; the parser wants them with the line's params.
        text.push('\n');
        text.push_str(ex);
    }
    let Ok(set) = text.parse::<rrule::RRuleSet>() else {
        return Vec::new();
    };
    let at = |t: i64| {
        DateTime::<Utc>::from_timestamp(t, 0).map(|d| d.with_timezone(&rrule::Tz::UTC))
    };
    let (Some(after), Some(before)) = (at(from), at(to)) else {
        return Vec::new();
    };
    set.after(after)
        .before(before)
        .all(MAX_OCCURRENCES)
        .dates
        .iter()
        .map(|d| d.timestamp())
        .collect()
}

/// "Mon 9 Nov" in the computer's time zone, for meeting notes.
pub fn local_day(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%a %-d %b").to_string())
        .unwrap_or_default()
}

/// "14:05" in the computer's time zone, for meeting labels.
pub fn local_time(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// A text value with its new lines kept (an event's description).
fn unescape_lines(text: &str) -> String {
    text.replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// Text values escape `,` `;` `\` and new lines.
fn unescape(text: &str) -> String {
    text.replace("\\n", " ")
        .replace("\\N", " ")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = "https://meet.google.com/abc-defg-hij";

    fn cal(events: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{events}END:VCALENDAR\r\n")
    }

    fn ts(text: &str) -> i64 {
        DateTime::parse_from_rfc3339(text).unwrap().timestamp()
    }

    #[test]
    fn only_secret_google_calendar_addresses_are_accepted() {
        let ok = "https://calendar.google.com/calendar/ical/me%40example.com/private-0123abcd/basic.ics";
        assert!(is_valid_source(ok));
        assert_eq!(source_label(ok), "me@example.com");
        for bad in [
            "http://calendar.google.com/calendar/ical/x/private-y/basic.ics",
            "https://calendar.google.com.evil.test/calendar/ical/x/private-y/basic.ics",
            "https://evil.test/calendar/ical/x/private-y/basic.ics",
            "https://user@calendar.google.com/calendar/ical/x/private-y/basic.ics",
            "https://calendar.google.com/calendar/u/0/r",
            "not a url",
        ] {
            assert!(!is_valid_source(bad), "{bad}");
        }
    }

    #[test]
    fn sources_are_kept_encrypted_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let s = CalendarSources::with_key(dir.path());
        let url = "https://calendar.google.com/calendar/ical/a%40b.c/private-1/basic.ics";
        s.add(url).unwrap();
        s.add(url).unwrap();
        assert!(s.add("https://evil.test/a.ics").is_err());
        assert_eq!(s.load(), [url]);
        let file = std::fs::read(dir.path().join(SOURCES_FILE)).unwrap();
        assert!(!String::from_utf8_lossy(&file).contains("private-1"), "stored encrypted");
        s.remove(0).unwrap();
        assert!(s.load().is_empty());
    }

    #[test]
    fn a_meet_event_in_utc_with_a_folded_description() {
        let ics = cal(&format!(
            "BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Weekly sync\\, team\r\nDTSTART:20261109T090000Z\r\nDTEND:20261109T093000Z\r\nDESCRIPTION:Join with Google Meet: {}\r\n -defg-hij\\nMore\r\nEND:VEVENT\r\n",
            "https://meet.google.com/abc"
        ));
        let list = parse(&ics, ts("2026-11-08T00:00:00Z"), ts("2026-11-16T00:00:00Z"));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Weekly sync, team");
        assert_eq!(list[0].url, ROOM);
        assert_eq!(list[0].start, ts("2026-11-09T09:00:00Z"));
        assert_eq!(list[0].end - list[0].start, 1800);
    }

    #[test]
    fn the_agenda_and_the_attendees_are_read_for_the_meeting_notes() {
        let ics = cal(&format!(
            "BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Review\r\nDTSTART:20261109T090000Z\r\nDTEND:20261109T100000Z\r\n\
             ATTENDEE;CN=Paul Lemoine;ROLE=REQ-PARTICIPANT:mailto:paul@example.com\r\n\
             ATTENDEE;ROLE=OPT-PARTICIPANT:mailto:ana@example.com\r\n\
             ATTENDEE;CN=Room 4;CUTYPE=RESOURCE:mailto:c_123@resource.calendar.google.com\r\n\
             DESCRIPTION:Release notes\\nDemo\\n\\n-::~:~::~:~:~\\nJoin with Google Meet: {ROOM}\r\nEND:VEVENT\r\n"
        ));
        let list = parse(&ics, ts("2026-11-08T00:00:00Z"), ts("2026-11-16T00:00:00Z"));
        assert_eq!(list[0].agenda, "Release notes\nDemo");
        assert_eq!(list[0].attendees, ["Paul Lemoine", "ana@example.com"]);
    }

    #[test]
    fn time_zones_all_day_events_and_events_without_meet_are_handled() {
        let ics = cal(&format!(
            "BEGIN:VEVENT\r\nUID:tz\r\nSUMMARY:Paris\r\nDTSTART;TZID=Europe/Paris:20261109T140000\r\nDTEND;TZID=Europe/Paris:20261109T150000\r\nLOCATION:{ROOM}\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:day\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261110\r\nDESCRIPTION:{ROOM}\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:none\r\nSUMMARY:Lunch\r\nDTSTART:20261109T120000Z\r\nEND:VEVENT\r\n"
        ));
        let list = parse(&ics, ts("2026-11-08T00:00:00Z"), ts("2026-11-16T00:00:00Z"));
        assert_eq!(list.len(), 1);
        // 14:00 in Paris in November is 13:00 UTC.
        assert_eq!(list[0].start, ts("2026-11-09T13:00:00Z"));
    }

    #[test]
    fn recurring_meetings_skip_excluded_moved_and_cancelled_occurrences() {
        let ics = cal(&format!(
            "BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/Paris:20261102T093000\r\nDTEND;TZID=Europe/Paris:20261102T094500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\r\nEXDATE;TZID=Europe/Paris:20261111T093000\r\nDESCRIPTION:{ROOM}\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:standup\r\nRECURRENCE-ID;TZID=Europe/Paris:20261113T093000\r\nSUMMARY:Standup (moved)\r\nDTSTART;TZID=Europe/Paris:20261113T110000\r\nDTEND;TZID=Europe/Paris:20261113T111500\r\nDESCRIPTION:{ROOM}\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:gone\r\nSTATUS:CANCELLED\r\nSUMMARY:Gone\r\nDTSTART:20261109T100000Z\r\nDESCRIPTION:{ROOM}\r\nEND:VEVENT\r\n"
        ));
        let list = parse(&ics, ts("2026-11-09T00:00:00Z"), ts("2026-11-14T00:00:00Z"));
        let starts: Vec<(String, i64)> = list.iter().map(|m| (m.title.clone(), m.start)).collect();
        assert_eq!(
            starts,
            [
                ("Standup".to_owned(), ts("2026-11-09T08:30:00Z")),
                ("Standup (moved)".to_owned(), ts("2026-11-13T10:00:00Z")),
            ]
        );
    }
}

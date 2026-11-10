//! Video meetings read from the user's calendars (Google Meet): which one is next, which one
//! starts now. Times are Unix seconds (UTC); reading the calendars is the infrastructure's job.

/// Host of Google Meet.
pub const MEET_HOST: &str = "meet.google.com";
/// Address that starts a new meeting.
pub const NEW_MEET_URL: &str = "https://meet.google.com/new";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meeting {
    pub title: String,
    pub start: i64,
    pub end: i64,
    /// The meeting's Google Meet address.
    pub url: String,
    /// The event's description (its agenda), without the joining instructions.
    pub agenda: String,
    /// Names (or addresses) of the people invited.
    pub attendees: Vec<String>,
}

impl Meeting {
    pub fn is_on(&self, now: i64) -> bool {
        self.start <= now && now < self.end
    }

    /// The text of a meeting's notes, ready to fill (the meeting copilot's page in the Easel).
    /// `when` is the time span as the user reads it ("Mon 9 Nov · 09:30–09:45").
    pub fn notes_template(&self, when: &str) -> String {
        let mut text = format!("{}\n{when}\n", self.title);
        if !self.attendees.is_empty() {
            text.push_str(&format!("With: {}\n", self.attendees.join(", ")));
        }
        if !self.agenda.trim().is_empty() {
            text.push_str(&format!("\nAgenda\n{}\n", self.agenda.trim()));
        }
        text.push_str("\nNotes\n- \n\nDecisions\n- \n\nNext steps\n- ");
        text
    }
}

/// A guest's answer to an invitation (iCal `PARTSTAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    NeedsAction,
    Accepted,
    Tentative,
    Declined,
    Delegated,
}

impl Reply {
    pub fn from_partstat(value: &str) -> Self {
        match value.trim().to_ascii_uppercase().as_str() {
            "ACCEPTED" => Self::Accepted,
            "TENTATIVE" => Self::Tentative,
            "DECLINED" => Self::Declined,
            "DELEGATED" => Self::Delegated,
            _ => Self::NeedsAction,
        }
    }
}

/// Whether a calendar event is still on its owner's calendar. Google's iCal export keeps the
/// events the owner removed: removing an invitation only declines it, so a series removed years
/// ago keeps repeating in the export. Cancelled events and those the owner (the calendar's
/// address) declined are gone; `guests` are the event's (address, reply) pairs.
pub fn is_on_calendar(cancelled: bool, owner: &str, guests: &[(&str, Reply)]) -> bool {
    let declined = guests.iter().any(|(address, reply)| {
        *reply == Reply::Declined
            && address
                .trim()
                .trim_start_matches("mailto:")
                .trim_start_matches("MAILTO:")
                .eq_ignore_ascii_case(owner.trim())
    });
    !cancelled && !declined
}

/// Google adds the joining instructions to an event's description between `-::~:~::~` lines:
/// they are not the agenda.
pub fn agenda_of(description: &str) -> String {
    let cut = description.find("-::~").unwrap_or(description.len());
    description[..cut].trim().to_owned()
}

/// A Google Meet room address (`https://meet.google.com/abc-defg-hij`), nothing else on that host.
pub fn is_meet_room(url: &str) -> bool {
    room_code(url).is_some()
}

/// The `abc-defg-hij` code of a Meet room address.
pub fn room_code(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://meet.google.com/")?;
    let code = rest.split(['?', '#', '/']).next()?;
    let parts: Vec<&str> = code.split('-').collect();
    let ok = parts.len() == 3
        && [3, 4, 3]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_lowercase()));
    ok.then_some(code)
}

/// Whether a tab's address is in the Meet room `room` (a room address). Meet may rewrite the
/// address it was opened on (`/meet/abc-defg-hij`, `?authuser=…`): the room code is its last
/// path segment.
pub fn in_room(tab_url: &str, room: &str) -> bool {
    let Some(code) = room_code(room) else {
        return false;
    };
    let Some(rest) = tab_url.strip_prefix("https://meet.google.com/") else {
        return false;
    };
    let path = rest.split(['?', '#']).next().unwrap_or("");
    path.trim_end_matches('/').rsplit('/').next() == Some(code)
}

/// The first Meet room address found in a text (an event's description, location…).
pub fn find_meet_link(text: &str) -> Option<String> {
    let mut rest = text;
    while let Some(at) = rest.find("https://meet.google.com/") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '<' | '>' | ',' | ';' | ')' | '\\'))
            .unwrap_or(tail.len());
        let candidate = &tail[..end];
        if let Some(code) = room_code(candidate) {
            return Some(format!("https://meet.google.com/{code}"));
        }
        rest = &tail["https://".len()..];
    }
    None
}

/// Meetings not over yet that start within `horizon` seconds, soonest first; the same room at
/// the same time (an event in two calendars) is kept once.
pub fn upcoming(meetings: &[Meeting], now: i64, horizon: i64) -> Vec<Meeting> {
    let mut list: Vec<Meeting> = meetings
        .iter()
        .filter(|m| m.end > now && m.start <= now + horizon)
        .cloned()
        .collect();
    list.sort_by(|a, b| (a.start, &a.title).cmp(&(b.start, &b.title)));
    list.dedup_by(|a, b| a.url == b.url && a.start == b.start);
    list
}

/// The meeting to show in the sidebar: the next one starting within `lead` seconds, or one that
/// started less than `keep` seconds ago (its card does not stay for the whole meeting). `hidden`
/// are the meetings (room, start) whose card the user closed.
pub fn next_soon(meetings: &[Meeting], now: i64, lead: i64, keep: i64, hidden: &[(String, i64)]) -> Option<Meeting> {
    upcoming(meetings, now, lead)
        .into_iter()
        .find(|m| now < m.start + keep && !hidden.iter().any(|(url, start)| *url == m.url && *start == m.start))
}

/// The meetings starting between `since` (excluded) and `now` (included): the ones to remind.
pub fn starting(meetings: &[Meeting], since: i64, now: i64) -> Vec<Meeting> {
    upcoming(meetings, now, 0)
        .into_iter()
        .filter(|m| m.start > since && m.start <= now)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(title: &str, start: i64, end: i64, code: &str) -> Meeting {
        Meeting {
            title: title.into(),
            start,
            end,
            url: format!("https://meet.google.com/{code}"),
            agenda: String::new(),
            attendees: Vec::new(),
        }
    }

    #[test]
    fn the_notes_start_from_the_meeting_agenda_and_attendees() {
        let mut meeting = m("Standup", 0, 900, "sss-ssss-sss");
        meeting.agenda = agenda_of("Release 0.7.18\nPiP bugs\n-::~:~::~:~:~\nJoin with Google Meet: …");
        meeting.attendees = vec!["Paul".into(), "Ana".into()];
        let text = meeting.notes_template("Mon 9 Nov · 09:30–09:45");
        assert!(text.starts_with("Standup\nMon 9 Nov · 09:30–09:45\nWith: Paul, Ana\n"));
        assert!(text.contains("Agenda\nRelease 0.7.18\nPiP bugs\n"));
        assert!(!text.contains("Join with Google Meet"));
        assert!(text.ends_with("Next steps\n- "));
        let bare = m("1:1", 0, 900, "aaa-aaaa-aaa").notes_template("now");
        assert!(!bare.contains("With:") && !bare.contains("Agenda"));
    }

    #[test]
    fn cancelled_and_declined_events_are_not_on_the_calendar() {
        let me = "paul@example.com";
        let accepted = [
            ("mailto:Paul@Example.com", Reply::Accepted),
            ("mailto:ana@example.com", Reply::Declined),
        ];
        assert!(is_on_calendar(false, me, &accepted));
        assert!(is_on_calendar(false, me, &[]));
        assert!(!is_on_calendar(true, me, &accepted));
        let removed = [
            ("mailto:PAUL@example.com", Reply::Declined),
            ("mailto:ana@example.com", Reply::Accepted),
        ];
        assert!(!is_on_calendar(false, me, &removed));
        assert_eq!(Reply::from_partstat("declined"), Reply::Declined);
        assert_eq!(Reply::from_partstat("X-OTHER"), Reply::NeedsAction);
    }

    #[test]
    fn only_real_meet_rooms_are_recognised() {
        assert_eq!(room_code("https://meet.google.com/abc-defg-hij?authuser=0"), Some("abc-defg-hij"));
        assert!(!is_meet_room("https://meet.google.com/new"));
        assert!(!is_meet_room("https://meet.google.com.evil.test/abc-defg-hij"));
        assert!(!is_meet_room("http://meet.google.com/abc-defg-hij"));
        assert!(!is_meet_room("https://meet.google.com/ABC-defg-hij"));
    }

    #[test]
    fn a_tab_is_in_a_room_even_when_meet_rewrote_its_address() {
        let room = "https://meet.google.com/abc-defg-hij";
        assert!(in_room(room, room));
        assert!(in_room("https://meet.google.com/meet/abc-defg-hij?authuser=0", room));
        assert!(in_room("https://meet.google.com/abc-defg-hij/", room));
        assert!(!in_room("https://meet.google.com/xyz-abcd-efg", room));
        assert!(!in_room("https://meet.google.com/", room));
        assert!(!in_room("https://evil.test/abc-defg-hij", room));
        assert!(!in_room(room, "https://meet.google.com/new"));
    }

    #[test]
    fn the_meet_link_is_found_in_an_event_text() {
        let text = "Join with Google Meet: https://meet.google.com/abc-defg-hij\\nOr dial: +33 1";
        assert_eq!(find_meet_link(text).as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        let help = "Learn more about Meet at: https://support.google.com/a/users/answer/9282720";
        assert_eq!(find_meet_link(help), None);
        assert_eq!(
            find_meet_link("<https://meet.google.com/new> then https://meet.google.com/xyz-abcd-efg>").as_deref(),
            Some("https://meet.google.com/xyz-abcd-efg")
        );
    }

    #[test]
    fn upcoming_meetings_are_sorted_and_deduplicated() {
        let list = [
            m("B", 200, 260, "bbb-bbbb-bbb"),
            m("Over", 0, 50, "ooo-oooo-ooo"),
            m("A", 120, 180, "aaa-aaaa-aaa"),
            m("A again", 120, 180, "aaa-aaaa-aaa"),
            m("Far", 10_000, 10_100, "fff-ffff-fff"),
        ];
        let up = upcoming(&list, 100, 1_000);
        assert_eq!(up.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    }

    #[test]
    fn the_sidebar_shows_the_meeting_going_on_or_starting_soon() {
        let list = [m("Now", 90, 150, "nnn-nnnn-nnn"), m("Later", 400, 500, "lll-llll-lll")];
        assert_eq!(next_soon(&list, 100, 600, 30, &[]).unwrap().title, "Now");
        assert!(list[0].is_on(100));
        assert_eq!(next_soon(&list, 200, 600, 30, &[]).unwrap().title, "Later");
        assert_eq!(next_soon(&list, 200, 60, 30, &[]), None);
    }

    #[test]
    fn the_card_of_a_meeting_leaves_a_while_after_its_start() {
        let list = [m("Long", 100, 4_000, "nnn-nnnn-nnn"), m("Later", 400, 500, "lll-llll-lll")];
        assert_eq!(next_soon(&list, 399, 600, 300, &[]).unwrap().title, "Long");
        // Five minutes in, the meeting still goes on: the next one takes the card.
        assert!(list[0].is_on(400));
        assert_eq!(next_soon(&list, 400, 600, 300, &[]).unwrap().title, "Later");
        assert_eq!(next_soon(&list, 400, 0, 300, &[]).unwrap().title, "Later");
        assert_eq!(next_soon(&list, 700, 600, 300, &[]), None);
    }

    #[test]
    fn a_card_the_user_closed_stays_closed_for_that_meeting_only() {
        let list = [m("Weekly", 100, 200, "www-wwww-www"), m("Weekly", 1_000, 1_100, "www-wwww-www")];
        let hidden = [(list[0].url.clone(), 100)];
        assert_eq!(next_soon(&list, 90, 60, 300, &hidden), None);
        assert_eq!(next_soon(&list, 90, 2_000, 300, &hidden).unwrap().start, 1_000);
    }

    #[test]
    fn a_meeting_is_reminded_once_when_it_starts() {
        let list = [m("Standup", 100, 160, "sss-ssss-sss")];
        assert!(starting(&list, 50, 99).is_empty());
        assert_eq!(starting(&list, 99, 100).len(), 1);
        assert!(starting(&list, 100, 130).is_empty());
    }
}

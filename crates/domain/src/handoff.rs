//! "Continue on another device": the rules shared by the two ends, without the network. A tab
//! travels as its address only (never cookies or a session), plus the playback position of a
//! video on YouTube or Twitch, so the other device resumes where this one stopped.

use std::collections::VecDeque;
use url::Url;

/// The pairing code: 12 characters of Crockford base32 (60 bits), shown as `ABCD-EFGH-JKMN`.
pub const CODE_CHARS: usize = 12;
const CODE_BITS: u32 = 5 * CODE_CHARS as u32;
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A message older (or further in the future, clocks differ) than this is refused: a copy
/// recorded on the network cannot be played again later.
pub const MAX_AGE_SECS: i64 = 10 * 60;

/// Keeps the 60 low bits of a random number: the secret of one pairing.
#[must_use]
pub fn code_from_random(random: u64) -> u64 {
    random & ((1 << CODE_BITS) - 1)
}

/// `ABCD-EFGH-JKMN`: groups of four, easy to read out and to type on the other device.
#[must_use]
pub fn format_code(code: u64) -> String {
    let mut out = String::with_capacity(CODE_CHARS + 2);
    for i in 0..CODE_CHARS {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        let shift = 5 * (CODE_CHARS - 1 - i);
        out.push(ALPHABET[((code >> shift) & 31) as usize] as char);
    }
    out
}

/// The code typed by the user: case, dashes and spaces do not matter, and the letters that
/// look like digits are read as them (O → 0, I and L → 1), as Crockford base32 does.
#[must_use]
pub fn parse_code(typed: &str) -> Option<u64> {
    let mut code = 0_u64;
    let mut count = 0;
    for c in typed.chars().filter(|c| !matches!(c, '-' | ' ' | '\t')) {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let value = ALPHABET.iter().position(|&a| a as char == c)?;
        code = (code << 5) | value as u64;
        count += 1;
        if count > CODE_CHARS {
            return None;
        }
    }
    (count == CODE_CHARS).then_some(code)
}

/// Only web pages travel (no `file:`, extension or internal page).
#[must_use]
pub fn can_continue(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
}

fn youtube_video(url: &Url) -> bool {
    match url.host_str() {
        Some("youtu.be") => url.path().len() > 1,
        Some("youtube.com" | "www.youtube.com" | "m.youtube.com") => {
            url.path() == "/watch" && url.query_pairs().any(|(k, v)| k == "v" && !v.is_empty())
        }
        _ => false,
    }
}

fn twitch_video(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("twitch.tv" | "www.twitch.tv" | "m.twitch.tv")
    ) && url.path().starts_with("/videos/")
}

/// True when the page is a video whose position can be resumed elsewhere (a live stream has
/// none: the other device simply joins it live).
#[must_use]
pub fn has_position(url: &Url) -> bool {
    youtube_video(url) || twitch_video(url)
}

/// The address to open on the other device: a YouTube or Twitch video starts at `seconds`
/// (the `t` parameter both sites read); any other page, or no position, is left as it is.
#[must_use]
pub fn resume_url(url: &Url, seconds: Option<f64>) -> Url {
    let Some(secs) = seconds.filter(|s| s.is_finite() && *s >= 1.0) else {
        return url.clone();
    };
    let secs = secs as u64;
    let t = if youtube_video(url) {
        format!("{secs}s")
    } else if twitch_video(url) {
        format!("{}h{}m{}s", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        return url.clone();
    };
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "t")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut out = url.clone();
    out.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("t", &t);
    out
}

/// True when a message sent at `sent` (Unix seconds) may still be accepted at `now`.
#[must_use]
pub fn is_fresh(sent: i64, now: i64) -> bool {
    (now - sent).abs() <= MAX_AGE_SECS
}

/// The identifiers of the last messages received, so one sent twice (a retry whose answer
/// was lost, or a copy replayed on the network) opens its tab only once.
#[derive(Debug, Default)]
pub struct Seen {
    ids: VecDeque<u64>,
}

impl Seen {
    const KEEP: usize = 256;

    /// True the first time `id` is seen.
    pub fn first_time(&mut self, id: u64) -> bool {
        if self.ids.contains(&id) {
            return false;
        }
        if self.ids.len() == Self::KEEP {
            self.ids.pop_front();
        }
        self.ids.push_back(id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn a_code_reads_back_whatever_the_case_dashes_and_lookalikes() {
        let code = code_from_random(u64::MAX - 12_345);
        let shown = format_code(code);
        assert_eq!(shown.len(), 14);
        assert_eq!(shown.matches('-').count(), 2);
        assert_eq!(parse_code(&shown), Some(code));
        assert_eq!(
            parse_code(&shown.to_lowercase().replace('-', " ")),
            Some(code)
        );
        assert_eq!(format_code(0), "0000-0000-0000");
        assert_eq!(parse_code("oooo-iiii-llll"), parse_code("0000-1111-1111"));
    }

    #[test]
    fn a_code_of_the_wrong_length_or_alphabet_is_refused() {
        assert_eq!(parse_code(""), None);
        assert_eq!(parse_code("ABCD-EFGH-JKM"), None);
        assert_eq!(parse_code("ABCD-EFGH-JKMNP"), None);
        assert_eq!(
            parse_code("ABCD-EFGH-JKMU"),
            None,
            "U is not in the alphabet"
        );
        assert!(code_from_random(u64::MAX) < 1 << 60);
    }

    #[test]
    fn only_web_pages_continue() {
        assert!(can_continue(&u("https://example.com/a")));
        assert!(!can_continue(&u("file:///C:/a.txt")));
        assert!(!can_continue(&u("about:blank")));
        assert!(!can_continue(&u("chrome-extension://abc/popup.html")));
    }

    #[test]
    fn a_youtube_video_resumes_at_its_position() {
        let watch = u("https://www.youtube.com/watch?v=abc&t=5s&list=x");
        assert!(has_position(&watch));
        assert_eq!(
            resume_url(&watch, Some(125.7)).as_str(),
            "https://www.youtube.com/watch?v=abc&list=x&t=125s"
        );
        assert_eq!(
            resume_url(&u("https://youtu.be/abc"), Some(61.0)).as_str(),
            "https://youtu.be/abc?t=61s"
        );
        assert!(!has_position(&u("https://www.youtube.com/")));
        assert!(!has_position(&u("https://www.youtube.com/watch")));
    }

    #[test]
    fn a_twitch_video_resumes_but_a_live_channel_does_not() {
        let vod = u("https://www.twitch.tv/videos/123");
        assert_eq!(
            resume_url(&vod, Some(3723.0)).as_str(),
            "https://www.twitch.tv/videos/123?t=1h2m3s"
        );
        let live = u("https://www.twitch.tv/somechannel");
        assert!(!has_position(&live));
        assert_eq!(resume_url(&live, Some(3723.0)), live);
    }

    #[test]
    fn other_pages_and_missing_positions_are_left_alone() {
        let page = u("https://example.com/watch?v=1");
        assert_eq!(resume_url(&page, Some(40.0)), page);
        let video = u("https://www.youtube.com/watch?v=abc");
        assert_eq!(resume_url(&video, None), video);
        assert_eq!(resume_url(&video, Some(0.4)), video);
        assert_eq!(resume_url(&video, Some(f64::NAN)), video);
    }

    #[test]
    fn old_or_replayed_messages_are_refused() {
        assert!(is_fresh(1_000, 1_000 + MAX_AGE_SECS));
        assert!(
            is_fresh(1_000 + 30, 1_000),
            "a clock a little ahead is fine"
        );
        assert!(!is_fresh(1_000, 1_001 + MAX_AGE_SECS));
        let mut seen = Seen::default();
        assert!(seen.first_time(7));
        assert!(!seen.first_time(7));
        for id in 100..100 + Seen::KEEP as u64 {
            seen.first_time(id);
        }
        assert!(seen.first_time(7), "only the last ones are remembered");
    }
}

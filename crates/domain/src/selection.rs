//! Text selected in a page (right click > Search / Open address). The text comes from an
//! untrusted page: it is bounded and normalised here before anything is built from it.

use url::{Host, Url};

use crate::omnibox::{web_address, SearchEngine};

/// Longest selection kept, in characters (a search query, not a document).
pub const MAX_SELECTION: usize = 500;

/// What a selection opens: the address it reads as, or a search for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionAction {
    Open(Url),
    Search(Url),
}

impl SelectionAction {
    pub fn url(&self) -> &Url {
        match self {
            Self::Open(url) | Self::Search(url) => url,
        }
    }
}

/// The selection as one line: whitespace runs (line breaks, tabs) become one space, control and
/// formatting characters go, and it is cut at `MAX_SELECTION` characters. `None` when empty.
pub fn clean_selection(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut count = 0;
    for word in raw
        .split(char::is_whitespace)
        .map(|w| w.chars().filter(|c| !is_invisible(*c)).collect::<String>())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            if count + 1 >= MAX_SELECTION {
                break;
            }
            out.push(' ');
            count += 1;
        }
        for c in word.chars() {
            if count == MAX_SELECTION {
                break;
            }
            out.push(c);
            count += 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Control characters, and the invisible ones that can disguise text (bidi overrides, zero
/// width characters, BOM).
fn is_invisible(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

/// What right click > the selection entry does with `text` (already cleaned): open it when it
/// is a web address, search it with `engine` otherwise. Only http(s) is ever opened.
pub fn selection_action(text: &str, engine: SearchEngine) -> SelectionAction {
    match selection_address(text) {
        Some(url) => SelectionAction::Open(url),
        None => SelectionAction::Search(engine.search_url(text)),
    }
}

/// The address a selection reads as, stricter than the command bar: the text around it in a
/// sentence (quotes, brackets, final full stop) is left out, and an e-mail address, a word with
/// a one-letter ending ("e.g") or a decimal number ("3.14") stays a search.
fn selection_address(text: &str) -> Option<Url> {
    let candidate = text
        .trim_start_matches(|c: char| "\"'“‘«([<".contains(c))
        .trim_end_matches(|c: char| "\"'”’»)]>.,;:!?".contains(c));
    if candidate.contains(char::is_whitespace) {
        return None;
    }
    // "javascript://…", "file:///…": a scheme other than http(s) is never turned into a host.
    if let Some((scheme, _)) = candidate.split_once("://") {
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return None;
        }
    }
    let url = web_address(candidate)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let typed = candidate
        .split_once("://")
        .map_or(candidate, |(_, rest)| rest);
    let plausible = match url.host()? {
        Host::Domain(domain) => {
            let tld = domain.rsplit('.').next().unwrap_or_default();
            domain == "localhost" || (tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
        }
        // "3.14" parses as 3.0.0.14: an IP address must be written out in full.
        Host::Ipv4(_) => typed.split(['/', ':']).next().unwrap_or_default().matches('.').count() == 3,
        Host::Ipv6(_) => true,
    };
    plausible.then_some(url)
}

/// The start of `text` for a menu label: `max` characters at most, an ellipsis when cut.
pub fn excerpt(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(text: &str) -> SelectionAction {
        selection_action(&clean_selection(text).unwrap(), SearchEngine::DuckDuckGo)
    }

    #[test]
    fn whitespace_is_normalised_and_controls_dropped() {
        assert_eq!(
            clean_selection("  Black\n\n hole\t \u{202E}sun\u{0007} ").as_deref(),
            Some("Black hole sun")
        );
        assert_eq!(clean_selection(" \n\t\u{200B} "), None);
    }

    #[test]
    fn selection_is_capped() {
        let long = "word ".repeat(400);
        let clean = clean_selection(&long).unwrap();
        assert!(clean.chars().count() <= MAX_SELECTION);
        assert!(!clean.ends_with(' '));
        let one = "é".repeat(MAX_SELECTION + 10);
        assert_eq!(clean_selection(&one).unwrap().chars().count(), MAX_SELECTION);
    }

    #[test]
    fn text_is_searched_and_encoded() {
        let SelectionAction::Search(url) = action("rust & co #1 100%") else {
            panic!("search expected");
        };
        assert_eq!(url.host_str(), Some("duckduckgo.com"));
        assert_eq!(url.query(), Some("q=rust+%26+co+%231+100%25"));
        let brave = selection_action("a b", SearchEngine::Brave);
        assert_eq!(brave.url().as_str(), "https://search.brave.com/search?q=a+b");
    }

    #[test]
    fn addresses_are_opened() {
        let open = |t: &str| match action(t) {
            SelectionAction::Open(url) => url.to_string(),
            other => panic!("{t}: {other:?}"),
        };
        assert_eq!(open("example.com/x"), "https://example.com/x");
        assert_eq!(open("(https://www.rust-lang.org/learn)."), "https://www.rust-lang.org/learn");
        assert_eq!(open("“example.org”"), "https://example.org/");
        assert_eq!(open("localhost:3000"), "http://localhost:3000/");
        assert_eq!(open("192.168.1.90:7878/a"), "http://192.168.1.90:7878/a");
        assert_eq!(open("HTTPS://Example.com"), "https://example.com/");
    }

    #[test]
    fn dangerous_or_unlikely_addresses_are_searched() {
        for text in [
            "javascript:alert(1)",
            "javascript:alert(document.domain)",
            "javascript://example.com/%0aalert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///C:/Windows/win.ini",
            "jane@example.com",
            "mailto:jane@example.com",
            "e.g.",
            "3.14",
            "go to example.com",
        ] {
            let result = action(text);
            assert!(
                matches!(&result, SelectionAction::Search(u) if u.host_str() == Some("duckduckgo.com")),
                "{text}: {result:?}"
            );
        }
    }

    #[test]
    fn excerpt_cuts_on_characters() {
        assert_eq!(excerpt("short", 30), "short");
        assert_eq!(excerpt("abcdefghij", 5), "abcd…");
        assert_eq!(excerpt("ééé ééé", 5), "ééé…");
    }
}

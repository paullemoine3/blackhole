//! Search engines by keyword: `gh rust` or `!gh rust` searches GitHub straight from the command
//! bar. A bare keyword needs at least two characters and something to search; `!gh` alone opens
//! the site, and `!gh` may also come last (`rust !gh`). An unknown `!bang` is searched as typed
//! (DuckDuckGo understands its own).

use url::Url;

use crate::{omnibox, SearchEngine};

/// Where the words go in an address.
pub const SLOT: &str = "%s";
pub const MAX_ENGINES: usize = 60;
const KEYWORD_MAX: usize = 16;
const NAME_MAX: usize = 40;
const URL_MAX: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordEngine {
    pub keyword: String,
    pub name: String,
    /// An http(s) address with `SLOT` after its host.
    pub url: String,
}

/// Why an engine is refused (its text key: `settings.kw_bad_<id>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Keyword,
    Url,
    Full,
}

impl Refused {
    pub fn id(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Url => "url",
            Self::Full => "full",
        }
    }
}

/// What a query asks of an engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit<'a> {
    pub engine: &'a KeywordEngine,
    /// The words searched; empty opens the site.
    pub words: String,
    pub url: Url,
}

/// The engines of a new profile.
pub fn defaults() -> Vec<KeywordEngine> {
    [
        ("wiki", "Wikipedia", "https://www.wikipedia.org/w/index.php?search=%s"),
        ("yt", "YouTube", "https://www.youtube.com/results?search_query=%s"),
        ("gh", "GitHub", "https://github.com/search?q=%s"),
        ("mdn", "MDN", "https://developer.mozilla.org/search?q=%s"),
        ("osm", "OpenStreetMap", "https://www.openstreetmap.org/search?query=%s"),
        ("so", "Stack Overflow", "https://stackoverflow.com/search?q=%s"),
    ]
    .map(|(keyword, name, url)| KeywordEngine { keyword: keyword.into(), name: name.into(), url: url.into() })
    .into()
}

/// A keyword as it is stored: lowercase letters, digits, `-` and `_`, without the `!`.
fn clean_keyword(raw: &str) -> Option<String> {
    let k = raw.trim().trim_start_matches('!').to_lowercase();
    let ok = !k.is_empty()
        && k.chars().count() <= KEYWORD_MAX
        && k.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'));
    ok.then_some(k)
}

/// An engine as typed in the Settings (or stored), checked. A missing name is the site's.
pub fn clean(keyword: &str, name: &str, url: &str) -> Result<KeywordEngine, Refused> {
    let keyword = clean_keyword(keyword).ok_or(Refused::Keyword)?;
    let raw = url.trim();
    let raw = if raw.contains("://") { raw.to_owned() } else { format!("https://{raw}") };
    // The words must never choose the host the query goes to.
    let parsed = Url::parse(&raw.replace(SLOT, "bh-words")).map_err(|_| Refused::Url)?;
    let host = parsed.host_str().unwrap_or_default();
    if raw.len() > URL_MAX
        || !raw.contains(SLOT)
        || !matches!(parsed.scheme(), "http" | "https")
        || host.is_empty()
        || host.contains("bh-words")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(Refused::Url);
    }
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    let name = if name.is_empty() { host.trim_start_matches("www.").to_owned() } else { name };
    Ok(KeywordEngine { keyword, name, url: raw })
}

/// Adds an engine, or replaces the one with its keyword.
pub fn add(list: &mut Vec<KeywordEngine>, engine: KeywordEngine) -> Result<(), Refused> {
    match list.iter().position(|e| e.keyword == engine.keyword) {
        Some(at) => list[at] = engine,
        None if list.len() >= MAX_ENGINES => return Err(Refused::Full),
        None => list.push(engine),
    }
    Ok(())
}

impl KeywordEngine {
    /// The results of `words`, or the site itself when there is none.
    pub fn url_for(&self, words: &str) -> Option<Url> {
        if words.is_empty() {
            let mut site = Url::parse(&self.url.replace(SLOT, "")).ok()?;
            site.set_path("/");
            site.set_query(None);
            site.set_fragment(None);
            return Some(site);
        }
        Url::parse(&self.url.replace(SLOT, &encode(words))).ok()
    }
}

/// Everything but unreserved characters is escaped: the words stay one value wherever they go.
fn encode(words: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(words.len());
    for b in words.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The engine `input` names, if any.
pub fn find<'a>(input: &str, engines: &'a [KeywordEngine]) -> Option<Hit<'a>> {
    let input = input.trim();
    let of = |k: &str| {
        let k = k.to_lowercase();
        engines.iter().find(|e| e.keyword == k)
    };
    let (first, rest) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
    let rest = rest.trim();
    let hit = |engine: &'a KeywordEngine, words: &str| {
        Some(Hit { engine, words: words.to_owned(), url: engine.url_for(words)? })
    };
    if let Some(engine) = first.strip_prefix('!').and_then(of) {
        return hit(engine, rest);
    }
    if let Some((words, last)) = input.rsplit_once(char::is_whitespace) {
        if let Some(engine) = last.strip_prefix('!').and_then(of) {
            return hit(engine, words.trim());
        }
    }
    if rest.is_empty() || first.chars().count() < 2 {
        return None;
    }
    of(first).and_then(|engine| hit(engine, rest))
}

/// What the user typed, as a navigable address: a keyword first, then `omnibox::resolve_input`.
pub fn resolve(input: &str, engines: &[KeywordEngine], engine: SearchEngine) -> Url {
    find(input, engines).map_or_else(|| omnibox::resolve_input(input, engine), |h| h.url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to(input: &str) -> String {
        resolve(input, &defaults(), SearchEngine::DuckDuckGo).to_string()
    }

    #[test]
    fn keywords_and_bangs_search_their_engine() {
        assert_eq!(to("gh rust wry"), "https://github.com/search?q=rust%20wry");
        assert_eq!(to("GH rust"), "https://github.com/search?q=rust");
        assert_eq!(to("!gh rust"), "https://github.com/search?q=rust");
        assert_eq!(to("rust lang !gh"), "https://github.com/search?q=rust%20lang");
        assert_eq!(to("!gh"), "https://github.com/", "a bang alone opens the site");
    }

    #[test]
    fn anything_else_is_searched_or_opened_as_before() {
        assert!(to("gh").starts_with("https://duckduckgo.com/"), "a bare keyword is a word");
        assert!(to("!zz rust").starts_with("https://duckduckgo.com/?q=%21zz"));
        assert!(to("github rust").starts_with("https://duckduckgo.com/"));
        assert_eq!(to("github.com"), "https://github.com/");
        let one = [clean("w", "", "https://w.test/%s").unwrap()];
        assert!(find("w something", &one).is_none(), "one letter needs its bang");
        assert_eq!(find("!w a/b", &one).unwrap().url.as_str(), "https://w.test/a%2Fb");
    }

    #[test]
    fn the_words_cannot_leave_their_place() {
        assert_eq!(to("gh a&b=c #d"), "https://github.com/search?q=a%26b%3Dc%20%23d");
        for bad in ["https://%s.test/", "https://user@x.test/%s", "javascript:%s", "https://x.test/", "", "https:///%s"] {
            assert_eq!(clean("ok", "", bad), Err(Refused::Url), "{bad}");
        }
        for bad in ["", "!", "a b", "a/b", "aaaaaaaaaaaaaaaaa"] {
            assert_eq!(clean(bad, "", "https://x.test/?q=%s"), Err(Refused::Keyword), "{bad}");
        }
    }

    #[test]
    fn engines_are_added_once_per_keyword() {
        let mut list = defaults();
        let n = list.len();
        let mine = clean("!GH", "  ", "www.mine.test/?q=%s").unwrap();
        assert_eq!((mine.keyword.as_str(), mine.name.as_str()), ("gh", "mine.test"));
        add(&mut list, mine).unwrap();
        assert_eq!(list.len(), n);
        assert_eq!(find("gh x", &list).unwrap().engine.name, "mine.test");
        for i in n..MAX_ENGINES {
            add(&mut list, clean(&format!("k{i}"), "", "x.test/%s").unwrap()).unwrap();
        }
        assert_eq!(add(&mut list, clean("more", "", "x.test/%s").unwrap()), Err(Refused::Full));
        for e in defaults() {
            assert_eq!(clean(&e.keyword, &e.name, &e.url), Ok(e.clone()), "a default passes its own check");
        }
    }
}

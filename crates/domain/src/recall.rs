//! Recall: the text of the pages read is remembered on this computer, so a page can be found
//! again from a few words of what it said. Pure rules: what may be remembered, how a query is
//! matched and ranked, where a page is filed. The store (encrypted, in `infrastructure`) and
//! the reading of pages (`app`) follow them.
//!
//! Off by default; never in a private window nor a black hole tab; never a page with a
//! password field (the shell does not even read it); never a site the user excluded.

use std::collections::BTreeSet;
use url::Url;

/// Characters of a page's text and title that are kept.
pub const TEXT_MAX: usize = 12_000;
pub const TITLE_MAX: usize = 200;
/// A page with less text than this is not worth remembering (a redirect, an empty shell).
pub const TEXT_MIN: usize = 200;
/// A query: its length and how many words of it are used.
pub const QUERY_MAX: usize = 200;
pub const MAX_TERMS: usize = 8;
/// Results shown for a query.
pub const MAX_HITS: usize = 20;
/// Pages are filed in `SHARDS` files of at most `PER_SHARD` pages each (the oldest leave).
pub const SHARDS: u8 = 64;
pub const PER_SHARD: usize = 32;
/// Characters of a result's excerpt.
const SNIPPET: usize = 160;

/// A remembered page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Its address without the fragment.
    pub url: String,
    pub title: String,
    pub text: String,
    /// When it was read last, Unix seconds.
    pub at: i64,
}

/// A page that matches a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub url: String,
    pub title: String,
    /// The part of the text where the query was found.
    pub snippet: String,
    pub at: i64,
    pub score: u32,
}

/// Whether the page at `url` may be remembered at all: a public web page. Addresses with
/// credentials, local hosts (a router, a NAS, a dev server) and bare IPs are left alone.
pub fn indexable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.host(), Some(url::Host::Domain(_)))
        && !crate::privacy::is_local(url)
}

/// The key a page is remembered under: its address without the fragment.
pub fn page_key(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.into()
}

/// Whether `host` is on the user's list of sites never remembered (the site itself or any
/// host under it; `www.` ignored).
pub fn excluded(host: &str, list: &BTreeSet<String>) -> bool {
    let host = crate::site_key(host);
    list.iter().any(|site| {
        host == *site || host.strip_suffix(site.as_str()).is_some_and(|rest| rest.ends_with('.'))
    })
}

/// A site as it is written in the exclusion list: a host name, lower case, without `www.`;
/// `None` for anything else (an address is reduced to its host).
pub fn site_of(input: &str) -> Option<String> {
    let input = input.trim();
    let host = match Url::parse(input) {
        Ok(url) if url.host_str().is_some() => url.host_str().unwrap_or_default().to_owned(),
        _ => input.split('/').next().unwrap_or_default().to_owned(),
    };
    let site = crate::site_key(&host);
    let valid = site.contains('.')
        && site.len() <= 253
        && site.split('.').all(|label| {
            !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    valid.then_some(site)
}

/// One character folded for matching: lower case, common Latin accents removed. Always one
/// character for one, so positions in the folded text are positions in the text.
fn fold_char(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    match lower {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '’' => '\'',
        other => other,
    }
}

pub fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// The words of a query, folded, without duplicates.
pub fn terms(query: &str) -> Vec<String> {
    let query: String = query.chars().take(QUERY_MAX).collect();
    let mut out: Vec<String> = Vec::new();
    for word in fold(&query).split_whitespace() {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric());
        if !word.is_empty() && !out.iter().any(|w| w == word) && out.len() < MAX_TERMS {
            out.push(word.to_owned());
        }
    }
    out
}

/// What is kept of a page's text: one line, bounded (see `highlights::clean`).
pub fn clean_text(raw: &str) -> String {
    crate::highlights::clean(raw, TEXT_MAX)
}

/// How well a page answers the words of a query; `None` unless every word is found in its
/// title, its address or its text. Words in the title weigh most.
pub fn score(page: &Page, terms: &[String]) -> Option<u32> {
    if terms.is_empty() {
        return None;
    }
    let (title, url, text) = (fold(&page.title), fold(&page.url), fold(&page.text));
    let mut total = 0;
    for term in terms {
        let in_title = title.contains(term.as_str());
        let in_url = url.contains(term.as_str());
        let in_text = text.matches(term.as_str()).take(5).count() as u32;
        if !in_title && !in_url && in_text == 0 {
            return None;
        }
        total += u32::from(in_title) * 8 + u32::from(in_url) * 3 + in_text;
    }
    // The whole query as written, in the text: the page says exactly that.
    if terms.len() > 1 && text.contains(&terms.join(" ")) {
        total += 6;
    }
    Some(total)
}

/// The part of `text` around the first word of the query found in it (the start of the text
/// when none is).
pub fn snippet(text: &str, terms: &[String]) -> String {
    let folded: Vec<char> = text.chars().map(fold_char).collect();
    let chars: Vec<char> = text.chars().collect();
    let found = terms
        .iter()
        .filter_map(|term| {
            let needle: Vec<char> = term.chars().collect();
            folded.windows(needle.len().max(1)).position(|w| w == needle.as_slice())
        })
        .min()
        .unwrap_or(0);
    let mut start = found.saturating_sub(SNIPPET / 3);
    // Starts on a word.
    while start > 0 && start < chars.len() && !chars[start - 1].is_whitespace() && found - start < SNIPPET / 2 {
        start -= 1;
    }
    let end = (start + SNIPPET).min(chars.len());
    let mut out: String = chars[start..end].iter().collect();
    if start > 0 {
        out.insert(0, '…');
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// The hit of a page for a query, if it matches.
pub fn hit(page: &Page, terms: &[String]) -> Option<Hit> {
    let score = score(page, terms)?;
    Some(Hit {
        url: page.url.clone(),
        title: page.title.clone(),
        snippet: snippet(&page.text, terms),
        at: page.at,
        score,
    })
}

/// Best first (then the page read last), at most `MAX_HITS`.
pub fn rank(hits: &mut Vec<Hit>) {
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(b.at.cmp(&a.at)).then_with(|| a.url.cmp(&b.url)));
    hits.truncate(MAX_HITS);
}

/// The file a page is kept in: a stable function of its key (FNV-1a), the same on every run.
pub fn shard_of(key: &str) -> u8 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in key.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    (hash % u32::from(SHARDS)) as u8
}

/// Files `page` among the pages of its shard: it replaces the page of the same address, and
/// the pages read longest ago leave beyond `PER_SHARD`.
pub fn upsert(pages: &mut Vec<Page>, page: Page) {
    pages.retain(|p| p.url != page.url);
    pages.push(page);
    if pages.len() > PER_SHARD {
        pages.sort_by_key(|p| std::cmp::Reverse(p.at));
        pages.truncate(PER_SHARD);
    }
}

/// A page ready to be remembered from what was read of it (untrusted text): `None` when the
/// address may not be remembered or there is too little text.
pub fn page(url: &Url, title: &str, text: &str, at: i64) -> Option<Page> {
    if !indexable(url) {
        return None;
    }
    let text = clean_text(text);
    if text.chars().count() < TEXT_MIN {
        return None;
    }
    Some(Page {
        url: page_key(url),
        title: crate::highlights::clean(title, TITLE_MAX),
        text,
        at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(raw: &str) -> Url {
        Url::parse(raw).unwrap()
    }

    fn p(url: &str, title: &str, text: &str, at: i64) -> Page {
        Page { url: url.into(), title: title.into(), text: text.into(), at }
    }

    #[test]
    fn only_public_web_pages_are_remembered() {
        assert!(indexable(&u("https://example.com/article")));
        for raw in [
            "http://localhost:3000/",
            "http://192.168.1.1/admin",
            "https://8.8.8.8/",
            "http://nas/",
            "http://printer.local/",
            "https://user:pw@example.com/",
            "file:///home/me/notes.txt",
            "about:blank",
        ] {
            assert!(!indexable(&u(raw)), "{raw}");
        }
        let long = "word ".repeat(100);
        let page = page(&u("https://example.com/a#part"), " Title\n", &long, 5).unwrap();
        assert_eq!((page.url.as_str(), page.title.as_str()), ("https://example.com/a", "Title"));
        assert!(super::page(&u("https://example.com/a"), "T", "too short", 5).is_none());
        assert!(super::page(&u("http://localhost/"), "T", &long, 5).is_none());
        let huge = "x".repeat(TEXT_MAX * 2);
        assert_eq!(super::page(&u("https://example.com/"), "", &huge, 0).unwrap().text.len(), TEXT_MAX);
    }

    #[test]
    fn excluded_sites_cover_their_subdomains_only() {
        let list: BTreeSet<String> = ["bank.test".to_owned(), "mail.example.com".to_owned()].into();
        assert!(excluded("bank.test", &list));
        assert!(excluded("www.bank.test", &list));
        assert!(excluded("login.bank.test", &list));
        assert!(excluded("mail.example.com", &list));
        assert!(!excluded("example.com", &list));
        assert!(!excluded("notbank.test", &list));
        assert!(!excluded("bank.test.evil.io", &list));
        assert_eq!(site_of(" https://WWW.Bank.test/login?x=1 ").as_deref(), Some("bank.test"));
        assert_eq!(site_of("mail.example.com/inbox").as_deref(), Some("mail.example.com"));
        for bad in ["", "localhost", "a b.com", "<script>", "exa mple.com", ".."] {
            assert_eq!(site_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn queries_ignore_case_accents_and_repeats() {
        assert_eq!(terms("  Évaporation des TROUS  noirs, évaporation! "), ["evaporation", "des", "trous", "noirs"]);
        assert!(terms(" ,;! ").is_empty());
        assert_eq!(terms(&"a b c d e f g h i j k".repeat(3)).len(), MAX_TERMS);
        assert_eq!(fold("L’Été À Noël").chars().count(), "L’Été À Noël".chars().count());
    }

    #[test]
    fn every_word_must_be_found_and_the_title_weighs_most() {
        let q = terms("hawking radiation");
        let titled = p("https://a.test/1", "Hawking radiation explained", "Black holes are not entirely black.", 1);
        let mention = p("https://a.test/2", "Black holes", "Stephen Hawking showed that radiation escapes.", 2);
        let exact = p("https://a.test/3", "Black holes", "This is called Hawking radiation, and it is faint.", 3);
        let partial = p("https://a.test/4", "Radiation", "Alpha, beta and gamma.", 4);
        let (s1, s2, s3) = (score(&titled, &q).unwrap(), score(&mention, &q).unwrap(), score(&exact, &q).unwrap());
        assert!(s1 > s3 && s3 > s2, "{s1} {s3} {s2}");
        assert_eq!(score(&partial, &q), None);
        assert_eq!(score(&titled, &[]), None);
        // Found by its address alone, and whatever the accents.
        assert!(score(&p("https://physics.test/hawking", "", "", 0), &terms("HAWKING")).is_some());
        assert!(score(&p("https://a.test/", "Évaporation", "", 0), &terms("evaporation")).is_some());
        let mut hits: Vec<Hit> = [&mention, &titled, &exact].iter().filter_map(|p| hit(p, &q)).collect();
        rank(&mut hits);
        let order: Vec<&str> = hits.iter().map(|h| h.url.as_str()).collect();
        assert_eq!(order, ["https://a.test/1", "https://a.test/3", "https://a.test/2"]);
    }

    #[test]
    fn the_excerpt_shows_where_the_words_are() {
        let text = format!("{} The event horizon is the point of no return. {}", "Intro. ".repeat(60), "Outro. ".repeat(60));
        let s = snippet(&text, &terms("HORIZON"));
        assert!(s.contains("event horizon"), "{s}");
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.chars().count() <= SNIPPET + 2);
        assert_eq!(snippet("Short text.", &terms("nothing")), "Short text.");
        assert_eq!(snippet("", &terms("x")), "");
        // Accents in the text do not shift the excerpt.
        assert!(snippet("Été à Noël, l’évaporation continue.", &terms("evaporation")).contains("évaporation"));
    }

    #[test]
    fn pages_are_filed_in_stable_shards_and_the_oldest_leave() {
        assert_eq!(shard_of("https://a.test/"), shard_of("https://a.test/"));
        assert!((0..200).map(|i| shard_of(&format!("https://a.test/{i}"))).all(|s| s < SHARDS));
        let spread: BTreeSet<u8> = (0..400).map(|i| shard_of(&format!("https://site{i}.test/"))).collect();
        assert!(spread.len() > usize::from(SHARDS) / 2, "{}", spread.len());
        let mut pages = Vec::new();
        for i in 0..PER_SHARD as i64 + 3 {
            upsert(&mut pages, p(&format!("https://a.test/{i}"), "", "", i));
        }
        assert_eq!(pages.len(), PER_SHARD);
        assert!(pages.iter().all(|p| p.at >= 3));
        upsert(&mut pages, p("https://a.test/10", "again", "", 99));
        assert_eq!(pages.len(), PER_SHARD);
        assert_eq!(pages.iter().filter(|p| p.url == "https://a.test/10").count(), 1);
    }
}

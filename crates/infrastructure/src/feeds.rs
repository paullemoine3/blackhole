//! Feeds of the Pulsars: asked without cookies, read (RSS, RDF, Atom), found from the address of
//! a page, and kept in `pulsars.json`. Everything that comes from a server is bounded, and is
//! text only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use blackhole_domain::i18n::message;
use blackhole_domain::privacy::is_local;
use blackhole_domain::pulsar::{self, Entry, Line, Pulsar, Pulsars};
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::updater::read_limited;

const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Entries read from a feed: the rest is older.
const MAX_ENTRIES: usize = 200;
/// Where a site that does not announce its feed often has it.
const GUESSES: [&str; 4] = ["/feed", "/rss.xml", "/atom.xml", "/index.xml"];

/// A feed as it was read.
#[derive(Debug, PartialEq, Eq)]
pub struct Feed {
    pub title: String,
    pub entries: Vec<Entry>,
}

/// The answer of a server.
pub struct Answer {
    /// `None`: nothing new since the marks given.
    pub feed: Option<Feed>,
    pub etag: String,
    pub modified: String,
}

struct Page {
    bytes: Vec<u8>,
    /// The address after redirections.
    url: Url,
    etag: String,
    modified: String,
    unchanged: bool,
}

/// Blocking. No cookie, no referrer: the server sees a request for a feed, nothing of the user's.
fn get(url: &Url, etag: &str, modified: &str) -> Result<Page, String> {
    let unreachable = || message("msg.pulsar_unreachable", &[("host", url.host_str().unwrap_or_default())]);
    let agent = ureq::AgentBuilder::new()
        .user_agent(concat!("Mozilla/5.0 BlackHole/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .redirects(5)
        .build();
    let mut req = agent
        .get(url.as_str())
        .set("Accept", "application/atom+xml,application/rss+xml,application/xml;q=0.9,text/html;q=0.7,*/*;q=0.5");
    if !etag.is_empty() {
        req = req.set("If-None-Match", etag);
    }
    if !modified.is_empty() {
        req = req.set("If-Modified-Since", modified);
    }
    let resp = req.call().map_err(|_| unreachable())?;
    let header = |name: &str| resp.header(name).unwrap_or_default().chars().take(300).collect::<String>();
    let (etag, modified) = (header("ETag"), header("Last-Modified"));
    let landed = Url::parse(resp.get_url()).map_err(|_| unreachable())?;
    let unchanged = resp.status() == 304;
    let bytes = if unchanged { Vec::new() } else { read_limited(resp, MAX_BYTES).map_err(|_| unreachable())? };
    Ok(Page { bytes, url: landed, etag, modified, unchanged })
}

/// Blocking: the feed of a Pulsar, asked again with the marks of last time.
pub fn refresh(feed: &Url, etag: &str, modified: &str) -> Result<Answer, String> {
    let page = get(feed, etag, modified)?;
    let feed = if page.unchanged {
        None
    } else {
        Some(parse(&text_of(&page.bytes), &page.url).ok_or_else(|| message("msg.pulsar_no_feed", &[]))?)
    };
    Ok(Answer { feed, etag: page.etag, modified: page.modified })
}

/// Blocking: the feed `input` (the address of a feed, or of a site) leads to.
pub fn find(input: &str) -> Result<(Url, Answer), String> {
    let none = || message("msg.pulsar_no_feed", &[]);
    let asked = blackhole_domain::omnibox::web_address(input).ok_or_else(|| message("msg.pulsar_address", &[]))?;
    let first = pulsar::known_feed(&asked).unwrap_or_else(|| asked.clone());
    let page = get(&first, "", "")?;
    let text = text_of(&page.bytes);
    let answer = |page: &Page, feed: Feed| Answer {
        feed: Some(feed),
        etag: page.etag.clone(),
        modified: page.modified.clone(),
    };
    if let Some(feed) = parse(&text, &page.url) {
        return Ok((page.url.clone(), answer(&page, feed)));
    }
    let mut candidates: Vec<Url> = announced(&text, &page.url).into_iter().collect();
    candidates.extend(GUESSES.iter().filter_map(|g| page.url.join(g).ok()));
    for candidate in candidates {
        // A page of the web does not send BlackHole to a machine of the local network.
        if is_local(&candidate) && !is_local(&asked) {
            continue;
        }
        let Ok(page) = get(&candidate, "", "") else { continue };
        if let Some(feed) = parse(&text_of(&page.bytes), &page.url) {
            return Ok((page.url.clone(), answer(&page, feed)));
        }
    }
    Err(none())
}

/// UTF-8, or the Latin encoding the file says it has.
fn text_of(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.trim_start_matches('\u{feff}').to_owned();
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_ascii_lowercase();
    if head.contains("iso-8859-1") || head.contains("iso-8859-15") || head.contains("windows-1252") {
        bytes.iter().map(|b| *b as char).collect()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// The first feed a page announces (`<link rel="alternate" type="application/rss+xml" href>`).
fn announced(html: &str, base: &Url) -> Option<Url> {
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while let Some(start) = lower[at..].find("<link").map(|i| at + i) {
        let end = lower[start..].find('>').map_or(lower.len(), |i| start + i);
        let tag = &html[start..end];
        let kind = attr(tag, "type").unwrap_or_default().to_ascii_lowercase();
        if matches!(kind.as_str(), "application/rss+xml" | "application/atom+xml")
            && attr(tag, "rel").is_some_and(|r| r.to_ascii_lowercase().contains("alternate"))
        {
            let found = attr(tag, "href").and_then(|h| base.join(&h.replace("&amp;", "&")).ok());
            if let Some(url) = found.filter(|u| matches!(u.scheme(), "http" | "https")) {
                return Some(url);
            }
        }
        at = end;
    }
    None
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    loop {
        let at = from + lower[from..].find(name)?;
        from = at + name.len();
        if !lower[..at].ends_with(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(value) = rest.strip_prefix('=') else { continue };
        let value = value.trim_start();
        return match value.chars().next()? {
            q @ ('"' | '\'') => value[1..].split(q).next().map(str::to_owned),
            _ => value.split(|c: char| c.is_whitespace() || c == '/').next().map(str::to_owned),
        };
    }
}

#[derive(Default)]
struct Draft {
    title: String,
    link: String,
    key: String,
    date: String,
}

/// Reads a feed; `None` when the text is not one.
pub fn parse(text: &str, base: &Url) -> Option<Feed> {
    let mut reader = Reader::from_str(text);
    let config = reader.config_mut();
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    let mut path: Vec<String> = Vec::new();
    let (mut is_feed, mut title) = (false, String::new());
    let mut entries = Vec::new();
    let mut draft: Option<Draft> = None;
    let mut buf = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local(&e);
                if path.is_empty() {
                    is_feed = matches!(name.as_str(), "rss" | "feed" | "rdf");
                    if !is_feed {
                        return None;
                    }
                }
                if matches!(name.as_str(), "item" | "entry") {
                    draft = Some(Draft::default());
                }
                atom_link(&name, &e, draft.as_mut());
                path.push(name);
                buf.clear();
                // A feed is not deep: anything else is an attack on the reader.
                if path.len() > 100 {
                    return None;
                }
            }
            Ok(Event::Empty(e)) => atom_link(&local(&e), &e, draft.as_mut()),
            Ok(Event::Text(t)) => buf.push_str(&t.decode().ok()?),
            Ok(Event::CData(t)) => buf.push_str(&t.decode().ok()?),
            Ok(Event::GeneralRef(r)) => buf.push_str(&resolve(&r)),
            Ok(Event::End(_)) => {
                let Some(name) = path.pop() else { continue };
                let parent = path.last().map(String::as_str).unwrap_or_default();
                let value = std::mem::take(&mut buf);
                if matches!(name.as_str(), "item" | "entry") {
                    if let Some(entry) = draft.take().and_then(|d| finish(d, base)) {
                        entries.push(entry);
                    }
                    if entries.len() >= MAX_ENTRIES {
                        break;
                    }
                } else if matches!(parent, "item" | "entry") {
                    if let Some(d) = draft.as_mut() {
                        match name.as_str() {
                            // `media:title` and the like come after the entry's own.
                            "title" if d.title.is_empty() => d.title = plain(&value),
                            "link" if d.link.is_empty() => d.link = value.trim().to_owned(),
                            "guid" | "id" if d.key.is_empty() => d.key = value.trim().to_owned(),
                            "pubdate" | "published" | "date" => d.date = value.trim().to_owned(),
                            "updated" if d.date.is_empty() => d.date = value.trim().to_owned(),
                            _ => {}
                        }
                    }
                } else if name == "title" && matches!(parent, "channel" | "feed") && title.is_empty() {
                    title = plain(&value);
                }
            }
            Ok(Event::Eof) => break,
            // A broken end: what was read so far is kept.
            Err(_) => break,
            Ok(_) => {}
        }
    }
    (is_feed && !(title.is_empty() && entries.is_empty())).then_some(Feed { title, entries })
}

fn local(e: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase()
}

/// `<link href rel>` of Atom: the entry's page, not its feed nor its attachments.
fn atom_link(name: &str, e: &BytesStart<'_>, draft: Option<&mut Draft>) {
    let (Some(d), "link") = (draft, name) else { return };
    let value = |wanted: &str| {
        e.attributes().with_checks(false).flatten().find(|a| a.key.as_ref() == wanted.as_bytes()).map(|a| {
            String::from_utf8_lossy(&a.value).replace("&amp;", "&")
        })
    };
    let rel = value("rel").unwrap_or_else(|| "alternate".into());
    if let (Some(href), "alternate", true) = (value("href"), rel.as_str(), d.link.is_empty()) {
        d.link = href;
    }
}

fn resolve(r: &BytesRef<'_>) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    entity(&r.decode().unwrap_or_default()).to_owned()
}

fn entity(name: &str) -> &'static str {
    match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" | "rsquo" | "lsquo" => "'",
        "eacute" => "é",
        "egrave" => "è",
        "agrave" => "à",
        "ccedil" => "ç",
        "hellip" => "…",
        "ndash" | "mdash" => "–",
        "laquo" => "«",
        "raquo" => "»",
        _ => " ",
    }
}

/// A title may be HTML: its tags go, its entities are read once more.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['<', '&']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if rest.starts_with('<') {
            let tag = rest[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
            match rest.find('>') {
                Some(end) if tag => {
                    out.push(' ');
                    rest = &rest[end + 1..];
                }
                _ => {
                    out.push('<');
                    rest = &rest[1..];
                }
            }
            continue;
        }
        match rest.find(';').filter(|end| *end < 10) {
            Some(end) => {
                let name = &rest[1..end];
                let code = name.strip_prefix('#').and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                });
                match code.and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push_str(entity(name)),
                }
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn finish(d: Draft, base: &Url) -> Option<Entry> {
    // RSS allows the id to be the address when there is no link.
    let link = if d.link.is_empty() { &d.key } else { &d.link };
    let url = base.join(link.trim()).ok().filter(|_| !link.trim().is_empty())?;
    let at = chrono::DateTime::parse_from_rfc3339(&d.date)
        .or_else(|_| chrono::DateTime::parse_from_rfc2822(&d.date))
        .map_or(0, |t| t.timestamp());
    pulsar::entry(&d.key, &d.title, url, at)
}

/// The Pulsars of a profile (`pulsars.json`), written on the background thread.
pub struct PulsarStore {
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct PulsarDto {
    id: u64,
    space: u64,
    name: String,
    feed: String,
    /// What a Pulsar of GitHub gathers (empty: a feed), and where.
    #[serde(default)]
    github: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    collapsed: bool,
    #[serde(default)]
    checked_at: i64,
    #[serde(default)]
    failures: u32,
    #[serde(default)]
    etag: String,
    #[serde(default)]
    modified: String,
    #[serde(default)]
    lines: Vec<LineDto>,
}

#[derive(Serialize, Deserialize)]
struct LineDto {
    key: String,
    title: String,
    url: String,
    #[serde(default)]
    at: i64,
    #[serde(default)]
    read: bool,
    #[serde(default)]
    note: String,
}

impl PulsarStore {
    pub fn new(data: &Path) -> Self {
        Self { path: data.join("pulsars.json") }
    }

    pub fn load(&self) -> Pulsars {
        crate::flush_writes();
        let dtos: Vec<PulsarDto> = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let list = dtos.into_iter().filter_map(|d| {
            // The address asked comes from the query, never from the file.
            let github = match d.github.as_str() {
                "" => None,
                kind => Some(pulsar::GithubQuery::new(pulsar::GithubKind::from_id(kind)?, &d.repo)?),
            };
            Some(Pulsar {
                id: d.id,
                space: d.space,
                name: d.name,
                feed: match &github {
                    Some(query) => query.page(),
                    None => Url::parse(&d.feed).ok()?,
                },
                github,
                pinned: d.pinned,
                collapsed: d.collapsed,
                checked_at: d.checked_at,
                failures: d.failures,
                etag: d.etag,
                modified: d.modified,
                lines: d
                    .lines
                    .into_iter()
                    .filter_map(|l| {
                        let mut entry = pulsar::entry(&l.key, &l.title, Url::parse(&l.url).ok()?, l.at)?;
                        entry.note = pulsar::clean(&l.note, 40);
                        Some(Line { entry, read: l.read })
                    })
                    .collect(),
            })
        });
        Pulsars::restore(list.collect())
    }

    pub fn save(&self, pulsars: &Pulsars) {
        let dtos: Vec<PulsarDto> = pulsars
            .all()
            .iter()
            .map(|p| PulsarDto {
                id: p.id,
                space: p.space,
                name: p.name.clone(),
                feed: p.feed.to_string(),
                github: p.github.as_ref().map(|g| g.kind.id().to_owned()).unwrap_or_default(),
                repo: p.github.as_ref().map(|g| g.repo.clone()).unwrap_or_default(),
                pinned: p.pinned,
                collapsed: p.collapsed,
                checked_at: p.checked_at,
                failures: p.failures,
                etag: p.etag.clone(),
                modified: p.modified.clone(),
                lines: p
                    .lines
                    .iter()
                    .map(|l| LineDto {
                        key: l.entry.key.clone(),
                        title: l.entry.title.clone(),
                        url: l.entry.url.to_string(),
                        at: l.entry.at,
                        read: l.read,
                        note: l.entry.note.clone(),
                    })
                    .collect(),
            })
            .collect();
        if let Ok(json) = serde_json::to_string(&dtos) {
            crate::write_later(self.path.clone(), json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://blog.test/feed.xml").unwrap()
    }

    #[test]
    fn rss_is_read_with_its_entities_and_relative_links() {
        let feed = parse(
            r#"<?xml version="1.0"?><!DOCTYPE rss [<!ENTITY a "b">]><rss version="2.0" xmlns:media="m"><channel><title>Le &amp; blog</title><link>https://blog.test/</link>
            <item><title><![CDATA[Une <b>pomme</b> &eacute;t&#233;]]></title><link>/one?a=1&amp;b=2</link><guid>g1</guid><pubDate>Wed, 01 Jul 2026 10:00:00 +0200</pubDate><media:title>no</media:title></item>
            <item><title>2 &lt; 3</title><guid>https://blog.test/two</guid></item>
            <item><title>bad</title><link>javascript:alert(1)</link></item></channel></rss>"#,
            &base(),
        )
        .unwrap();
        assert_eq!(feed.title, "Le & blog");
        assert_eq!(feed.entries.len(), 2);
        let e = &feed.entries[0];
        assert_eq!((e.title.as_str(), e.url.as_str(), e.key.as_str()), ("Une pomme été", "https://blog.test/one?a=1&b=2", "g1"));
        assert_eq!(e.at, 1_782_892_800);
        assert_eq!((feed.entries[1].title.as_str(), feed.entries[1].url.as_str()), ("2 < 3", "https://blog.test/two"));
    }

    #[test]
    fn atom_is_read_with_the_page_of_each_entry() {
        let feed = parse(
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Releases</title><link rel="self" href="https://x.test/self"/>
            <entry><id>tag:1</id><link rel="enclosure" href="/file"/><link rel="alternate" href="https://x.test/v1?a=1&amp;b=2"/><title type="html">v1 &amp;amp; more</title><updated>2026-07-01T08:00:00Z</updated></entry></feed>"#,
            &base(),
        )
        .unwrap();
        assert_eq!(feed.title, "Releases");
        let e = &feed.entries[0];
        assert_eq!((e.title.as_str(), e.url.as_str(), e.at), ("v1 & more", "https://x.test/v1?a=1&b=2", 1_782_892_800));
    }

    #[test]
    fn a_page_is_no_feed_but_announces_one() {
        let html = r#"<!doctype html><html><head><link rel="stylesheet" href="a.css"><LINK REL='alternate' TYPE="application/rss+xml" title="x" HREF='/rss?a=1&amp;b=2'></head><body>hi</body></html>"#;
        assert!(parse(html, &base()).is_none());
        assert_eq!(announced(html, &base()).unwrap().as_str(), "https://blog.test/rss?a=1&b=2");
        assert!(announced("<link rel=alternate type=application/rss+xml href=javascript:x>", &base()).is_none());
        assert!(announced("<link", &base()).is_none());
        for bad in ["", "not xml", "{}", "<rss>", "<rss></rss>"] {
            assert!(parse(bad, &base()).is_none(), "{bad}");
        }
    }

    #[test]
    fn hostile_feeds_end_without_harm() {
        let deep = format!("<rss>{}", "<a>".repeat(5_000));
        assert!(parse(&deep, &base()).is_none());
        let bomb = r#"<!DOCTYPE rss [<!ENTITY a "aaaa"><!ENTITY b "&a;&a;">]><rss><channel><title>&b;&b;x</title></channel></rss>"#;
        assert_eq!(parse(bomb, &base()).unwrap().title.trim(), "x", "entities of the document are never expanded");
        let many = format!("<rss><channel><title>t</title>{}</channel></rss>", "<item><link>/a</link></item>".repeat(1_000));
        assert_eq!(parse(&many, &base()).unwrap().entries.len(), MAX_ENTRIES);
        // Cut in the middle: what was whole is kept.
        let cut = "<rss><channel><title>t</title><item><link>/a</link></item><item><link>/b";
        assert_eq!(parse(cut, &base()).unwrap().entries.len(), 1);
        assert_eq!(text_of(b"<?xml encoding='ISO-8859-1'?>\xe9"), "<?xml encoding='ISO-8859-1'?>é");
    }

    #[test]
    fn pulsars_are_kept_between_launches() {
        let dir = tempfile::tempdir().unwrap();
        let store = PulsarStore::new(dir.path());
        assert!(store.load().all().is_empty());
        let mut p = Pulsars::default();
        let id = p.add(3, "Blog", base()).unwrap();
        p.merge(id, vec![pulsar::entry("k", "T", base(), 5).unwrap()], 10);
        p.open(id, "k");
        p.set_pinned(id, true);
        p.set_marks(id, "e".into(), "m".into());
        let query = pulsar::GithubQuery::new(pulsar::GithubKind::Assigned, "o/r").unwrap();
        let gh = p.add_github(1, "Assigned", query).unwrap();
        let mut line = pulsar::entry("g", "G", base(), 1).unwrap();
        line.note = "r #1".into();
        p.merge(gh, vec![line], 10);
        store.save(&p);
        assert_eq!(store.load(), p);
        std::fs::write(dir.path().join("pulsars.json"), "oops").unwrap();
        assert!(store.load().all().is_empty());
    }
}

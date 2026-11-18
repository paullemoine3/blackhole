//! Command bar suggestions (Ctrl/⌘+L, new tab): which pages of the history are worth offering
//! and in what order. Pure rules, shared by the desktop palette (which receives the prepared
//! pool and keeps the same matching in `palette.html`) and the Android core.
//!
//! - One row per page: `key` folds http/https, `www.`, a trailing slash, the `#fragment`,
//!   tracking parameters and the order of the query parameters.
//! - Pages nobody wants to go back to by typing stay out: sign-in, OAuth and SSO steps,
//!   callbacks, logout, redirect pages (`is_noise`).
//! - Frecency: how often and how recently a page was visited (`frecency`), so a page seen once
//!   months ago comes after the one opened every day.
//! - A site visited through several pages also gets its home page (`pool`), the row most people
//!   look for when they type the start of a name.
//! - Matching (`match_quality`): every word typed must be found; the start of the host or of the
//!   title is worth more than the middle of a word, the address path least of all. One letter
//!   only matches the start of a host, title or word.

use std::collections::HashMap;

use url::Url;

use crate::clean_link;
use crate::history::HistoryEntry;

/// How many history rows the palette receives, best first.
pub const POOL_MAX: usize = 400;
/// A site with this many distinct pages in the history also gets its home page as a row.
const ROOT_FROM_PAGES: usize = 3;
/// Above this frecency a page counts as a habit: the matching stops mattering much more.
const FRECENCY_HALF: f64 = 120.0;

/// One page the command bar may offer.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub url: Url,
    pub title: String,
    pub key: String,
    pub frecency: f64,
}

/// The same page whatever way its address was written: no scheme, no `www.`, no trailing
/// slash, no fragment, no tracking parameters, query parameters in order.
pub fn key(url: &Url) -> String {
    let cleaned = clean_link::strip_tracking(url).unwrap_or_else(|| url.clone());
    let host = cleaned.host_str().unwrap_or_default().to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let mut out = String::with_capacity(url.as_str().len());
    out.push_str(host);
    if let Some(port) = cleaned.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    let path = cleaned.path();
    let path = path.strip_suffix("/index.html").unwrap_or(path);
    out.push_str(path.trim_end_matches('/'));
    let mut pairs: Vec<(String, String)> = cleaned
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if !pairs.is_empty() {
        pairs.sort();
        out.push('?');
        let joined: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
        out.push_str(&joined.join("&"));
    }
    out
}

/// `key` of an address given as text; the text itself when it is not an address.
pub fn key_of_str(raw: &str) -> String {
    Url::parse(raw).map_or_else(|_| raw.to_owned(), |u| key(&u))
}

const NOISE_HOST_LABELS: [&str; 10] = [
    "login", "logon", "auth", "sso", "signin", "accounts", "oauth", "idp", "adfs", "secure-login",
];
const NOISE_HOSTS: [&str; 3] = ["appleid.apple.com", "login.microsoftonline.com", "id.atlassian.com"];
const NOISE_SEGMENTS: [&str; 31] = [
    "login", "log-in", "log_in", "signin", "sign-in", "sign_in", "logon", "logout", "log-out",
    "signout", "sign-out", "sign_out", "oauth", "oauth2", "authorize", "authorization",
    "callback", "sso", "saml", "saml2", "auth", "2fa", "mfa", "verify", "consent", "session",
    "sessions", "redirect", "redir", "forgot-password", "reset-password",
];
const NOISE_PARAMS: [&str; 9] = [
    "redirect_uri", "client_id", "id_token", "access_token", "samlrequest", "samlresponse",
    "code_challenge", "oauth_token", "ticket",
];

/// A page nobody wants back by typing its name: a sign-in or SSO step, an OAuth callback, a
/// logout, a redirect. Also anything that is not http(s).
pub fn is_noise(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return true;
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let bare = host.strip_prefix("www.").unwrap_or(&host);
    if NOISE_HOSTS.iter().any(|h| bare == *h || bare.ends_with(&format!(".{h}"))) {
        return true;
    }
    // `login.example.com`, `auth.example.com`, `accounts.google.com` (not `example.com` itself).
    let first = bare.split('.').next().unwrap_or_default();
    if bare.contains('.') && NOISE_HOST_LABELS.contains(&first) {
        return true;
    }
    let segment_noise = url
        .path_segments()
        .into_iter()
        .flatten()
        .any(|s| NOISE_SEGMENTS.contains(&s.to_ascii_lowercase().as_str()));
    if segment_noise {
        return true;
    }
    let mut code = false;
    let mut state = false;
    for (k, _) in url.query_pairs() {
        let k = k.to_ascii_lowercase();
        if NOISE_PARAMS.contains(&k.as_str()) {
            return true;
        }
        code |= k == "code";
        state |= k == "state";
    }
    // An OAuth answer coming back to a site: `?code=…&state=…`.
    code && state
}

/// How much a page is part of one's habits: its visits, weighed by how long ago the last one
/// was (Firefox-like buckets). `now` and `visited_at` in seconds.
pub fn frecency(visits: u32, visited_at: u64, now: u64) -> f64 {
    let days = now.saturating_sub(visited_at) / 86_400;
    let weight = match days {
        0..=3 => 100.0,
        4..=13 => 70.0,
        14..=30 => 50.0,
        31..=90 => 30.0,
        _ => 10.0,
    };
    // Visits count less and less: a page opened 200 times is not 200 times better.
    let visits = f64::from(visits.max(1));
    weight * (1.0 + visits.ln() * 1.5)
}

/// The history rows worth offering, one per page, best first (`POOL_MAX` at most). Pages of a
/// site seen through `ROOT_FROM_PAGES` pages or more add its home page when it is missing.
pub fn pool(entries: &[HistoryEntry], now: u64) -> Vec<Suggestion> {
    let mut by_key: HashMap<String, Suggestion> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    // Per site (host key): distinct pages, summed frecency, an https address to build a root.
    let mut sites: HashMap<String, (usize, f64, Url)> = HashMap::new();
    for e in entries {
        if is_noise(&e.url) {
            continue;
        }
        let k = key(&e.url);
        let f = frecency(e.visits, e.visited_at, now);
        let host = k.split(['/', '?']).next().unwrap_or_default().to_owned();
        let site = sites.entry(host).or_insert_with(|| (0, 0.0, e.url.clone()));
        site.1 += f;
        match by_key.get_mut(&k) {
            Some(s) => {
                s.frecency += f;
                // The https address and a real title win over an http copy or a bare host.
                if s.url.scheme() == "http" && e.url.scheme() == "https" {
                    s.url = e.url.clone();
                }
                if (s.title.is_empty() || looks_like_host(&s.title)) && !e.title.trim().is_empty() {
                    s.title = e.title.trim().to_owned();
                }
            }
            None => {
                site.0 += 1;
                order.push(k.clone());
                by_key.insert(
                    k.clone(),
                    Suggestion {
                        url: e.url.clone(),
                        title: e.title.trim().to_owned(),
                        key: k,
                        frecency: f,
                    },
                );
            }
        }
    }
    for (host, (pages, total, sample)) in sites {
        if pages < ROOT_FROM_PAGES || by_key.contains_key(&host) {
            continue;
        }
        let mut root = sample;
        root.set_path("/");
        root.set_query(None);
        root.set_fragment(None);
        if is_noise(&root) {
            continue;
        }
        order.push(host.clone());
        by_key.insert(
            host.clone(),
            Suggestion {
                title: host.clone(),
                key: host,
                url: root,
                // A little under the site's pages put together: it stands for all of them.
                frecency: total * 0.8,
            },
        );
    }
    let mut out: Vec<Suggestion> = order.into_iter().filter_map(|k| by_key.remove(&k)).collect();
    out.sort_by(|a, b| b.frecency.total_cmp(&a.frecency));
    out.truncate(POOL_MAX);
    out
}

fn looks_like_host(title: &str) -> bool {
    !title.contains(char::is_whitespace) && title.contains('.')
}

/// Frecency brought to 0–1 (half way at `FRECENCY_HALF`), the share the palette adds to the
/// quality of a match.
pub fn habit(frecency: f64) -> f64 {
    frecency / (frecency + FRECENCY_HALF)
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || "-_./·:()|,–—[]'\"".contains(c)
}

/// Where `word` starts in `text`: 0 at the very start, 1 at the start of a word, 2 inside one.
fn place(text: &str, word: &str) -> Option<u8> {
    let mut best: Option<u8> = None;
    let mut from = 0;
    while let Some(i) = text[from..].find(word) {
        let at = from + i;
        let p = if at == 0 {
            0
        } else if text[..at].chars().next_back().is_some_and(is_boundary) {
            1
        } else {
            2
        };
        best = Some(best.map_or(p, |b| b.min(p)));
        if p == 0 {
            break;
        }
        from = at + word.len().max(1);
        if from >= text.len() {
            break;
        }
    }
    best
}

/// How well a page answers what was typed, 0–1, `None` when a word is missing. Words are
/// separated by spaces; each must be found in the title or the address.
pub fn match_quality(query: &str, title: &str, url: &Url) -> Option<f64> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Some(0.0);
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    let title = title.to_lowercase();
    let rest = {
        let s = url.as_str().to_lowercase();
        s.split_once(&host).map_or(s.clone(), |(_, r)| r.to_owned())
    };
    let single_letter = words.len() == 1 && words[0].chars().count() == 1;
    let mut total = 0.0;
    for w in &words {
        // The typed address itself: `github.com/rust` is matched against host + path.
        let joined = format!("{host}{rest}");
        let mut best: f64 = 0.0;
        if host.starts_with(w.as_str()) || joined.starts_with(w.as_str()) {
            best = 1.0;
        }
        match place(&title, w) {
            Some(0) => best = best.max(0.9),
            Some(1) => best = best.max(0.75),
            Some(_) if !single_letter => best = best.max(0.35),
            _ => {}
        }
        match place(&host, w) {
            Some(0) | Some(1) => best = best.max(0.7),
            Some(_) if !single_letter => best = best.max(0.3),
            _ => {}
        }
        match place(&rest, w) {
            Some(0) | Some(1) if !single_letter => best = best.max(0.45),
            Some(_) if !single_letter && w.chars().count() >= 3 => best = best.max(0.2),
            _ => {}
        }
        if best == 0.0 {
            return None;
        }
        total += best;
    }
    Some(total / words.len() as f64)
}

/// The final order of a history row for what was typed: the match first, the habit after.
pub fn score(quality: f64, frecency: f64) -> f64 {
    quality * 100.0 + habit(frecency) * 45.0
}

/// The pool ranked for `query`, best first, `limit` rows at most.
pub fn rank<'a>(pool: &'a [Suggestion], query: &str, limit: usize) -> Vec<&'a Suggestion> {
    let mut found: Vec<(f64, &Suggestion)> = pool
        .iter()
        .filter_map(|s| match_quality(query, &s.title, &s.url).map(|q| (score(q, s.frecency), s)))
        .collect();
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    found.into_iter().take(limit).map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn entry(url: &str, title: &str, visits: u32, at: u64) -> HistoryEntry {
        HistoryEntry {
            url: u(url),
            title: title.to_owned(),
            visited_at: at,
            visits,
        }
    }

    const DAY: u64 = 86_400;
    const NOW: u64 = 1_000 * DAY;

    #[test]
    fn the_same_page_written_differently_has_one_key() {
        let k = key(&u("https://www.example.com/a/?b=2&a=1#top"));
        assert_eq!(k, "example.com/a?a=1&b=2");
        assert_eq!(key(&u("http://example.com/a?a=1&b=2&utm_source=x")), k);
        assert_eq!(key(&u("https://example.com/")), "example.com");
        assert_eq!(key(&u("https://example.com/index.html")), "example.com");
        assert_ne!(key(&u("https://example.com:8080/")), "example.com");
        assert_eq!(key_of_str("not a url"), "not a url");
    }

    #[test]
    fn sign_in_steps_and_redirects_are_noise() {
        for noise in [
            "https://accounts.google.com/v3/signin/identifier?continue=x",
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            "https://github.com/login?return_to=%2F",
            "https://github.com/session",
            "https://app.example.com/auth/callback?code=1&state=2",
            "https://example.com/?code=abc&state=xyz",
            "https://example.com/connect?redirect_uri=https://a.b/&client_id=1",
            "https://sso.company.fr/",
            "https://example.com/logout",
            "about:blank",
        ] {
            assert!(is_noise(&u(noise)), "{noise}");
        }
        assert!(!is_noise(&u("https://github.com/rust-lang/rust")));
        assert!(!is_noise(&u("https://example.com/blog/how-to-login-faster")));
        assert!(!is_noise(&u("https://www.google.com/search?q=code")));
        assert!(!is_noise(&u("https://www.accounting-today.com/")));
    }

    #[test]
    fn habits_beat_a_page_seen_once_long_ago() {
        let daily = frecency(40, NOW - DAY, NOW);
        let once_old = frecency(1, NOW - 200 * DAY, NOW);
        let once_new = frecency(1, NOW, NOW);
        assert!(daily > once_new && once_new > once_old);
        assert!(habit(daily) < 1.0 && habit(daily) > habit(once_new));
    }

    #[test]
    fn the_pool_has_one_row_per_page_and_no_noise() {
        let pool = pool(
            &[
                entry("https://www.example.com/a", "A page", 2, NOW),
                entry("http://example.com/a/", "example.com", 3, NOW - DAY),
                entry("https://example.com/a?utm_source=news", "A page", 1, NOW - DAY),
                entry("https://accounts.google.com/signin", "Sign in", 9, NOW),
            ],
            NOW,
        );
        assert_eq!(pool.len(), 1, "{pool:?}");
        assert_eq!(pool[0].url.scheme(), "https");
        assert_eq!(pool[0].title, "A page");
    }

    #[test]
    fn a_site_seen_through_several_pages_gets_its_home_page() {
        let pool = pool(
            &[
                entry("https://docs.rs/a/latest", "a - Rust", 1, NOW),
                entry("https://docs.rs/b/latest", "b - Rust", 1, NOW),
                entry("https://docs.rs/c/latest", "c - Rust", 1, NOW),
                entry("https://once.test/x", "x", 1, NOW),
            ],
            NOW,
        );
        let root = pool.iter().find(|s| s.key == "docs.rs").expect("home page added");
        assert_eq!(root.url.as_str(), "https://docs.rs/");
        assert!(pool.iter().all(|s| s.key != "once.test"));
        // The home page stands for all its pages: it comes first.
        assert_eq!(pool[0].key, "docs.rs");
    }

    #[test]
    fn every_word_must_match_and_the_start_of_a_name_wins() {
        let gh = u("https://github.com/rust-lang/rust");
        assert_eq!(match_quality("git", "rust-lang/rust", &gh), Some(1.0));
        assert!(match_quality("rust lang", "rust-lang/rust: Empowering", &gh).unwrap() > 0.7);
        assert_eq!(match_quality("rust python", "rust-lang/rust", &gh), None);
        // One letter: only the start of a host, a title or a word.
        let mail = u("https://mail.proton.me/u/0/inbox");
        assert!(match_quality("p", "Inbox | Proton Mail", &mail).is_some());
        assert_eq!(match_quality("x", "Inbox | Proton Mail", &mail), None);
        // Typing the address itself.
        assert_eq!(match_quality("github.com/rust", "", &gh), Some(1.0));
        // The middle of a word in the path is a weak match.
        let q = match_quality("lang", "Something", &gh).unwrap();
        assert!(q < 0.5, "{q}");
    }

    #[test]
    fn ranking_puts_the_answer_before_an_old_page_that_only_mentions_it() {
        let pool = pool(
            &[
                entry("https://news.example/rare?ref=github", "Old article", 1, NOW - 300 * DAY),
                entry("https://github.com/", "GitHub", 30, NOW - DAY),
                entry("https://gist.github.com/me", "My gists", 2, NOW - 20 * DAY),
            ],
            NOW,
        );
        let ranked = rank(&pool, "git", 5);
        assert_eq!(ranked[0].url.as_str(), "https://github.com/");
        assert_eq!(ranked[1].url.host_str(), Some("gist.github.com"));
        assert_eq!(ranked.last().unwrap().title, "Old article");
        assert!(rank(&pool, "zzz", 5).is_empty());
    }
}

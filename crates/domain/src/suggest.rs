//! Command bar suggestions (Ctrl/⌘+L, new tab): which rows answer what is typed and in what
//! order. Pure rules: the desktop shell ranks with them at every key press (`UiCommand::Suggest`,
//! the palette only draws), the Android core too. `suggest_eval.rs` measures them on a history
//! close to a real one.
//!
//! - One row per page: `key` folds http/https, `www.`, a trailing slash, the `#fragment`,
//!   tracking parameters and the order of the query parameters.
//! - Pages nobody wants to go back to by typing stay out: sign-in, OAuth and SSO steps,
//!   callbacks, logout, redirect pages (`is_noise`).
//! - Habits (`frecency`): the last ten visits, each by its age and how the page was reached (an
//!   address typed or a result chosen counts most, a link less, a step of a redirect nothing).
//! - A site visited through several pages also gets its home page (`pool`); a page of results of
//!   a search engine becomes a search made before (`search_terms`), offered by its words.
//! - Matching (`match_quality`): every word typed must be found, case and accents aside (`fold`);
//!   the start of the host or of the title is worth more than the middle of a word; a word of 4
//!   letters or more may carry one typo.
//! - What was chosen before for what is typed comes first (`Adaptive`), then the space shown
//!   breaks ties (`Context::space`). `pick` keeps room for commands; `completion` fills the field
//!   with the address of a daily site.

use std::collections::HashMap;

use url::Url;

use crate::clean_link;
use crate::history::{HistoryEntry, VisitKind};

/// How many history rows the ranking looks at, best habits first.
pub const POOL_MAX: usize = 400;
/// A site with this many distinct pages in the history also gets its home page as a row.
const ROOT_FROM_PAGES: usize = 3;
/// Above this frecency a page counts as a habit: the matching stops mattering much more.
const FRECENCY_HALF: f64 = 120.0;

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
const NOISE_SEGMENTS: [&str; 36] = [
    "login", "log-in", "log_in", "signin", "sign-in", "sign_in", "logon", "logout", "log-out",
    "signout", "sign-out", "sign_out", "oauth", "oauth2", "authorize", "authorization",
    "callback", "sso", "saml", "saml2", "auth", "2fa", "mfa", "verify", "consent", "session",
    "sessions", "redirect", "redir", "forgot-password", "reset-password",
    // French sites.
    "connexion", "deconnexion", "se-connecter", "identification", "authentification",
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


// ---- recency and habits ----------------------------------------------------------------------

/// Points of one visit by its age (Firefox-like buckets).
fn age_points(at: u64, now: u64) -> f64 {
    match now.saturating_sub(at) / 86_400 {
        0..=3 => 100.0,
        4..=13 => 70.0,
        14..=30 => 50.0,
        31..=90 => 30.0,
        _ => 10.0,
    }
}

/// Weight of a visit by how the page was reached: typed or chosen counts most, a step of a
/// redirect nothing.
fn kind_weight(kind: VisitKind) -> f64 {
    match kind {
        VisitKind::Typed => 2.0,
        VisitKind::Bookmark => 1.4,
        VisitKind::Link => 1.0,
        VisitKind::Redirect => 0.0,
    }
}

/// How much a page is part of one's habits. The last visits (`HistoryEntry::recent`, ten at
/// most) are each worth their age's points times their kind's weight; their mean is scaled by
/// the number of visits, less and less (a page opened 200 times is not 200 times better). An
/// entry written before 0.8.18 (no `recent`) counts its last visit as a link.
pub fn frecency(entry: &HistoryEntry, now: u64) -> f64 {
    let mean = if entry.recent.is_empty() {
        age_points(entry.visited_at, now)
    } else {
        let sum: f64 = entry.recent.iter().map(|v| age_points(v.at, now) * kind_weight(v.kind)).sum();
        sum / entry.recent.len() as f64
    };
    let visits = f64::from(entry.visits.max(1));
    mean * (1.0 + visits.ln() * 1.5)
}

/// Frecency brought to 0–1 (half way at `FRECENCY_HALF`).
pub fn habit(frecency: f64) -> f64 {
    frecency / (frecency + FRECENCY_HALF)
}

// ---- searches made before ----------------------------------------------------------------------

/// Hosts of search engines and the parameter of their query (the engines of the Settings, plus
/// the common others).
const ENGINES: [(&str, &str, &str); 9] = [
    ("duckduckgo.com", "", "q"),
    ("search.brave.com", "/search", "q"),
    ("startpage.com", "", "query"),
    ("qwant.com", "", "q"),
    ("mojeek.com", "/search", "q"),
    ("google.", "/search", "q"),
    ("bing.com", "/search", "q"),
    ("ecosia.org", "/search", "q"),
    ("search.yahoo.com", "/search", "p"),
];

/// The words of a search, when `url` is a page of results of a search engine.
pub fn search_terms(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let (_, _, param) = ENGINES.iter().find(|(h, p, _)| {
        let on = if h.ends_with('.') { host.starts_with(h) } else { host == *h };
        on && (p.is_empty() || url.path().starts_with(p))
    })?;
    let terms = url.query_pairs().find(|(k, _)| k == param)?.1.split_whitespace().collect::<Vec<_>>().join(" ");
    (!terms.is_empty() && terms.chars().count() <= 200).then_some(terms)
}

// ---- the pool of history rows ------------------------------------------------------------------

/// One page (or one search made before) the command bar may offer.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub url: Url,
    /// The page's title; the words searched for a search.
    pub title: String,
    pub key: String,
    pub frecency: f64,
    /// Spaces the page was seen in (`HistoryEntry::spaces`).
    pub spaces: Vec<u64>,
    /// A page of results: what was searched.
    pub search: Option<String>,
}

/// The history rows worth offering, one per page and one per search, best first (`POOL_MAX` at
/// most). Pages of a site seen through `ROOT_FROM_PAGES` pages or more add its home page when it
/// is missing; the result pages of a search engine never do.
pub fn pool(entries: &[HistoryEntry], now: u64) -> Vec<Suggestion> {
    let mut by_key: HashMap<String, Suggestion> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    // Per site (host key): distinct pages, summed frecency, spaces, an address to build a root.
    let mut sites: HashMap<String, (usize, f64, Vec<u64>, Url)> = HashMap::new();
    for e in entries {
        if is_noise(&e.url) {
            continue;
        }
        let f = frecency(e, now);
        let terms = search_terms(&e.url);
        let k = match &terms {
            Some(t) => format!("search:{}", fold(t)),
            None => key(&e.url),
        };
        if terms.is_none() {
            let host = k.split(['/', '?']).next().unwrap_or_default().to_owned();
            let site = sites.entry(host).or_insert_with(|| (0, 0.0, Vec::new(), e.url.clone()));
            site.1 += f;
            for s in &e.spaces {
                if !site.2.contains(s) {
                    site.2.push(*s);
                }
            }
            if !by_key.contains_key(&k) {
                site.0 += 1;
            }
        }
        match by_key.get_mut(&k) {
            Some(s) => {
                s.frecency += f;
                for sp in &e.spaces {
                    if !s.spaces.contains(sp) {
                        s.spaces.push(*sp);
                    }
                }
                // The https address and a real title win over an http copy or a bare host.
                if s.url.scheme() == "http" && e.url.scheme() == "https" {
                    s.url = e.url.clone();
                }
                if terms.is_none()
                    && (s.title.is_empty() || looks_like_host(&s.title))
                    && !e.title.trim().is_empty()
                {
                    s.title = e.title.trim().to_owned();
                }
            }
            None => {
                order.push(k.clone());
                by_key.insert(
                    k.clone(),
                    Suggestion {
                        url: e.url.clone(),
                        title: terms.clone().unwrap_or_else(|| e.title.trim().to_owned()),
                        key: k,
                        frecency: f,
                        spaces: e.spaces.clone(),
                        search: terms,
                    },
                );
            }
        }
    }
    for (host, (pages, total, spaces, sample)) in sites {
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
                spaces,
                search: None,
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

// ---- what the user chose before ----------------------------------------------------------------

/// Choices kept at most (the weakest go first).
const ADAPTIVE_MAX: usize = 500;
/// A choice weighs half as much after this many days.
const ADAPTIVE_HALF_DAYS: f64 = 30.0;
/// What was typed is kept up to this many characters.
const ADAPTIVE_INPUT_MAX: usize = 40;

/// One choice: what was typed (folded), the row chosen (its `key`), how often (decaying), when.
#[derive(Debug, Clone, PartialEq)]
pub struct Learned {
    pub input: String,
    pub key: String,
    pub uses: f64,
    pub at: u64,
}

/// What the user chose in the command bar for what they typed (Firefox's "adaptive history"):
/// typing "ma" and taking the mail makes the mail come first for "ma", "mai", "mail". Kept on
/// disk (`suggest.json`), never in a private window nor for a black hole tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Adaptive {
    entries: Vec<Learned>,
}

impl Adaptive {
    pub fn restore(mut entries: Vec<Learned>) -> Self {
        entries.retain(|e| !e.input.is_empty() && !e.key.is_empty() && e.uses.is_finite() && e.uses > 0.0);
        entries.truncate(ADAPTIVE_MAX);
        Self { entries }
    }

    pub fn entries(&self) -> &[Learned] {
        &self.entries
    }

    fn decayed(e: &Learned, now: u64) -> f64 {
        let days = now.saturating_sub(e.at) as f64 / 86_400.0;
        e.uses * 0.5f64.powf(days / ADAPTIVE_HALF_DAYS)
    }

    /// `key` was chosen for `input` (as typed). False when nothing was kept (empty input).
    pub fn record(&mut self, input: &str, key: &str, now: u64) -> bool {
        let input: String = fold(input.trim()).chars().take(ADAPTIVE_INPUT_MAX).collect();
        if input.is_empty() || key.is_empty() {
            return false;
        }
        match self.entries.iter().position(|e| e.input == input && e.key == key) {
            Some(i) => {
                let e = &mut self.entries[i];
                e.uses = Self::decayed(e, now) + 1.0;
                e.at = now;
            }
            None => {
                if self.entries.len() >= ADAPTIVE_MAX {
                    let weakest = self
                        .entries
                        .iter()
                        .enumerate()
                        .min_by(|a, b| Self::decayed(a.1, now).total_cmp(&Self::decayed(b.1, now)))
                        .map(|(i, _)| i);
                    if let Some(i) = weakest {
                        self.entries.remove(i);
                    }
                }
                self.entries.push(Learned { input, key: key.to_owned(), uses: 1.0, at: now });
            }
        }
        true
    }

    /// How much choices made before vouch for `key` when `query` is typed, 0–1: choices made
    /// for a longer input that starts with it ("mail" chosen, "ma" typed) or for a shorter one
    /// it starts with ("ma" chosen, "mail" typed), decayed with time. One choice gives 0.5.
    pub fn boost(&self, query: &str, key: &str, now: u64) -> f64 {
        let q = fold(query.trim());
        if q.is_empty() {
            return 0.0;
        }
        let uses: f64 = self
            .entries
            .iter()
            .filter(|e| e.key == key && (e.input.starts_with(&q) || (e.input.chars().count() >= 2 && q.starts_with(&e.input))))
            .map(|e| Self::decayed(e, now))
            .sum();
        uses / (uses + 1.0)
    }

    /// The newest `max` choices, what goes to a paired device.
    pub fn shared(&self, max: usize) -> Vec<Learned> {
        let mut out = self.entries.clone();
        out.sort_by_key(|e| std::cmp::Reverse(e.at));
        out.truncate(max);
        out
    }

    /// Merges the choices of a paired device: for one input and one row, the latest choice
    /// wins (idempotent, the same in both directions). Returns whether anything changed.
    pub fn merge(&mut self, others: &[Learned]) -> bool {
        let mut changed = false;
        for o in others {
            let input: String = fold(o.input.trim()).chars().take(ADAPTIVE_INPUT_MAX).collect();
            if input.is_empty() || o.key.is_empty() || !o.uses.is_finite() || o.uses <= 0.0 {
                continue;
            }
            let uses = o.uses.min(1000.0);
            match self.entries.iter_mut().find(|e| e.input == input && e.key == o.key) {
                Some(e) if (o.at, uses) > (e.at, e.uses) => {
                    e.at = o.at;
                    e.uses = uses;
                    changed = true;
                }
                Some(_) => {}
                None => {
                    self.entries.push(Learned { input, key: o.key.clone(), uses, at: o.at });
                    changed = true;
                }
            }
        }
        if self.entries.len() > ADAPTIVE_MAX {
            self.entries.sort_by_key(|e| std::cmp::Reverse(e.at));
            self.entries.truncate(ADAPTIVE_MAX);
        }
        changed
    }

    /// Forgets the choices of pages that are gone (history cleared or one page forgotten).
    pub fn forget(&mut self, key: &str) {
        self.entries.retain(|e| e.key != key);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ---- matching ----------------------------------------------------------------------------------

/// Lower case without accents (and œ, æ, ß spelled out): "Réglages" and "reglages" are one word.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        for l in c.to_lowercase() {
            match l {
                'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => out.push('a'),
                'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => out.push('c'),
                'ď' | 'đ' => out.push('d'),
                'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => out.push('e'),
                'ĝ' | 'ğ' | 'ġ' | 'ģ' => out.push('g'),
                'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => out.push('i'),
                'ñ' | 'ń' | 'ņ' | 'ň' => out.push('n'),
                'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => out.push('o'),
                'ŕ' | 'ř' => out.push('r'),
                'ś' | 'š' | 'ş' | 'ș' => out.push('s'),
                'ţ' | 'ť' | 'ț' => out.push('t'),
                'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => out.push('u'),
                'ý' | 'ÿ' => out.push('y'),
                'ź' | 'ż' | 'ž' => out.push('z'),
                'œ' => out.push_str("oe"),
                'æ' => out.push_str("ae"),
                'ß' => out.push_str("ss"),
                // Typographic apostrophes and dashes match the plain ones typed.
                '’' | '‘' => out.push('\''),
                '–' | '—' => out.push('-'),
                other => out.push(other),
            }
        }
    }
    out
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || "-_./·:()|,[]'\"?&=#+@!".contains(c)
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

/// Optimal string alignment distance (one swap of two neighbours counts one), stopped past 2.
fn edits(a: &[char], b: &[char]) -> usize {
    if a.len().abs_diff(b.len()) > 1 {
        return 2;
    }
    let mut prev2 = vec![0usize; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            cur[j] = d;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// `word` (4 letters or more) is the start of `token` with one typo: a letter wrong, missing,
/// in excess, or two letters swapped ("gtihub" for "github").
fn typo_of(word: &[char], token: &str) -> bool {
    if word.len() < 4 {
        return false;
    }
    let t: Vec<char> = token.chars().collect();
    (word.len().saturating_sub(1)..=word.len() + 1)
        .filter(|n| *n >= 3 && *n <= t.len())
        .any(|n| edits(word, &t[..n]) <= 1)
}

fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(is_boundary).filter(|t| !t.is_empty())
}

/// How well a page answers what was typed, 0–1, `None` when a word is missing. Words are
/// separated by spaces; each must be found in the title or the address. Case and accents do
/// not matter; a word of 4 letters or more may carry one typo (worth less).
pub fn match_quality(query: &str, title: &str, url: Option<&Url>) -> Option<f64> {
    let words: Vec<String> = fold(query).split_whitespace().map(str::to_owned).collect();
    if words.is_empty() {
        return Some(0.0);
    }
    let host = url.and_then(Url::host_str).map(fold).unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    let title = fold(title);
    let rest = match url {
        Some(u) => {
            let s = fold(u.as_str());
            match s.split_once(host.as_str()) {
                Some((_, r)) if !host.is_empty() => r.to_owned(),
                _ => String::new(),
            }
        }
        None => String::new(),
    };
    let joined = format!("{host}{rest}");
    let single_letter = words.len() == 1 && words[0].chars().count() == 1;
    let mut total = 0.0;
    for w in &words {
        let mut best: f64 = 0.0;
        if !host.is_empty() && (host.starts_with(w.as_str()) || joined.starts_with(w.as_str())) {
            best = 1.0;
        }
        match place(&title, w) {
            Some(0) => best = best.max(0.9),
            Some(1) => best = best.max(0.75),
            Some(_) if !single_letter => best = best.max(0.35),
            _ => {}
        }
        match place(&host, w) {
            Some(0 | 1) => best = best.max(0.7),
            Some(_) if !single_letter => best = best.max(0.3),
            _ => {}
        }
        match place(&rest, w) {
            Some(0 | 1) if !single_letter => best = best.max(0.45),
            Some(_) if !single_letter && w.chars().count() >= 3 => best = best.max(0.2),
            _ => {}
        }
        if best == 0.0 {
            // One typo, at the start of a word only: the host's first label, then the title.
            let chars: Vec<char> = w.chars().collect();
            if tokens(&host).next().is_some_and(|t| typo_of(&chars, t)) {
                best = 0.5;
            } else if tokens(&title).any(|t| typo_of(&chars, t)) {
                best = 0.4;
            } else if tokens(&host).skip(1).chain(tokens(&rest)).any(|t| typo_of(&chars, t)) {
                best = 0.2;
            }
        }
        if best == 0.0 {
            return None;
        }
        total += best;
    }
    Some(total / words.len() as f64)
}

// ---- ranking -----------------------------------------------------------------------------------

/// What a row of the command bar stands for; the order says which one a page found several
/// ways is offered as (an open tab first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RowKind {
    /// A tab of the space shown.
    Tab,
    /// A tab of another space.
    OtherTab,
    Favorite,
    Bookmark,
    History,
    /// A search made before (its words in `title`).
    Search,
    /// A command of the app (`title` is its label, no address).
    Action,
}

/// A candidate row, built by the shell from the tabs, quick access, bookmarks, the history pool
/// and the actions. `id` is what the row acts on (a tab, an action), `key` what folds the same
/// page reached several ways.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    pub id: String,
    pub title: String,
    pub url: Option<Url>,
    pub key: String,
    pub frecency: f64,
    pub spaces: Vec<u64>,
}

/// What the ranking knows besides the rows: the space shown, the choices made before, the time.
pub struct Context<'a> {
    pub space: u64,
    pub adaptive: &'a Adaptive,
    pub now: u64,
}

/// A row kept for a query: its index in the candidates and its score (higher first).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ranked {
    pub index: usize,
    pub score: f64,
}

fn kind_bonus(kind: RowKind) -> f64 {
    match kind {
        RowKind::Tab => 30.0,
        RowKind::OtherTab => 18.0,
        RowKind::Favorite => 25.0,
        RowKind::Bookmark => 15.0,
        RowKind::History | RowKind::Action => 0.0,
        RowKind::Search => -8.0,
    }
}

/// The score of a row: how well it matches (0–100), the habit of visiting it (0–45), the choices
/// made before for what is typed (0–70), what it is (an open tab, quick access…), and a little
/// for a page seen in the space shown. An action named by what is typed comes before any site.
pub fn score(quality: f64, frecency: f64, adaptive: f64, kind: RowKind, here: bool) -> f64 {
    if kind == RowKind::Action && quality >= 0.9 {
        return 1000.0 + adaptive * 70.0;
    }
    quality * 100.0 + habit(frecency) * 45.0 + adaptive * 70.0 + kind_bonus(kind) + if here { 10.0 } else { 0.0 }
}

/// The candidates that answer `query`, one per page (`key`), best first. A page found several
/// ways takes its best score and is offered as its most useful kind (an open tab first). A row
/// that does not match may still come when the user chose it before for this input.
pub fn rank_rows(rows: &[Row], query: &str, ctx: &Context) -> Vec<Ranked> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut best: HashMap<&str, (f64, usize)> = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        let url = match row.kind {
            RowKind::Search | RowKind::Action => None,
            _ => row.url.as_ref(),
        };
        let learned = ctx.adaptive.boost(query, &row.key, ctx.now);
        let quality = match match_quality(query, &row.title, url) {
            Some(q) => q,
            None if learned >= 0.5 => 0.3,
            None => continue,
        };
        let here = row.spaces.contains(&ctx.space);
        let s = score(quality, row.frecency, learned, row.kind, here);
        best.entry(row.key.as_str())
            .and_modify(|(top, at)| {
                if s > *top {
                    *top = s;
                }
                if row.kind < rows[*at].kind {
                    *at = i;
                }
            })
            .or_insert((s, i));
    }
    let mut out: Vec<Ranked> = best.into_values().map(|(score, index)| Ranked { index, score }).collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.index.cmp(&b.index)));
    out
}

/// The rows shown under the field: `room` at most; up to `actions` commands keep a place even
/// when sites score more (a command is never hidden behind history), and searches made before
/// take `searches` places at most. Best first.
pub fn pick(rows: &[Row], ranked: &[Ranked], room: usize, actions: usize, searches: usize) -> Vec<Ranked> {
    let is = |r: &Ranked, k: RowKind| rows[r.index].kind == k;
    let mut kept_searches = 0;
    let candidates: Vec<Ranked> = ranked
        .iter()
        .copied()
        .filter(|r| {
            if is(r, RowKind::Search) {
                kept_searches += 1;
                kept_searches <= searches
            } else {
                true
            }
        })
        .collect();
    let mut picked: Vec<Ranked> = candidates.iter().copied().take(room).collect();
    let want = actions.min(candidates.iter().filter(|r| is(r, RowKind::Action)).count());
    let have = picked.iter().filter(|r| is(r, RowKind::Action)).count();
    if have < want {
        let extra: Vec<Ranked> = candidates
            .iter()
            .copied()
            .filter(|r| is(r, RowKind::Action) && !picked.contains(r))
            .take(want - have)
            .collect();
        let others = room.saturating_sub(have + extra.len());
        let sites: Vec<Ranked> = picked.iter().copied().filter(|r| !is(r, RowKind::Action)).take(others).collect();
        picked = picked.into_iter().filter(|r| is(r, RowKind::Action)).chain(extra).chain(sites).collect();
        picked.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.index.cmp(&b.index)));
    }
    picked
}

/// The address the field completes what is typed with (grey text, Tab or → takes it, Enter
/// opens it), and the page it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// The whole address shown, without scheme ("github.com", "github.com/rust-lang").
    pub text: String,
    pub url: Url,
}

/// The completion of `query`: only for one word of 2 letters or more that starts the address of
/// the best site row, and only when that site is a habit (an open tab, quick access, a
/// bookmark, a page often visited or chosen before for this input). The host alone opens the
/// site's home page; a longer input opens the row's page.
pub fn completion(rows: &[Row], ranked: &[Ranked], query: &str, ctx: &Context) -> Option<Completion> {
    let q = query.trim().to_lowercase();
    if q.chars().count() < 2 || q.contains(char::is_whitespace) || q.contains("://") {
        return None;
    }
    let top = ranked.iter().find(|r| !matches!(rows[r.index].kind, RowKind::Action | RowKind::Search))?;
    let row = &rows[top.index];
    let url = row.url.as_ref().filter(|u| matches!(u.scheme(), "http" | "https"))?;
    let sure = matches!(row.kind, RowKind::Tab | RowKind::OtherTab | RowKind::Favorite | RowKind::Bookmark)
        || habit(row.frecency) >= 0.3
        || ctx.adaptive.boost(query, &row.key, ctx.now) > 0.0;
    if !sure {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let bare = host.strip_prefix("www.").unwrap_or(&host);
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let site = format!("{bare}{port}");
    if site.starts_with(&q) {
        let mut root = url.clone();
        root.set_path("/");
        root.set_query(None);
        root.set_fragment(None);
        return Some(Completion { text: site, url: root });
    }
    let path = url.path().trim_end_matches('/');
    let whole = format!("{site}{path}");
    (whole.len() > site.len() && whole.starts_with(&q) && url.query().is_none())
        .then(|| Completion { text: whole, url: url.clone() })
}

#[cfg(test)]
#[path = "suggest_eval.rs"]
mod eval;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Visit;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    pub(super) fn entry(url: &str, title: &str, visits: u32, at: u64) -> HistoryEntry {
        HistoryEntry {
            url: u(url),
            title: title.to_owned(),
            visited_at: at,
            visits,
            recent: Vec::new(),
            spaces: Vec::new(),
            changed: at,
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
    fn typed_and_recent_visits_weigh_more_than_old_links_and_redirect_steps() {
        let mut typed = entry("https://a.test/", "A", 3, NOW);
        typed.recent = vec![Visit { at: NOW, kind: VisitKind::Typed }; 3];
        let mut linked = entry("https://b.test/", "B", 3, NOW);
        linked.recent = vec![Visit { at: NOW, kind: VisitKind::Link }; 3];
        let mut stepped = entry("https://c.test/", "C", 3, NOW);
        stepped.recent = vec![Visit { at: NOW, kind: VisitKind::Redirect }; 3];
        assert!(frecency(&typed, NOW) > frecency(&linked, NOW));
        assert_eq!(frecency(&stepped, NOW), 0.0, "a step of a redirect is no habit");
        // Ten visits spread over the last days beat one burst long ago.
        let mut regular = entry("https://d.test/", "D", 10, NOW);
        regular.recent = (0..10).map(|i| Visit { at: NOW - i * 2 * DAY, kind: VisitKind::Link }).collect();
        let mut burst = entry("https://e.test/", "E", 10, NOW - DAY);
        burst.recent = (0..9).map(|_| Visit { at: NOW - 200 * DAY, kind: VisitKind::Link }).collect();
        burst.recent.insert(0, Visit { at: NOW - DAY, kind: VisitKind::Link });
        assert!(frecency(&regular, NOW) > frecency(&burst, NOW));
        // Older files: no visit list, the last visit counts as a link.
        let old = entry("https://f.test/", "F", 1, NOW);
        assert!(frecency(&old, NOW) > 0.0);
        assert!(habit(frecency(&typed, NOW)) < 1.0);
    }

    #[test]
    fn searches_are_read_from_the_result_pages_of_the_engines() {
        assert_eq!(search_terms(&u("https://duckduckgo.com/?q=rust+%20 borrow&ia=web")).as_deref(), Some("rust borrow"));
        assert_eq!(search_terms(&u("https://www.google.fr/search?q=m%C3%A9t%C3%A9o")).as_deref(), Some("météo"));
        assert_eq!(search_terms(&u("https://www.startpage.com/do/search?query=x")).as_deref(), Some("x"));
        assert_eq!(search_terms(&u("https://www.google.com/maps?q=paris")), None, "not a result page");
        assert_eq!(search_terms(&u("https://duckduckgo.com/")), None);
        assert_eq!(search_terms(&u("https://example.com/?q=x")), None);
    }

    #[test]
    fn the_pool_has_one_row_per_page_and_no_noise() {
        let pool = pool(
            &[
                entry("https://www.example.com/a", "A page", 2, NOW),
                entry("http://example.com/a/", "example.com", 3, NOW - DAY),
                entry("https://example.com/a?utm_source=news", "A page", 1, NOW - DAY),
                entry("https://accounts.google.com/signin", "Sign in", 9, NOW),
                entry("https://duckduckgo.com/?q=Café+noir", "Café noir at DuckDuckGo", 1, NOW),
                entry("https://duckduckgo.com/?q=cafe+noir&t=h", "cafe noir at DuckDuckGo", 1, NOW),
                entry("https://duckduckgo.com/?q=other", "other at DuckDuckGo", 1, NOW),
            ],
            NOW,
        );
        let pages: Vec<_> = pool.iter().filter(|s| s.search.is_none()).collect();
        assert_eq!(pages.len(), 1, "{pool:?}");
        assert_eq!(pages[0].url.scheme(), "https");
        assert_eq!(pages[0].title, "A page");
        let searches: Vec<_> = pool.iter().filter(|s| s.search.is_some()).collect();
        assert_eq!(searches.len(), 2, "one row per search, accents and case folded: {searches:?}");
        assert!(pool.iter().all(|s| s.key != "duckduckgo.com"), "result pages make no home page");
    }

    #[test]
    fn a_site_seen_through_several_pages_gets_its_home_page() {
        let mut a = entry("https://docs.rs/a/latest", "a - Rust", 1, NOW);
        a.spaces = vec![2];
        let pool = pool(
            &[
                a,
                entry("https://docs.rs/b/latest", "b - Rust", 1, NOW),
                entry("https://docs.rs/c/latest", "c - Rust", 1, NOW),
                entry("https://once.test/x", "x", 1, NOW),
            ],
            NOW,
        );
        let root = pool.iter().find(|s| s.key == "docs.rs").expect("home page added");
        assert_eq!(root.url.as_str(), "https://docs.rs/");
        assert_eq!(root.spaces, vec![2]);
        assert!(pool.iter().all(|s| s.key != "once.test"));
        assert_eq!(pool[0].key, "docs.rs");
    }

    #[test]
    fn accents_case_and_one_typo_do_not_matter() {
        let s = u("https://example.com/settings");
        assert!(match_quality("reglages", "Réglages", Some(&s)).unwrap() >= 0.9);
        assert!(match_quality("ŒUVRE", "Les oeuvres", None).is_some());
        let gh = u("https://github.com/");
        assert_eq!(match_quality("gtihub", "GitHub", Some(&gh)), Some(0.5), "two letters swapped");
        assert!(match_quality("githb", "", Some(&gh)).is_some(), "a letter missing");
        assert_eq!(match_quality("gtih", "", Some(&gh)), Some(0.5));
        assert_eq!(match_quality("gti", "", Some(&gh)), None, "no typo under 4 letters");
        assert_eq!(match_quality("zzzzhub", "GitHub", Some(&gh)), None);
    }

    #[test]
    fn every_word_must_match_and_the_start_of_a_name_wins() {
        let gh = u("https://github.com/rust-lang/rust");
        assert_eq!(match_quality("git", "rust-lang/rust", Some(&gh)), Some(1.0));
        assert!(match_quality("rust lang", "rust-lang/rust: Empowering", Some(&gh)).unwrap() > 0.7);
        assert_eq!(match_quality("rust python", "rust-lang/rust", Some(&gh)), None);
        let mail = u("https://mail.proton.me/u/0/inbox");
        assert!(match_quality("p", "Inbox | Proton Mail", Some(&mail)).is_some());
        assert_eq!(match_quality("x", "Inbox | Proton Mail", Some(&mail)), None);
        assert_eq!(match_quality("github.com/rust", "", Some(&gh)), Some(1.0));
        assert!(match_quality("lang", "Something", Some(&gh)).unwrap() < 0.5);
        assert_eq!(match_quality("zoom", "Zoom in", None), Some(0.9), "an action has no address");
    }

    #[test]
    fn choices_of_two_devices_merge_the_latest_way() {
        let mut a = Adaptive::default();
        let mut b = Adaptive::default();
        a.record("ma", "mail.test", NOW);
        b.record("ma", "mail.test", NOW + DAY);
        b.record("ma", "mail.test", NOW + DAY);
        b.record("gi", "github.com", NOW);
        assert!(a.merge(&b.shared(100)));
        assert!(!a.merge(&b.shared(100)), "twice changes nothing");
        assert_eq!(a.entries().len(), 2);
        assert!(a.boost("ma", "mail.test", NOW + DAY) > 0.6, "the later choice (used twice) won");
        assert!(!b.merge(&a.shared(100)), "b already had the latest");
        assert_eq!(a.shared(1).len(), 1);
    }

    #[test]
    fn choices_are_learned_by_prefix_and_fade() {
        let mut a = Adaptive::default();
        assert!(a.record("Ma", "mail.test", NOW));
        assert!(!a.record("  ", "x", NOW));
        assert_eq!(a.boost("ma", "mail.test", NOW), 0.5);
        assert_eq!(a.boost("mail", "mail.test", NOW), 0.5, "a longer input starting with a choice");
        assert_eq!(a.boost("m", "mail.test", NOW), 0.5, "a shorter input of a choice");
        assert_eq!(a.boost("ma", "other.test", NOW), 0.0);
        a.record("ma", "mail.test", NOW);
        assert!(a.boost("ma", "mail.test", NOW) > 0.6);
        assert!(a.boost("ma", "mail.test", NOW + 120 * DAY) < 0.2, "old choices fade");
        a.forget("mail.test");
        assert_eq!(a.boost("ma", "mail.test", NOW), 0.0);
        for i in 0..(ADAPTIVE_MAX + 20) {
            a.record(&format!("q{i}"), "k", NOW);
        }
        assert_eq!(a.entries().len(), ADAPTIVE_MAX);
    }

    fn row(kind: RowKind, title: &str, url: &str, frecency: f64) -> Row {
        let url = (!url.is_empty()).then(|| u(url));
        Row {
            kind,
            id: title.to_owned(),
            title: title.to_owned(),
            key: url.as_ref().map_or_else(|| format!("action:{title}"), key),
            url,
            frecency,
            spaces: Vec::new(),
        }
    }

    #[test]
    fn a_page_found_several_ways_is_offered_once_as_its_open_tab() {
        let rows = vec![
            row(RowKind::History, "GitHub", "https://github.com/", 500.0),
            row(RowKind::Tab, "GitHub", "http://www.github.com", 0.0),
            row(RowKind::Favorite, "GitHub", "https://github.com", 0.0),
        ];
        let ctx = Context { space: 1, adaptive: &Adaptive::default(), now: NOW };
        let ranked = rank_rows(&rows, "git", &ctx);
        assert_eq!(ranked.len(), 1);
        assert_eq!(rows[ranked[0].index].kind, RowKind::Tab);
        assert!(ranked[0].score > 100.0 + 30.0, "the history's habit counts for the tab");
    }

    #[test]
    fn pick_keeps_room_for_commands_and_two_searches_at_most() {
        let mut rows: Vec<Row> = (0..10).map(|i| row(RowKind::History, &format!("zoom page {i}"), &format!("https://z{i}.test/"), 200.0)).collect();
        rows.push(row(RowKind::Action, "Page zoom reset", "", 0.0));
        for i in 0..4 {
            let mut s = row(RowKind::Search, &format!("zoom tips {i}"), "", 0.0);
            s.key = format!("search:zoom tips {i}");
            rows.push(s);
        }
        let ctx = Context { space: 1, adaptive: &Adaptive::default(), now: NOW };
        let ranked = rank_rows(&rows, "zoom", &ctx);
        let picked = pick(&rows, &ranked, 8, 3, 2);
        assert_eq!(picked.len(), 8);
        assert!(picked.iter().any(|r| rows[r.index].kind == RowKind::Action), "the command keeps a place");
        assert!(picked.iter().filter(|r| rows[r.index].kind == RowKind::Search).count() <= 2);
    }

    #[test]
    fn the_field_completes_a_habit_only() {
        let ctx = Context { space: 1, adaptive: &Adaptive::default(), now: NOW };
        let rows = vec![row(RowKind::History, "GitHub", "https://www.github.com/rust-lang/rust", 400.0)];
        let ranked = rank_rows(&rows, "git", &ctx);
        let c = completion(&rows, &ranked, "git", &ctx).expect("a habit");
        assert_eq!(c.text, "github.com");
        assert_eq!(c.url.as_str(), "https://www.github.com/");
        let ranked = rank_rows(&rows, "github.com/ru", &ctx);
        let c = completion(&rows, &ranked, "github.com/ru", &ctx).expect("a path");
        assert_eq!(c.text, "github.com/rust-lang/rust");
        assert!(completion(&rows, &ranked, "g", &ctx).is_none(), "one letter is too little");
        let rare = vec![row(RowKind::History, "GitHub", "https://github.com/", 5.0)];
        let ranked = rank_rows(&rare, "git", &ctx);
        assert!(completion(&rare, &ranked, "git", &ctx).is_none(), "not a habit");
        let mut learned = Adaptive::default();
        learned.record("gi", &rare[0].key, NOW);
        let ctx = Context { space: 1, adaptive: &learned, now: NOW };
        let ranked = rank_rows(&rare, "git", &ctx);
        assert!(completion(&rare, &ranked, "git", &ctx).is_some(), "chosen before");
        assert!(completion(&rare, &ranked, "rust lang", &ctx).is_none());
    }
}

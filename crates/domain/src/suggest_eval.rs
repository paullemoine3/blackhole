//! Evaluation of the command bar's ranking: a history close to a real one (a few months of
//! habits, sign-in steps, searches, a local NAS, pages seen once long ago) and what each query
//! must bring first. Every change of weights is measured here rather than judged by eye; a new
//! case goes here when a query gave a poor answer.

use super::*;
use crate::history::{HistoryEntry, Visit, VisitKind};

const DAY: u64 = 86_400;
const NOW: u64 = 20_000 * DAY;

/// A page visited `n` times, the last `days` ago, every visit of `kind`, in `space`.
fn page(url: &str, title: &str, n: u32, days: u64, kind: VisitKind, space: u64) -> HistoryEntry {
    let last = NOW - days * DAY;
    HistoryEntry {
        url: Url::parse(url).unwrap(),
        title: title.to_owned(),
        visited_at: last,
        visits: n,
        recent: (0..u64::from(n.min(10))).map(|i| Visit { at: last - i * 2 * DAY, kind }).collect(),
        spaces: vec![space],
        changed: last,
    }
}

fn history() -> Vec<HistoryEntry> {
    use VisitKind::*;
    vec![
        page("https://github.com/", "GitHub", 40, 0, Typed, 1),
        page("https://github.com/rust-lang/rust", "rust-lang/rust: Empowering everyone", 6, 3, Link, 1),
        page("https://github.com/notifications", "Notifications", 8, 1, Link, 1),
        page("https://gist.github.com/", "Your gists", 2, 40, Link, 1),
        page("https://github.com/login?return_to=%2F", "Sign in to GitHub", 12, 0, Redirect, 1),
        page("https://accounts.google.com/v3/signin/identifier?continue=x", "Sign in - Google Accounts", 9, 2, Link, 1),
        page("https://mail.google.com/mail/u/0/", "Inbox (3) - Gmail", 30, 0, Typed, 1),
        page("https://www.marmiton.org/recettes/recette_crepes.aspx", "Recette de crêpes", 1, 25, Link, 2),
        page("https://www.youtube.com/", "YouTube", 25, 0, Typed, 2),
        page("https://www.youtube.com/watch?v=a1", "Lofi beats to code to - YouTube", 3, 2, Link, 2),
        page("https://www.youtube.com/watch?v=b2", "Rust in 100 seconds - YouTube", 1, 9, Link, 1),
        page("https://docs.rs/serde/latest/serde/", "serde - Rust", 4, 5, Link, 1),
        page("https://docs.rs/tokio/latest/tokio/", "tokio - Rust", 3, 6, Link, 1),
        page("https://docs.rs/url/latest/url/", "url - Rust", 2, 8, Link, 1),
        page("https://duckduckgo.com/?q=m%C3%A9t%C3%A9o+paris", "météo paris at DuckDuckGo", 4, 1, Typed, 1),
        page("https://duckduckgo.com/?q=rust+borrow+checker", "rust borrow checker at DuckDuckGo", 1, 30, Typed, 1),
        page("https://fr.wikipedia.org/wiki/R%C3%A9seau_de_neurones_artificiels", "Réseau de neurones artificiels — Wikipédia", 2, 12, Link, 1),
        page("http://192.168.1.20:5000/", "Synology DiskStation", 6, 4, Typed, 1),
        page("https://www.lemonde.fr/", "Le Monde.fr - Actualités", 15, 1, Typed, 2),
        page("https://www.lemonde.fr/economie/article/2024/01/01/x.html", "Les marchés en 2024", 1, 200, Link, 2),
        page("https://blog.example.com/2019/how-git-works", "Old notes", 1, 400, Link, 1),
        page("https://www.leboncoin.fr/", "leboncoin, site de petites annonces", 7, 3, Typed, 2),
        page("https://www.ma-banque.fr/connexion", "Connexion - Ma Banque", 10, 1, Link, 1),
        page("https://www.notion.so/Work-plan-1", "Work plan", 5, 2, Link, 2),
        page("https://www.notion.so/Holidays-2", "Holidays plan", 5, 2, Link, 1),
    ]
}

struct Bar {
    rows: Vec<Row>,
    adaptive: Adaptive,
    space: u64,
}

impl Bar {
    fn new() -> Self {
        let rows = pool(&history(), NOW)
            .into_iter()
            .map(|s| Row {
                kind: if s.search.is_some() { RowKind::Search } else { RowKind::History },
                id: s.key.clone(),
                title: s.title,
                url: Some(s.url),
                key: s.key,
                frecency: s.frecency,
                spaces: s.spaces,
            })
            .collect();
        Self { rows, adaptive: Adaptive::default(), space: 1 }
    }

    /// Titles of the rows shown for `query`, best first (the palette's eight places).
    fn top(&self, query: &str) -> Vec<String> {
        let ctx = Context { space: self.space, adaptive: &self.adaptive, now: NOW };
        let ranked = rank_rows(&self.rows, query, &ctx);
        pick(&self.rows, &ranked, 8, 3, 2).iter().map(|r| self.rows[r.index].title.clone()).collect()
    }

    fn first(&self, query: &str) -> String {
        self.top(query).into_iter().next().unwrap_or_default()
    }
}

#[test]
fn a_few_letters_bring_the_site_used_every_day() {
    let bar = Bar::new();
    assert_eq!(bar.first("git"), "GitHub");
    assert_eq!(bar.first("you"), "YouTube");
    assert_eq!(bar.first("lem"), "Le Monde.fr - Actualités");
    assert_eq!(bar.first("gm"), "Inbox (3) - Gmail", "the title's word, not only the host");
    assert_eq!(bar.first("leb"), "leboncoin, site de petites annonces");
}

#[test]
fn typos_and_accents_still_find_it() {
    let bar = Bar::new();
    assert_eq!(bar.first("gtihub"), "GitHub");
    assert_eq!(bar.first("youtueb"), "YouTube");
    assert_eq!(bar.first("reseau neurones"), "Réseau de neurones artificiels — Wikipédia");
    assert_eq!(bar.first("crepes"), "Recette de crêpes");
}

#[test]
fn searches_made_before_come_back_by_their_words() {
    let bar = Bar::new();
    assert_eq!(bar.first("meteo"), "météo paris");
    assert!(bar.top("rust").contains(&"rust borrow checker".to_owned()));
}

#[test]
fn sign_in_steps_never_show() {
    let bar = Bar::new();
    for q in ["sign", "connexion", "login", "accounts", "banque"] {
        let top = bar.top(q);
        assert!(top.iter().all(|t| !t.contains("Sign in") && !t.contains("Connexion")), "{q}: {top:?}");
    }
}

#[test]
fn an_old_page_that_only_mentions_the_word_comes_last() {
    let bar = Bar::new();
    let top = bar.top("git");
    assert_eq!(top.last().map(String::as_str), Some("Old notes"), "{top:?}");
    assert!(top.contains(&"rust-lang/rust: Empowering everyone".to_owned()));
}

#[test]
fn a_local_nas_is_found_by_its_name_or_address() {
    let bar = Bar::new();
    assert_eq!(bar.first("syno"), "Synology DiskStation");
    assert_eq!(bar.first("192.168"), "Synology DiskStation");
}

#[test]
fn a_site_with_several_pages_offers_its_home() {
    let bar = Bar::new();
    assert_eq!(bar.first("docs"), "docs.rs");
}

#[test]
fn the_space_shown_breaks_a_tie() {
    let mut bar = Bar::new();
    bar.space = 2;
    assert_eq!(bar.first("plan"), "Work plan");
    bar.space = 1;
    assert_eq!(bar.first("plan"), "Holidays plan");
}

#[test]
fn a_choice_made_before_comes_first_next_time() {
    let mut bar = Bar::new();
    assert_ne!(bar.first("r"), "Rust in 100 seconds - YouTube");
    let key = bar.rows.iter().find(|r| r.title == "Rust in 100 seconds - YouTube").unwrap().key.clone();
    bar.adaptive.record("ru", &key, NOW);
    bar.adaptive.record("rus", &key, NOW);
    assert_eq!(bar.first("ru"), "Rust in 100 seconds - YouTube");
    assert_eq!(bar.first("rust"), "Rust in 100 seconds - YouTube");
}

#[test]
fn the_field_completes_the_daily_sites_only() {
    let bar = Bar::new();
    let ctx = Context { space: 1, adaptive: &bar.adaptive, now: NOW };
    let complete = |q: &str| {
        let ranked = rank_rows(&bar.rows, q, &ctx);
        completion(&bar.rows, &ranked, q, &ctx).map(|c| c.text)
    };
    assert_eq!(complete("gi").as_deref(), Some("github.com"));
    assert_eq!(complete("lemo").as_deref(), Some("lemonde.fr"));
    assert_eq!(complete("github.com/rust").as_deref(), Some("github.com/rust-lang/rust"));
    assert_eq!(complete("blog"), None, "a page seen once long ago is no habit");
    assert_eq!(complete("meteo"), None, "a search is not an address");
}

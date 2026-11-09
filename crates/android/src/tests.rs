use crate::bridge::{Effect, MobileCommand as C, NativeEvent as E, PageEvent};
use crate::core::{Core, MAX_LIVE};
use blackhole_domain::PrivacyPolicy;
use serde_json::{json, Value};

fn core(dir: &tempfile::TempDir) -> Core {
    let policy = PrivacyPolicy::with_ads(
        vec!["tracker.example".to_owned()],
        vec!["ads.example".to_owned()],
    );
    Core::with_policy(dir.path(), policy)
}

fn opened(out: &[Effect]) -> Option<(u64, String, String)> {
    out.iter().find_map(|e| match e {
        Effect::Open { tab, url, profile, .. } => Some((*tab, url.clone(), profile.clone())),
        _ => None,
    })
}

fn ui(out: &[Effect], kind: &str) -> Option<Value> {
    out.iter().find_map(|e| match e {
        Effect::Ui { msg } if msg["type"] == kind => Some(msg.clone()),
        _ => None,
    })
}

fn html_kind(out: &[Effect]) -> Option<String> {
    out.iter().find_map(|e| match e {
        Effect::Html { html, .. } => {
            let at = html.find(r#""kind":""#)? + 8;
            Some(html[at..].split('"').next()?.to_owned())
        }
        _ => None,
    })
}

fn go(c: &mut Core, input: &str) -> u64 {
    let out = c.command(C::Navigate {
        input: input.into(),
        new_tab: true,
        private: false,
    });
    opened(&out).map(|o| o.0).expect("a tab opened")
}

#[test]
fn an_address_typed_on_the_start_page_opens_a_tab_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    assert_eq!(c.boot()[0], Effect::Show { tab: None });
    let out = c.command(C::Navigate {
        input: "example.org".into(),
        new_tab: false,
        private: false,
    });
    let (tab, url, profile) = opened(&out).unwrap();
    assert_eq!(url, "https://example.org/");
    assert_eq!(profile, "Default");
    assert!(out.contains(&Effect::Show { tab: Some(tab) }));
    let state = ui(&out, "state").unwrap();
    assert_eq!(state["current"], json!(tab));
    assert_eq!(state["tabs"][0]["id"], json!(tab));
    // Typed again in the bar: same tab, no second view.
    let again = c.command(C::Navigate {
        input: "rust lang".into(),
        new_tab: false,
        private: false,
    });
    assert!(opened(&again).is_none());
    assert!(again.iter().any(|e| matches!(e, Effect::Load { tab: t, url } if *t == tab && url.starts_with("https://duckduckgo.com/"))));
}

#[test]
fn private_tabs_leave_no_history_and_their_data_goes_with_the_last_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let out = c.command(C::Navigate {
        input: "https://secret.example/".into(),
        new_tab: true,
        private: true,
    });
    let (tab, _, profile) = opened(&out).unwrap();
    assert_eq!(profile, "private");
    assert_eq!(ui(&out, "state").unwrap()["private"], json!(true));
    c.event(E::Finished {
        tab,
        url: "https://secret.example/".into(),
        title: "Secret".into(),
    });
    let history = c.command(C::History { query: String::new() });
    assert_eq!(ui(&history, "history").unwrap()["items"], json!([]));
    let closed = c.command(C::Close { tab });
    assert!(closed.contains(&Effect::ClearProfile { profile: "private".into() }));
    assert!(closed.contains(&Effect::Show { tab: None }));
}

#[test]
fn a_visited_page_lands_in_history_and_suggestions() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://www.rust-lang.org/");
    c.event(E::Finished {
        tab,
        url: "https://www.rust-lang.org/learn".into(),
        title: "Learn Rust".into(),
    });
    let h = ui(&c.command(C::History { query: "learn".into() }), "history").unwrap();
    assert_eq!(h["items"][0]["url"], json!("https://www.rust-lang.org/learn"));
    let s = ui(&c.command(C::Suggest { text: "rust".into() }), "suggest").unwrap();
    assert_eq!(s["items"][0]["kind"], json!("search"));
    assert!(s["items"].as_array().unwrap().iter().any(|i| i["kind"] == "tab"));
    c.command(C::ClearHistory);
    let h = ui(&c.command(C::History { query: String::new() }), "history").unwrap();
    assert_eq!(h["items"], json!([]));
}

#[test]
fn an_early_title_does_not_rename_the_previous_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://a.example/");
    c.event(E::Finished {
        tab,
        url: "https://a.example/".into(),
        title: "Alpha".into(),
    });
    c.event(E::Started {
        tab,
        url: "https://b.example/".into(),
    });
    c.event(E::Title {
        tab,
        title: "Beta".into(),
    });
    c.event(E::Finished {
        tab,
        url: "https://b.example/".into(),
        title: "Beta".into(),
    });
    let h = ui(&c.command(C::History { query: String::new() }), "history").unwrap();
    let titles: Vec<(String, String)> = h["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["url"].as_str().unwrap().to_owned(), i["title"].as_str().unwrap().to_owned()))
        .collect();
    assert!(titles.contains(&("https://a.example/".into(), "Alpha".into())), "{titles:?}");
    assert!(titles.contains(&("https://b.example/".into(), "Beta".into())), "{titles:?}");
}

#[test]
fn only_a_few_views_stay_alive() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let first = go(&mut c, "https://a.example/");
    let mut drops = Vec::new();
    for host in ["b", "c", "d", "e"] {
        let out = c.command(C::Navigate {
            input: format!("https://{host}.example/"),
            new_tab: true,
            private: false,
        });
        drops.extend(out.iter().filter_map(|e| match e {
            Effect::Drop { tab } => Some(*tab),
            _ => None,
        }));
    }
    assert_eq!(MAX_LIVE, 4);
    assert_eq!(drops, vec![first]);
    // Coming back recreates the view at the tab's address.
    let back = c.command(C::Activate { tab: first });
    assert_eq!(opened(&back).map(|o| o.1), Some("https://a.example/".into()));
}

#[test]
fn closing_shows_the_successor_and_undo_brings_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let a = go(&mut c, "https://a.example/");
    let b = go(&mut c, "https://b.example/");
    let out = c.command(C::Close { tab: b });
    assert!(out.contains(&Effect::Drop { tab: b }));
    assert!(out.contains(&Effect::Show { tab: Some(a) }));
    assert_eq!(ui(&out, "toast").unwrap()["undo"], json!(true));
    let undo = c.command(C::UndoClose);
    assert_eq!(opened(&undo).map(|o| o.1), Some("https://b.example/".into()));
}

#[test]
fn a_quick_access_tile_is_put_to_sleep_not_removed() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let p = go(&mut c, "https://pinned.example/");
    c.command(C::Pin { tab: p });
    let out = c.command(C::Close { tab: p });
    assert!(out.contains(&Effect::Drop { tab: p }));
    let state = ui(&out, "state").unwrap();
    assert!(state["tabs"].as_array().unwrap().iter().any(|t| t["id"] == json!(p) && t["pinned"] == json!(true)));
}

#[test]
fn a_failed_upgrade_offers_the_plain_site_and_only_the_shell_grants_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let out = c.command(C::Navigate {
        input: "http://plain.example/".into(),
        new_tab: true,
        private: false,
    });
    let (tab, url, _) = opened(&out).unwrap();
    assert_eq!(url, "https://plain.example/");
    let failed = c.event(E::Failed {
        tab,
        url: url.clone(),
        code: -6,
        online: true,
    });
    assert_eq!(html_kind(&failed).as_deref(), Some("http_only"));
    // The page's button only asks for the menu; a page elsewhere gets nothing.
    assert!(c.page_event(tab + 1, PageEvent::ErrorMenu { x: 0.0, y: 0.0 }).is_empty());
    let menu = c.page_event(tab, PageEvent::ErrorMenu { x: 0.0, y: 0.0 });
    assert_eq!(ui(&menu, "error_menu").unwrap()["host"], json!("plain.example"));
    let proceed = c.command(C::ErrorProceed { tab });
    assert!(proceed.contains(&Effect::Load {
        tab,
        url: "http://plain.example/".into()
    }));
    // The plain site is no longer upgraded.
    assert!(c
        .event(E::Navigating {
            tab,
            url: "http://plain.example/page".into()
        })
        .is_empty());
}

#[test]
fn certificate_errors_are_refused_until_the_user_accepts_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://self-signed.example/");
    let ssl = E::SslError {
        tab,
        url: "https://self-signed.example/".into(),
        primary: 3,
    };
    let out = c.event(ssl.clone());
    assert_eq!(out[0], Effect::Ssl { proceed: false });
    assert_eq!(html_kind(&out).as_deref(), Some("certificate"));
    // The WebView's own error for the same address is not shown twice.
    assert!(c
        .event(E::Failed {
            tab,
            url: "https://self-signed.example/".into(),
            code: -11,
            online: true
        })
        .is_empty());
    c.command(C::ErrorProceed { tab });
    assert_eq!(c.event(ssl), vec![Effect::Ssl { proceed: true }]);
}

#[test]
fn an_hsts_site_gets_no_exception() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://google.com/");
    c.event(E::SslError {
        tab,
        url: "https://google.com/".into(),
        primary: 1,
    });
    let out = c.command(C::ErrorProceed { tab });
    assert!(matches!(out.as_slice(), [Effect::Ui { msg }] if msg["type"] == "toast"));
}

#[test]
fn page_navigations_are_filtered() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://news.example/");
    let tracker = c.event(E::Navigating {
        tab,
        url: "https://px.tracker.example/c".into(),
    });
    assert_eq!(tracker[0], Effect::Cancel);
    assert_eq!(html_kind(&tracker).as_deref(), Some("blocked"));
    let stripped = c.event(E::Navigating {
        tab,
        url: "https://shop.example/?id=3&utm_source=x".into(),
    });
    assert_eq!(
        stripped,
        vec![
            Effect::Cancel,
            Effect::Load {
                tab,
                url: "https://shop.example/?id=3".into()
            }
        ]
    );
    let intent = c.event(E::Navigating {
        tab,
        url: "intent://scan/#Intent;scheme=zxing;end".into(),
    });
    assert_eq!(intent[0], Effect::Cancel);
    assert!(ui(&intent, "external").is_some());
    assert!(c
        .event(E::Navigating {
            tab,
            url: "file:///sdcard/secret".into()
        })
        .contains(&Effect::Cancel));
    assert!(c.command(C::OpenExternal { url: "file:///x".into() }).is_empty());
    assert_eq!(
        c.command(C::OpenExternal { url: "mailto:a@b.example".into() }),
        vec![Effect::External { url: "mailto:a@b.example".into() }]
    );
}

#[test]
fn blocked_requests_feed_the_privacy_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://news.example/");
    c.event(E::Finished {
        tab,
        url: "https://news.example/".into(),
        title: "News".into(),
    });
    let blocker = c.blocker();
    assert!(blocker.should_block("https://cdn.tracker.example/p.js", "https://news.example/"));
    assert!(blocker.should_block("https://ads.example/b.gif", "https://news.example/"));
    assert!(!blocker.should_block("https://cdn.example/app.js", "https://news.example/"));
    assert!(!blocker.should_block("data:text/plain,x", "https://news.example/"));
    let report = ui(&c.command(C::PrivacyReport), "report").unwrap();
    assert_eq!(report["site"], json!("news.example"));
    assert_eq!((report["trackers"].clone(), report["ads"].clone()), (json!(1), json!(1)));
    assert_eq!(report["grade"], json!("B"));
    c.command(C::SetSetting {
        name: "trackers".into(),
        value: json!(false),
    });
    assert!(!blocker.should_block("https://cdn.tracker.example/p.js", "https://news.example/"));
}

#[test]
fn back_leaves_a_tab_for_the_start_page_then_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    go(&mut c, "https://a.example/");
    assert!(c.event(E::BackExhausted).contains(&Effect::Show { tab: None }));
    assert_eq!(c.event(E::BackExhausted), vec![Effect::Exit]);
}

#[test]
fn only_secure_downloads_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let ok = c.event(E::Download {
        url: "https://files.example/a.zip".into(),
        mime: "application/zip".into(),
        disposition: String::new(),
    });
    assert!(matches!(ok[0], Effect::Download { .. }));
    let refused = c.event(E::Download {
        url: "http://files.example/a.zip".into(),
        mime: String::new(),
        disposition: String::new(),
    });
    assert!(refused.iter().all(|e| !matches!(e, Effect::Download { .. })));
}

#[test]
fn settings_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut c = core(&dir);
        c.command(C::SetSetting { name: "theme".into(), value: json!("light") });
        c.command(C::SetSetting { name: "engine".into(), value: json!("brave") });
        c.command(C::SetSetting { name: "cookie_banners".into(), value: json!(false) });
        assert!(c.command(C::SetSetting { name: "theme".into(), value: json!("neon") }).is_empty());
        go(&mut c, "https://kept.example/");
    }
    let c = core(&dir);
    let Effect::Ui { msg } = c.state() else { panic!() };
    assert_eq!(msg["settings"]["theme"], json!("light"));
    assert_eq!(msg["settings"]["engine"], json!("brave"));
    assert_eq!(msg["settings"]["cookie_banners"], json!(false));
    assert_eq!(msg["tabs"][0]["url"], json!("https://kept.example/"));
}

#[test]
fn spaces_get_their_own_profile() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    c.command(C::AddSpace {
        name: "Work".into(),
        color: "#2dd4bf".into(),
    });
    let out = c.command(C::Navigate {
        input: "https://work.example/".into(),
        new_tab: true,
        private: false,
    });
    let (_, _, profile) = opened(&out).unwrap();
    assert!(profile.starts_with("space-"), "{profile}");
    let state = ui(&out, "state").unwrap();
    assert_eq!(state["space"]["name"], json!("Work"));
    assert_eq!(state["spaces"].as_array().unwrap().len(), 2);
}

#[test]
fn json_entry_points_ignore_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    assert_eq!(c.command_json("{"), "[]");
    assert_eq!(c.event_json(r#"{"ev":"nope"}"#), "[]");
    assert_eq!(c.page_event_json(1, r#"{"cmd":"clear_site_data"}"#), "[]");
    assert!(c.command_json(r#"{"cmd":"ready"}"#).contains(r#""type":"state""#));
}

#[test]
fn the_error_page_escapes_site_text() {
    let html = crate::error_page(&json!({"kind": "unreachable", "url": "https://x.example/</script><script>alert(1)"}));
    assert!(!html.contains("</script><script>alert(1)"));
    assert!(html.contains("\\u003c/script>"));
}

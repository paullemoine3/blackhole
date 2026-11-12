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
    assert!(c.boot().contains(&Effect::Show { tab: None }));
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
    // Forgetting a visit from a filtered list keeps the filter.
    let other = go(&mut c, "https://example.org/");
    c.event(E::Finished {
        tab: other,
        url: "https://example.org/".into(),
        title: "Example".into(),
    });
    let out = c.command(C::ForgetHistory {
        url: "https://www.rust-lang.org/learn".into(),
        query: "learn".into(),
    });
    let h = ui(&out, "history").unwrap();
    assert_eq!((h["query"].clone(), h["items"].clone()), (json!("learn"), json!([])));
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
    // The shield's count: two numbers for the tab on screen, never the whole state.
    let out = c.event(E::Blocked { tab });
    assert_eq!(out.len(), 1);
    let counts = ui(&out, "blocked").unwrap();
    assert_eq!((counts["site"].clone(), counts["total"].clone()), (json!(2), json!(2)));
    assert!(c.event(E::Blocked { tab: tab + 1 }).is_empty());
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
fn try_again_on_the_error_page_lets_the_page_that_loads_be_seen() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://down.example/");
    let failed = c.event(E::Failed {
        tab,
        url: "https://down.example/".into(),
        code: -6,
        online: true,
    });
    assert_eq!(html_kind(&failed).as_deref(), Some("unreachable"));
    assert_eq!(ui(&failed, "state").unwrap()["error"], json!("unreachable"));
    // The page's button loads the address again: the engine asks, then the page arrives.
    assert!(c
        .event(E::Navigating {
            tab,
            url: "https://down.example/".into()
        })
        .is_empty());
    c.event(E::Started {
        tab,
        url: "https://down.example/".into(),
    });
    let done = c.event(E::Finished {
        tab,
        url: "https://down.example/".into(),
        title: "Back up".into(),
    });
    let state = ui(&done, "state").expect("the loaded page is reported");
    assert_eq!(state["error"], json!(null));
}

#[test]
fn a_blocked_page_offers_no_exception_the_core_could_not_grant() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://example.com/");
    let out = c.event(E::BlockedPage {
        tab,
        url: "https://stats.g.doubleclick.net/x".into(),
    });
    assert_eq!(html_kind(&out).as_deref(), Some("blocked"));
    let html = out
        .iter()
        .find_map(|e| match e {
            Effect::Html { html, .. } => Some(html.clone()),
            _ => None,
        })
        .unwrap();
    assert!(html.contains("\"allow\":false"));
    assert!(c.page_event(tab, PageEvent::ErrorMenu { x: 0.0, y: 0.0 }).is_empty());
}

#[test]
fn the_error_page_escapes_site_text() {
    let html = crate::error_page(&json!({"kind": "unreachable", "url": "https://x.example/</script><script>alert(1)"}));
    assert!(!html.contains("</script><script>alert(1)"));
    assert!(html.contains("\\u003c/script>"));
}

fn has_toast(out: &[Effect], text: &str) -> bool {
    ui(out, "toast").is_some_and(|t| t["text"].as_str().unwrap_or_default().contains(text))
}

const REPO_TAG: &str = "https://github.com/paullemoine3/blackhole/releases/tag/";

#[test]
fn a_manual_update_check_finds_the_apk_signed_like_the_app_then_installs_it_when_it_matches() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let out = c.command(C::CheckUpdate { manual: true });
    assert!(matches!(&out[0], Effect::UpdateProbe { url } if url.ends_with("/releases/latest")));
    assert_eq!(ui(&out, "state").unwrap()["update"]["status"], "checking");
    // A second check while one runs does nothing.
    assert!(c.command(C::CheckUpdate { manual: true }).is_empty());
    let out = c.event(E::UpdateLatest {
        url: format!("{REPO_TAG}v999.1.0"),
    });
    let Effect::UpdateSums { tag, url } = &out[0] else { panic!("{out:?}") };
    assert_eq!(tag, "v999.1.0");
    assert!(url.ends_with("/releases/download/v999.1.0/SHA256SUMS.txt"));
    let sums = format!(
        "{}  BlackHole-android.apk\n{}  BlackHole-android-debug.apk\n",
        "a".repeat(64),
        "b".repeat(64)
    );
    let out = c.event(E::UpdateSums {
        tag: "v999.1.0".into(),
        sums: sums.clone(),
        debug: true,
    });
    assert!(has_toast(&out, "999.1.0 is available"));
    let update = &ui(&out, "state").unwrap()["update"];
    assert_eq!(update["status"], "available");
    assert_eq!(update["version"], "999.1.0");
    // The debug-signed app gets the debug APK: a release-signed one could not replace it.
    let out = c.command(C::InstallUpdate);
    assert!(matches!(&out[0], Effect::UpdateDownload { name, sha256, url }
        if name == "BlackHole-android-debug.apk" && *sha256 == "b".repeat(64) && url.starts_with("https://github.com/")));
    // A file that is not the one listed is thrown away, never installed.
    let out = c.event(E::UpdateDownloaded { sha256: "c".repeat(64) });
    assert_eq!(out[0], Effect::UpdateDiscard);
    assert!(!out.contains(&Effect::UpdateInstall));
    assert_eq!(ui(&out, "state").unwrap()["update"]["retry"], true);
    // Install again (retry), the right file this time.
    assert!(matches!(c.command(C::InstallUpdate)[0], Effect::UpdateDownload { .. }));
    let out = c.event(E::UpdateDownloaded { sha256: "B".repeat(64) });
    assert_eq!(out[0], Effect::UpdateInstall);
    // A late answer of the network changes nothing now.
    assert!(c.event(E::UpdateLatest { url: format!("{REPO_TAG}v999.2.0") }).is_empty());
}

#[test]
fn update_checks_say_up_to_date_refuse_foreign_redirects_and_run_daily_on_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    c.command(C::CheckUpdate { manual: true });
    let out = c.event(E::UpdateLatest { url: format!("{REPO_TAG}v0.0.1") });
    assert!(has_toast(&out, "up to date"));
    assert_eq!(ui(&out, "state").unwrap()["update"]["status"], "latest");
    // A redirect anywhere else is no release.
    c.command(C::CheckUpdate { manual: true });
    let out = c.event(E::UpdateLatest { url: "https://evil.example/releases/tag/v999.0.0".into() });
    assert!(!out.iter().any(|e| matches!(e, Effect::UpdateSums { .. })));
    // A release without an APK signed like the app.
    c.command(C::CheckUpdate { manual: true });
    c.event(E::UpdateLatest { url: format!("{REPO_TAG}v999.0.0") });
    let out = c.event(E::UpdateSums {
        tag: "v999.0.0".into(),
        sums: format!("{}  BlackHole-android-debug.apk\n", "b".repeat(64)),
        debug: false,
    });
    assert_eq!(ui(&out, "state").unwrap()["update"]["status"], "failed");
    assert!(c.command(C::InstallUpdate).is_empty(), "nothing to install");
    // Just checked: the app coming back does not check again; a day later it does, silently.
    assert!(c.event(E::Resume).is_empty());
    c.prefs.update_checked -= 25 * 3600;
    let out = c.event(E::Resume);
    assert!(matches!(out[0], Effect::UpdateProbe { .. }));
    let out = c.event(E::UpdateLatest { url: format!("{REPO_TAG}v0.0.1") });
    assert!(ui(&out, "toast").is_none(), "an automatic check that finds nothing says nothing");
    // Turned off in Settings: never on its own.
    c.command(C::SetSetting { name: "auto_update".into(), value: json!(false) });
    c.prefs.update_checked = 0;
    assert!(c.event(E::Resume).is_empty());
    let failed = c.command(C::CheckUpdate { manual: true });
    assert!(!failed.is_empty());
    let out = c.event(E::UpdateFailed { error: "timeout\nstack".into() });
    assert!(has_toast(&out, "Update: timeout"));
}

#[test]
fn page_icons_are_kept_by_site_never_for_private_tabs_and_forgotten_with_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://www.example.org/");
    c.event(E::Started { tab, url: "https://www.example.org/".into() });
    let png = "data:image/png;base64,iVBORw0KGgo=".to_owned();
    let out = c.event(E::Favicon { tab, data: png.clone() });
    let icon = ui(&out, "icon").unwrap();
    assert_eq!(icon["host"], "example.org");
    assert!(c.event(E::Favicon { tab, data: png.clone() }).is_empty(), "same icon again");
    assert!(c.event(E::Favicon { tab, data: "data:text/html,<b>".into() }).is_empty());
    let ready = c.command(C::Ready);
    assert_eq!(ui(&ready, "icons").unwrap()["icons"]["example.org"], json!(png));
    let out = c.command(C::Navigate { input: "https://p.example/".into(), new_tab: true, private: true });
    let private = opened(&out).unwrap().0;
    c.event(E::Started { tab: private, url: "https://p.example/".into() });
    assert!(c.event(E::Favicon { tab: private, data: png.clone() }).is_empty());
    c.command(C::ClearHistory);
    assert_eq!(ui(&c.command(C::Ready), "icons").unwrap()["icons"], json!({}));
}

#[test]
fn sites_get_the_camera_only_through_the_interface_answer() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://meet.example/");
    let kinds = vec!["camera".to_owned(), "microphone".to_owned()];
    // Asked on screen: the interface asks the user.
    let out = c.event(E::Permission { id: 1, tab, origin: "https://meet.example".into(), kinds: kinds.clone() });
    let ask = ui(&out, "permission").unwrap();
    assert_eq!(ask["host"], "meet.example");
    assert_eq!(ask["kinds"], json!(["camera", "microphone"]));
    assert!(!out.iter().any(|e| matches!(e, Effect::Permission { .. })));
    assert_eq!(c.command(C::PermissionAnswer { id: 1, allow: true }), vec![Effect::Permission { id: 1, allow: true }]);
    assert!(c.command(C::PermissionAnswer { id: 1, allow: true }).is_empty(), "answered once");
    // Remembered for the site until the app closes.
    assert_eq!(
        c.event(E::Permission { id: 2, tab, origin: "https://meet.example".into(), kinds: kinds.clone() }),
        vec![Effect::Permission { id: 2, allow: true }]
    );
    // Plain http, an unknown kind or a tab that is not on screen: refused at once.
    let deny = |id| vec![Effect::Permission { id, allow: false }];
    assert_eq!(c.event(E::Permission { id: 3, tab, origin: "http://meet.example".into(), kinds: kinds.clone() }), deny(3));
    assert_eq!(c.event(E::Permission { id: 4, tab, origin: "https://meet.example".into(), kinds: vec!["usb".into()] }), deny(4));
    let other = go(&mut c, "https://other.example/");
    assert_eq!(c.event(E::Permission { id: 5, tab, origin: "https://x.example".into(), kinds: kinds.clone() }), deny(5));
    // Pending when its tab closes: refused.
    c.event(E::Permission { id: 6, tab: other, origin: "https://other.example".into(), kinds: vec!["location".into()] });
    let out = c.command(C::Close { tab: other });
    assert!(out.contains(&Effect::Permission { id: 6, allow: false }));
    // Clearing site data forgets what was allowed.
    c.command(C::Activate { tab });
    c.command(C::ClearSiteData);
    assert!(ui(&c.event(E::Permission { id: 7, tab, origin: "https://meet.example".into(), kinds }), "permission").is_some());
}

#[test]
fn shared_text_opens_its_address_or_searches_it() {
    use crate::core::shared_target;
    use blackhole_domain::SearchEngine;
    let engine = SearchEngine::DuckDuckGo;
    assert_eq!(
        shared_target("Look at this: https://example.org/a?b=1 (great)", engine).unwrap().as_str(),
        "https://example.org/a?b=1"
    );
    assert_eq!(shared_target("example.org", engine).unwrap().as_str(), "https://example.org/");
    assert!(shared_target("black holes", engine).unwrap().as_str().starts_with("https://duckduckgo.com/"));
    assert!(shared_target("javascript:alert(1)", engine).unwrap().as_str().starts_with("https://duckduckgo.com/"));
    assert!(shared_target("   ", engine).is_none());
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let out = c.event(E::Shared { text: "read https://example.org/x".into() });
    assert_eq!(opened(&out).unwrap().1, "https://example.org/x");
}

#[test]
fn desktop_site_follows_the_tab_and_web_settings_reach_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    assert!(c.boot().contains(&Effect::WebPrefs { text_zoom: 100, pull_refresh: true }));
    let tab = go(&mut c, "https://example.org/");
    let out = c.command(C::DesktopSite { tab });
    assert!(out.contains(&Effect::DesktopMode { tab, on: true }));
    assert_eq!(ui(&out, "state").unwrap()["desktop"], true);
    // A view created again (released, then shown) keeps the desktop site.
    c.live.remove(tab);
    let out = c.command(C::Activate { tab });
    assert!(out.iter().any(|e| matches!(e, Effect::Open { desktop: true, .. })));
    assert!(c.command(C::DesktopSite { tab }).contains(&Effect::DesktopMode { tab, on: false }));
    let out = c.command(C::SetSetting { name: "text_zoom".into(), value: json!(130) });
    assert!(out.contains(&Effect::WebPrefs { text_zoom: 130, pull_refresh: true }));
    assert!(c.command(C::SetSetting { name: "text_zoom".into(), value: json!(3000) }).is_empty());
    assert_eq!(c.command(C::Stop), vec![Effect::Nav { tab, action: "stop" }]);
    // "Download link" follows the download rules: https only.
    assert!(c.command(C::DownloadUrl { url: "https://example.org/f.pdf".into() }).iter().any(|e| matches!(e, Effect::Download { .. })));
    assert!(!c.command(C::DownloadUrl { url: "http://example.org/f.pdf".into() }).iter().any(|e| matches!(e, Effect::Download { .. })));
}

#[test]
fn a_recreated_activity_gets_the_tab_on_screen_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://example.org/");
    // The system recreates the activity, the process (and the core) lives on: no view exists.
    let out = c.boot();
    assert_eq!(opened(&out).map(|o| o.0), Some(tab), "the view is created again");
    assert!(out.contains(&Effect::Show { tab: Some(tab) }));
    assert_eq!(ui(&out, "state").unwrap()["current"], json!(tab));
    // On the start page, it stays there.
    c.command(C::Home);
    let out = c.boot();
    assert!(opened(&out).is_none());
    assert!(out.contains(&Effect::Show { tab: None }));
}

#[test]
fn the_go_suggestion_shows_the_whole_address_typed() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let out = c.command(C::Suggest { text: "example.org/docs/a.html?x=1".into() });
    let first = &ui(&out, "suggest").unwrap()["items"][0];
    assert_eq!(first["kind"], "go");
    assert_eq!(first["title"], "example.org/docs/a.html?x=1");
    let out = c.command(C::Suggest { text: "example.org".into() });
    assert_eq!(ui(&out, "suggest").unwrap()["items"][0]["title"], "example.org");
}

#[test]
fn a_dead_renderer_is_reloaded_once_on_screen_and_only_dropped_when_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let hidden = go(&mut c, "https://one.example/");
    let shown = go(&mut c, "https://two.example/");
    // Hidden: the view goes, nothing else (it loads again when the tab is shown).
    assert_eq!(c.event(E::Crashed { tab: hidden }), vec![Effect::Drop { tab: hidden }]);
    assert!(opened(&c.command(C::Activate { tab: hidden })).is_some());
    c.command(C::Activate { tab: shown });
    // On screen: a new view on the same address.
    let out = c.event(E::Crashed { tab: shown });
    assert_eq!(out[0], Effect::Drop { tab: shown });
    assert_eq!(opened(&out).map(|o| (o.0, o.1)), Some((shown, "https://two.example/".to_owned())));
    // Dead again at once: the start page, with a word about it.
    let out = c.event(E::Crashed { tab: shown });
    assert!(opened(&out).is_none());
    assert!(out.contains(&Effect::Show { tab: None }));
    assert!(has_toast(&out, "This page keeps crashing"));
}

#[test]
fn an_empty_error_answer_gets_our_page_and_a_page_of_the_server_stays() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let tab = go(&mut c, "https://api.example/");
    c.event(E::Started {
        tab,
        url: "https://api.example/".into(),
    });
    let probe = |title: &str, text: &str, rich: bool| {
        json!({"href": "https://api.example/", "title": title, "text": text, "rich": rich}).to_string()
    };
    let status = |c: &mut Core, status: u16, probe: String| {
        c.event(E::HttpStatus {
            tab,
            url: "https://api.example/".into(),
            status,
            probe: serde_json::to_string(&probe).unwrap(),
        })
    };
    // The server's own page (a title, or a form) is shown as sent; a 404 is never ours.
    assert!(status(&mut c, 403, probe("Access denied", "Ask your administrator", false)).is_empty());
    assert!(status(&mut c, 500, probe("", "", true)).is_empty());
    assert!(status(&mut c, 404, probe("", "", false)).is_empty());
    // Another address than the tab's: ignored.
    assert!(c
        .event(E::HttpStatus {
            tab,
            url: "https://other.example/".into(),
            status: 403,
            probe: serde_json::to_string(&probe("", "", false)).unwrap(),
        })
        .is_empty());
    // The engine's own page for an empty answer is no page of the server.
    let engine = json!({"href": "chrome-error://chromewebdata/", "title": "Webpage not available", "text": "", "rich": true}).to_string();
    let out = status(&mut c, 502, engine);
    assert_eq!(html_kind(&out).as_deref(), Some("server"));
    c.command(C::Reload);
    c.event(E::Started { tab, url: "https://api.example/".into() });
    let out = status(&mut c, 403, probe("", "403 Forbidden", false));
    assert_eq!(html_kind(&out).as_deref(), Some("forbidden"));
    assert_eq!(ui(&out, "state").unwrap()["error"], json!("forbidden"));
}

#[test]
fn a_server_gets_credentials_only_from_the_interface_for_the_tab_on_screen() {
    use crate::bridge::Secret;
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let nas = go(&mut c, "http://nas.example/");
    let other = go(&mut c, "https://other.example/");
    // A hidden tab waits; the question comes when it is shown.
    assert!(c.event(E::Auth { id: 7, tab: nas, host: "NAS.example".into(), realm: r#"Basic realm="Box""#.into() }).is_empty());
    let out = c.command(C::Activate { tab: nas });
    let ask = ui(&out, "auth").unwrap();
    assert_eq!((ask["host"].clone(), ask["realm"].clone()), (json!("nas.example"), json!("Box")));
    // (The address typed in plain http was upgraded: the password would travel encrypted.)
    assert_eq!((ask["secure"].clone(), ask["retry"].clone()), (json!(true), json!(false)));
    // A second challenge of the same realm (a sub-resource) asks nothing more…
    assert!(c.event(E::Auth { id: 8, tab: nas, host: "nas.example".into(), realm: "Box".into() }).is_empty());
    // …and one answer serves both.
    let out = c.command(C::AuthAnswer { id: ask["id"].as_u64().unwrap(), user: "me".into(), password: Secret("pw".into()) });
    for id in [7, 8] {
        assert!(out.contains(&Effect::HttpAuth { id, user: Some("me".into()), password: Some(Secret("pw".into())) }));
    }
    // Asked again before the page loaded: the credentials were refused.
    let again = ui(&c.event(E::Auth { id: 9, tab: nas, host: "nas.example".into(), realm: "Box".into() }), "auth").unwrap();
    assert_eq!(again["retry"], json!(true));
    // Closing the question cancels; closing the tab cancels what is left.
    let out = c.command(C::AuthCancel { id: again["id"].as_u64().unwrap() });
    assert!(out.contains(&Effect::HttpAuth { id: 9, user: None, password: None }));
    c.event(E::Auth { id: 10, tab: nas, host: "nas.example".into(), realm: "Box".into() });
    let out = c.command(C::Close { tab: nas });
    assert!(out.contains(&Effect::HttpAuth { id: 10, user: None, password: None }));
    assert!(ui(&out, "auth_gone").is_some());
    // A tab that does not exist is refused at once; a password never shows in a debug print.
    assert_eq!(c.event(E::Auth { id: 11, tab: other + 99, host: "x.example".into(), realm: String::new() }),
        vec![Effect::HttpAuth { id: 11, user: None, password: None }]);
    assert!(!format!("{:?}", Secret("hunter2".into())).contains("hunter2"));
    // A page cannot answer for the user.
    assert!(serde_json::from_str::<PageEvent>(r#"{"cmd":"auth_answer","id":1,"user":"a","password":"b"}"#).is_err());
}

fn popup_of(out: &[Effect]) -> Option<(u64, u64, String)> {
    out.iter().find_map(|e| match e {
        Effect::Popup { tab, opener, profile, .. } => Some((*tab, *opener, profile.clone())),
        _ => None,
    })
}

#[test]
fn a_sign_in_popup_keeps_its_opener_and_a_plain_window_loads_in_the_tab() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let other = go(&mut c, "https://other.example/");
    let app = go(&mut c, "https://app.example/");
    // Only the page on screen opens a window.
    assert!(c.event(E::NewWindow { tab: other, sized: true }).is_empty());
    let out = c.event(E::NewWindow { tab: app, sized: false });
    let (popup, opener, profile) = popup_of(&out).unwrap();
    assert_eq!((opener, profile.as_str()), (app, "Default"));
    assert!(out.contains(&Effect::Show { tab: Some(popup) }));
    let state = ui(&out, "state").unwrap();
    assert_eq!((state["popup"].clone(), state["private"].clone()), (json!(true), json!(false)));
    // Never listed with the tabs, and a popup opens no popup.
    assert_eq!(state["tabs"].as_array().unwrap().len(), 2);
    assert!(c.event(E::NewWindow { tab: popup, sized: true }).is_empty());
    // A known sign-in page stays in the popup; it leaves no history.
    let url = "https://accounts.google.com/o/oauth2/auth?client_id=x";
    assert!(c.event(E::Navigating { tab: popup, url: url.into() }).is_empty());
    c.event(E::Finished { tab: popup, url: url.into(), title: "Sign in".into() });
    let h = ui(&c.command(C::History { query: "accounts".into() }), "history").unwrap();
    assert_eq!(h["items"], json!([]));
    // `window.close()`: the opener is back, its view was never dropped.
    let out = c.event(E::ClosedWindow { tab: popup });
    assert!(out.contains(&Effect::Drop { tab: popup }) && out.contains(&Effect::Show { tab: Some(app) }));
    assert!(opened(&out).is_none());
    assert_eq!(ui(&out, "state").unwrap()["popup"], json!(false));

    // A plain `target=_blank` link: no popup is kept, the address loads in the tab that asked.
    let (popup, ..) = popup_of(&c.event(E::NewWindow { tab: app, sized: false })).unwrap();
    let out = c.event(E::Navigating { tab: popup, url: "http://news.example/a?utm_source=x".into() });
    assert_eq!(out[0], Effect::Cancel);
    assert!(out.contains(&Effect::Drop { tab: popup }) && out.contains(&Effect::Show { tab: Some(app) }));
    assert!(out.contains(&Effect::Load { tab: app, url: "https://news.example/a".into() }));
    // A script address never gets a window; a blank or sized one is a popup.
    let (popup, ..) = popup_of(&c.event(E::NewWindow { tab: app, sized: false })).unwrap();
    let out = c.event(E::Started { tab: popup, url: "javascript:alert(1)".into() });
    assert!(out.contains(&Effect::Drop { tab: popup }) && !out.iter().any(|e| matches!(e, Effect::Load { .. })));
    let (popup, ..) = popup_of(&c.event(E::NewWindow { tab: app, sized: true })).unwrap();
    assert!(c.event(E::Started { tab: popup, url: "about:blank".into() }).is_empty());
    assert!(c.event(E::Navigating { tab: popup, url: "https://shop.example/pay".into() }).is_empty());
    // Back with nothing left closes it; showing another tab drops one left behind.
    assert!(c.event(E::BackExhausted).contains(&Effect::Show { tab: Some(app) }));
    let (popup, ..) = popup_of(&c.event(E::NewWindow { tab: app, sized: true })).unwrap();
    assert!(c.command(C::Activate { tab: other }).contains(&Effect::Drop { tab: popup }));
    assert!(c.event(E::ClosedWindow { tab: popup }).is_empty());
    // The popup of a private tab is private, on the private profile.
    let out = c.command(C::Navigate { input: "https://secret.example/".into(), new_tab: true, private: true });
    let private = opened(&out).unwrap().0;
    let out = c.event(E::NewWindow { tab: private, sized: true });
    assert_eq!(popup_of(&out).unwrap().2, "private");
    assert_eq!(ui(&out, "state").unwrap()["private"], json!(true));
    let out = c.command(C::ClosePrivate);
    assert!(out.contains(&Effect::Show { tab: None }));
    assert_eq!(ui(&out, "state").unwrap()["popup"], json!(false));
}

#[test]
fn the_interface_follows_the_language_setting_then_the_system() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let texts = ui(&c.command(C::Ready), "i18n").unwrap();
    assert_eq!((texts["lang"].clone(), texts["s"]["mobile.tab_closed"].clone()), (json!("en"), json!("Tab closed")));
    // The system's first known language, when the setting says "system".
    assert!(c.event(E::Locale { tags: vec!["de-DE".into(), "en-US".into()] }).is_empty());
    let out = c.event(E::Locale { tags: vec!["de-DE".into(), "fr-FR".into()] });
    assert_eq!(ui(&out, "i18n").unwrap()["s"]["mobile.tab_closed"], json!("Onglet fermé"));
    let tab = go(&mut c, "https://example.org/");
    assert!(has_toast(&c.command(C::Close { tab }), "Onglet fermé"));
    // The core's own messages and the error page follow.
    assert!(has_toast(
        &c.command(C::Navigate { input: "https://ads.example/".into(), new_tab: true, private: false }),
        "Shields : publicité bloquée"
    ));
    let tab = go(&mut c, "https://down.example/");
    let out = c.event(E::Failed { tab, url: "https://down.example/".into(), code: -6, online: true });
    assert!(out.iter().any(|e| matches!(e, Effect::Html { html, .. } if html.contains(r#""lang":"fr""#))));
    // The setting wins over the system, and is kept.
    let out = c.command(C::SetLanguage { language: "en".into() });
    assert_eq!(ui(&out, "i18n").unwrap()["lang"], json!("en"));
    assert_eq!(ui(&out, "state").unwrap()["settings"]["language"], json!("en"));
    assert!(c.command(C::SetLanguage { language: "xx".into() }).is_empty());
    drop(c);
    let mut c = core(&dir);
    c.event(E::Locale { tags: vec!["fr-FR".into()] });
    assert_eq!(ui(&c.command(C::Ready), "i18n").unwrap()["lang"], json!("en"));
}

/// Every text the mobile interface, the core and the shell ask for exists in the catalog (the
/// domain's tests check that French has the same keys). Keys are found by their spelling:
/// `T('…')`, `Tn('…')`, `data-i18n…="…"` in the page, `t("…")`, `tf("…")`, `tn("…")`,
/// `text_f(lang, "…")`, `text_n(lang, "…")` in Rust, `toastKey("…")` in Kotlin.
#[test]
fn every_text_asked_for_is_in_the_catalog() {
    let en = blackhole_domain::i18n::catalog("en").merged();
    let has = |key: &str| en.contains_key(&*format!("mobile.{key}")) || en.contains_key(&*format!("mobile.{key}.other"));
    let root = env!("CARGO_MANIFEST_DIR");
    let mut files = vec![format!("{root}/ui/mobile.html")];
    for dir in [format!("{root}/src"), format!("{root}/../../android/app/src/main/java/app/blackhole/browser")] {
        for entry in std::fs::read_dir(dir).unwrap() {
            files.push(entry.unwrap().path().to_string_lossy().into_owned());
        }
    }
    let marks = [
        "T('", "Tn('", "data-i18n=\"", "data-i18n-ph=\"", "data-i18n-aria=\"", ".t(\"", ".tf(\"", ".tn(\"",
        "text_f(lang, \"", "text_n(lang, \"", "toastKey(\"",
    ];
    let (mut used, mut missing) = (0, Vec::new());
    for file in files.iter().filter(|f| !f.ends_with("tests.rs")) {
        let text = std::fs::read_to_string(file).unwrap();
        for mark in marks {
            for (at, _) in text.match_indices(mark) {
                // `T(` must not be the tail of another name (`el.setAttribute('…`).
                if mark.starts_with('T') && text[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                let key: String = text[at + mark.len()..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.').collect();
                used += 1;
                if !has(&key) {
                    missing.push(format!("{file}: {key}"));
                }
            }
        }
    }
    assert!(used > 150, "only {used} keys found: the scan is broken");
    assert!(missing.is_empty(), "texts missing from the catalog: {missing:#?}");
}

fn tab_of(state: &Value, id: u64) -> Option<Value> {
    state["tabs"].as_array().unwrap().iter().find(|t| t["id"] == json!(id)).cloned()
}

#[test]
fn tabs_are_renamed_filed_into_folders_and_a_folder_goes_with_its_tabs_until_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let news = go(&mut c, "https://news.example/");
    let mail = go(&mut c, "https://mail.example/");
    c.event(E::Finished { tab: news, url: "https://news.example/".into(), title: "The News".into() });
    // A name of the user's own is not replaced by the page's title; a blank one hands it back.
    let state = ui(&c.command(C::RenameTab { tab: news, name: "  Morning   read ".into() }), "state").unwrap();
    assert_eq!(tab_of(&state, news).unwrap()["title"], json!("Morning read"));
    c.event(E::Title { tab: news, title: "Breaking".into() });
    let state = ui(&c.command(C::RenameTab { tab: news, name: String::new() }), "state").unwrap();
    assert_eq!(tab_of(&state, news).unwrap()["title"], json!("Breaking"));
    // A new folder takes the tab; it is listed with the space's folders.
    let state = ui(&c.command(C::AddFolder { name: "Reading".into(), tab: Some(news) }), "state").unwrap();
    let folder = state["folders"][0]["id"].as_u64().unwrap();
    assert_eq!(state["folders"][0]["name"], json!("Reading"));
    assert_eq!(tab_of(&state, news).unwrap()["folder"], json!(folder));
    let state = ui(&c.command(C::ToggleFolder { folder }), "state").unwrap();
    assert_eq!(state["folders"][0]["collapsed"], json!(true));
    let state = ui(&c.command(C::RenameFolder { folder, name: "Later".into() }), "state").unwrap();
    assert_eq!(state["folders"][0]["name"], json!("Later"));
    // "Closing" a folder tab puts it to sleep: it keeps its place.
    c.command(C::Activate { tab: news });
    let out = c.command(C::Close { tab: news });
    assert!(out.contains(&Effect::Drop { tab: news }));
    let state = ui(&out, "state").unwrap();
    assert_eq!(tab_of(&state, news).unwrap()["live"], json!(false));
    // Out of the folder and back in, then the folder is deleted with what it holds.
    let state = ui(&c.command(C::MoveTabToFolder { tab: news, folder: None }), "state").unwrap();
    assert_eq!(tab_of(&state, news).unwrap()["folder"], Value::Null);
    c.command(C::MoveTabToFolder { tab: news, folder: Some(folder) });
    c.command(C::Activate { tab: news });
    let out = c.command(C::DeleteFolder { folder });
    assert!(out.contains(&Effect::Drop { tab: news }) && out.contains(&Effect::Show { tab: Some(mail) }));
    let said = ui(&out, "toast").unwrap();
    assert_eq!((said["text"].clone(), said["action"].clone()), (json!("Folder “Later” deleted"), json!("undo_removal")));
    let state = ui(&out, "state").unwrap();
    assert!(tab_of(&state, news).is_none() && state["folders"].as_array().unwrap().is_empty());
    let state = ui(&c.command(C::UndoRemoval), "state").unwrap();
    assert_eq!(tab_of(&state, news).unwrap()["folder"], json!(folder));
    // Nothing of this for a private tab.
    let private = opened(&c.command(C::Navigate { input: "https://secret.example/".into(), new_tab: true, private: true })).unwrap().0;
    assert!(c.command(C::RenameTab { tab: private, name: "x".into() }).is_empty());
    assert!(c.command(C::MoveTabToFolder { tab: private, folder: Some(folder) }).is_empty());
}

#[test]
fn a_tab_moves_to_another_space_and_a_space_is_deleted_until_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let keep = go(&mut c, "https://keep.example/");
    let moved = go(&mut c, "https://moved.example/");
    let state = ui(&c.command(C::AddSpace { name: "Work".into(), color: "#2dd4bf".into() }), "state").unwrap();
    let work = state["space"]["id"].as_u64().unwrap();
    let home = state["spaces"][0]["id"].as_u64().unwrap();
    // The only space with tabs comes back on screen; the last space is never deleted later.
    c.command(C::SwitchSpace { space: home });
    c.command(C::Activate { tab: moved });
    let out = c.command(C::MoveTabToSpace { tab: moved, space: work });
    // Its view goes (other cookies there) and the tab seen before takes over.
    assert!(out.contains(&Effect::Drop { tab: moved }) && out.contains(&Effect::Show { tab: Some(keep) }));
    assert!(has_toast(&out, "Moved to Work"));
    assert!(tab_of(&ui(&out, "state").unwrap(), moved).is_none());
    c.command(C::SwitchSpace { space: work });
    let out = c.command(C::Activate { tab: moved });
    assert_eq!(opened(&out).map(|o| (o.0, o.2)), Some((moved, format!("space-{work}"))));
    let state = ui(&c.command(C::RenameSpace { space: work, name: "Office".into() }), "state").unwrap();
    assert_eq!(state["space"]["name"], json!("Office"));
    // Deleting the space on screen: its views go, another space shows, Undo brings it back.
    let out = c.command(C::DeleteSpace { space: work });
    assert!(out.contains(&Effect::Drop { tab: moved }));
    assert!(has_toast(&out, "Space “Office” deleted"));
    let state = ui(&out, "state").unwrap();
    assert_eq!((state["spaces"].as_array().unwrap().len(), state["space"]["id"].clone()), (1, json!(home)));
    assert!(c.command(C::DeleteSpace { space: home }).is_empty());
    let state = ui(&c.command(C::UndoRemoval), "state").unwrap();
    assert_eq!(state["spaces"].as_array().unwrap().len(), 2);
    // Undo works once.
    assert_eq!(ui(&c.command(C::UndoRemoval), "state").unwrap()["spaces"].as_array().unwrap().len(), 2);
}

#[test]
fn other_tabs_close_at_once_and_loose_tabs_are_tidied_by_site() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = core(&dir);
    let mut tabs = Vec::new();
    for address in ["a.shop.example/1", "b.shop.example/2", "shop.example/3", "docs.example/a", "docs.example/b", "solo.example/"] {
        tabs.push(go(&mut c, &format!("https://{address}")));
    }
    let pinned = tabs[5];
    c.command(C::Pin { tab: pinned });
    // Five loose tabs: no card yet. A sixth one, and the site with three tabs is offered.
    assert_eq!(ui(&c.command(C::Ready), "state").unwrap()["tidy"], Value::Null);
    let extra = go(&mut c, "https://docs.example/c");
    let state = ui(&c.command(C::Ready), "state").unwrap();
    assert_eq!(state["tidy"], json!({"tabs": 6, "folders": 2}));
    let out = c.command(C::Tidy);
    let said = ui(&out, "toast").unwrap();
    assert_eq!((said["text"].clone(), said["action"].clone()), (json!("6 tabs tidied up"), json!("untidy")));
    let state = ui(&out, "state").unwrap();
    assert_eq!(state["folders"].as_array().unwrap().len(), 2);
    assert!(tab_of(&state, tabs[0]).unwrap()["folder"].is_u64());
    assert_eq!(state["tidy"], Value::Null);
    let state = ui(&c.command(C::Untidy), "state").unwrap();
    assert!(state["folders"].as_array().unwrap().is_empty());
    // Dismissed: the card stays away.
    assert_eq!(ui(&c.command(C::DismissTidy), "state").unwrap()["tidy"], Value::Null);
    // Close the others: the loose tabs but this one; quick access stays.
    c.command(C::Activate { tab: tabs[0] });
    let out = c.command(C::CloseOtherTabs { tab: extra });
    assert!(out.contains(&Effect::Show { tab: Some(extra) }));
    assert!(has_toast(&out, "5 tabs closed"));
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::Ui { msg } if msg["type"] == "state")).count(), 1);
    let state = ui(&out, "state").unwrap();
    let left: Vec<u64> = state["tabs"].as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap()).collect();
    assert_eq!(left.len(), 2);
    assert!(left.contains(&pinned) && left.contains(&extra));
}

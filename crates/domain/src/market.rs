//! Rules of the extension market: a signed index (`market/market.json`) that lists real
//! WebExtensions, each pinned per platform to an official GitHub release asset and its SHA-256.
//! Parsing, signatures and downloads live in the infrastructure; what is decided here is pure.

use std::cmp::Ordering;

/// Format of the index this build understands. A newer index is refused (the build keeps the
/// one it has) rather than half read.
pub const SCHEMA: u32 = 1;

/// Key of the running system in an entry's `platforms` table. `None`: no extension host there
/// (Linux preview, Android), so no market entry can be installed.
pub fn platform_key(os: &str) -> Option<&'static str> {
    match os {
        "macos" => Some("macos"),
        "windows" => Some("windows"),
        _ => None,
    }
}

/// An id names files on disk (`webext/<id>.zip`): lowercase letters, digits and inner hyphens.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Only release downloads hosted by GitHub are followed:
/// `https://github.com/<owner>/<repo>/releases/download/<tag>/<file>`.
pub fn trusted_package_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    parts.len() == 6
        && parts[2] == "releases"
        && parts[3] == "download"
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && p.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+' | b'%')
                })
        })
}

/// A hex SHA-256 (64 hex digits).
pub fn valid_sha256(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Dotted numeric versions (`2026.1026.1812`, `v4.9.134`) compared part by part; a missing part
/// counts as 0. `None` when either is not of that form.
pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    fn parts(v: &str) -> Option<Vec<u64>> {
        v.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().ok())
            .collect()
    }
    let (a, b) = (parts(a)?, parts(b)?);
    let len = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    Some((0..len).map(|i| at(&a, i).cmp(&at(&b, i))).find(|o| o.is_ne()).unwrap_or(Ordering::Equal))
}

/// Whether `offered` should replace what is installed. Without a recorded version (installs that
/// predate the record) reinstalling is harmless. A version the index no longer pins is never
/// "outdated" when it is newer: falling back to an older list must not offer a downgrade.
pub fn is_upgrade(offered: &str, installed: Option<&str>) -> bool {
    match installed {
        None => true,
        Some(v) => match compare_versions(offered, v) {
            Some(order) => order == Ordering::Greater,
            None => offered.trim_start_matches('v') != v.trim_start_matches('v'),
        },
    }
}

/// Permissions `new` asks for that `old` did not, in the order of `new`.
pub fn added_permissions(old: &[String], new: &[String]) -> Vec<String> {
    new.iter().filter(|p| !old.contains(p)).cloned().collect()
}

/// One line of the permissions an extension asks for, as the market shows them: `key` names
/// the text (`perm.<key>` in the interface catalogs), `items` the sites of `some_sites` or the
/// raw names of `other`, `sensitive` lines come first and are marked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionLine {
    pub key: &'static str,
    pub sensitive: bool,
    pub items: Vec<String>,
}

/// What a manifest permission (or host pattern) means for the person installing it.
fn permission_kind(p: &str) -> (&'static str, bool) {
    if p == "<all_urls>" {
        return ("all_sites", true);
    }
    if let Some((_, rest)) = p.split_once("://") {
        let host = rest.split('/').next().unwrap_or_default();
        return if host == "*" { ("all_sites", true) } else { ("some_sites", true) };
    }
    match p {
        "tabs" => ("tabs", false),
        "activeTab" => ("active_tab", false),
        "storage" | "unlimitedStorage" => ("storage", false),
        "scripting" | "userScripts" => ("scripting", true),
        "webRequest" | "webRequestBlocking" | "webRequestAuthProvider" => ("web_request", true),
        "declarativeNetRequest" | "declarativeNetRequestWithHostAccess" | "declarativeNetRequestFeedback" => ("block_content", false),
        "cookies" => ("cookies", true),
        "clipboardRead" => ("clipboard_read", true),
        "clipboardWrite" => ("clipboard_write", false),
        "contextMenus" | "menus" => ("context_menus", false),
        "notifications" => ("notifications", false),
        "alarms" | "idle" | "offscreen" | "background" | "sidePanel" | "unlimited" => ("background", false),
        "history" | "topSites" | "sessions" => ("history", true),
        "bookmarks" => ("bookmarks", false),
        "downloads" | "downloads.open" => ("downloads", false),
        "webNavigation" => ("navigation", false),
        "nativeMessaging" => ("native_messaging", true),
        "debugger" => ("debugger", true),
        "management" => ("management", true),
        "privacy" | "proxy" => ("network_settings", true),
        _ => ("other", false),
    }
}

/// Host of a site pattern (`*://*.youtube.com/*` → `youtube.com`).
fn pattern_host(p: &str) -> String {
    let rest = p.split_once("://").map_or(p, |(_, r)| r);
    let host = rest.split('/').next().unwrap_or_default();
    host.trim_start_matches("*.").to_owned()
}

/// The permissions of a manifest as lines a person can read: one per kind, sensitive first,
/// then in the order they first appear. Specific sites disappear behind "all sites".
pub fn permission_lines(perms: &[String]) -> Vec<PermissionLine> {
    let mut lines: Vec<PermissionLine> = Vec::new();
    for p in perms {
        let (key, sensitive) = permission_kind(p);
        let item = match key {
            "some_sites" => Some(pattern_host(p)),
            "other" => Some(p.clone()),
            _ => None,
        };
        match lines.iter_mut().find(|l| l.key == key) {
            Some(line) => {
                if let Some(item) = item.filter(|i| !line.items.contains(i)) {
                    line.items.push(item);
                }
            }
            None => lines.push(PermissionLine { key, sensitive, items: item.into_iter().collect() }),
        }
    }
    if lines.iter().any(|l| l.key == "all_sites") {
        lines.retain(|l| l.key != "some_sites");
    }
    // Stable: equal keys keep their order.
    lines.sort_by_key(|l| !l.sensitive);
    lines
}

/// What an update would let the extension do that the installed version could not: lines of
/// `new` missing from `old`, and the new sites or names of lines both have. Access to all sites
/// covers any site.
pub fn added_permission_lines(old: &[String], new: &[String]) -> Vec<PermissionLine> {
    let before = permission_lines(old);
    let all_sites_before = before.iter().any(|l| l.key == "all_sites");
    permission_lines(new)
        .into_iter()
        .filter_map(|mut line| {
            if line.key == "some_sites" && all_sites_before {
                return None;
            }
            match before.iter().find(|b| b.key == line.key) {
                None => Some(line),
                Some(b) => {
                    line.items.retain(|i| !b.items.contains(i));
                    (!line.items.is_empty()).then_some(line)
                }
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Written for a newer build.
    Schema,
    /// Older than an index already accepted on this device (replayed).
    Older,
    /// Past its `expires` date (frozen: its publisher stopped signing it, or it is replayed).
    Expired,
}

/// Whether a correctly signed index may replace the one in use. `last_seq` is the sequence of
/// the index accepted before on this device, if any. Times are UNIX seconds.
pub fn accept(
    schema: u32,
    seq: u64,
    expires: u64,
    now: u64,
    last_seq: Option<u64>,
) -> Result<(), Refusal> {
    if schema != SCHEMA {
        return Err(Refusal::Schema);
    }
    if last_seq.is_some_and(|last| seq < last) {
        return Err(Refusal::Older);
    }
    if expires <= now {
        return Err(Refusal::Expired);
    }
    Ok(())
}

/// Release asset names are matched against a pattern with `*` wildcards
/// (`dist-chrome-*.zip`).
pub fn asset_matches(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || name.len() < first.len() + last.len() || !name.ends_with(last) {
        return false;
    }
    let mut at = first.len();
    let end = name.len() - last.len();
    for mid in &parts[1..parts.len() - 1] {
        match name[at..end].find(mid) {
            Some(i) => at += i + mid.len(),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platforms_with_an_extension_host() {
        assert_eq!(platform_key("macos"), Some("macos"));
        assert_eq!(platform_key("windows"), Some("windows"));
        assert_eq!(platform_key("linux"), None);
        assert_eq!(platform_key("android"), None);
    }

    #[test]
    fn ids_are_safe_file_names() {
        assert!(valid_id("ublock-origin-lite"));
        assert!(valid_id("x1"));
        for bad in ["", "-a", "a-", "A", "a/b", "..", "a.b", "a b", &"a".repeat(65)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn only_github_release_downloads_are_trusted() {
        assert!(trusted_package_url(
            "https://github.com/bitwarden/clients/releases/download/browser-v2026.10.1/dist-chrome-2026.10.1.zip"
        ));
        for bad in [
            "http://github.com/a/b/releases/download/t/f.zip",
            "https://github.com.evil.org/a/b/releases/download/t/f.zip",
            "https://objects.githubusercontent.com/a/b/releases/download/t/f.zip",
            "https://github.com/a/b/archive/t/f.zip",
            "https://github.com/a/b/releases/download/../f.zip",
            "https://github.com/a/b/releases/download/t/f.zip?x=1",
            "https://github.com/a/b/releases/download/t/sub/f.zip",
        ] {
            assert!(!trusted_package_url(bad), "{bad}");
        }
    }

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(compare_versions("4.9.135", "4.9.134"), Some(Ordering::Greater));
        assert_eq!(compare_versions("10.6.10", "10.6.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v2.1.1", "2.1.1"), Some(Ordering::Equal));
        assert_eq!(compare_versions("2026.10", "2026.10.0"), Some(Ordering::Equal));
        assert_eq!(compare_versions("2026.1026.1812", "2026.1110.903"), Some(Ordering::Less));
        assert_eq!(compare_versions("1.0-beta", "1.0"), None);
    }

    #[test]
    fn upgrades_never_go_back() {
        assert!(is_upgrade("4.9.135", Some("4.9.134")));
        assert!(!is_upgrade("4.9.134", Some("4.9.134")));
        assert!(!is_upgrade("4.9.133", Some("4.9.134")));
        assert!(is_upgrade("1", None));
        assert!(is_upgrade("1.0-rc2", Some("1.0-rc1")));
        assert!(!is_upgrade("v1.0-rc1", Some("1.0-rc1")));
    }

    #[test]
    fn added_permissions_are_the_new_ones() {
        let s = |v: &[&str]| v.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        assert_eq!(
            added_permissions(&s(&["storage", "tabs"]), &s(&["storage", "scripting", "tabs"])),
            s(&["scripting"])
        );
        assert!(added_permissions(&s(&["a", "b"]), &s(&["b"])).is_empty());
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn permissions_read_as_lines() {
        let lines = permission_lines(&strings(&[
            "storage", "tabs", "<all_urls>", "unlimitedStorage", "https://*/*", "scripting", "fooBar",
        ]));
        let keys: Vec<_> = lines.iter().map(|l| l.key).collect();
        assert_eq!(keys, ["all_sites", "scripting", "storage", "tabs", "other"]);
        assert!(lines[0].sensitive && !lines[2].sensitive);
        assert_eq!(lines[4].items, ["fooBar"]);
        let sites = permission_lines(&strings(&["*://*.youtube.com/*", "https://www.youtube.com/*", "*://m.youtube.com/*"]));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].key, "some_sites");
        assert_eq!(sites[0].items, ["youtube.com", "www.youtube.com", "m.youtube.com"]);
        // Every site covers specific ones.
        let both = permission_lines(&strings(&["*://*.youtube.com/*", "*://*/*"]));
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].key, "all_sites");
    }

    #[test]
    fn every_kind_of_permission_has_its_text() {
        let samples = strings(&[
            "<all_urls>", "*://a.org/*", "tabs", "activeTab", "storage", "scripting", "webRequest",
            "declarativeNetRequest", "cookies", "clipboardRead", "clipboardWrite", "contextMenus",
            "notifications", "alarms", "history", "bookmarks", "downloads", "webNavigation",
            "nativeMessaging", "debugger", "management", "proxy", "somethingElse",
        ]);
        let keys: Vec<_> = samples.iter().map(|p| permission_kind(p).0).collect();
        assert_eq!(keys.len(), 23);
        for (lang, _) in crate::i18n::LANGUAGES {
            for key in &keys {
                let k = format!("settings.perm_{key}");
                assert_ne!(crate::i18n::catalog(lang).get(&k), k, "{lang}: {k}");
            }
        }
    }

    #[test]
    fn what_an_update_adds() {
        let old = strings(&["storage", "*://*.youtube.com/*"]);
        assert!(added_permission_lines(&old, &old).is_empty());
        // The same meaning written another way adds nothing.
        assert!(added_permission_lines(&old, &strings(&["unlimitedStorage", "https://youtube.com/*"])).is_empty());
        let added = added_permission_lines(&old, &strings(&["storage", "*://*.youtube.com/*", "*://vimeo.com/*", "cookies"]));
        assert_eq!(added.iter().map(|l| l.key).collect::<Vec<_>>(), ["some_sites", "cookies"]);
        assert_eq!(added[0].items, ["vimeo.com"]);
        let all = strings(&["<all_urls>"]);
        assert!(added_permission_lines(&all, &strings(&["*://vimeo.com/*", "http://*/*"])).is_empty());
        assert_eq!(added_permission_lines(&old, &all)[0].key, "all_sites");
        assert_eq!(added_permission_lines(&strings(&["a"]), &strings(&["a", "b"]))[0].items, ["b"]);
    }

    #[test]
    fn index_freshness() {
        assert_eq!(accept(1, 10, 100, 50, None), Ok(()));
        assert_eq!(accept(1, 10, 100, 50, Some(10)), Ok(()));
        assert_eq!(accept(1, 9, 100, 50, Some(10)), Err(Refusal::Older));
        assert_eq!(accept(1, 10, 50, 50, None), Err(Refusal::Expired));
        assert_eq!(accept(2, 10, 100, 50, None), Err(Refusal::Schema));
    }

    #[test]
    fn asset_patterns() {
        assert!(asset_matches("dist-chrome-*.zip", "dist-chrome-2026.10.1.zip"));
        assert!(!asset_matches("dist-chrome-*.zip", "dist-firefox-2026.10.1.zip"));
        assert!(asset_matches("uBOLite_*.chromium.zip", "uBOLite_2026.1026.1812.chromium.zip"));
        assert!(!asset_matches("uBOLite_*.chromium.zip", "uBOLite_2026.1026.1812.firefox.zip"));
        assert!(asset_matches("ChromeExtension.zip", "ChromeExtension.zip"));
        assert!(asset_matches("ghostery-chromium-*.zip", "ghostery-chromium-10.6.10.zip"));
    }
}

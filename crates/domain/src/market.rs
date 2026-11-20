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

/// The repository named by what the person typed: `owner/repo`, `github.com/owner/repo` or any
/// `https://github.com/owner/repo/…` address (`.git` dropped). GitHub's own name rules: letters,
/// digits, `-`, `_` and `.`, never `.` or `..` alone.
pub fn github_repo(input: &str) -> Option<(String, String)> {
    let s = input.trim();
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let s = s.strip_prefix("www.").unwrap_or(s);
    let s = s.strip_prefix("github.com/").unwrap_or(s);
    let mut parts = s.split(['/', '?', '#']);
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let ok = |p: &str| {
        (1..=100).contains(&p.len())
            && p != "."
            && p != ".."
            && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    // Account names: letters, digits and hyphens only (so `gitlab.com/a/b` is no owner).
    let account = owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    (ok(owner) && account && ok(repo) && !input.trim().contains(' ')).then(|| (owner.to_owned(), repo.to_owned()))
}

/// The id of an extension added from a GitHub repository: `gh-<owner>-<repo>`, a safe file name
/// (`valid_id`) that no market entry uses.
pub fn sideload_id(owner: &str, repo: &str) -> String {
    let mut id = String::from("gh-");
    for c in format!("{owner}-{repo}").chars() {
        let c = if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' };
        if !(c == '-' && id.ends_with('-')) {
            id.push(c);
        }
    }
    let mut id: String = id.chars().take(64).collect();
    while id.ends_with('-') {
        id.pop();
    }
    id
}

/// How well a release file fits the extension host of `platform` (higher is better); `None`
/// when it cannot run there or is not an extension package. WebView2 (Windows) takes Chromium
/// builds; WKWebView (macOS) runs Chromium builds too and Firefox-style ones as a fallback.
pub fn asset_rank(platform: &str, name: &str) -> Option<u8> {
    if !matches!(platform, "windows" | "macos") {
        return None;
    }
    let n = name.to_ascii_lowercase();
    if !n.ends_with(".zip") || ["source", "src.zip", "-src", "_src"].iter().any(|s| n.contains(s)) {
        return None;
    }
    let firefox = n.contains("firefox") || n.contains("gecko") || n.contains("mozilla");
    let safari = n.contains("safari") || n.contains("xcode");
    let chromium = ["chrome", "chromium", "edge", "opera", "mv3"].iter().any(|s| n.contains(s));
    match (platform, safari, chromium, firefox) {
        (_, true, _, _) => None,
        (_, _, true, _) => Some(3),
        ("windows", _, _, true) => None,
        ("macos", _, _, true) => Some(1),
        ("windows" | "macos", ..) => Some(2),
        _ => None,
    }
}

/// The pattern that finds the same kind of file in a later release: the version in its name
/// becomes `*` (`ext-chrome-1.4.0.zip` → `ext-chrome-*.zip`); a name without it stays as is.
pub fn asset_pattern(name: &str, tag: &str) -> String {
    let version = tag.trim_start_matches(|c: char| !c.is_ascii_digit());
    if version.is_empty() || !name.contains(version) {
        return name.to_owned();
    }
    name.replace(version, "*")
}

/// The name shown for an extension added from GitHub: its manifest's, unless that is a locale
/// placeholder (`__MSG_appName__`) or empty, then the repository's.
pub fn display_name(manifest_name: &str, repo: &str) -> String {
    let n = manifest_name.trim();
    if n.is_empty() || n.starts_with("__MSG_") {
        repo.to_owned()
    } else {
        n.chars().take(80).collect()
    }
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

/// Add from a link (0.8.25): an extension package offered by an address, for extensions with no
/// release on GitHub. HTTPS only, no user name or password in it, 2 KB at most. A store page
/// (Chrome Web Store, Edge Add-ons) stands for its package (`store_package_url`).
pub fn direct_package_url(input: &str) -> Option<url::Url> {
    let s = input.trim();
    if s.is_empty() || s.len() > 2048 || s.contains(char::is_whitespace) {
        return None;
    }
    let url = url::Url::parse(s).ok()?;
    let host = url.host_str()?;
    let ok = url.scheme() == "https" && url.username().is_empty() && url.password().is_none() && host.contains(['.', ':']);
    ok.then(|| store_package_url(&url).unwrap_or(url))
}

/// The package behind a store page: `chromewebstore.google.com/detail/<name>/<id>` (and the old
/// `chrome.google.com/webstore/detail/…`) or `microsoftedge.microsoft.com/addons/detail/…/<id>`,
/// asked from the store's own download address. `None` for any other address.
pub fn store_package_url(url: &url::Url) -> Option<url::Url> {
    let host = url.host_str()?;
    let id = url.path_segments()?.rfind(|p| !p.is_empty())?;
    let store_id = id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b));
    let detail = url.path().contains("/detail/");
    if !store_id || !detail {
        return None;
    }
    let to = match host {
        "chromewebstore.google.com" | "chrome.google.com" => format!(
            "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=130.0&acceptformat=crx2,crx3&x=id%3D{id}%26uc"
        ),
        "microsoftedge.microsoft.com" => format!(
            "https://edge.microsoft.com/extensionwebstorebase/v1/crx?response=redirect&x=id%3D{id}%26installsource%3Dondemand%26uc"
        ),
        _ => return None,
    };
    url::Url::parse(&to).ok()
}

/// The id of an extension added from a link: `url-<host>-<hash>` (`valid_id`), stable for the
/// same address, never a market or GitHub id.
pub fn direct_id(url: &url::Url) -> String {
    // FNV-1a: a short, stable fingerprint of the whole address.
    let hash = url.as_str().bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    let mut slug = String::new();
    for c in url.host_str().unwrap_or("web").trim_start_matches("www.").chars() {
        let c = if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' };
        if !(c == '-' && (slug.is_empty() || slug.ends_with('-'))) {
            slug.push(c);
        }
    }
    let slug: String = slug.chars().take(40).collect();
    format!("url-{}-{:08x}", slug.trim_end_matches('-'), hash as u32)
}

/// The file name an address gives (its last path part), else its host.
pub fn direct_file_name(url: &url::Url) -> String {
    url.path_segments()
        .and_then(|mut p| p.next_back())
        .filter(|n| !n.is_empty())
        .map(|n| n.chars().take(120).collect())
        .unwrap_or_else(|| url.host_str().unwrap_or("").to_owned())
}

/// The zip inside a package: a Chromium `.crx` (versions 2 and 3: a `Cr24` header, keys and
/// signatures, then the zip) loses its header; a `.zip` or Firefox `.xpi` is already one.
/// `None` when it is neither.
pub fn crx_payload(bytes: &[u8]) -> Option<&[u8]> {
    const ZIP: &[u8] = b"PK\x03\x04";
    if bytes.starts_with(ZIP) {
        return Some(bytes);
    }
    let word = |at: usize| -> Option<usize> {
        let b = bytes.get(at..at + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    if !bytes.starts_with(b"Cr24") {
        return None;
    }
    let start = match word(4)? {
        2 => 16usize.checked_add(word(8)?)?.checked_add(word(12)?)?,
        3 => 12usize.checked_add(word(8)?)?,
        _ => return None,
    };
    bytes.get(start..).filter(|zip| zip.starts_with(ZIP))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_to_a_package_are_https_and_store_pages_stand_for_their_package() {
        let ok = direct_package_url(" https://example.com/ext/dark-1.2.crx ").unwrap();
        assert_eq!(ok.as_str(), "https://example.com/ext/dark-1.2.crx");
        for bad in ["http://example.com/a.crx", "https://user:pw@example.com/a.crx", "https://localhost/a.crx", "file:///a.crx", "https://a b/x", "ftp://x.y/a"] {
            assert!(direct_package_url(bad).is_none(), "{bad}");
        }
        let id = "cjpalhdlnbpafiamejdnhcphjbkeiagm";
        let chrome = direct_package_url(&format!("https://chromewebstore.google.com/detail/ublock-origin/{id}")).unwrap();
        assert!(chrome.as_str().starts_with("https://clients2.google.com/service/update2/crx?") && chrome.as_str().contains(id));
        let edge = direct_package_url(&format!("https://microsoftedge.microsoft.com/addons/detail/some/{id}")).unwrap();
        assert!(edge.as_str().starts_with("https://edge.microsoft.com/extensionwebstorebase/") && edge.as_str().contains(id));
        // Not a store id (q is out of a–p): the address stays as it is.
        let other = format!("https://chromewebstore.google.com/detail/x/{}", "q".repeat(32));
        assert_eq!(direct_package_url(&other).unwrap().as_str(), other);
    }

    #[test]
    fn a_link_names_a_stable_safe_id_and_a_file() {
        let a = url::Url::parse("https://www.Example.com/ext/dark-1.2.crx").unwrap();
        let b = url::Url::parse("https://example.com/ext/other.crx").unwrap();
        let id = direct_id(&a);
        assert!(valid_id(&id) && id.starts_with("url-example-com-"), "{id}");
        assert_eq!(id, direct_id(&a));
        assert_ne!(id, direct_id(&b));
        assert!(valid_id(&direct_id(&url::Url::parse(&format!("https://{}.example.com/a", "x".repeat(60))).unwrap())));
        assert_eq!(direct_file_name(&a), "dark-1.2.crx");
        assert_eq!(direct_file_name(&url::Url::parse("https://example.com/").unwrap()), "example.com");
    }

    #[test]
    fn a_crx_loses_its_header() {
        let zip = b"PK\x03\x04rest".to_vec();
        assert_eq!(crx_payload(&zip), Some(&zip[..]));
        let mut v3 = b"Cr24".to_vec();
        v3.extend(3u32.to_le_bytes());
        v3.extend(5u32.to_le_bytes());
        v3.extend(b"HEADR");
        v3.extend(&zip);
        assert_eq!(crx_payload(&v3), Some(&zip[..]));
        let mut v2 = b"Cr24".to_vec();
        v2.extend(2u32.to_le_bytes());
        v2.extend(2u32.to_le_bytes());
        v2.extend(1u32.to_le_bytes());
        v2.extend(b"KKS");
        v2.extend(&zip);
        assert_eq!(crx_payload(&v2), Some(&zip[..]));
        let mut broken = v3.clone();
        broken[8] = 200;
        assert_eq!(crx_payload(&broken), None);
        assert_eq!(crx_payload(b"<html>"), None);
        assert_eq!(crx_payload(b"Cr24\x03\x00"), None);
    }

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
    fn repositories_from_what_is_typed() {
        let r = |s: &str| github_repo(s).map(|(o, r)| format!("{o}/{r}"));
        assert_eq!(r("darkreader/darkreader").as_deref(), Some("darkreader/darkreader"));
        assert_eq!(r(" https://github.com/ajayyy/SponsorBlock/releases ").as_deref(), Some("ajayyy/SponsorBlock"));
        assert_eq!(r("github.com/a-b/c.d.git").as_deref(), Some("a-b/c.d"));
        assert_eq!(r("http://www.github.com/x/y?tab=readme").as_deref(), Some("x/y"));
        for bad in ["", "darkreader", "a/..", "a b/c", "https://gitlab.com/a/b/c", "a/b c", "../x"] {
            assert_eq!(r(bad), None, "{bad}");
        }
    }

    #[test]
    fn sideloaded_ids_are_safe_and_apart_from_the_market() {
        assert_eq!(sideload_id("Hendrizzzz", "ghostify"), "gh-hendrizzzz-ghostify");
        assert_eq!(sideload_id("a-b", "c.d_e"), "gh-a-b-c-d-e");
        let long = sideload_id(&"x".repeat(80), "y");
        assert!(valid_id(&long) && long.len() <= 64);
        assert!(valid_id(&sideload_id("-_-", "--x")));
    }

    #[test]
    fn release_files_for_each_system() {
        assert_eq!(asset_rank("windows", "dist-chrome-1.0.zip"), Some(3));
        assert_eq!(asset_rank("windows", "dist-firefox-1.0.zip"), None);
        assert_eq!(asset_rank("macos", "dist-firefox-1.0.zip"), Some(1));
        assert_eq!(asset_rank("macos", "uBOLite_1.chromium.zip"), Some(3));
        assert_eq!(asset_rank("windows", "extension.zip"), Some(2));
        assert_eq!(asset_rank("windows", "ext-safari.zip"), None);
        assert_eq!(asset_rank("windows", "source-code.zip"), None);
        assert_eq!(asset_rank("windows", "ext.xpi"), None);
        assert_eq!(asset_rank("linux", "ext-chrome.zip"), None);
    }

    #[test]
    fn patterns_follow_the_version() {
        assert_eq!(asset_pattern("tab-tamer-chromium-1.4.0.zip", "v1.4.0"), "tab-tamer-chromium-*.zip");
        assert_eq!(asset_pattern("dist-chrome-2026.10.1.zip", "browser-v2026.10.1"), "dist-chrome-*.zip");
        assert_eq!(asset_pattern("ChromeExtension.zip", "6.1.7"), "ChromeExtension.zip");
        assert!(asset_matches(&asset_pattern("a-1.4.0.zip", "1.4.0"), "a-1.5.2.zip"));
        assert_eq!(display_name("__MSG_extName__", "ghostify"), "ghostify");
        assert_eq!(display_name(" Tab Tamer ", "tab-tamer"), "Tab Tamer");
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

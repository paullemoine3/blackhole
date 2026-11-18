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

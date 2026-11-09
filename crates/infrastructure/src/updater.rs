//! Self-update from GitHub releases.
//!
//! The latest release is found by following `github.com/<repo>/releases/latest`, which redirects
//! to its tag page: unlike the REST API (60 anonymous requests an hour, shared by everyone behind
//! the same address — an office network exhausts it), the site has no such quota. The
//! `SHA256SUMS.txt` published with the release lists its assets; the platform one is downloaded
//! over HTTPS, checked against it, then handed to the platform installer (Inno Setup on Windows,
//! an app-bundle swap on macOS).

use crate::catalog;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub const REPO: &str = "paullemoine3/blackhole";
const SUMS: &str = "SHA256SUMS.txt";
const MAX_BYTES: u64 = 400 * 1024 * 1024;
/// Android APKs of a release (see the `android` job of the release workflow).
pub const ANDROID_APK: &str = "BlackHole-android.apk";
pub const ANDROID_DEBUG_APK: &str = "BlackHole-android-debug.apk";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub asset: String,
    pub url: String,
    pub sha256: String,
}

/// `1.2.3`, `v1.2` or `1.2.3-rc1` → (1, 2, 3); anything else is not a version.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(c), Some(n)) => c > n,
        _ => false,
    }
}

/// Release asset that updates the running platform (see the release workflow for the names).
pub fn wants_asset(os: &str, name: &str) -> bool {
    match os {
        "windows" => name.starts_with("BlackHole-Setup-") && name.ends_with("-x64.exe"),
        "macos" => name == "BlackHole-macos-universal.zip",
        // An APK only replaces one signed with the same key: release key or Gradle's debug key.
        "android" => name == ANDROID_APK,
        "android-debug" => name == ANDROID_DEBUG_APK,
        _ => false,
    }
}

/// Only release downloads hosted by GitHub are ever followed.
pub fn trusted_url(url: &str) -> bool {
    url.starts_with("https://github.com/")
}

/// Tags and asset names end up in a URL: only plain file-name characters are accepted.
fn plain_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

/// The page that redirects to the latest release's tag (see [`tag_of_latest`]).
pub fn latest_url() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

/// The checksums published with release `tag` (`None` for a tag that is not a plain name).
pub fn sums_url(tag: &str) -> Option<String> {
    plain_name(tag).then(|| download_url(tag, SUMS))
}

fn download_url(tag: &str, asset: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}/{asset}")
}

/// The tag `releases/latest` redirected to (`…/releases/tag/v1.2.3`). A repository without a
/// release redirects to its release list instead: `None`. Drafts and pre-releases are never
/// "latest" for GitHub.
pub fn tag_of_latest(final_url: &str) -> Option<&str> {
    let prefix = format!("https://github.com/{REPO}/releases/tag/");
    let rest = final_url.strip_prefix(prefix.as_str())?;
    let tag = rest.split(['?', '#']).next()?.trim_end_matches('/');
    (plain_name(tag) && parse_version(tag).is_some()).then_some(tag)
}

/// `(hash, file)` pairs of `sha256sum` output (`<hash>  <name>` per line, `*` marks binary mode).
fn sum_lines(sums: &str) -> impl Iterator<Item = (String, &str)> {
    sums.lines().filter_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*');
        (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| (hash.to_ascii_lowercase(), file))
    })
}

/// Finds `name` in `sha256sum` output.
pub fn find_sum(sums: &str, name: &str) -> Option<String> {
    sum_lines(sums).find_map(|(hash, file)| (file == name).then_some(hash))
}

/// The platform asset of release `tag`, as listed (with its checksum) in its `SHA256SUMS.txt`.
pub fn release_from_sums(tag: &str, sums: &str, os: &str) -> Option<Release> {
    if !plain_name(tag) {
        return None;
    }
    let (sha256, asset) =
        sum_lines(sums).find(|(_, file)| wants_asset(os, file) && plain_name(file))?;
    Some(Release {
        version: tag.trim_start_matches('v').to_owned(),
        url: download_url(tag, asset),
        asset: asset.to_owned(),
        sha256,
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(concat!("BlackHole/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
}

fn read_limited(resp: ureq::Response, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("download too large".into());
    }
    Ok(bytes)
}

/// Blocking. `Ok(None)` means up to date (or nothing published yet). Run it off the UI thread.
pub fn check(current: &str) -> Result<Option<Release>, String> {
    let agent = agent();
    // HEAD: only the redirect matters, not the page.
    let resp = agent
        .head(&latest_url())
        .call()
        .map_err(|e| e.to_string())?;
    let Some(tag) = tag_of_latest(resp.get_url()).map(str::to_owned) else {
        return Ok(None);
    };
    if !is_newer(&tag, current) {
        return Ok(None);
    }
    let sums = read_limited(
        agent
            .get(&download_url(&tag, SUMS))
            .call()
            .map_err(|e| e.to_string())?,
        1 << 20,
    )?;
    let sums = String::from_utf8_lossy(&sums);
    release_from_sums(&tag, &sums, std::env::consts::OS)
        .map(Some)
        .ok_or_else(|| format!("release {tag} has no verified download for this system"))
}

pub fn updates_dir(data: &Path) -> PathBuf {
    data.join("updates")
}

/// Blocking download + checksum verification; returns the file to hand to [`apply`].
pub fn download(data: &Path, release: &Release) -> Result<PathBuf, String> {
    if !trusted_url(&release.url) {
        return Err("refusing an update hosted outside GitHub".into());
    }
    let dir = updates_dir(data);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let resp = agent()
        .get(&release.url)
        .call()
        .map_err(|e| e.to_string())?;
    let bytes = read_limited(resp, MAX_BYTES)?;
    catalog::verify(&bytes, &release.sha256)?;
    let name = Path::new(&release.asset)
        .file_name()
        .ok_or("bad asset name")?;
    let dest = dir.join(name);
    fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// `…/BlackHole.app/Contents/MacOS/BlackHole` → `…/BlackHole.app`.
pub fn bundle_path(exe: &Path) -> Option<PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    let inside =
        exe.parent()?.file_name()? == "MacOS" && exe.parent()?.parent()?.file_name()? == "Contents";
    (inside && bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

/// Builds run from a cargo `target` directory are never self-updated.
fn is_dev_build(exe: &Path) -> bool {
    exe.ancestors()
        .any(|p| p.file_name().is_some_and(|n| n == "target"))
}

/// Whether this installation can replace itself.
pub fn supported() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    if is_dev_build(&exe) {
        return false;
    }
    match std::env::consts::OS {
        "windows" => true,
        "macos" => bundle_path(&exe).is_some(),
        _ => false,
    }
}

/// Starts the platform installer for a downloaded update. The caller must exit right after.
pub fn apply(downloaded: &Path) -> Result<(), String> {
    apply_impl(downloaded)
}

#[cfg(windows)]
fn apply_impl(installer: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if !install_dir_writable() {
        let quoted = installer.to_string_lossy().replace('\'', "''");
        let script = format!(
            "Start-Process -FilePath '{quoted}' -Verb RunAs -ArgumentList \
             '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/CLOSEAPPLICATIONS'"
        );
        return std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    // The installer relaunches BlackHole itself once it is done (silent runs only).
    std::process::Command::new(installer)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
        ])
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Installations in `Program Files` (made by an elevated setup) cannot be replaced by a plain
/// process: ask Windows for elevation (one UAC prompt) instead of failing silently.
#[cfg(windows)]
fn install_dir_writable() -> bool {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        return true;
    };
    let probe = dir.join(".blackhole-write-test");
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(probe);
    ok
}

/// Waits for the old process to exit, swaps the bundle (restoring it on failure), relaunches.
#[cfg(target_os = "macos")]
const SWAP_SCRIPT: &str = r#"
while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
rm -rf "$3.old"
if mv "$3" "$3.old"; then
  if mv "$2" "$3"; then rm -rf "$3.old"; else mv "$3.old" "$3"; fi
fi
open "$3"
"#;

#[cfg(target_os = "macos")]
fn apply_impl(zip: &Path) -> Result<(), String> {
    let target = std::env::current_exe()
        .ok()
        .and_then(|exe| bundle_path(&exe))
        .ok_or("not running from an app bundle")?;
    // The swap needs write access to the folder holding the app (e.g. /Applications for a
    // standard user); fail visibly rather than relaunching the old version.
    let parent = target.parent().ok_or("bad app location")?;
    let probe = parent.join(".blackhole-write-test");
    if fs::write(&probe, b"").is_err() {
        return Err(format!(
            "cannot write to {}: move BlackHole to a folder you own (e.g. ~/Applications)",
            parent.display()
        ));
    }
    let _ = fs::remove_file(probe);
    let stage = zip.with_extension("app-stage");
    let _ = fs::remove_dir_all(&stage);
    let status = std::process::Command::new("ditto")
        .args(["-x", "-k"])
        .arg(zip)
        .arg(&stage)
        .status()
        .map_err(|e| e.to_string())?;
    let fresh = stage.join("BlackHole.app");
    if !status.success() || !fresh.is_dir() {
        return Err("could not unpack the update".into());
    }
    let _ = std::process::Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&fresh)
        .status();
    std::process::Command::new("sh")
        .arg("-c")
        .arg(SWAP_SCRIPT)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(&fresh)
        .arg(&target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn apply_impl(_: &Path) -> Result<(), String> {
    Err("self-update is not supported on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMS_0_2_0: &str = "\
1111111111111111111111111111111111111111111111111111111111111111  BlackHole-Setup-0.2.0-x64.exe
2222222222222222222222222222222222222222222222222222222222222222  BlackHole-android.apk
3333333333333333333333333333333333333333333333333333333333333333  BlackHole-macos-universal.dmg
4444444444444444444444444444444444444444444444444444444444444444  BlackHole-macos-universal.zip
5555555555555555555555555555555555555555555555555555555555555555  BlackHole-windows-x64.zip
";

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.2"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.0.0-rc1"), Some((1, 0, 0)));
        assert_eq!(parse_version("nightly"), None);
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("v0.8.3", "0.8.2"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("junk", "0.1.0"));
    }

    #[test]
    fn reads_the_tag_the_latest_release_redirects_to() {
        let tag = |u: &str| tag_of_latest(u).map(str::to_owned);
        let base = format!("https://github.com/{REPO}/releases");
        assert_eq!(tag(&format!("{base}/tag/v0.8.2")), Some("v0.8.2".into()));
        assert_eq!(
            tag(&format!("{base}/tag/v0.8.2/?x=1")),
            Some("v0.8.2".into())
        );
        // No release yet: GitHub shows the (empty) release list.
        assert_eq!(tag(&base), None);
        assert_eq!(tag(&format!("{base}/latest")), None);
        assert_eq!(
            tag("https://github.com/someone/else/releases/tag/v9.0.0"),
            None
        );
        assert_eq!(tag(&format!("{base}/tag/nightly")), None);
        assert_eq!(tag(&format!("{base}/tag/v1.0.0%2F..%2Fx")), None);
    }

    #[test]
    fn picks_the_platform_asset_listed_in_the_checksums() {
        let win = release_from_sums("v0.2.0", SUMS_0_2_0, "windows").unwrap();
        assert_eq!(win.version, "0.2.0");
        assert_eq!(win.asset, "BlackHole-Setup-0.2.0-x64.exe");
        assert_eq!(
            win.url,
            format!(
                "https://github.com/{REPO}/releases/download/v0.2.0/BlackHole-Setup-0.2.0-x64.exe"
            )
        );
        assert_eq!(win.sha256, "1".repeat(64));
        assert!(trusted_url(&win.url));
        let mac = release_from_sums("v0.2.0", SUMS_0_2_0, "macos").unwrap();
        assert_eq!(mac.asset, "BlackHole-macos-universal.zip");
        assert_eq!(mac.sha256, "4".repeat(64));
        assert!(release_from_sums("v0.2.0", SUMS_0_2_0, "linux").is_none());
        assert!(
            release_from_sums("v0.2.0", "not a checksum file", "macos").is_none(),
            "no checksum, no update"
        );
        assert!(release_from_sums("../v0.2.0", SUMS_0_2_0, "macos").is_none());
    }

    /// Network: `cargo test -p blackhole-infrastructure updater -- --ignored`.
    #[test]
    #[ignore]
    fn finds_the_published_release_without_the_api() {
        let found = check("0.0.1")
            .expect("check")
            .expect("a release is published");
        assert!(trusted_url(&found.url));
        assert_eq!(found.sha256.len(), 64);
        assert!(check("999.0.0").expect("check").is_none());
    }

    #[test]
    fn an_apk_only_updates_one_signed_with_the_same_key() {
        let sums = format!(
            "{}  BlackHole-android.apk\n{}  BlackHole-android-debug.apk\n",
            "5".repeat(64),
            "6".repeat(64)
        );
        let signed = release_from_sums("v0.9.0", &sums, "android").unwrap();
        assert_eq!(signed.asset, ANDROID_APK);
        assert_eq!(signed.sha256, "5".repeat(64));
        assert!(trusted_url(&signed.url));
        let debug = release_from_sums("v0.9.0", &sums, "android-debug").unwrap();
        assert_eq!(debug.asset, ANDROID_DEBUG_APK);
        let only_debug = format!("{}  BlackHole-android-debug.apk\n", "6".repeat(64));
        assert!(release_from_sums("v0.9.0", &only_debug, "android").is_none());
        assert_eq!(
            sums_url("v0.9.0").unwrap(),
            format!("https://github.com/{REPO}/releases/download/v0.9.0/SHA256SUMS.txt")
        );
        assert!(sums_url("../x").is_none());
        assert!(latest_url().ends_with("/releases/latest"));
    }

    #[test]
    fn reads_checksums_from_sha256sum_output() {
        let h1 = "a".repeat(64);
        let h2 = "B".repeat(64);
        let sums = format!("{h1}  one.zip\n{h2} *two.exe\nshort  three\n");
        assert_eq!(find_sum(&sums, "one.zip"), Some(h1));
        assert_eq!(find_sum(&sums, "two.exe"), Some("b".repeat(64)));
        assert_eq!(find_sum(&sums, "three"), None);
        assert_eq!(find_sum(&sums, "missing"), None);
    }

    #[test]
    fn locates_the_app_bundle_and_dev_builds() {
        let exe = Path::new("/Applications/BlackHole.app/Contents/MacOS/BlackHole");
        assert_eq!(
            bundle_path(exe),
            Some(PathBuf::from("/Applications/BlackHole.app"))
        );
        assert_eq!(
            bundle_path(Path::new("/repo/target/release/blackhole")),
            None
        );
        assert!(is_dev_build(Path::new("/repo/target/release/blackhole")));
        assert!(!is_dev_build(exe));
    }

    #[test]
    fn download_refuses_foreign_hosts() {
        let r = Release {
            version: "9.9.9".into(),
            asset: "x.exe".into(),
            url: "http://github.com/x".into(),
            sha256: "0".repeat(64),
        };
        let dir = tempfile::tempdir().unwrap();
        assert!(download(dir.path(), &r).is_err());
    }
}

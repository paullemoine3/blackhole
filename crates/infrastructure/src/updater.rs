//! Self-update from GitHub releases.
//!
//! The latest release is read from the GitHub API; its platform asset is downloaded over HTTPS,
//! checked against the `SHA256SUMS.txt` published next to it, then handed to the platform
//! installer (Inno Setup on Windows, an app-bundle swap on macOS).

use crate::catalog;
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub const REPO: &str = "paullemoine3/blackhole";
const SUMS: &str = "SHA256SUMS.txt";
const MAX_BYTES: u64 = 400 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub asset: String,
    pub url: String,
    pub sha256: String,
}

struct Candidate {
    version: String,
    asset: String,
    url: String,
    sums_url: String,
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
        _ => false,
    }
}

/// Only release downloads hosted by GitHub are ever followed.
fn trusted_url(url: &str) -> bool {
    url.starts_with("https://github.com/")
}

/// Looks for a newer release in the API payload of `releases/latest`.
fn pick(json: &Value, current: &str, os: &str) -> Option<Candidate> {
    if json["draft"].as_bool().unwrap_or(false) || json["prerelease"].as_bool().unwrap_or(false) {
        return None;
    }
    let version = json["tag_name"]
        .as_str()?
        .trim_start_matches('v')
        .to_owned();
    if !is_newer(&version, current) {
        return None;
    }
    let assets = json["assets"].as_array()?;
    let url_of = |pred: &dyn Fn(&str) -> bool| {
        assets.iter().find_map(|a| {
            let name = a["name"].as_str()?;
            let url = a["browser_download_url"].as_str()?;
            (pred(name) && trusted_url(url)).then(|| (name.to_owned(), url.to_owned()))
        })
    };
    let (asset, url) = url_of(&|n| wants_asset(os, n))?;
    let (_, sums_url) = url_of(&|n| n == SUMS)?;
    Some(Candidate {
        version,
        asset,
        url,
        sums_url,
    })
}

/// Finds `name` in `sha256sum` output (`<hash>  <name>` per line, `*` marks binary mode).
pub fn find_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
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
    let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = match agent
        .get(&api)
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(resp) => resp,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let json: Value =
        serde_json::from_slice(&read_limited(resp, 4 * 1024 * 1024)?).map_err(|e| e.to_string())?;
    let Some(c) = pick(&json, current, std::env::consts::OS) else {
        return Ok(None);
    };
    let sums = read_limited(
        agent.get(&c.sums_url).call().map_err(|e| e.to_string())?,
        1 << 20,
    )?;
    let sums = String::from_utf8_lossy(&sums);
    let sha256 = find_sum(&sums, &c.asset).ok_or("no checksum published for the update")?;
    Ok(Some(Release {
        version: c.version,
        asset: c.asset,
        url: c.url,
        sha256,
    }))
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
    use serde_json::json;

    fn release(tag: &str) -> Value {
        json!({
            "tag_name": tag, "draft": false, "prerelease": false,
            "assets": [
                {"name": "BlackHole-Setup-0.2.0-x64.exe", "browser_download_url": "https://github.com/o/r/releases/download/v0.2.0/BlackHole-Setup-0.2.0-x64.exe"},
                {"name": "BlackHole-macos-universal.zip", "browser_download_url": "https://github.com/o/r/releases/download/v0.2.0/BlackHole-macos-universal.zip"},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/o/r/releases/download/v0.2.0/SHA256SUMS.txt"},
            ],
        })
    }

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.2"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.0.0-rc1"), Some((1, 0, 0)));
        assert_eq!(parse_version("nightly"), None);
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("junk", "0.1.0"));
    }

    #[test]
    fn picks_the_platform_asset_of_a_newer_release() {
        let win = pick(&release("v0.2.0"), "0.1.0", "windows").unwrap();
        assert_eq!(win.asset, "BlackHole-Setup-0.2.0-x64.exe");
        assert_eq!(win.version, "0.2.0");
        assert!(win.sums_url.ends_with("SHA256SUMS.txt"));
        let mac = pick(&release("v0.2.0"), "0.1.0", "macos").unwrap();
        assert_eq!(mac.asset, "BlackHole-macos-universal.zip");
        assert!(pick(&release("v0.2.0"), "0.2.0", "windows").is_none());
        assert!(pick(&release("v0.2.0"), "0.1.0", "linux").is_none());
    }

    #[test]
    fn ignores_drafts_prereleases_and_foreign_hosts() {
        let mut r = release("v0.2.0");
        r["prerelease"] = json!(true);
        assert!(pick(&r, "0.1.0", "windows").is_none());
        let mut r = release("v0.2.0");
        r["assets"][0]["browser_download_url"] = json!("https://evil.example/setup.exe");
        assert!(pick(&r, "0.1.0", "windows").is_none());
        let mut r = release("v0.2.0");
        r["assets"].as_array_mut().unwrap().pop();
        assert!(
            pick(&r, "0.1.0", "windows").is_none(),
            "no checksums, no update"
        );
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

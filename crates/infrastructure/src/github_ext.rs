//! Extensions added from a GitHub repository (market step 3): not reviewed by BlackHole, so
//! trusted on first use. The person names a repository; its latest release is read from the
//! REST API (a person's click, not a background poll, so the anonymous quota is enough), the
//! package for this system is downloaded, hashed (checked against GitHub's own digest when the
//! release publishes one) and its manifest read; nothing is installed before the person has
//! seen the hash and the permissions. The installed package, its hash and its permissions are
//! then pinned (`extensions-github.json`): an update is a new release found by the same file
//! pattern, prepared and shown the same way before it replaces anything.

use crate::{catalog, market, updater};
use blackhole_domain::{
    i18n::message,
    market::{self as rules, PermissionLine},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

const FILE: &str = "extensions-github.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// GitHub's SHA-256 of the file (`digest: "sha256:…"`), when the release has one.
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub owner: String,
    pub repo: String,
    pub tag: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// The release's files that can run here, best first (`asset_rank`).
    pub fn for_platform(&self, platform: &str) -> Vec<(&Asset, u8)> {
        let mut out: Vec<_> = self
            .assets
            .iter()
            .filter_map(|a| rules::asset_rank(platform, &a.name).map(|r| (a, r)))
            .collect();
        out.sort_by_key(|(_, r)| std::cmp::Reverse(*r));
        out
    }
}

/// A downloaded package, checked, waiting for the person's yes.
#[derive(Clone)]
pub struct Prepared {
    pub id: String,
    pub owner: String,
    pub repo: String,
    pub tag: String,
    pub name: String,
    pub version: String,
    pub asset: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    /// GitHub published a digest and it matches (else there was none to compare with).
    pub digest_checked: bool,
    pub permissions: Vec<String>,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared").field("id", &self.id).field("tag", &self.tag).finish()
    }
}

/// An installed extension from GitHub, pinned to the package the person accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pinned {
    pub id: String,
    pub owner: String,
    pub repo: String,
    pub name: String,
    pub version: String,
    pub tag: String,
    pub asset: String,
    /// Finds the same kind of file in later releases (`asset_pattern`).
    pub pattern: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub permissions: Vec<String>,
    pub installed_at: u64,
}

impl Pinned {
    pub fn repo_path(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// The same shape as a market entry, for the lists, the switches and the loading at launch.
    pub fn entry(&self) -> market::Entry {
        let package = market::Package {
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            size: self.size,
        };
        market::Entry {
            id: self.id.clone(),
            name: self.name.clone(),
            description: Default::default(),
            homepage: format!("https://github.com/{}", self.repo_path()),
            category: "github".into(),
            version: self.version.clone(),
            platforms: rules::platform_key(std::env::consts::OS)
                .map(|k| (k.to_owned(), package))
                .into_iter()
                .collect(),
            permissions: self.permissions.clone(),
            source: None,
        }
    }
}

/// Why GitHub had nothing to give, in the interface's language.
fn api_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(404, _) => message("msg.github_no_release", &[]),
        ureq::Error::Status(403 | 429, _) => message("msg.github_rate_limited", &[]),
        other => updater::reach_error(other),
    }
}

/// Reads the release answer of the REST API: files outside the repository's own downloads are
/// left out (`trusted_package_url` and the same owner and repository).
pub fn parse_release(owner: &str, repo: &str, json: &serde_json::Value) -> Option<Release> {
    let tag = json["tag_name"].as_str()?.to_owned();
    let prefix = format!("https://github.com/{owner}/{repo}/releases/download/").to_ascii_lowercase();
    let assets = json["assets"]
        .as_array()?
        .iter()
        .filter_map(|a| {
            let url = a["browser_download_url"].as_str()?.to_owned();
            if !rules::trusted_package_url(&url) || !url.to_ascii_lowercase().starts_with(&prefix) {
                return None;
            }
            Some(Asset {
                name: a["name"].as_str()?.to_owned(),
                url,
                size: a["size"].as_u64().unwrap_or(0),
                digest: a["digest"]
                    .as_str()
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .filter(|d| rules::valid_sha256(d))
                    .map(str::to_ascii_lowercase),
            })
        })
        .collect();
    Some(Release { owner: owner.to_owned(), repo: repo.to_owned(), tag, assets })
}

/// Blocking: the latest published release of `owner/repo`. Run it off the UI thread.
pub fn latest_release(owner: &str, repo: &str) -> Result<Release, String> {
    let resp = updater::agent()
        .get(&format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(api_error)?;
    let json: serde_json::Value =
        serde_json::from_slice(&updater::read_limited(resp, 4 * 1024 * 1024)?).map_err(|e| e.to_string())?;
    parse_release(owner, repo, &json).ok_or_else(|| message("msg.github_no_release", &[]))
}

/// Checks a downloaded package: GitHub's digest when there is one, a readable manifest.
pub fn check_package(release: &Release, asset: &Asset, bytes: Vec<u8>) -> Result<Prepared, String> {
    let sha256 = market::hex(&Sha256::digest(&bytes));
    if let Some(digest) = &asset.digest {
        if *digest != sha256 {
            return Err(message("msg.github_digest_mismatch", &[]));
        }
    }
    let (version, permissions) =
        market::manifest_info(&bytes).map_err(|_| message("msg.github_not_extension", &[]))?;
    let name = rules::display_name(&market::manifest_name(&bytes).unwrap_or_default(), &release.repo);
    Ok(Prepared {
        id: rules::sideload_id(&release.owner, &release.repo),
        owner: release.owner.clone(),
        repo: release.repo.clone(),
        tag: release.tag.clone(),
        name,
        version: version.trim_start_matches('v').to_owned(),
        asset: asset.name.clone(),
        url: asset.url.clone(),
        size: bytes.len() as u64,
        sha256,
        digest_checked: asset.digest.is_some(),
        permissions,
        bytes,
    })
}

/// Blocking: downloads `asset_name` of `release` and checks it.
pub fn prepare(release: &Release, asset_name: &str) -> Result<Prepared, String> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| message("msg.github_no_package", &[]))?;
    let bytes = catalog::fetch(&asset.url)?;
    check_package(release, asset, bytes)
}

/// What an update of `pinned` brings, if its repository has a newer release with the same kind
/// of file: that release and the file's name.
pub fn update_of(pinned: &Pinned, release: &Release) -> Option<String> {
    if release.tag == pinned.tag {
        return None;
    }
    release
        .assets
        .iter()
        .find(|a| rules::asset_matches(&pinned.pattern, &a.name))
        .map(|a| a.name.clone())
}

/// What `prepared` would let the extension do that `pinned` could not (all of it for a first
/// install).
pub fn added_lines(pinned: Option<&Pinned>, prepared: &Prepared) -> Vec<PermissionLine> {
    match pinned {
        Some(p) => rules::added_permission_lines(&p.permissions, &prepared.permissions),
        None => rules::permission_lines(&prepared.permissions),
    }
}

fn path(data: &Path) -> PathBuf {
    data.join(FILE)
}

pub fn load(data: &Path) -> Vec<Pinned> {
    fs::read(path(data))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Pinned>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| rules::valid_id(&p.id) && rules::trusted_package_url(&p.url) && rules::valid_sha256(&p.sha256))
        .collect()
}

fn save(data: &Path, list: &[Pinned]) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
    let dest = path(data);
    let tmp = dest.with_extension("part");
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())
}

/// Blocking: stores the accepted package (its hash checked again) and pins it. Returns the pin
/// and the package's path for the extension host.
pub fn install(data: &Path, prepared: &Prepared, now: u64) -> Result<(Pinned, PathBuf), String> {
    let path = catalog::store(data, &prepared.id, &prepared.version, &prepared.sha256, &prepared.bytes)?;
    let pinned = Pinned {
        id: prepared.id.clone(),
        owner: prepared.owner.clone(),
        repo: prepared.repo.clone(),
        name: prepared.name.clone(),
        version: prepared.version.clone(),
        tag: prepared.tag.clone(),
        asset: prepared.asset.clone(),
        pattern: rules::asset_pattern(&prepared.asset, &prepared.tag),
        url: prepared.url.clone(),
        sha256: prepared.sha256.clone(),
        size: prepared.size,
        permissions: prepared.permissions.clone(),
        installed_at: now,
    };
    let mut list = load(data);
    list.retain(|p| p.id != pinned.id);
    list.push(pinned.clone());
    save(data, &list)?;
    Ok((pinned, path))
}

pub fn remove(data: &Path, id: &str) {
    catalog::remove(data, id);
    let mut list = load(data);
    let before = list.len();
    list.retain(|p| p.id != id);
    if list.len() != before {
        let _ = save(data, &list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn package(manifest: &str) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut out);
        w.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(manifest.as_bytes()).unwrap();
        w.finish().unwrap();
        out.into_inner()
    }

    fn release() -> Release {
        let api = json!({
            "tag_name": "v1.4.0",
            "assets": [
                {"name": "tab-tamer-chromium-1.4.0.zip", "size": 10,
                 "browser_download_url": "https://github.com/someone/tab-tamer/releases/download/v1.4.0/tab-tamer-chromium-1.4.0.zip"},
                {"name": "tab-tamer-firefox-1.4.0.zip", "size": 10,
                 "browser_download_url": "https://github.com/someone/tab-tamer/releases/download/v1.4.0/tab-tamer-firefox-1.4.0.zip"},
                {"name": "evil.zip", "size": 10,
                 "browser_download_url": "https://github.com/other/repo/releases/download/v1/evil.zip"},
                {"name": "notes.txt", "size": 1,
                 "browser_download_url": "https://github.com/someone/tab-tamer/releases/download/v1.4.0/notes.txt"}
            ]
        });
        parse_release("someone", "tab-tamer", &api).unwrap()
    }

    #[test]
    fn keeps_only_the_repository_files_that_can_run_here() {
        let r = release();
        assert_eq!(r.tag, "v1.4.0");
        assert!(r.assets.iter().all(|a| a.name != "evil.zip"));
        let win: Vec<_> = r.for_platform("windows").iter().map(|(a, _)| a.name.clone()).collect();
        assert_eq!(win, ["tab-tamer-chromium-1.4.0.zip"]);
        let mac: Vec<_> = r.for_platform("macos").iter().map(|(a, _)| a.name.clone()).collect();
        assert_eq!(mac, ["tab-tamer-chromium-1.4.0.zip", "tab-tamer-firefox-1.4.0.zip"]);
    }

    #[test]
    fn checks_the_digest_and_reads_the_manifest() {
        let mut r = release();
        let zip = package(r#"{"name":"__MSG_name__","version":"1.4.0","permissions":["tabs"],"host_permissions":["*://*.example.com/*"]}"#);
        let sha = market::hex(&Sha256::digest(&zip));
        r.assets[0].digest = Some(sha.clone());
        let p = check_package(&r, &r.assets[0].clone(), zip.clone()).unwrap();
        assert_eq!((p.id.as_str(), p.name.as_str(), p.version.as_str()), ("gh-someone-tab-tamer", "tab-tamer", "1.4.0"));
        assert!(p.digest_checked && p.sha256 == sha);
        assert_eq!(p.permissions, ["*://*.example.com/*", "tabs"]);
        r.assets[0].digest = Some("0".repeat(64));
        assert!(check_package(&r, &r.assets[0].clone(), zip).is_err());
        assert!(check_package(&r, &r.assets[0].clone(), b"not a zip".to_vec()).is_err());
    }

    #[test]
    fn pins_what_was_accepted_and_finds_its_updates() {
        let dir = tempfile::tempdir().unwrap();
        let r = release();
        let zip = package(r#"{"name":"Tab Tamer","version":"1.4.0","permissions":["tabs"]}"#);
        let p = check_package(&r, &r.assets[0].clone(), zip).unwrap();
        assert_eq!(added_lines(None, &p).len(), 1);
        let (pinned, path) = install(dir.path(), &p, 7).unwrap();
        assert!(path.is_file());
        assert_eq!(pinned.pattern, "tab-tamer-chromium-*.zip");
        assert_eq!(load(dir.path()), std::slice::from_ref(&pinned));
        assert_eq!(pinned.entry().package().map(|_| ()), rules::platform_key(std::env::consts::OS).map(|_| ()));
        // Same release: nothing to update. A newer one with the same kind of file: that file.
        assert_eq!(update_of(&pinned, &r), None);
        let mut next = r.clone();
        next.tag = "v1.5.0".into();
        next.assets[0].name = "tab-tamer-chromium-1.5.0.zip".into();
        assert_eq!(update_of(&pinned, &next).as_deref(), Some("tab-tamer-chromium-1.5.0.zip"));
        next.assets.remove(0);
        assert_eq!(update_of(&pinned, &next), None);
        remove(dir.path(), &pinned.id);
        assert!(load(dir.path()).is_empty() && !catalog::is_installed(dir.path(), &pinned.id));
    }
}

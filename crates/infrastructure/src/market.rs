//! Extension market: the list of installable WebExtensions, each pinned per platform to an
//! official GitHub release asset and its SHA-256.
//!
//! The list is `market/market.json` at the root of the repository. This build carries a copy
//! (`builtin`), the floor used offline. The `Market` workflow signs the same file (Ed25519) and
//! publishes it as the `market` pre-release; the app fetches `market.json` + `market.json.sig`
//! from `releases/download/market/` (no REST API, so no anonymous quota), checks the signature
//! against the keys compiled below, refuses an older or expired index, and keeps it in
//! `<data>/market/`. A package is then installed exactly as before: HTTPS, size cap, SHA-256.

use crate::updater;
use blackhole_domain::market as rules;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// Public halves of the keys that sign the published index (hex). Two may coexist while a key
/// is being replaced.
pub const PUBLIC_KEYS: &[&str] = &["ca74d3307c073672e7a7f3625d5c079717c969a99b75e039663a615942c16562"];

pub const RELEASE_TAG: &str = "market";
pub const INDEX_FILE: &str = "market.json";
pub const SIG_FILE: &str = "market.json.sig";
const MAX_INDEX: u64 = 2 * 1024 * 1024;
const BUILTIN: &str = include_str!("../../../market/market.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

/// How the release tracker finds new versions (read by `bh-market`, ignored by the app).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub repo: String,
    #[serde(default)]
    pub tag_prefix: String,
    /// Asset name pattern (`*` wildcards) per platform key.
    pub assets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// Language → text; English is the fallback.
    #[serde(default)]
    pub description: BTreeMap<String, String>,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub category: String,
    pub version: String,
    /// Platform key (`macos`, `windows`) → package.
    pub platforms: BTreeMap<String, Package>,
    /// Permissions and host permissions of the package's manifest.
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

impl Entry {
    /// Package for the running system; `None` where it has no extension host or no build.
    pub fn package(&self) -> Option<&Package> {
        self.package_for(std::env::consts::OS)
    }

    pub fn package_for(&self, os: &str) -> Option<&Package> {
        rules::platform_key(os).and_then(|k| self.platforms.get(k))
    }

    pub fn description_in(&self, lang: &str) -> &str {
        self.description
            .get(lang)
            .or_else(|| self.description.get("en"))
            .map_or("", String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    /// UNIX time of the signature: grows with every publication (0 in the repository's copy).
    #[serde(default)]
    pub seq: u64,
    /// UNIX time after which the signed index is no longer trusted (0 in the repository's copy).
    #[serde(default)]
    pub expires: u64,
    pub extensions: Vec<Entry>,
}

impl Index {
    pub fn find(&self, id: &str) -> Option<&Entry> {
        self.extensions.iter().find(|e| e.id == id)
    }

    /// Whether this list came signed from the network (as opposed to the copy in this build).
    pub fn is_signed(&self) -> bool {
        self.seq > 0
    }

    /// Entries that can be installed on the running system.
    pub fn for_this_system(&self) -> impl Iterator<Item = &Entry> {
        self.extensions.iter().filter(|e| e.package().is_some())
    }

    /// Every rule a list must follow before anything in it is offered.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != rules::SCHEMA {
            return Err(format!("unsupported market schema {}", self.schema));
        }
        let mut seen = std::collections::BTreeSet::new();
        for e in &self.extensions {
            if !rules::valid_id(&e.id) || !seen.insert(e.id.as_str()) {
                return Err(format!("bad or duplicate id {:?}", e.id));
            }
            if e.name.trim().is_empty() || e.version.trim().is_empty() {
                return Err(format!("{}: missing name or version", e.id));
            }
            if e.platforms.is_empty() {
                return Err(format!("{}: no package", e.id));
            }
            for (platform, p) in &e.platforms {
                if !matches!(platform.as_str(), "macos" | "windows") {
                    return Err(format!("{}: unknown platform {platform}", e.id));
                }
                if !rules::trusted_package_url(&p.url) {
                    return Err(format!("{}: untrusted URL {}", e.id, p.url));
                }
                if !rules::valid_sha256(&p.sha256) {
                    return Err(format!("{}: bad SHA-256 for {platform}", e.id));
                }
            }
        }
        Ok(())
    }
}

pub fn parse(json: &[u8]) -> Result<Index, String> {
    let index: Index = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    index.validate()?;
    Ok(index)
}

/// The list carried by this build (`market/market.json` when it was compiled).
pub fn builtin() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| parse(BUILTIN.as_bytes()).expect("market/market.json is valid"))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    // Pairs of hex digits; an odd tail fails.
    s.trim()
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            // Digits only: `from_str_radix` would take a sign ("+f").
            if pair.len() != 2 || !pair.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()
        })
        .collect()
}

/// Whether `sig` (hex) is a signature of `json` by one of `keys` (hex public keys).
pub fn signed_by(json: &[u8], sig: &str, keys: &[&str]) -> bool {
    let Some(sig) = unhex(sig).and_then(|b| <[u8; 64]>::try_from(b).ok()) else {
        return false;
    };
    let sig = Signature::from_bytes(&sig);
    keys.iter().any(|k| {
        unhex(k)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .and_then(|b| VerifyingKey::from_bytes(&b).ok())
            .is_some_and(|key| key.verify_strict(json, &sig).is_ok())
    })
}

/// A signed index, checked: signature first, then its content.
pub fn decode(json: &[u8], sig: &str, keys: &[&str]) -> Result<Index, String> {
    if !signed_by(json, sig, keys) {
        return Err("market index: bad signature".into());
    }
    let index = parse(json)?;
    if !index.is_signed() {
        return Err("market index: no sequence".into());
    }
    Ok(index)
}

pub fn cache_dir(data: &Path) -> PathBuf {
    data.join("market")
}

fn read_cache(data: &Path, keys: &[&str]) -> Option<Index> {
    let dir = cache_dir(data);
    let json = fs::read(dir.join(INDEX_FILE)).ok()?;
    let sig = fs::read_to_string(dir.join(SIG_FILE)).ok()?;
    decode(&json, &sig, keys).ok()
}

/// Sequence of the index accepted before on this device, expired or not.
fn last_seq(data: &Path, keys: &[&str]) -> Option<u64> {
    read_cache(data, keys).map(|i| i.seq)
}

/// The signed index kept on this device, if it is still valid at `now`.
pub fn cached(data: &Path, now: u64) -> Option<Index> {
    cached_with(data, now, PUBLIC_KEYS)
}

fn cached_with(data: &Path, now: u64, keys: &[&str]) -> Option<Index> {
    read_cache(data, keys)
        .filter(|i| rules::accept(i.schema, i.seq, i.expires, now, None).is_ok())
}

/// The list in use: the signed one kept on this device while valid, else the build's own.
pub fn current(data: &Path, now: u64) -> Index {
    cached(data, now).unwrap_or_else(|| builtin().clone())
}

/// Checks a downloaded index and keeps it when it may replace the one in use.
pub fn accept_download(
    data: &Path,
    json: &[u8],
    sig: &str,
    now: u64,
    keys: &[&str],
) -> Result<Index, String> {
    let index = decode(json, sig, keys)?;
    rules::accept(index.schema, index.seq, index.expires, now, last_seq(data, keys)).map_err(
        |why| match why {
            rules::Refusal::Schema => "market index: written for a newer BlackHole".to_owned(),
            rules::Refusal::Older => "market index: older than the one already here".to_owned(),
            rules::Refusal::Expired => "market index: expired".to_owned(),
        },
    )?;
    let dir = cache_dir(data);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // The signature goes last: a crash in between leaves a pair that fails to verify (ignored).
    for (name, bytes) in [(INDEX_FILE, json), (SIG_FILE, sig.trim().as_bytes())] {
        let dest = dir.join(name);
        let tmp = dest.with_extension("part");
        fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    }
    Ok(index)
}

pub fn download_url(file: &str) -> String {
    format!(
        "https://github.com/{}/releases/download/{RELEASE_TAG}/{file}",
        updater::REPO
    )
}

/// Blocking: downloads the published index, checks it and keeps it. Run it off the UI thread.
pub fn fetch(data: &Path, now: u64) -> Result<Index, String> {
    let agent = updater::agent();
    let get = |file: &str| -> Result<Vec<u8>, String> {
        let resp = agent
            .get(&download_url(file))
            .call()
            .map_err(updater::reach_error)?;
        updater::read_limited(resp, MAX_INDEX)
    };
    let json = get(INDEX_FILE)?;
    let sig = get(SIG_FILE)?;
    accept_download(data, &json, &String::from_utf8_lossy(&sig), now, PUBLIC_KEYS)
}

/// Version and permissions (`permissions` + `host_permissions`, sorted) of a WebExtension
/// package (zip): its root manifest, or the shallowest one when it has a top folder.
pub fn manifest_info(zip: &[u8]) -> Result<(String, Vec<String>), String> {
    let manifest = manifest(zip)?;
    let version = manifest["version"]
        .as_str()
        .ok_or("manifest without version")?
        .to_owned();
    let mut permissions = std::collections::BTreeSet::new();
    for key in ["permissions", "host_permissions"] {
        for p in manifest[key].as_array().into_iter().flatten() {
            if let Some(p) = p.as_str() {
                permissions.insert(p.to_owned());
            }
        }
    }
    Ok((version, permissions.into_iter().collect()))
}

/// The `name` of a package's manifest (possibly a `__MSG_…__` locale placeholder).
pub fn manifest_name(zip: &[u8]) -> Option<String> {
    manifest(zip).ok()?["name"].as_str().map(str::to_owned)
}

/// The manifest of a WebExtension package, parsed.
fn manifest(zip: &[u8]) -> Result<serde_json::Value, String> {
    use std::io::Read;
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| e.to_string())?;
    let name = archive
        .file_names()
        .filter(|n| *n == "manifest.json" || n.ends_with("/manifest.json"))
        .min_by_key(|n| n.matches('/').count())
        .map(str::to_owned)
        .ok_or("no manifest.json in the package")?;
    let mut text = String::new();
    archive
        .by_name(&name)
        .map_err(|e| e.to_string())?
        .take(4 * 1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key() -> (SigningKey, String) {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = hex(key.verifying_key().as_bytes());
        (key, public)
    }

    fn signed(seq: u64, expires: u64) -> (Vec<u8>, String) {
        let mut index = builtin().clone();
        index.seq = seq;
        index.expires = expires;
        let json = serde_json::to_vec_pretty(&index).unwrap();
        let sig = hex(&key().0.sign(&json).to_bytes());
        (json, sig)
    }

    #[test]
    fn builtin_list_is_valid_and_covers_both_systems() {
        let index = builtin();
        assert!(!index.is_signed());
        assert!(index.extensions.len() >= 6);
        for e in &index.extensions {
            assert!(e.package_for("macos").is_some(), "{}", e.id);
            assert!(e.package_for("windows").is_some(), "{}", e.id);
            assert!(e.package_for("linux").is_none());
            assert!(!e.description_in("fr").is_empty() && !e.description_in("en").is_empty());
            let source = e.source.as_ref().expect("tracked by the release workflow");
            assert!(source.assets.contains_key("macos") && source.assets.contains_key("windows"));
        }
        // Bitwarden: Firefox-style build for WKWebView, Chromium build for WebView2.
        let bw = index.find("bitwarden").unwrap();
        assert!(bw.package_for("macos").unwrap().url.contains("dist-firefox"));
        assert!(bw.package_for("windows").unwrap().url.contains("dist-chrome"));
    }

    #[test]
    fn signature_is_required() {
        let (_, public) = key();
        let (json, sig) = signed(100, 200);
        assert!(decode(&json, &sig, &[&public]).is_ok());
        let mut tampered = json.clone();
        let at = tampered.iter().position(|b| *b == b'4').unwrap();
        tampered[at] = b'5';
        assert!(decode(&tampered, &sig, &[&public]).is_err());
        let other = hex(SigningKey::from_bytes(&[8; 32]).verifying_key().as_bytes());
        assert!(decode(&json, &sig, &[&other]).is_err());
        assert!(decode(&json, "zz", &[&public]).is_err());
        // An unsigned (sequence 0) list is never taken from the network.
        let (json0, sig0) = signed(0, 200);
        assert!(decode(&json0, &sig0, &[&public]).is_err());
    }

    #[test]
    fn rejects_unsafe_entries() {
        let mut index = builtin().clone();
        index.extensions[0].id = "../x".into();
        assert!(index.validate().is_err());
        let mut index = builtin().clone();
        index.extensions[0].platforms.get_mut("macos").unwrap().url =
            "https://example.org/releases/download/a/b.zip".into();
        assert!(index.validate().is_err());
        let mut index = builtin().clone();
        let dup = index.extensions[0].clone();
        index.extensions.push(dup);
        assert!(index.validate().is_err());
    }

    #[test]
    fn keeps_newer_indexes_only() {
        let dir = tempfile::tempdir().unwrap();
        let (_, public) = key();
        let keys = [public.as_str()];
        let (json, sig) = signed(100, 1_000);
        assert_eq!(accept_download(dir.path(), &json, &sig, 50, &keys).unwrap().seq, 100);
        assert_eq!(cached_with(dir.path(), 60, &keys).unwrap().seq, 100);
        // Replayed older index: refused, the newer one stays.
        let (old, old_sig) = signed(90, 1_000);
        assert!(accept_download(dir.path(), &old, &old_sig, 60, &keys).is_err());
        assert_eq!(cached_with(dir.path(), 60, &keys).unwrap().seq, 100);
        // Expired: neither accepted nor used, the build's own list takes over.
        let (late, late_sig) = signed(110, 70);
        assert!(accept_download(dir.path(), &late, &late_sig, 80, &keys).is_err());
        assert!(cached_with(dir.path(), 1_000, &keys).is_none());
        let (next, next_sig) = signed(120, 2_000);
        assert_eq!(accept_download(dir.path(), &next, &next_sig, 1_000, &keys).unwrap().seq, 120);
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(unhex(&hex(&[0, 1, 254, 255])).unwrap(), vec![0, 1, 254, 255]);
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
        assert!(unhex("+f").is_none());
    }
}

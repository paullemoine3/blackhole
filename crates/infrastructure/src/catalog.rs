//! Installed WebExtensions from the market (`crate::market`). Every entry is pinned to an
//! official release URL and a SHA-256: nothing that does not match the hash is ever installed.

use crate::market::Entry;
use blackhole_domain::market as rules;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub fn webext_dir(data: &Path) -> PathBuf {
    data.join("webext")
}

pub fn package_path(data: &Path, id: &str) -> PathBuf {
    webext_dir(data).join(format!("{id}.zip"))
}

pub fn is_installed(data: &Path, id: &str) -> bool {
    package_path(data, id).is_file()
}

fn version_path(data: &Path, id: &str) -> PathBuf {
    webext_dir(data).join(format!("{id}.version"))
}

/// Version recorded when the package was stored; `None` for installs that predate the record.
pub fn installed_version(data: &Path, id: &str) -> Option<String> {
    let v = fs::read_to_string(version_path(data, id)).ok()?;
    let v = v.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    Some(v.trim_start_matches('v').to_owned()).filter(|v| !v.is_empty())
}

/// An installed extension is outdated when the market pins a newer version than the one stored
/// (or when no version was recorded: reinstalling is harmless). Never a downgrade, so going back
/// to the build's own list (offline, expired index) offers nothing.
pub fn needs_update(data: &Path, entry: &Entry) -> bool {
    entry.package().is_some()
        && is_installed(data, &entry.id)
        && rules::is_upgrade(&entry.version, installed_version(data, &entry.id).as_deref())
}

pub fn remove(data: &Path, id: &str) {
    if !rules::valid_id(id) {
        return;
    }
    let _ = fs::remove_file(package_path(data, id));
    let _ = fs::remove_file(version_path(data, id));
    let _ = fs::remove_dir_all(webext_dir(data).join("unpacked").join(id));
}

/// Fails unless `bytes` hash to `expected` (hex SHA-256).
pub fn verify(bytes: &[u8], expected: &str) -> Result<(), String> {
    let got: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!("checksum mismatch ({got})"))
    }
}

/// Stores a verified package atomically (temp file then rename).
pub fn store(
    data: &Path,
    id: &str,
    version: &str,
    sha256: &str,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    if !rules::valid_id(id) {
        return Err("bad extension id".into());
    }
    verify(bytes, sha256)?;
    let dest = package_path(data, id);
    fs::create_dir_all(webext_dir(data)).map_err(|e| e.to_string())?;
    let tmp = dest.with_extension("part");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    let record = version_path(data, id);
    let tmp = record.with_extension("part");
    fs::write(&tmp, version).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &record).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Blocking download + verification + install. Run it off the UI thread.
pub fn install(data: &Path, entry: &Entry) -> Result<PathBuf, String> {
    let package = entry
        .package()
        .ok_or_else(|| "no package for this system".to_owned())?;
    if !rules::trusted_package_url(&package.url) {
        return Err("refusing a source that is not a GitHub release".into());
    }
    let resp = ureq::get(&package.url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("package too large".into());
    }
    store(data, &entry.id, &entry.version, &package.sha256, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::Package;
    use std::collections::BTreeMap;

    // sha256("abc")
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[cfg(any(target_os = "macos", windows))]
    fn entry(version: &str) -> Entry {
        let package = Package {
            url: "https://github.com/a/b/releases/download/t/f.zip".into(),
            sha256: ABC.into(),
            size: 3,
        };
        Entry {
            id: "t".into(),
            name: "t".into(),
            description: BTreeMap::new(),
            homepage: String::new(),
            category: String::new(),
            version: version.into(),
            platforms: ["macos", "windows"]
                .into_iter()
                .map(|k| (k.to_owned(), package.clone()))
                .collect(),
            permissions: Vec::new(),
            source: None,
        }
    }

    #[test]
    fn rejects_tampered_package() {
        let dir = tempfile::tempdir().unwrap();
        assert!(store(dir.path(), "t", "1", ABC, b"abd").is_err());
        assert!(!is_installed(dir.path(), "t"));
    }

    #[test]
    fn rejects_unsafe_ids() {
        let dir = tempfile::tempdir().unwrap();
        assert!(store(dir.path(), "../t", "1", ABC, b"abc").is_err());
    }

    #[test]
    fn stores_verified_package() {
        let dir = tempfile::tempdir().unwrap();
        store(dir.path(), "t", "1", ABC, b"abc").unwrap();
        assert!(is_installed(dir.path(), "t"));
        assert_eq!(installed_version(dir.path(), "t").as_deref(), Some("1"));
        remove(dir.path(), "t");
        assert!(!is_installed(dir.path(), "t"));
    }

    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn detects_outdated_install() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!needs_update(dir.path(), &entry("1")));
        store(dir.path(), "t", "1", ABC, b"abc").unwrap();
        assert!(!needs_update(dir.path(), &entry("1")));
        assert!(needs_update(dir.path(), &entry("2")));
        // An older list (offline, expired index) never offers a downgrade.
        store(dir.path(), "t", "3", ABC, b"abc").unwrap();
        assert!(!needs_update(dir.path(), &entry("2")));
        fs::remove_file(version_path(dir.path(), "t")).unwrap();
        assert!(needs_update(dir.path(), &entry("1")));
        remove(dir.path(), "t");
        assert!(!needs_update(dir.path(), &entry("2")));
    }
}

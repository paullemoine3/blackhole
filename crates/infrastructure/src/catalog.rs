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

fn permissions_path(data: &Path, id: &str) -> PathBuf {
    webext_dir(data).join(format!("{id}.permissions.json"))
}

/// Permissions of the installed package (its manifest), recorded at install; read from the
/// package once (then recorded) for installs that predate the record. `None`: not installed or
/// no readable manifest.
pub fn installed_permissions(data: &Path, id: &str) -> Option<Vec<String>> {
    if !rules::valid_id(id) || !is_installed(data, id) {
        return None;
    }
    let record = permissions_path(data, id);
    if let Some(perms) = fs::read(&record)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
    {
        return Some(perms);
    }
    let (_, perms) = crate::market::manifest_info(&fs::read(package_path(data, id)).ok()?).ok()?;
    write_permissions(data, id, &perms);
    Some(perms)
}

fn write_permissions(data: &Path, id: &str, perms: &[String]) {
    let record = permissions_path(data, id);
    let tmp = record.with_extension("part");
    if let Ok(json) = serde_json::to_vec(perms) {
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &record);
        }
    }
}

pub fn remove(data: &Path, id: &str) {
    if !rules::valid_id(id) {
        return;
    }
    let _ = fs::remove_file(package_path(data, id));
    let _ = fs::remove_file(version_path(data, id));
    let _ = fs::remove_file(permissions_path(data, id));
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
    // What this version may do: the next update shows what it would add.
    match crate::market::manifest_info(bytes) {
        Ok((_, perms)) => write_permissions(data, id, &perms),
        Err(_) => {
            let _ = fs::remove_file(permissions_path(data, id));
        }
    }
    Ok(dest)
}

/// Blocking download + verification + install. Run it off the UI thread.
pub fn install(data: &Path, entry: &Entry) -> Result<PathBuf, String> {
    let package = entry
        .package()
        .ok_or_else(|| "no package for this system".to_owned())?;
    let bytes = fetch(&package.url)?;
    store(data, &entry.id, &entry.version, &package.sha256, &bytes)
}

/// Blocking download of a package from a GitHub release (64 MB at most). The caller checks it.
pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if !rules::trusted_package_url(url) {
        return Err("refusing a source that is not a GitHub release".into());
    }
    let resp = ureq::get(url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("package too large".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // sha256("abc")
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    // Systems with an extension host only (elsewhere no entry has a package for the system).
    #[cfg(any(target_os = "macos", windows))]
    fn entry(version: &str) -> Entry {
        use crate::market::Package;
        use std::collections::BTreeMap;
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

    fn package(manifest: &str) -> Vec<u8> {
        use std::io::Write;
        let mut out = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut out);
        w.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(manifest.as_bytes()).unwrap();
        w.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn remembers_the_permissions_of_the_installed_package() {
        let dir = tempfile::tempdir().unwrap();
        let zip = package(r#"{"version":"1","permissions":["tabs","storage"],"host_permissions":["<all_urls>"]}"#);
        let sha = crate::market::hex(&Sha256::digest(&zip));
        assert_eq!(installed_permissions(dir.path(), "t"), None);
        store(dir.path(), "t", "1", &sha, &zip).unwrap();
        assert_eq!(installed_permissions(dir.path(), "t").unwrap(), ["<all_urls>", "storage", "tabs"]);
        // An install from before the record: read from the package, then recorded.
        fs::remove_file(permissions_path(dir.path(), "t")).unwrap();
        assert_eq!(installed_permissions(dir.path(), "t").unwrap().len(), 3);
        assert!(permissions_path(dir.path(), "t").is_file());
        remove(dir.path(), "t");
        assert!(!permissions_path(dir.path(), "t").exists());
        // A package without a manifest: nothing known.
        store(dir.path(), "t", "1", ABC, b"abc").unwrap();
        assert_eq!(installed_permissions(dir.path(), "t"), None);
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

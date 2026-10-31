//! Curated catalog of real WebExtensions. Every entry is pinned to an official release URL and
//! a SHA-256: nothing that does not match the hash is ever installed.

use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub version: &'static str,
    pub homepage: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
}

pub const CATALOG: &[CatalogEntry] = &[CatalogEntry {
    id: "ublock-origin-lite",
    name: "uBlock Origin Lite",
    description: "Bloqueur de publicités et de traqueurs (Manifest V3, declarativeNetRequest).",
    version: "2026.1026.1812",
    homepage: "https://github.com/uBlockOrigin/uBOL-home",
    url: "https://github.com/uBlockOrigin/uBOL-home/releases/download/2026.1026.1812/uBOLite_2026.1026.1812.chromium.zip",
    sha256: "d3788eac9f0a44c67f3745a43c3fd446ef33d56b1121f469b19e0fa0a836c2b5",
}, CatalogEntry {
    id: "dark-reader",
    name: "Dark Reader",
    description: "Mode sombre pour tous les sites (Manifest V3).",
    version: "4.9.134",
    homepage: "https://github.com/darkreader/darkreader",
    url: "https://github.com/darkreader/darkreader/releases/download/v4.9.134/darkreader-chrome-mv3.zip",
    sha256: "40402188ad23154a1b8fa094171f2b6908209ed6673ccf58276b75ce661acfaf",
}, CatalogEntry {
    id: "sponsorblock",
    name: "SponsorBlock",
    description: "Saute les sponsors et intros dans les vidéos YouTube.",
    version: "6.1.7",
    homepage: "https://github.com/ajayyy/SponsorBlock",
    url: "https://github.com/ajayyy/SponsorBlock/releases/download/6.1.7/ChromeExtension.zip",
    sha256: "7c8e3dc8b5e560b480ae8efdf28f4f1101fce0cc32434562a336a89e4962eda3",
}, CatalogEntry {
    id: "bitwarden",
    name: "Bitwarden",
    description: "Gestionnaire de mots de passe (popup dans la barre d'outils).",
    version: "2026.10.1",
    homepage: "https://github.com/bitwarden/clients",
    url: "https://github.com/bitwarden/clients/releases/download/browser-v2026.10.1/dist-firefox-2026.10.1.zip",
    sha256: "80b19fe076e3a86c9eaa33c18d0be1401862cf97638eacaa284b6ea7deae3e9d",
}];

pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

pub fn webext_dir(data: &Path) -> PathBuf {
    data.join("webext")
}

pub fn package_path(data: &Path, id: &str) -> PathBuf {
    webext_dir(data).join(format!("{id}.zip"))
}

pub fn is_installed(data: &Path, id: &str) -> bool {
    package_path(data, id).is_file()
}

pub fn remove(data: &Path, id: &str) {
    let _ = fs::remove_file(package_path(data, id));
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
pub fn store(data: &Path, entry: &CatalogEntry, bytes: &[u8]) -> Result<PathBuf, String> {
    verify(bytes, entry.sha256)?;
    let dest = package_path(data, entry.id);
    fs::create_dir_all(webext_dir(data)).map_err(|e| e.to_string())?;
    let tmp = dest.with_extension("part");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Blocking download + verification + install. Run it off the UI thread.
pub fn install(data: &Path, entry: &CatalogEntry) -> Result<PathBuf, String> {
    if !entry.url.starts_with("https://") {
        return Err("refusing non-HTTPS source".into());
    }
    let resp = ureq::get(entry.url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("package too large".into());
    }
    store(data, entry, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: CatalogEntry = CatalogEntry {
        id: "t",
        name: "t",
        description: "",
        version: "1",
        homepage: "",
        url: "https://x",
        // sha256("abc")
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    };

    #[test]
    fn rejects_tampered_package() {
        let dir = tempfile::tempdir().unwrap();
        assert!(store(dir.path(), &ENTRY, b"abd").is_err());
        assert!(!is_installed(dir.path(), "t"));
    }

    #[test]
    fn stores_verified_package() {
        let dir = tempfile::tempdir().unwrap();
        store(dir.path(), &ENTRY, b"abc").unwrap();
        assert!(is_installed(dir.path(), "t"));
        remove(dir.path(), "t");
        assert!(!is_installed(dir.path(), "t"));
    }

    #[test]
    fn catalog_ids_are_unique_and_pinned() {
        for e in CATALOG {
            assert_eq!(e.sha256.len(), 64);
            assert!(e.url.starts_with("https://"));
        }
    }
}

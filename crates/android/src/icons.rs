//! Site icons (favicons) for the interface: the address bar, quick access, tabs, history.
//!
//! The WebView hands the shell the icon of each page it loads (`onReceivedIcon`); the shell sends
//! it as a small PNG `data:` URL, checked here before it is kept (bounded size, PNG base64 only:
//! the interface's CSP allows `data:` images and nothing else). Icons are kept by site (host
//! without `www.`, like the interface's tiles) in `icons.json`, never for private tabs.

use blackhole_infrastructure::write_later;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Sites whose icon is kept; the least recently seen goes first.
pub(crate) const MAX_ICONS: usize = 300;
/// A 64 px PNG is a few kilobytes: anything bigger is refused.
pub(crate) const MAX_DATA: usize = 24 * 1024;
const PREFIX: &str = "data:image/png;base64,";

#[derive(Debug, Default)]
pub(crate) struct Icons {
    path: PathBuf,
    /// Site → (data URL, last seen).
    map: HashMap<String, (String, u64)>,
    seq: u64,
}

/// The key of a host: lower case, without `www.`.
pub(crate) fn site_key(host: &str) -> String {
    host.trim_start_matches("www.").to_ascii_lowercase()
}

/// A PNG `data:` URL of a reasonable size, in plain base64.
pub(crate) fn valid(data: &str) -> bool {
    data.len() <= MAX_DATA
        && data.strip_prefix(PREFIX).is_some_and(|b64| {
            !b64.is_empty() && b64.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        })
}

impl Icons {
    pub(crate) fn load(dir: &Path) -> Self {
        let path = dir.join("icons.json");
        let saved: HashMap<String, String> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let mut icons = Self {
            path,
            ..Self::default()
        };
        for (site, data) in saved.into_iter().filter(|(s, d)| !s.is_empty() && valid(d)).take(MAX_ICONS) {
            icons.seq += 1;
            icons.map.insert(site, (data, icons.seq));
        }
        icons
    }

    /// Keeps `data` as the icon of `host`; `true` when it changed (the interface is told).
    pub(crate) fn set(&mut self, host: &str, data: &str) -> bool {
        let site = site_key(host);
        if site.is_empty() || !valid(data) {
            return false;
        }
        self.seq += 1;
        let changed = !matches!(self.map.get(&site), Some((d, _)) if d == data);
        self.map.insert(site, (data.to_owned(), self.seq));
        while self.map.len() > MAX_ICONS {
            let Some(oldest) = self.map.iter().min_by_key(|(_, (_, seen))| *seen).map(|(k, _)| k.clone()) else {
                break;
            };
            self.map.remove(&oldest);
        }
        if changed {
            self.save();
        }
        changed
    }

    pub(crate) fn clear(&mut self) {
        self.map.clear();
        self.save();
    }

    fn save(&self) {
        let plain: HashMap<&String, &String> = self.map.iter().map(|(k, (d, _))| (k, d)).collect();
        if let Ok(raw) = serde_json::to_string(&plain) {
            write_later(self.path.clone(), raw);
        }
    }

    /// Every icon, for the interface when it (re)loads.
    pub(crate) fn all(&self) -> HashMap<&str, &str> {
        self.map.iter().map(|(k, (d, _))| (k.as_str(), d.as_str())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_small_png_data_urls_are_kept() {
        assert!(valid("data:image/png;base64,iVBORw0KGgo="));
        assert!(!valid("data:image/svg+xml;base64,PHN2Zz4="));
        assert!(!valid("data:image/png;base64,"));
        assert!(!valid("data:image/png;base64,abc\"onerror=x"));
        assert!(!valid(&format!("{PREFIX}{}", "A".repeat(MAX_DATA))));
        assert!(!valid("https://example.org/favicon.ico"));
    }

    #[test]
    fn icons_are_kept_by_site_bounded_and_saved() {
        let dir = tempfile::tempdir().unwrap();
        let mut icons = Icons::load(dir.path());
        assert!(icons.set("www.Example.org", "data:image/png;base64,AAAA"));
        assert!(!icons.set("example.org", "data:image/png;base64,AAAA"), "same icon: nothing to tell");
        assert_eq!(icons.all().get("example.org"), Some(&"data:image/png;base64,AAAA"));
        for i in 0..MAX_ICONS + 5 {
            icons.set(&format!("s{i}.example"), "data:image/png;base64,BBBB");
        }
        assert_eq!(icons.map.len(), MAX_ICONS);
        assert!(!icons.all().contains_key("example.org"), "the least recently seen went first");
        blackhole_infrastructure::flush_writes();
        assert_eq!(Icons::load(dir.path()).map.len(), MAX_ICONS);
    }
}

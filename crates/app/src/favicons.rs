//! Site icons reported by the pages themselves, remembered per host across launches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use url::Url;

const MAX_URL_LEN: usize = 20_000;
const MAX_HOSTS: usize = 2_000;

pub struct Favicons {
    path: PathBuf,
    by_host: HashMap<String, String>,
}

/// Host key shared by tabs and icons: lower-case, without a leading `www.`.
pub fn host_key(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    Some(
        host.strip_prefix("www.")
            .map_or(host.clone(), str::to_owned),
    )
}

impl Favicons {
    pub fn load(data: &Path) -> Self {
        blackhole_infrastructure::flush_writes();
        let path = data.join("favicons.json");
        let by_host = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self { path, by_host }
    }

    /// Remembers `icon` for `page`'s host. Returns whether anything changed.
    pub fn set(&mut self, page: &Url, icon: &str) -> bool {
        let Some(host) = host_key(page) else {
            return false;
        };
        let acceptable = icon.len() <= MAX_URL_LEN
            && Url::parse(icon).is_ok_and(|u| matches!(u.scheme(), "https" | "http" | "data"));
        if !acceptable || self.by_host.get(&host).is_some_and(|known| known == icon) {
            return false;
        }
        if self.by_host.len() >= MAX_HOSTS && !self.by_host.contains_key(&host) {
            return false;
        }
        self.by_host.insert(host, icon.to_owned());
        if let Ok(raw) = serde_json::to_string(&self.by_host) {
            blackhole_infrastructure::write_later(self.path.clone(), raw);
        }
        true
    }

    pub fn all(&self) -> &HashMap<String, String> {
        &self.by_host
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_valid_icons_per_host_and_persists_them() {
        let dir = std::env::temp_dir().join(format!("bh-fav-{}", std::process::id()));
        let page = Url::parse("https://www.example.com/a").unwrap();
        let mut f = Favicons::load(&dir);
        assert!(f.set(&page, "https://example.com/icon.png"));
        assert!(!f.set(&page, "https://example.com/icon.png"));
        assert!(!f.set(&page, "javascript:alert(1)"));
        assert_eq!(
            Favicons::load(&dir)
                .all()
                .get("example.com")
                .map(String::as_str),
            Some("https://example.com/icon.png")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

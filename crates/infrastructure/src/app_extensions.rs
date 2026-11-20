//! The system's app extensions (on macOS, the Safari extensions of installed apps) turned on in
//! a profile: their ids, kept in `app_extensions.json`.

use std::path::{Path, PathBuf};

fn file(data: &Path) -> PathBuf {
    data.join("app_extensions.json")
}

/// Ids of the app extensions turned on in the profile at `data` (none if the file is missing).
pub fn enabled(data: &Path) -> Vec<String> {
    std::fs::read_to_string(file(data))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn is_enabled(data: &Path, id: &str) -> bool {
    enabled(data).iter().any(|e| e == id)
}

/// Turns an app extension on or off; written at once, since the extension host reads it next.
pub fn set_enabled(data: &Path, id: &str, on: bool) {
    let mut list = enabled(data);
    list.retain(|e| e != id);
    if on {
        list.push(id.to_owned());
    }
    let _ = crate::write_atomic_now(&file(data), &serde_json::to_string(&list).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_turned_on_are_remembered_once_and_can_be_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("profile");
        assert!(enabled(&data).is_empty());
        set_enabled(&data, "a", true);
        set_enabled(&data, "b", true);
        set_enabled(&data, "a", true);
        assert_eq!(enabled(&data), ["b", "a"]);
        assert!(is_enabled(&data, "a"));
        set_enabled(&data, "a", false);
        assert!(!is_enabled(&data, "a"));
        assert_eq!(enabled(&data), ["b"]);
    }
}

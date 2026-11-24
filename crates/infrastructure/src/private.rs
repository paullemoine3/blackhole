//! The throwaway profile of a private window: what it starts with, and the removal of those a
//! crash or a forced quit left behind.

use std::path::{Path, PathBuf};

const PREFIX: &str = "BlackHole-private-";

/// The folder of the private window of process `pid`.
#[must_use]
pub fn dir(temp: &Path, pid: u32) -> PathBuf {
    temp.join(format!("{PREFIX}{pid}"))
}

/// A private window follows the settings of the profile (Shields, search engine, DNS, language):
/// a copy, never written back. No tab, history, cookie nor easel comes along. The folder is the
/// user's alone, and starts empty (a dead process may have had the same number).
pub fn prepare(profile: &Path, data: &Path) {
    let _ = std::fs::remove_dir_all(data);
    if std::fs::create_dir_all(data).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(data, std::fs::Permissions::from_mode(0o700));
    }
    let _ = std::fs::copy(profile.join("settings.json"), data.join("settings.json"));
}

/// Removes the folders of private windows whose process is gone.
pub fn sweep(temp: &Path, alive: impl Fn(u32) -> bool) {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return;
    };
    for entry in entries.flatten() {
        let pid = entry
            .file_name()
            .to_str()
            .and_then(|n| n.strip_prefix(PREFIX))
            .and_then(|n| n.parse().ok());
        if pid.is_some_and(|pid| !alive(pid)) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_window_starts_with_the_settings_and_nothing_else() {
        let (profile, temp) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        for file in ["settings.json", "workspace.json", "history.json", "favicons.json"] {
            std::fs::write(profile.path().join(file), "{}").unwrap();
        }
        let data = dir(temp.path(), 7);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("workspace.json"), "left by a dead window").unwrap();
        prepare(profile.path(), &data);
        let files: Vec<_> = std::fs::read_dir(&data).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(files, ["settings.json"]);
    }

    #[test]
    fn only_the_folders_of_dead_windows_are_swept() {
        let temp = tempfile::tempdir().unwrap();
        for pid in [7, 8] {
            std::fs::create_dir_all(dir(temp.path(), pid)).unwrap();
        }
        std::fs::create_dir_all(temp.path().join("BlackHole-private-x")).unwrap();
        std::fs::create_dir_all(temp.path().join("other-7")).unwrap();
        sweep(temp.path(), |pid| pid == 8);
        assert!(!dir(temp.path(), 7).exists());
        assert!(dir(temp.path(), 8).exists());
        assert!(temp.path().join("BlackHole-private-x").exists());
        assert!(temp.path().join("other-7").exists());
    }
}

//! Rules about downloaded files.

/// Whether a file with this extension runs code when opened (a program, a script, an installer
/// or a disk image): the shell only reveals it in the file manager, never opens it.
pub fn is_program(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "exe"
            | "msi"
            | "bat"
            | "cmd"
            | "ps1"
            | "com"
            | "scr"
            | "app"
            | "command"
            | "sh"
            | "jar"
            | "vbs"
            | "js"
            | "lnk"
            | "pkg"
            | "dmg"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_and_installers_are_told_apart_from_documents() {
        for ext in ["exe", "MSI", "sh", "Dmg", "pkg", "js"] {
            assert!(is_program(ext), "{ext}");
        }
        for ext in ["pdf", "zip", "png", "txt", ""] {
            assert!(!is_program(ext), "{ext}");
        }
    }
}

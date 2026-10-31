//! Filesystem helpers for the extensions folder (composition-root concerns: opening Finder,
//! writing the bundled example).

use std::{fs, io, path::Path, process::Command};

const SAMPLE_ID: &str = "cookie-banner-hider";
const SAMPLE_MANIFEST: &str = r#"{
  "name": "Cookie banner hider",
  "version": "1.0",
  "description": "Example extension: hides common cookie consent banners and re-enables scrolling.",
  "content_scripts": [
    { "matches": ["<all_urls>"], "css": ["hide.css"], "js": ["unlock.js"] }
  ]
}
"#;
const SAMPLE_CSS: &str = "#onetrust-banner-sdk, #CybotCookiebotDialog, .cookie-banner, .cookie-consent, .cc-window, [id*=\"cookie-banner\"], [class*=\"cookie-banner\"] { display: none !important; }\n";
const SAMPLE_JS: &str = "document.documentElement.style.overflow = document.documentElement.style.overflow === 'hidden' ? '' : document.documentElement.style.overflow;\n";

/// Writes the bundled example extension; existing files are left untouched.
pub fn install_sample(dir: &Path) -> io::Result<()> {
    let root = dir.join(SAMPLE_ID);
    fs::create_dir_all(&root)?;
    for (name, body) in [
        ("manifest.json", SAMPLE_MANIFEST),
        ("hide.css", SAMPLE_CSS),
        ("unlock.js", SAMPLE_JS),
    ] {
        let path = root.join(name);
        if !path.exists() {
            fs::write(path, body)?;
        }
    }
    Ok(())
}

/// Shows the folder in Finder / Explorer.
pub fn reveal(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "open"
    };
    let _ = Command::new(opener).arg(dir).spawn();
}

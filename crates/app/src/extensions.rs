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
/// Never `html` or `body`: some sites mark the page itself (glosbe.com: `<body class="cookie-consent">`),
/// and hiding it left them blank.
const SAMPLE_CSS: &str = ":is(#onetrust-banner-sdk, #CybotCookiebotDialog, .cookie-banner, .cookie-consent, .cc-window, [id*=\"cookie-banner\"], [class*=\"cookie-banner\"]):not(html, body) { display: none !important; }\n";
/// The example's style sheet up to 0.8.14, replaced where it was left as written.
const OLD_SAMPLE_CSS: [&str; 1] = [
    "#onetrust-banner-sdk, #CybotCookiebotDialog, .cookie-banner, .cookie-consent, .cc-window, [id*=\"cookie-banner\"], [class*=\"cookie-banner\"] { display: none !important; }\n",
];
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

/// Brings an installed example up to date when its style sheet is an old one left as written
/// (a sheet the user changed is theirs). Returns whether it was replaced.
pub fn refresh_sample(dir: &Path) -> bool {
    let css = dir.join(SAMPLE_ID).join("hide.css");
    let Ok(text) = fs::read_to_string(&css) else {
        return false;
    };
    // The same text with Windows line ends counts as left as written.
    let text = text.replace("\r\n", "\n");
    OLD_SAMPLE_CSS.contains(&text.as_str()) && fs::write(css, SAMPLE_CSS).is_ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_never_hides_the_page_and_old_copies_are_brought_up_to_date() {
        assert!(SAMPLE_CSS.contains(":not(html, body)"));
        let dir = std::env::temp_dir().join(format!("bh-sample-ext-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // Nothing installed: nothing to do.
        assert!(!refresh_sample(&dir));
        fs::create_dir_all(dir.join(SAMPLE_ID)).unwrap();
        let css = dir.join(SAMPLE_ID).join("hide.css");
        fs::write(&css, OLD_SAMPLE_CSS[0]).unwrap();
        assert!(refresh_sample(&dir));
        assert_eq!(fs::read_to_string(&css).unwrap(), SAMPLE_CSS);
        // Up to date, or changed by the user: left alone.
        assert!(!refresh_sample(&dir));
        fs::write(&css, ".cookie-consent { display: none !important; }").unwrap();
        assert!(!refresh_sample(&dir));
        assert!(fs::read_to_string(&css).unwrap().starts_with(".cookie-consent"));
        let _ = fs::remove_dir_all(&dir);
    }
}

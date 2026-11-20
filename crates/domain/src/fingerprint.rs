//! Fingerprinting protection: the sites left alone by the page script (`ui/fingerprint.js`).
//! The script makes what a page reads back to identify the device (canvas and WebGL pixels,
//! audio samples, the graphics card's name, the core count) vary by site and by session, so the
//! same device does not look the same on two sites, nor on one site after a restart.

/// Sites whose work is drawing, calls or documents: a readback that varies could spoil a file
/// they export, and they do not track across sites. Their subdomains are included.
pub const EXEMPT_SITES: [&str; 9] = [
    "docs.google.com",
    "sheets.google.com",
    "slides.google.com",
    "drive.google.com",
    "meet.google.com",
    "teams.microsoft.com",
    "zoom.us",
    "figma.com",
    "photopea.com",
];

/// Whether pages of `host` are left untouched.
pub fn exempt(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    EXEMPT_SITES
        .iter()
        .any(|site| host == *site || host.ends_with(&format!(".{site}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_and_calls_are_left_alone_not_their_neighbours() {
        assert!(exempt("meet.google.com"));
        assert!(exempt("DOCS.google.com."));
        assert!(exempt("us02web.zoom.us"));
        assert!(!exempt("google.com"));
        assert!(!exempt("notzoom.us"));
        assert!(!exempt("example.com"));
    }
}

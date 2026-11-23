//! Rules about downloaded files.

/// How the engine shows a file of the disk in a tab, like other browsers do (0.8.27).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer {
    Pdf,
    Image,
    Video,
    Audio,
    Text,
}

/// The viewer of a file extension, if the engine shows such files itself. Never a format that
/// runs scripts (HTML, SVG, XHTML, XML): a page of the disk is not opened in a tab.
pub fn viewer_for(extension: &str) -> Option<Viewer> {
    Some(match extension.to_ascii_lowercase().as_str() {
        "pdf" => Viewer::Pdf,
        "png" | "jpg" | "jpeg" | "jfif" | "gif" | "webp" | "avif" | "bmp" | "ico" => Viewer::Image,
        "mp4" | "m4v" | "webm" | "ogv" => Viewer::Video,
        "mp3" | "m4a" | "aac" | "ogg" | "oga" | "opus" | "wav" | "flac" => Viewer::Audio,
        "txt" | "log" | "md" | "csv" | "tsv" | "json" | "ini" | "toml" | "yaml" | "yml" => Viewer::Text,
        _ => return None,
    })
}

/// A `file:` address a tab may show: a local file (no host) the engine views itself
/// (`viewer_for`). Any other `file:` address stays refused.
pub fn viewable_file(url: &url::Url) -> bool {
    url.scheme() == "file"
        && url.host_str().unwrap_or_default().is_empty()
        && url
            .path_segments()
            .and_then(|mut s| s.next_back())
            .and_then(|name| name.rsplit_once('.'))
            .is_some_and(|(_, ext)| viewer_for(ext).is_some())
}

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

/// The file name of a picture saved from the page menu (0.8.25): the last part of its address
/// (`last_segment`, still percent-encoded), stripped of what a file name cannot hold, with
/// `extension` (the format read from the file itself, not the one the address claims). "image"
/// when the address says nothing usable.
pub fn image_file_name(last_segment: &str, extension: &str) -> String {
    let decoded = percent_decode(last_segment);
    let stem = match decoded.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => decoded.as_str(),
    };
    let clean: String = stem
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .take(80)
        .collect();
    let clean = clean.trim_matches(|c: char| c == '.' || c == ' ' || c == '_');
    // Names Windows keeps for devices.
    let reserved = matches!(
        clean.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "LPT1" | "LPT2" | "LPT3"
    );
    let stem = if clean.is_empty() || reserved { "image" } else { clean };
    format!("{stem}.{extension}")
}

/// `%20` -> ` `; a broken escape or a result that is not UTF-8 is kept as written.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_picture_is_named_after_its_address_with_its_real_format() {
        assert_eq!(image_file_name("Sunset%20over%20Lyon.jpg", "jpg"), "Sunset over Lyon.jpg");
        // The address says .php, the file is a PNG.
        assert_eq!(image_file_name("thumb.php", "png"), "thumb.png");
        assert_eq!(image_file_name("photo", "webp"), "photo.webp");
        assert_eq!(image_file_name("", "png"), "image.png");
        assert_eq!(image_file_name("a%3Cb%3E%3F.gif", "gif"), "a_b.gif");
        // No way out of the Downloads folder.
        assert_eq!(image_file_name("..%2F..%2Fevil.png", "png"), "evil.png");
        assert_eq!(image_file_name("con.png", "png"), "image.png");
        assert_eq!(image_file_name("%E2%82%AC%ZZ.png", "png"), "€%ZZ.png");
        assert_eq!(image_file_name(&"x".repeat(300), "png").len(), 84);
    }

    #[test]
    fn programs_and_installers_are_told_apart_from_documents() {
        for ext in ["exe", "MSI", "sh", "Dmg", "pkg", "js"] {
            assert!(is_program(ext), "{ext}");
        }
        for ext in ["pdf", "zip", "png", "txt", ""] {
            assert!(!is_program(ext), "{ext}");
        }
    }

    #[test]
    fn the_engine_views_documents_pictures_and_media_never_pages() {
        assert_eq!(viewer_for("PDF"), Some(Viewer::Pdf));
        assert_eq!(viewer_for("webp"), Some(Viewer::Image));
        assert_eq!(viewer_for("mp4"), Some(Viewer::Video));
        assert_eq!(viewer_for("flac"), Some(Viewer::Audio));
        assert_eq!(viewer_for("json"), Some(Viewer::Text));
        for ext in ["html", "htm", "svg", "xhtml", "xml", "exe", "js", "zip", ""] {
            assert_eq!(viewer_for(ext), None, "{ext}");
        }
        let url = |s: &str| url::Url::parse(s).unwrap();
        assert!(viewable_file(&url("file:///C:/Users/me/Downloads/report.pdf")));
        assert!(viewable_file(&url("file:///home/me/Downloads/photo%20one.JPG")));
        assert!(!viewable_file(&url("file:///C:/Users/me/Downloads/page.html")));
        assert!(!viewable_file(&url("file:///C:/Users/me/Downloads/")));
        assert!(!viewable_file(&url("file://server/share/report.pdf")));
        assert!(!viewable_file(&url("https://example.com/report.pdf")));
    }

}

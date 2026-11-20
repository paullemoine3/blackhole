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
}

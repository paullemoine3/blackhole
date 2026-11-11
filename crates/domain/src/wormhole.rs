//! Wormhole: a text or a file sent to a paired device, over the local network, sealed with the
//! pair's key (see `handoff`). Pure rules: what may be sent, how it is cut in parts, and what a
//! received file may be called. What comes from the other device is checked here like anything
//! from outside: a paired device is trusted to be the user's, not to be well behaved.

/// Largest text and file, in bytes.
pub const TEXT_MAX: usize = 64 * 1024;
pub const FILE_MAX: u64 = 64 * 1024 * 1024;
/// Bytes of the content carried by one part (written in hexadecimal, a packet stays well
/// under the transport's limit).
pub const PART_BYTES: usize = 3_000;
/// Longest file name kept, in characters.
pub const NAME_MAX: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    File,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::File => "file",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "text" => Some(Self::Text),
            "file" => Some(Self::File),
            _ => None,
        }
    }

    /// The largest content of this kind.
    pub fn limit(self) -> u64 {
        match self {
            Self::Text => TEXT_MAX as u64,
            Self::File => FILE_MAX,
        }
    }
}

/// How many parts a content of `size` bytes travels in (an empty one still takes one).
pub fn parts(size: u64) -> u32 {
    let parts = size.div_ceil(PART_BYTES as u64).max(1);
    u32::try_from(parts).unwrap_or(u32::MAX)
}

/// The bytes of part `part` of a content of `size` bytes: `(offset, length)`.
pub fn part_range(size: u64, part: u32) -> Option<(u64, usize)> {
    if part >= parts(size) {
        return None;
    }
    let offset = u64::from(part) * PART_BYTES as u64;
    let length = (size - offset.min(size)).min(PART_BYTES as u64) as usize;
    Some((offset, length))
}

/// Whether what a device announces may be received: a known kind within its limit, cut the
/// way this side would cut it, with a SHA-256 to check it against.
pub fn acceptable(kind: &str, size: u64, announced_parts: u32, sha: &str) -> Option<Kind> {
    let kind = Kind::from_id(kind)?;
    let sound = size <= kind.limit()
        && (size > 0 || kind == Kind::File)
        && announced_parts == parts(size)
        && sha.len() == 64
        && sha.bytes().all(|b| b.is_ascii_hexdigit());
    sound.then_some(kind)
}

/// A received text as it may be shown and copied: control characters dropped (line breaks and
/// tabs stay), bounded.
pub fn clean_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().filter(|c| matches!(c, '\n' | '\t') || !c.is_control()) {
        if out.len() + c.len_utf8() > TEXT_MAX {
            break;
        }
        out.push(c);
    }
    out
}

/// The name a received file is saved under: the last component of what was announced, without
/// anything a file system or a shell could take for something else. Never empty, never a path.
pub fn safe_name(announced: &str) -> String {
    let last = announced.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut name: String = last
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c })
        .take(NAME_MAX)
        .collect();
    // No hidden file, no name Windows cuts short ("file. " is "file").
    name = name.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_owned();
    let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|c: char| c.is_ascii_digit()));
    if reserved {
        name.insert(0, '_');
    }
    if name.is_empty() {
        name.push_str("file");
    }
    name
}

/// A size for people: "512 B", "1.4 MB".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_is_cut_in_parts_that_cover_it_exactly() {
        assert_eq!(parts(0), 1);
        assert_eq!(parts(1), 1);
        assert_eq!(parts(PART_BYTES as u64), 1);
        assert_eq!(parts(PART_BYTES as u64 + 1), 2);
        let size = PART_BYTES as u64 * 3 + 10;
        let total: usize = (0..parts(size)).map(|p| part_range(size, p).unwrap().1).sum();
        assert_eq!(total as u64, size);
        assert_eq!(part_range(size, 3), Some((PART_BYTES as u64 * 3, 10)));
        assert_eq!(part_range(size, 4), None);
        assert_eq!(part_range(0, 0), Some((0, 0)));
    }

    #[test]
    fn only_sound_announcements_are_received() {
        let sha = "a".repeat(64);
        assert_eq!(acceptable("text", 12, 1, &sha), Some(Kind::Text));
        assert_eq!(acceptable("file", FILE_MAX, parts(FILE_MAX), &sha), Some(Kind::File));
        assert_eq!(acceptable("file", 0, 1, &sha), Some(Kind::File));
        assert_eq!(acceptable("text", 0, 1, &sha), None);
        assert_eq!(acceptable("text", TEXT_MAX as u64 + 1, parts(TEXT_MAX as u64 + 1), &sha), None);
        assert_eq!(acceptable("file", FILE_MAX + 1, parts(FILE_MAX + 1), &sha), None);
        assert_eq!(acceptable("file", 10_000, 1, &sha), None, "parts must match the size");
        assert_eq!(acceptable("file", 10, 1, "abc"), None);
        assert_eq!(acceptable("file", 10, 1, &"z".repeat(64)), None);
        assert_eq!(acceptable("folder", 10, 1, &sha), None);
    }

    #[test]
    fn a_received_file_name_is_never_a_path_nor_a_trap() {
        assert_eq!(safe_name("report.pdf"), "report.pdf");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name(r"C:\Users\me\AppData\evil.exe"), "evil.exe");
        assert_eq!(safe_name("a<b>:c\"|?*.txt"), "a_b__c____.txt");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("notes. "), "notes");
        assert_eq!(safe_name("con.txt"), "_con.txt");
        assert_eq!(safe_name("COM1"), "_COM1");
        assert_eq!(safe_name("comet.txt"), "comet.txt");
        assert_eq!(safe_name(""), "file");
        assert_eq!(safe_name("///"), "file");
        assert_eq!(safe_name(".."), "file");
        assert_eq!(safe_name("a\u{0}b\nc"), "a_b_c");
        assert_eq!(safe_name(&"é".repeat(300)).chars().count(), NAME_MAX);
    }

    #[test]
    fn a_received_text_is_bounded_and_printable() {
        assert_eq!(clean_text("line one\n\tline\u{7} two\u{0}"), "line one\n\tline two");
        assert_eq!(clean_text(&"é".repeat(TEXT_MAX)).len(), TEXT_MAX);
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(FILE_MAX), "64.0 MB");
    }
}

//! Wormhole: the two ends of a text or a file travelling between paired devices (the rules are
//! in `domain::wormhole`, the network in `handoff`). An [`Outbound`] hands out the parts of what
//! is sent; an [`Inbound`] takes the parts in any order, any number of times, and only gives the
//! result once its SHA-256 is the one announced. A file is written beside its final place and
//! renamed there when it is whole: a transfer that stops leaves nothing behind.

use blackhole_domain::i18n::message;
use blackhole_domain::wormhole::{self, Kind};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    out
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let digit = |b: u8| char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok());
    let mut out = Vec::with_capacity(text.len() / 2);
    let mut digits = text.bytes();
    while let Some(high) = digits.next() {
        // An odd number of digits is not a sequence of bytes.
        out.push(digit(high)? << 4 | digit(digits.next()?)?);
    }
    Some(out)
}

enum Source {
    Memory(Vec<u8>),
    File(File),
}

/// Something being sent to a device.
pub struct Outbound {
    pub id: u64,
    pub device: String,
    pub kind: Kind,
    /// The file's name (empty for a text).
    pub name: String,
    pub size: u64,
    pub sha: String,
    pub parts: u32,
    /// The next part to send, and how many the other device confirmed.
    pub next: u32,
    pub acked: u32,
    /// The other device said yes (a text needs no answer from its user, a file does).
    pub accepted: bool,
    pub since: Instant,
    source: Source,
}

impl Outbound {
    fn new(id: u64, device: &str, kind: Kind, name: String, size: u64, sha: String, source: Source) -> Self {
        Self {
            id,
            device: device.to_owned(),
            kind,
            name,
            size,
            sha,
            parts: wormhole::parts(size),
            next: 0,
            acked: 0,
            accepted: false,
            since: Instant::now(),
            source,
        }
    }

    /// A text to send; refused when empty or too long.
    pub fn text(id: u64, device: &str, text: &str) -> Result<Self, String> {
        let bytes = text.as_bytes().to_vec();
        if bytes.is_empty() || bytes.len() > wormhole::TEXT_MAX {
            return Err(message("msg.beam_text_size", &[]));
        }
        let sha = hex(&Sha256::digest(&bytes));
        Ok(Self::new(id, device, Kind::Text, String::new(), bytes.len() as u64, sha, Source::Memory(bytes)))
    }

    /// A file to send; refused when it is not a plain file or is too large. Its content is
    /// read once here for its SHA-256, then part by part as it is sent.
    pub fn file(id: u64, device: &str, path: &Path) -> Result<Self, String> {
        let unreadable = |_| message("msg.beam_unreadable", &[]);
        let meta = std::fs::metadata(path).map_err(unreadable)?;
        if !meta.is_file() {
            return Err(message("msg.beam_unreadable", &[]));
        }
        if meta.len() > wormhole::FILE_MAX {
            let max = wormhole::human_size(wormhole::FILE_MAX);
            return Err(message("msg.beam_too_large", &[("max", &max)]));
        }
        let mut file = File::open(path).map_err(unreadable)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0; 64 * 1024];
        let mut size = 0u64;
        loop {
            let n = file.read(&mut buf).map_err(unreadable)?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > wormhole::FILE_MAX {
                let max = wormhole::human_size(wormhole::FILE_MAX);
                return Err(message("msg.beam_too_large", &[("max", &max)]));
            }
            hasher.update(&buf[..n]);
        }
        let name = wormhole::safe_name(&path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()));
        Ok(Self::new(id, device, Kind::File, name, size, hex(&hasher.finalize()), Source::File(file)))
    }

    /// The content of a part, as it travels (hexadecimal). `None` when the part does not exist
    /// or the file can no longer be read (it changed since it was announced).
    pub fn part(&mut self, part: u32) -> Option<String> {
        let (offset, length) = wormhole::part_range(self.size, part)?;
        match &mut self.source {
            Source::Memory(bytes) => {
                let start = usize::try_from(offset).ok()?;
                bytes.get(start..start + length).map(hex)
            }
            Source::File(file) => {
                let mut buf = vec![0; length];
                file.seek(SeekFrom::Start(offset)).ok()?;
                file.read_exact(&mut buf).ok()?;
                Some(hex(&buf))
            }
        }
    }

    pub fn done(&self) -> bool {
        self.acked >= self.parts
    }
}

enum Sink {
    Memory(Vec<u8>),
    File { file: File, partial: PathBuf, dir: PathBuf },
}

/// What arrived, whole and checked.
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    Text(String),
    File { name: String, path: PathBuf },
}

/// Something being received from a device.
pub struct Inbound {
    pub id: u64,
    pub device: String,
    pub name: String,
    pub size: u64,
    sha: String,
    got: Vec<bool>,
    received: u32,
    sink: Sink,
    /// When a part last arrived (a transfer that stalls is dropped).
    pub last: Instant,
}

impl Inbound {
    /// A text, gathered in memory. The announcement was checked by `wormhole::acceptable`.
    pub fn text(id: u64, device: &str, size: u64, sha: &str) -> Self {
        let size = size.min(wormhole::TEXT_MAX as u64);
        Self {
            id,
            device: device.to_owned(),
            name: String::new(),
            size,
            sha: sha.to_ascii_lowercase(),
            got: vec![false; wormhole::parts(size) as usize],
            received: 0,
            sink: Sink::Memory(vec![0; size as usize]),
            last: Instant::now(),
        }
    }

    /// A file, written to `<dir>/<name>.<id>.part` until it is whole.
    pub fn file(id: u64, device: &str, name: &str, size: u64, sha: &str, dir: &Path) -> Result<Self, String> {
        let failed = |_| message("msg.beam_cannot_save", &[]);
        if size > wormhole::FILE_MAX {
            return Err(message("msg.beam_cannot_save", &[]));
        }
        let name = wormhole::safe_name(name);
        std::fs::create_dir_all(dir).map_err(failed)?;
        let partial = dir.join(format!("{name}.{id:016x}.part"));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&partial)
            .map_err(failed)?;
        Ok(Self {
            id,
            device: device.to_owned(),
            name,
            size,
            sha: sha.to_ascii_lowercase(),
            got: vec![false; wormhole::parts(size) as usize],
            received: 0,
            sink: Sink::File { file, partial, dir: dir.to_path_buf() },
            last: Instant::now(),
        })
    }

    /// Takes a part (again is fine). `Ok(true)` once every part is in; an error when the part
    /// is not one of this transfer or cannot be written: the transfer is then to be aborted.
    pub fn put(&mut self, part: u32, data: &str) -> Result<bool, String> {
        let broken = || message("msg.beam_interrupted", &[]);
        let (offset, length) = wormhole::part_range(self.size, part).ok_or_else(broken)?;
        let bytes = unhex(data).filter(|b| b.len() == length).ok_or_else(broken)?;
        let slot = part as usize;
        if !self.got.get(slot).copied().ok_or_else(broken)? {
            match &mut self.sink {
                Sink::Memory(all) => {
                    let start = offset as usize;
                    all.get_mut(start..start + length).ok_or_else(broken)?.copy_from_slice(&bytes);
                }
                Sink::File { file, .. } => {
                    file.seek(SeekFrom::Start(offset)).map_err(|_| broken())?;
                    file.write_all(&bytes).map_err(|_| broken())?;
                }
            }
            self.got[slot] = true;
            self.received += 1;
        }
        self.last = Instant::now();
        Ok(self.received as usize == self.got.len())
    }

    /// The result, once every part is in and the whole matches the announced SHA-256.
    pub fn finish(self) -> Result<Done, String> {
        let wrong = || message("msg.beam_corrupt", &[]);
        match self.sink {
            Sink::Memory(bytes) => {
                if hex(&Sha256::digest(&bytes)) != self.sha {
                    return Err(wrong());
                }
                String::from_utf8(bytes).map(|t| Done::Text(wormhole::clean_text(&t))).map_err(|_| wrong())
            }
            Sink::File { mut file, partial, dir } => {
                let checked = (|| {
                    file.flush().ok()?;
                    file.seek(SeekFrom::Start(0)).ok()?;
                    let mut hasher = Sha256::new();
                    let mut buf = vec![0; 64 * 1024];
                    loop {
                        let n = file.read(&mut buf).ok()?;
                        if n == 0 {
                            break;
                        }
                        hasher.update(&buf[..n]);
                    }
                    (hex(&hasher.finalize()) == self.sha).then_some(())
                })();
                drop(file);
                if checked.is_none() {
                    let _ = std::fs::remove_file(&partial);
                    return Err(wrong());
                }
                let path = crate::unique_path(&dir, &self.name);
                if std::fs::rename(&partial, &path).is_err() {
                    let _ = std::fs::remove_file(&partial);
                    return Err(message("msg.beam_cannot_save", &[]));
                }
                Ok(Done::File { name: self.name, path })
            }
        }
    }

    /// Drops the transfer and what was written of it.
    pub fn abort(self) {
        if let Sink::File { file, partial, .. } = self.sink {
            drop(file);
            let _ = std::fs::remove_file(partial);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Carries every part of `out` into `inbound`, in the order given.
    fn carry(out: &mut Outbound, inbound: &mut Inbound, order: impl Iterator<Item = u32>) -> bool {
        let mut whole = false;
        for part in order {
            let data = out.part(part).expect("a part");
            whole = inbound.put(part, &data).expect("accepted");
        }
        whole
    }

    #[test]
    fn a_text_arrives_whole_whatever_the_order_of_its_parts() {
        let text = "é😀 line\n".repeat(900);
        let mut out = Outbound::text(7, "d", &text).unwrap();
        assert!(out.parts > 2);
        assert!(wormhole::acceptable("text", out.size, out.parts, &out.sha).is_some());
        let mut inbound = Inbound::text(7, "d", out.size, &out.sha);
        // Backwards, and the first part twice.
        assert!(!carry(&mut out, &mut inbound, [0, 0].into_iter()));
        let parts = out.parts;
        assert!(carry(&mut out, &mut inbound, (0..parts).rev()));
        assert_eq!(inbound.finish(), Ok(Done::Text(text)));
        assert!(Outbound::text(1, "d", "").is_err());
        assert!(Outbound::text(1, "d", &"x".repeat(wormhole::TEXT_MAX + 1)).is_err());
    }

    #[test]
    fn a_file_is_saved_under_a_free_name_only_when_it_is_whole_and_right() {
        let (from, to) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let content: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
        let source = from.path().join("photo.bin");
        std::fs::write(&source, &content).unwrap();
        std::fs::write(to.path().join("photo.bin"), b"already here").unwrap();

        let mut out = Outbound::file(9, "d", &source).unwrap();
        assert_eq!((out.name.as_str(), out.size), ("photo.bin", 20_000));
        let mut inbound = Inbound::file(9, "d", "../photo.bin", out.size, &out.sha, to.path()).unwrap();
        let parts = out.parts;
        assert!(carry(&mut out, &mut inbound, 0..parts));
        let Ok(Done::File { name, path }) = inbound.finish() else { panic!("not saved") };
        assert_eq!(name, "photo.bin");
        assert_ne!(path, to.path().join("photo.bin"), "the file already there is kept");
        assert_eq!(path.parent(), Some(to.path()));
        assert_eq!(std::fs::read(&path).unwrap(), content);
        assert_eq!(std::fs::read(to.path().join("photo.bin")).unwrap(), b"already here");

        // A wrong SHA-256, a part of the wrong size, a stopped transfer: nothing is left.
        let mut out = Outbound::file(10, "d", &source).unwrap();
        let mut bad = Inbound::file(10, "d", "x.bin", out.size, &"0".repeat(64), to.path()).unwrap();
        let parts = out.parts;
        assert!(carry(&mut out, &mut bad, 0..parts));
        assert!(bad.finish().is_err());
        let mut odd = Inbound::file(11, "d", "y.bin", out.size, &out.sha, to.path()).unwrap();
        assert!(odd.put(0, "abcd").is_err());
        assert!(odd.put(parts, "").is_err());
        odd.abort();
        let left: Vec<String> = std::fs::read_dir(to.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left.len(), 2, "{left:?}");
        assert!(Outbound::file(1, "d", from.path()).is_err(), "a folder is not sent");
        assert!(Outbound::file(1, "d", &from.path().join("missing")).is_err());
    }
}

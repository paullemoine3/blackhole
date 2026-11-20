//! "Copy image" of the page menu (0.8.25): the shell downloads the picture the person
//! right-clicked (http(s) only, bounded), decodes it and gives the clipboard its pixels. The
//! engines have no API for it (WebView2, WKWebView), and a page cannot read a picture of another
//! site (a canvas drawn from it is tainted). "Save image" downloads it the same way and writes it
//! in the Downloads folder (`save`).

use crate::updater;
use std::time::Duration;

/// Larger files are refused (a photo of a camera is a few MB).
const MAX_BYTES: u64 = 30 * 1024 * 1024;
/// Pixels at most (the clipboard holds them raw: 4 bytes each).
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// A picture ready for the clipboard: width, height and RGBA bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Blocking: downloads the picture at `url`. `referer` (the page's origin) is sent as a browser
/// would: some sites refuse a picture asked without it.
pub fn fetch(url: &str, referer: Option<&str>) -> Result<Vec<u8>, String> {
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("not a web address".into());
    }
    let agent = ureq::AgentBuilder::new()
        .user_agent(concat!("Mozilla/5.0 BlackHole/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(30))
        .build();
    let mut req = agent.get(url).set("Accept", "image/avif,image/webp,image/png,image/*;q=0.8,*/*;q=0.5");
    if let Some(r) = referer.filter(|r| r.starts_with("https://") || r.starts_with("http://")) {
        req = req.set("Referer", r);
    }
    let resp = req.call().map_err(|e| e.to_string())?;
    updater::read_limited(resp, MAX_BYTES)
}

/// The extension of a picture, read from its first bytes (PNG, JPEG, GIF, WebP, AVIF, BMP, ICO,
/// TIFF, SVG). `None`: not a picture (an error page, a script), which is not saved.
pub fn extension_of(bytes: &[u8]) -> Option<&'static str> {
    if let Ok(format) = image::guess_format(bytes) {
        return format.extensions_str().first().copied();
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let head = head.trim_start_matches('\u{feff}').trim_start();
    (head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"))).then_some("svg")
}

/// Blocking: "Save image" of the page menu (0.8.25). Downloads the picture at `url` (as `fetch`)
/// and writes it in `dir` under a free name taken from its address, with the extension of its
/// real format (`domain::downloads::image_file_name`). Returns the file written.
pub fn save(url: &str, referer: Option<&str>, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let bytes = fetch(url, referer)?;
    let ext = extension_of(&bytes).ok_or("not a picture")?;
    let last = url::Url::parse(url)
        .ok()
        .and_then(|u| u.path_segments().and_then(|mut s| s.rfind(|p| !p.is_empty()).map(str::to_owned)))
        .unwrap_or_default();
    let name = blackhole_domain::downloads::image_file_name(&last, ext);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = crate::unique_path(dir, &name);
    // `create_new`: never over a file that appeared meanwhile.
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut file, &bytes).map_err(|e| {
        let _ = std::fs::remove_file(&path);
        e.to_string()
    })?;
    Ok(path)
}

/// Decodes PNG, JPEG, GIF (first frame), WebP, BMP and ICO into RGBA pixels.
pub fn decode(bytes: &[u8]) -> Result<Pixels, String> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("image too large".into());
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    Ok(Pixels { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn decodes_a_picture_into_rgba_pixels() {
        let p = decode(&png(3, 2)).unwrap();
        assert_eq!((p.width, p.height, p.rgba.len()), (3, 2, 24));
        assert_eq!(&p.rgba[4..8], &[1, 0, 7, 255]);
    }

    #[test]
    fn refuses_what_is_not_a_picture_or_not_on_the_web() {
        assert!(decode(b"<html>not an image</html>").is_err());
        assert!(fetch("file:///etc/passwd", None).is_err());
        assert!(fetch("data:image/png;base64,AAAA", None).is_err());
    }

    #[test]
    fn a_picture_is_known_by_its_bytes_not_its_address() {
        assert_eq!(extension_of(&png(1, 1)), Some("png"));
        assert_eq!(extension_of(b"\xff\xd8\xff\xe0\x00\x10JFIF"), Some("jpg"));
        assert_eq!(extension_of(b"GIF89a...."), Some("gif"));
        assert_eq!(extension_of(b"  <?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>"), Some("svg"));
        assert_eq!(extension_of(b"<!doctype html><title>404</title>"), None);
        assert_eq!(extension_of(b""), None);
    }
}

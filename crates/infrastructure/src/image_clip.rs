//! "Copy image" of the page menu (0.8.25): the shell downloads the picture the person
//! right-clicked (http(s) only, bounded), decodes it and gives the clipboard its pixels. The
//! engines have no API for it (WebView2, WKWebView), and a page cannot read a picture of another
//! site (a canvas drawn from it is tainted).

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
}

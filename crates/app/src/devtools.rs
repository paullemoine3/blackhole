//! Dev-only UI driver (`--features devtools`): replays `bh_script.txt` and saves
//! screenshots of the app window to `bh_shots/` (macOS and Windows).
//! The scratch directory is `/tmp` on macOS and the system temp dir elsewhere.
//!
//! Script lines: `wait MS`, `shot NAME`, `cmd JSON` (a `UiCommand`),
//! `eval chrome|bar|home|palette|peek|settings|easel|live|content JS`, `shots NAME` (settings window),
//! `shote NAME` (easel window), `quit`.

use super::*;
use std::fs;
use std::path::PathBuf;

pub fn scratch() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    }
}

fn shots_dir() -> PathBuf {
    scratch().join("bh_shots")
}

pub struct Dev {
    steps: Vec<String>,
    idx: usize,
    next_at: Instant,
}

impl Dev {
    pub fn load() -> Option<Self> {
        let raw = fs::read_to_string(scratch().join("bh_script.txt")).ok()?;
        let steps = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_owned)
            .collect();
        let _ = fs::create_dir_all(shots_dir());
        Some(Self {
            steps,
            idx: 0,
            next_at: Instant::now() + Duration::from_millis(500),
        })
    }
}

impl Running {
    /// True while the script still has steps to run (keeps the event loop ticking).
    pub fn dev_active(&self) -> bool {
        self.dev.as_ref().is_some_and(|d| d.idx < d.steps.len())
    }

    pub fn dev_tick(&mut self, event_loop: &ActiveEventLoop) {
        let Some(mut dev) = self.dev.take() else {
            return;
        };
        while Instant::now() >= dev.next_at && dev.idx < dev.steps.len() {
            let line = dev.steps[dev.idx].clone();
            dev.idx += 1;
            let delay = self.dev_run(&line, event_loop);
            dev.next_at = Instant::now() + delay;
        }
        self.dev = Some(dev);
    }

    fn dev_run(&mut self, line: &str, event_loop: &ActiveEventLoop) -> Duration {
        let (op, rest) = line.split_once(' ').unwrap_or((line, ""));
        let mut delay = Duration::from_millis(60);
        match op {
            "wait" => delay = Duration::from_millis(rest.trim().parse().unwrap_or(100)),
            "shot" => self.dev_capture(&self.window, rest.trim()),
            "shots" => {
                if let Some(win) = &self.settings {
                    self.dev_capture(&win.window, rest.trim());
                }
            }
            "shote" => {
                if let Some(win) = &self.easel {
                    self.dev_capture(&win.window, rest.trim());
                }
            }
            "cmd" => match serde_json::from_str::<UiCommand>(rest) {
                Ok(command) => self.handle_ui(command),
                Err(e) => eprintln!("[dev] bad cmd {rest}: {e}"),
            },
            "eval" => {
                let (target, js) = rest.split_once(' ').unwrap_or(("chrome", rest));
                let view = match target {
                    "settings" => match &self.settings {
                        Some(win) => &win.view,
                        None => return delay,
                    },
                    "easel" => match &self.easel {
                        Some(win) => &win.view,
                        None => return delay,
                    },
                    "etab" => self.easel_panel.view.as_ref().unwrap_or(&self.home),
                    "library" => self.library_panel.view.as_ref().unwrap_or(&self.home),
                    "live" => match self.live.values().next() {
                        Some(view) => view,
                        None => return delay,
                    },
                    "chrome" => &self.chrome,
                    "flyout" => &self.flyout,
                    "bar" => &self.bar,
                    "home" => &self.home,
                    "palette" => &self.palette,
                    "peek" => self.peek.as_ref().map_or(&self.home, |p| &p.page),
                    _ => self.active_view(),
                };
                let result =
                    view.evaluate_script_with_callback(js, |r| eprintln!("[dev] eval -> {r}"));
                if let Err(e) = result {
                    eprintln!("[dev] eval failed: {e}");
                }
            }
            // An error page that cannot be caused on demand (no network, wrong clock): shown in
            // the active tab for its screenshot.
            "errpage" => {
                use blackhole_domain::error_page::{CertProblem, ErrorPage};
                let page = match rest.trim() {
                    "offline" => ErrorPage::Offline,
                    "clock" => ErrorPage::ClockWrong,
                    "redirects" => ErrorPage::TooManyRedirects,
                    "http_only" => ErrorPage::HttpOnly,
                    "expired" => ErrorPage::Certificate(CertProblem::Expired),
                    _ => ErrorPage::Unreachable,
                };
                let tab = self.svc.workspace().active_tab().map(|t| t.id);
                if let (Some(tab), Ok(url)) = (tab, url::Url::parse("https://example.test/")) {
                    self.show_error(tab, url, page);
                }
            }
            "quit" => event_loop.exit(),
            _ => eprintln!("[dev] unknown step: {line}"),
        }
        delay
    }

    fn dev_capture(&self, window: &Window, name: &str) {
        let path = shots_dir().join(format!("{name}.png"));
        match capture_rgba(window) {
            Some((w, h, rgba)) => match image::RgbaImage::from_raw(w, h, rgba) {
                Some(img) if img.save(&path).is_ok() => {
                    eprintln!("[dev] saved {}", path.display());
                }
                _ => eprintln!("[dev] screenshot failed"),
            },
            None => eprintln!("[dev] screenshot failed"),
        }
    }
}

/// Grabs the window region in-process: CoreGraphics lets an app capture its own windows
/// without the Screen Recording permission that the `screencapture` tool would need.
#[cfg(target_os = "macos")]
fn capture_rgba(window: &Window) -> Option<(u32, u32, Vec<u8>)> {
    use core_graphics::{
        display::CGDisplay,
        geometry::{CGPoint, CGRect, CGSize},
        window::{kCGNullWindowID, kCGWindowImageDefault, kCGWindowListOptionOnScreenOnly},
    };
    let scale = window.scale_factor();
    let origin = window.inner_position().ok()?.to_logical::<f64>(scale);
    let size = window.inner_size().to_logical::<f64>(scale);
    let rect = CGRect::new(
        &CGPoint::new(origin.x, origin.y),
        &CGSize::new(size.width, size.height),
    );
    let shot = CGDisplay::screenshot(
        rect,
        kCGWindowListOptionOnScreenOnly,
        kCGNullWindowID,
        kCGWindowImageDefault,
    )?;
    let (w, h, stride) = (shot.width(), shot.height(), shot.bytes_per_row());
    let data = shot.data();
    let bytes = data.bytes();
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        // BGRA rows padded to `stride`; an indexed loop keeps the crate's 1.80 floor (`as_chunks` is 1.88).
        for x in 0..w {
            let at = y * stride + x * 4;
            rgba.extend_from_slice(&[bytes[at + 2], bytes[at + 1], bytes[at], 255]);
        }
    }
    Some((w as u32, h as u32, rgba))
}

/// Crops the window's client area out of a monitor capture (the web views are composed by
/// the compositor, so the window must be visible and not covered).
#[cfg(windows)]
fn capture_rgba(window: &Window) -> Option<(u32, u32, Vec<u8>)> {
    let origin = window.inner_position().ok()?;
    let size = window.inner_size();
    let monitor = xcap::Monitor::from_point(origin.x, origin.y).ok()?;
    let shot = monitor.capture_image().ok()?;
    let (left, top) = (origin.x - monitor.x().ok()?, origin.y - monitor.y().ok()?);
    let (left, top) = (u32::try_from(left).ok()?, u32::try_from(top).ok()?);
    let w = size.width.min(shot.width().saturating_sub(left));
    let h = size.height.min(shot.height().saturating_sub(top));
    if w == 0 || h == 0 {
        return None;
    }
    let (full_w, pixels) = (shot.width() as usize, shot.into_raw());
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in top..top + h {
        let start = (y as usize * full_w + left as usize) * 4;
        rgba.extend_from_slice(&pixels[start..start + w as usize * 4]);
    }
    Some((w, h, rgba))
}

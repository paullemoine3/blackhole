//! Dev-only UI driver (`--features devtools`): replays `/tmp/bh_script.txt` and saves
//! screenshots of the app window to `/tmp/bh_shots/` (macOS only).
//!
//! Script lines: `wait MS`, `shot NAME`, `cmd JSON` (a `UiCommand`),
//! `eval chrome|bar|home|palette|peek|settings|content JS`, `shots NAME` (settings window), `quit`.

use super::*;
use std::fs;

const SCRIPT: &str = "/tmp/bh_script.txt";
const SHOTS: &str = "/tmp/bh_shots";

pub struct Dev {
    steps: Vec<String>,
    idx: usize,
    next_at: Instant,
}

impl Dev {
    pub fn load() -> Option<Self> {
        let raw = fs::read_to_string(SCRIPT).ok()?;
        let steps = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_owned)
            .collect();
        let _ = fs::create_dir_all(SHOTS);
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
                    "chrome" => &self.chrome,
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
            "quit" => event_loop.exit(),
            _ => eprintln!("[dev] unknown step: {line}"),
        }
        delay
    }

    /// Grabs the window region in-process: CoreGraphics lets an app capture its own windows
    /// without the Screen Recording permission that the `screencapture` tool would need.
    fn dev_capture(&self, window: &Window, name: &str) {
        use core_graphics::{
            display::CGDisplay,
            geometry::{CGPoint, CGRect, CGSize},
            window::{kCGNullWindowID, kCGWindowImageDefault, kCGWindowListOptionOnScreenOnly},
        };
        let scale = window.scale_factor();
        let Ok(origin) = window.inner_position() else {
            return;
        };
        let origin = origin.to_logical::<f64>(scale);
        let size = window.inner_size().to_logical::<f64>(scale);
        let rect = CGRect::new(
            &CGPoint::new(origin.x, origin.y),
            &CGSize::new(size.width, size.height),
        );
        let path = format!("{SHOTS}/{name}.png");
        let Some(shot) = CGDisplay::screenshot(
            rect,
            kCGWindowListOptionOnScreenOnly,
            kCGNullWindowID,
            kCGWindowImageDefault,
        ) else {
            eprintln!("[dev] screenshot failed");
            return;
        };
        let (w, h, stride) = (shot.width(), shot.height(), shot.bytes_per_row());
        let data = shot.data();
        let bytes = data.bytes();
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for px in bytes[y * stride..y * stride + w * 4].chunks_exact(4) {
                rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
        }
        match image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
            Some(img) if img.save(&path).is_ok() => eprintln!("[dev] saved {path}"),
            _ => eprintln!("[dev] screenshot failed"),
        }
    }
}

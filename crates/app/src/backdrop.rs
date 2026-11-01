//! Paints the window's own client area in the shell colour.
//! On Windows the native window has no background brush, so whatever the child web views do not
//! cover (while they are being resized, or after a minimize/restore) flashes white.

use std::{num::NonZeroU32, rc::Rc};
use winit::window::Window;

/// Same colour as the web views' shell background (16, 15, 23).
const SHELL: u32 = 0x0010_0F17;

pub struct Backdrop {
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    window: Rc<Window>,
}

impl Backdrop {
    pub fn new(window: &Rc<Window>) -> Option<Self> {
        let context = softbuffer::Context::new(window.clone()).ok()?;
        let surface = softbuffer::Surface::new(&context, window.clone()).ok()?;
        let mut backdrop = Self {
            surface,
            window: window.clone(),
        };
        backdrop.paint();
        Some(backdrop)
    }

    pub fn paint(&mut self) {
        let size = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if self.surface.resize(w, h).is_err() {
            return;
        }
        if let Ok(mut buffer) = self.surface.buffer_mut() {
            buffer.fill(SHELL);
            let _ = buffer.present();
        }
    }
}

//! Copying from the menus (0.8.25): text goes to the system clipboard through the shell
//! (`arboard`), not through `execCommand('copy')` in the command bar, which fails when its view
//! is not focused by the system; "Copy image" downloads the picture on a thread and gives the
//! clipboard its pixels (`blackhole_infrastructure::image_clip`).

use std::sync::mpsc::{channel, Receiver, Sender};

use blackhole_domain::TabId;
use blackhole_infrastructure::image_clip::{self, Pixels};
use url::Url;

use crate::i18n::t;
use crate::ipc::UiCommand;
use crate::shell::notices::Notice;
use crate::state::Running;

pub struct Clips {
    tx: Sender<Result<Pixels, String>>,
    rx: Receiver<Result<Pixels, String>>,
    /// A picture is being downloaded (one at a time).
    busy: bool,
}

impl Clips {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, busy: false }
    }
}

/// Puts `text` on the system clipboard.
pub(crate) fn copy_text(text: &str) -> bool {
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_owned())).is_ok()
}

fn copy_pixels(p: Pixels) -> bool {
    let image = arboard::ImageData {
        width: p.width as usize,
        height: p.height as usize,
        bytes: p.rgba.into(),
    };
    arboard::Clipboard::new().and_then(|mut c| c.set_image(image)).is_ok()
}

impl Running {
    pub(crate) fn handle_clipboard(&mut self, command: UiCommand) {
        match command {
            UiCommand::CopyText { text, quiet } => {
                // A copy too large for a menu is refused rather than cut silently.
                let ok = text.len() <= blackhole_domain::selection::MAX_COPY * 4 && copy_text(&text);
                if !ok {
                    self.notify(Notice::Problem, &t("toast.copy_failed"), serde_json::Value::Null);
                } else if !quiet {
                    self.toast(&t("toast.copied"));
                }
            }
            UiCommand::CopyImage { url, tab } => self.copy_image(TabId(tab), &url),
            _ => {}
        }
    }

    /// Mod+Shift+C: the address of the page shown, clean when the Shields clean links.
    pub(crate) fn copy_page_link(&mut self) {
        let Some(url) = self
            .svc
            .workspace()
            .active_tab()
            .map(|t| t.url.clone())
            .filter(|u| matches!(u.scheme(), "http" | "https"))
        else {
            return;
        };
        let url = if self.svc.settings().shields.clean_links {
            blackhole_domain::clean_link::clean(&url).unwrap_or(url)
        } else {
            url
        };
        if copy_text(url.as_str()) {
            self.toast(&t("toast.link_copied"));
        } else {
            self.notify(Notice::Problem, &t("toast.copy_failed"), serde_json::Value::Null);
        }
    }

    /// Downloads the picture with the page's origin as `Referer` (as the page itself asked it).
    fn copy_image(&mut self, tab: TabId, url: &str) {
        let Some(url) = Url::parse(url).ok().filter(|u| matches!(u.scheme(), "http" | "https")) else {
            return;
        };
        if self.clips.busy {
            return;
        }
        let referer = self
            .tabs
            .get(&tab)
            .and_then(|v| v.url().ok())
            .and_then(|raw| Url::parse(&raw).ok())
            .filter(|page| !(page.scheme() == "https" && url.scheme() == "http"))
            .map(|page| page.origin().ascii_serialization() + "/");
        self.clips.busy = true;
        let tx = self.clips.tx.clone();
        let url = url.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(image_clip::fetch(&url, referer.as_deref()).and_then(|b| image_clip::decode(&b)));
        });
    }

    /// Called every turn of the event loop: a picture downloaded goes to the clipboard.
    pub(crate) fn poll_clipboard(&mut self) {
        while let Ok(result) = self.clips.rx.try_recv() {
            self.clips.busy = false;
            match result.map(copy_pixels) {
                Ok(true) => self.toast(&t("toast.image_copied")),
                Ok(false) => self.notify(Notice::Problem, &t("toast.copy_failed"), serde_json::Value::Null),
                Err(why) => {
                    tracing::warn!("copy image: {why}");
                    self.notify(Notice::Problem, &t("toast.image_copy_failed"), serde_json::Value::Null);
                }
            }
        }
    }
}

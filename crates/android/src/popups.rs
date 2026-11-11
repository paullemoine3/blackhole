//! Windows opened by pages (`window.open`, `target=_blank`), with the desktop's rule
//! (`domain::popup::new_window`): a sign-in popup (Google, Microsoft, Apple, GitHub…, or a
//! blank or sized window a script then drives) gets a view of its own that keeps
//! `window.opener`, on its opener's profile; anything else loads in the tab, as before.
//!
//! A popup is a session tab over its opener: never in the tab list, the history or the icons;
//! it goes when it closes itself (`window.close()`), on back, or when another tab is shown.

use crate::bridge::Effect;
use crate::core::Core;
use blackhole_domain::popup::{new_window, NewWindow};
use url::Url;

/// Popup ids start here, above the private tabs' (and still exact as a JavaScript number).
pub(crate) const POPUP_BASE: u64 = 1 << 52;

#[derive(Debug)]
pub(crate) struct Popup {
    pub id: u64,
    pub opener: u64,
    /// The page asked for a size or a position: the mark of a script popup.
    sized: bool,
    /// Its first address was judged.
    decided: bool,
}

impl Core {
    pub(crate) fn is_popup(tab: u64) -> bool {
        tab >= POPUP_BASE
    }

    pub(crate) fn opener_of(&self, tab: u64) -> Option<u64> {
        self.popups.iter().find(|p| p.id == tab).map(|p| p.opener)
    }

    /// `onCreateWindow` of the page on screen, on a user gesture. No effect: nothing opens.
    pub(crate) fn window_asked(&mut self, opener: u64, sized: bool) -> Vec<Effect> {
        // Only the tab on screen, and never a popup of a popup.
        if self.current != Some(opener) || Self::is_popup(opener) || !self.exists(opener) {
            return Vec::new();
        }
        let id = self.next_popup;
        self.next_popup += 1;
        self.popups.push(Popup {
            id,
            opener,
            sized,
            decided: false,
        });
        let mut out = vec![Effect::Popup {
            tab: id,
            opener,
            profile: self.profile_of(opener),
            script: self.page_script(),
            desktop: self.desktop.contains(&opener),
        }];
        // The opener stays alive under it: it was the last tab shown (`MAX_LIVE` is at least 2).
        out.extend(self.show(id, None));
        out.push(self.state());
        out
    }

    /// The first address a popup loads decides what it is. `None`: it goes on as a popup
    /// (or `tab` is no undecided popup); otherwise the popup is gone and these effects follow.
    pub(crate) fn popup_first_address(&mut self, tab: u64, raw: &str) -> Option<Vec<Effect>> {
        let popup = self.popups.iter_mut().find(|p| p.id == tab && !p.decided)?;
        popup.decided = true;
        let (opener, sized) = (popup.opener, popup.sized);
        match new_window(raw, sized) {
            NewWindow::Popup => None,
            NewWindow::Deny => Some(self.close_popup(tab)),
            // A plain link or `window.open(url)`: it loads in the tab that asked, as it did
            // before popups existed here (filtered like any navigation).
            NewWindow::Tab => {
                let mut out = self.close_popup(tab);
                if let Some(going) = Url::parse(raw).ok().and_then(|u| self.evaluate(&u).ok()) {
                    out.push(Effect::Load {
                        tab: opener,
                        url: going.to_string(),
                    });
                }
                Some(out)
            }
        }
    }

    /// Closes a popup: its opener comes back when the popup was on screen.
    pub(crate) fn close_popup(&mut self, tab: u64) -> Vec<Effect> {
        let Some(at) = self.popups.iter().position(|p| p.id == tab) else {
            return Vec::new();
        };
        let opener = self.popups.remove(at).opener;
        let mut out = self.forget_view(tab);
        if self.current == Some(tab) {
            out.extend(if self.exists(opener) { self.show(opener, None) } else { self.home() });
        }
        out.push(self.state());
        out
    }

    /// Popups other than `keep` go: a popup lives only while it is on screen.
    pub(crate) fn drop_popups(&mut self, keep: Option<u64>) -> Vec<Effect> {
        let gone: Vec<u64> = self.popups.iter().map(|p| p.id).filter(|id| Some(*id) != keep).collect();
        self.popups.retain(|p| Some(p.id) == keep);
        gone.into_iter().flat_map(|id| self.forget_view(id)).collect()
    }

    /// Everything kept for a view that is destroyed for good, and its `Drop`.
    pub(crate) fn forget_view(&mut self, tab: u64) -> Vec<Effect> {
        self.live.remove(tab);
        self.errors.remove(&tab);
        self.upgraded.remove(&tab);
        self.pages.remove(&tab);
        self.desktop.remove(&tab);
        self.crashes.remove(&tab);
        let mut out = self.permissions_of_closed(tab);
        out.extend(self.auth_of_closed(tab));
        out.push(Effect::Drop { tab });
        out
    }
}

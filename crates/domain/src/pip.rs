//! Picture in Picture: what a video leaving its floating window means.

/// How a video left its floating window, as the shell knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipLeft {
    /// The shell floated this video itself (tab switch, lost focus, the player card); a page
    /// cannot ask for a tab switch on its own.
    pub floated_by_shell: bool,
    /// The shell has just brought it back itself (back on its tab, the card's buttons).
    pub left_by_shell: bool,
    /// The video is paused: the window's close button pauses it, "back to tab" does not.
    pub paused: bool,
    /// Its tab is already the one on screen.
    pub tab_shown: bool,
    /// The engine pauses a video whose window is closed (WebKit), so a playing video means
    /// "back to tab". WebView2 does not: there the two buttons cannot be told apart.
    pub close_pauses: bool,
}

/// Whether the user pressed the floating window's "back to tab" button, so its tab should be
/// shown. Closing the window (it pauses the video) leaves everything as it is.
pub fn back_to_tab(left: PipLeft) -> bool {
    left.close_pauses
        && left.floated_by_shell
        && !left.left_by_shell
        && !left.paused
        && !left.tab_shown
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK: PipLeft = PipLeft {
        floated_by_shell: true,
        left_by_shell: false,
        paused: false,
        tab_shown: false,
        close_pauses: true,
    };

    #[test]
    fn back_to_tab_button_shows_the_tab() {
        assert!(back_to_tab(BACK));
    }

    #[test]
    fn close_button_pauses_and_switches_nothing() {
        assert!(!back_to_tab(PipLeft { paused: true, ..BACK }));
    }

    #[test]
    fn the_shell_bringing_it_back_is_no_request() {
        assert!(!back_to_tab(PipLeft { left_by_shell: true, ..BACK }));
    }

    #[test]
    fn a_page_cannot_switch_tabs_by_itself() {
        assert!(!back_to_tab(PipLeft { floated_by_shell: false, ..BACK }));
    }

    #[test]
    fn engines_that_do_not_pause_on_close_never_switch() {
        assert!(!back_to_tab(PipLeft { close_pauses: false, ..BACK }));
    }

    #[test]
    fn nothing_to_do_when_the_tab_is_on_screen() {
        assert!(!back_to_tab(PipLeft { tab_shown: true, ..BACK }));
    }
}

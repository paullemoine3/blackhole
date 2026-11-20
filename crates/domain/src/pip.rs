//! Picture in Picture: what a video leaving its floating window means.

use std::time::Duration;

/// How long after the shell's probe stopped seeing a floating video the page's own "left
/// Picture in Picture" still counts as that video. WebKit switches the video back to inline
/// as soon as "back to tab" is pressed, but only fires `webkitpresentationmodechanged` once the
/// window's closing animation is over: the probe (every 500 ms while a video floats) usually
/// sees it first. Longer than that animation, short enough not to pair two unrelated events.
pub const LEFT_EVENT_GRACE: Duration = Duration::from_millis(3000);

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
    /// The engine pauses this video when its window is closed, so a playing video means "back
    /// to tab". WebKit always does; WebView2 does for a file or a network video but cannot
    /// pause a live stream: there the two buttons cannot be told apart.
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

/// Share of the browser window another window must hide before a video floats.
pub const COVERED_SHARE: f64 = 0.5;

/// Share of the rectangle `own` (`x, y, w, h`) that `other` covers.
pub fn overlap_share(own: (f64, f64, f64, f64), other: (f64, f64, f64, f64)) -> f64 {
    let w = (own.0 + own.2).min(other.0 + other.2) - own.0.max(other.0);
    let h = (own.1 + own.3).min(other.1 + other.3) - own.1.max(other.1);
    if own.2 <= 0.0 || own.3 <= 0.0 || w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    w * h / (own.2 * own.3)
}

/// Whether the window `own` is hidden enough by the frontmost window of another application
/// for its video to float (`x, y, w, h`, both in the same unit).
pub fn window_covered(own: (f64, f64, f64, f64), front: (f64, f64, f64, f64)) -> bool {
    overlap_share(own, front) >= COVERED_SHARE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_share_is_the_hidden_part_of_the_window() {
        let own = (0.0, 0.0, 1000.0, 800.0);
        assert_eq!(overlap_share(own, (1200.0, 0.0, 800.0, 600.0)), 0.0);
        assert_eq!(overlap_share(own, (-10.0, -10.0, 2000.0, 1000.0)), 1.0);
        assert!((overlap_share(own, (500.0, 0.0, 1000.0, 800.0)) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_window_is_covered_from_half_of_it_hidden() {
        let own = (0.0, 0.0, 1000.0, 800.0);
        assert!(window_covered(own, (500.0, 0.0, 1000.0, 800.0)));
        assert!(!window_covered(own, (600.0, 0.0, 1000.0, 800.0)));
        assert!(!window_covered((0.0, 0.0, 0.0, 0.0), own));
    }

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
    fn a_video_the_engine_cannot_pause_on_close_never_switches() {
        assert!(!back_to_tab(PipLeft { close_pauses: false, ..BACK }));
    }

    #[test]
    fn nothing_to_do_when_the_tab_is_on_screen() {
        assert!(!back_to_tab(PipLeft { tab_shown: true, ..BACK }));
    }
}

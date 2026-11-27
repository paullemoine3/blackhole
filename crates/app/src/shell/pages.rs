//! The shell's pages, embedded in the binary.

pub(crate) const CHROME_HTML: &str = include_str!("../../ui/index.html");

/// The start page, with the welcome tour appended (onboarding.html closes the document).
pub(crate) const HOME_HTML: &str = concat!(include_str!("../../ui/home.html"), include_str!("../../ui/onboarding.html"));

pub(crate) const BAR_HTML: &str = include_str!("../../ui/bar.html");

pub(crate) const PALETTE_HTML: &str = include_str!("../../ui/palette.html");

pub(crate) const SPLITBAR_HTML: &str = include_str!("../../ui/splitbar.html");

pub(crate) const PEEK_HTML: &str = include_str!("../../ui/peek.html");

pub(crate) const DRAG_HTML: &str = include_str!("../../ui/drag.html");

pub(crate) const HOTZONE_HTML: &str = include_str!("../../ui/hotzone.html");

#[cfg(windows)]
pub(crate) const EXT_POPUP_HTML: &str = include_str!("../../ui/extpopup.html");

pub(crate) const SETTINGS_HTML: &str = include_str!("../../ui/settings.html");

pub(crate) const EASEL_HTML: &str = include_str!("../../ui/easel.html");

pub(crate) const LIBRARY_HTML: &str = include_str!("../../ui/library.html");

//! BlackHole for Android: the Rust core behind the Kotlin shell (`android/`).
//!
//! The shell owns the `android.webkit.WebView`s, gestures and system integration; this crate
//! owns the browser's state and rules, reusing `domain`, `application` and `infrastructure`
//! unchanged. They talk in JSON (see [`bridge`]): the shell interface sends `MobileCommand`s,
//! pages `PageEvent`s (untrusted), the shell `NativeEvent`s, and the core answers `Effect`s.
//! Everything but [`jni`] builds and is tested on the host.

pub mod bridge;
mod core;
pub mod devices;
mod live;

pub use crate::core::{error_page, Blocker, Core, MobilePrefs, MAX_LIVE};
pub use bridge::{Effect, MobileCommand, NativeEvent, PageEvent};
pub use devices::Network;

#[cfg(target_os = "android")]
mod jni;

#[cfg(test)]
mod tests;

//! Passkeys (WebAuthn) on macOS. WebKit only lets a browser built on WKWebView reach the
//! system's passkeys (iCloud Keychain, the 1Password or Dashlane apps) with an Apple entitlement
//! (`com.apple.developer.web-browser.public-key-credential`) that needs a Developer ID signature;
//! without it a request is refused at once, with nothing on screen. `ui/passkeys.js` then tells
//! the page there is no platform authenticator (password manager extensions take over) and
//! reports a refused request; these rules decide when the user hears about it.

use std::collections::BTreeMap;

/// A site whose passkey request was refused is told about again after this long, at most.
pub const TELL_AGAIN_AFTER_MS: u64 = 10 * 60 * 1000;
/// Sites remembered at most (the oldest is forgotten first).
const MAX_SITES: usize = 64;

/// The sites already told that their passkey request could not be served (this session only).
#[derive(Debug, Default)]
pub struct Refusals {
    told: BTreeMap<String, u64>,
}

impl Refusals {
    /// A request of `site` was refused at `now_ms`: true when the user should be told (once a
    /// site, again after `TELL_AGAIN_AFTER_MS`). A page without a site is never worth a word.
    pub fn should_tell(&mut self, site: &str, now_ms: u64) -> bool {
        if site.is_empty() {
            return false;
        }
        if let Some(&at) = self.told.get(site) {
            if now_ms.saturating_sub(at) < TELL_AGAIN_AFTER_MS {
                return false;
            }
        }
        if self.told.len() >= MAX_SITES && !self.told.contains_key(site) {
            if let Some(oldest) = self.told.iter().min_by_key(|(_, at)| **at).map(|(s, _)| s.clone()) {
                self.told.remove(&oldest);
            }
        }
        self.told.insert(site.to_owned(), now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_is_told_once_then_again_after_a_while() {
        let mut r = Refusals::default();
        assert!(r.should_tell("github.com", 1_000));
        assert!(!r.should_tell("github.com", 1_000 + 5_000), "a retry says nothing more");
        assert!(r.should_tell("google.com", 2_000), "another site is told");
        assert!(r.should_tell("github.com", 1_000 + TELL_AGAIN_AFTER_MS));
        assert!(!r.should_tell("", 3_000), "no site, no word");
    }

    #[test]
    fn the_list_stays_bounded() {
        let mut r = Refusals::default();
        for i in 0..200u64 {
            assert!(r.should_tell(&format!("s{i}.test"), i));
        }
        assert!(r.told.len() <= MAX_SITES);
        assert!(r.should_tell("s0.test", 300), "the oldest were forgotten");
    }
}

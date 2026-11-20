//! Encrypted DNS: the resolvers offered, the address of a custom one, and how each engine is
//! told (WebView2's own DNS over HTTPS; on macOS a system DNS profile the user installs).

/// A DNS resolver offered in Settings: its DNS over HTTPS address and DNS over TLS host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolver {
    pub id: &'static str,
    pub name: &'static str,
    pub doh: &'static str,
    pub dot: &'static str,
}

/// The resolvers offered, the default first: Quad9 (a Swiss non-profit, no logs, blocks
/// malware domains), then Mullvad, Cloudflare and AdGuard.
pub const RESOLVERS: [Resolver; 4] = [
    Resolver { id: "quad9", name: "Quad9", doh: "https://dns.quad9.net/dns-query", dot: "dns.quad9.net" },
    Resolver { id: "mullvad", name: "Mullvad", doh: "https://dns.mullvad.net/dns-query", dot: "dns.mullvad.net" },
    Resolver {
        id: "cloudflare",
        name: "Cloudflare",
        doh: "https://cloudflare-dns.com/dns-query",
        dot: "one.one.one.one",
    },
    Resolver {
        id: "adguard",
        name: "AdGuard",
        doh: "https://dns.adguard-dns.com/dns-query",
        dot: "dns.adguard-dns.com",
    },
];

/// The resolver of a new profile.
pub const DEFAULT_RESOLVER: &str = "quad9";

pub fn resolver(id: &str) -> Option<&'static Resolver> {
    RESOLVERS.iter().find(|r| r.id == id)
}

/// The DNS over HTTPS address of a setting: a known resolver's id, or the address of a custom
/// one (`https://`, a host, no credentials, no spaces). `None`: nothing usable.
pub fn doh_template(choice: &str) -> Option<String> {
    if let Some(r) = resolver(choice) {
        return Some(r.doh.to_owned());
    }
    let url = url::Url::parse(choice.trim()).ok()?;
    let usable = url.scheme() == "https"
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && !choice.trim().contains(char::is_whitespace);
    usable.then(|| choice.trim().to_owned())
}

/// The name shown for a setting: the resolver's, or the host of a custom address.
pub fn display_name(choice: &str) -> String {
    match resolver(choice) {
        Some(r) => r.name.to_owned(),
        None => url::Url::parse(choice)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| choice.to_owned()),
    }
}

/// Escapes a value of a Chromium feature parameter (`Feature:name/value`): `/`, `:`, `,`, `%`
/// and anything outside plain ASCII letters and digits.
fn feature_escape(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// WebView2 (Chromium) switches that turn its own DNS over HTTPS on with `template`. Falls back
/// to the system's DNS when the resolver cannot be reached (an office network that blocks it):
/// pages keep loading.
pub fn chromium_switch(template: &str) -> String {
    format!(
        "--enable-features=DnsOverHttps:Fallback/true/Templates/{}",
        feature_escape(template)
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// A stable UUID-shaped identifier made from `seed` (FNV-1a), so installing the profile again
/// with the same resolver gives the same payload.
fn uuid_of(seed: &str) -> String {
    let mut a: u64 = 0xcbf2_9ce4_8422_2325;
    let mut b: u64 = 0x8422_2325_cbf2_9ce4;
    for byte in seed.bytes() {
        a = (a ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        b = (b ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3 ^ 0x5bd1_e995);
    }
    let hex = format!("{a:016X}{b:016X}");
    format!("{}-{}-4{}-8{}-{}", &hex[0..8], &hex[8..12], &hex[13..16], &hex[17..20], &hex[20..32])
}

/// Identifier of the macOS profile: the same for every resolver, so a new one replaces it.
pub const PROFILE_ID: &str = "app.blackhole.dns";

/// A macOS configuration profile (`.mobileconfig`) that sets the system's DNS over HTTPS
/// resolver: WebKit uses the system's DNS, which no app setting can change. `title` is its
/// name in System Settings.
pub fn mobileconfig(template: &str, title: &str) -> String {
    let (url, title) = (xml_escape(template), xml_escape(title));
    let inner = uuid_of(&format!("{template}#dns"));
    let outer = uuid_of(&format!("{template}#profile"));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <array>
    <dict>
      <key>DNSSettings</key>
      <dict>
        <key>DNSProtocol</key>
        <string>HTTPS</string>
        <key>ServerURL</key>
        <string>{url}</string>
      </dict>
      <key>PayloadDisplayName</key>
      <string>{title}</string>
      <key>PayloadIdentifier</key>
      <string>{PROFILE_ID}.settings</string>
      <key>PayloadType</key>
      <string>com.apple.dnsSettings.managed</string>
      <key>PayloadUUID</key>
      <string>{inner}</string>
      <key>PayloadVersion</key>
      <integer>1</integer>
    </dict>
  </array>
  <key>PayloadDisplayName</key>
  <string>{title}</string>
  <key>PayloadIdentifier</key>
  <string>{PROFILE_ID}</string>
  <key>PayloadRemovalDisallowed</key>
  <false/>
  <key>PayloadType</key>
  <string>Configuration</string>
  <key>PayloadUUID</key>
  <string>{outer}</string>
  <key>PayloadVersion</key>
  <integer>1</integer>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_resolver_or_a_custom_https_address() {
        assert_eq!(doh_template("quad9").as_deref(), Some("https://dns.quad9.net/dns-query"));
        assert_eq!(resolver(DEFAULT_RESOLVER).map(|r| r.id), Some("quad9"));
        assert_eq!(
            doh_template(" https://dns.nextdns.io/abc123 ").as_deref(),
            Some("https://dns.nextdns.io/abc123")
        );
        for bad in ["http://dns.example/dns-query", "https://user:pw@dns.example/q", "nonsense", "", "https://dns.example/a b"] {
            assert_eq!(doh_template(bad), None, "{bad}");
        }
        assert_eq!(display_name("mullvad"), "Mullvad");
        assert_eq!(display_name("https://dns.nextdns.io/abc123"), "dns.nextdns.io");
    }

    #[test]
    fn webview2_gets_an_escaped_template_and_keeps_a_fallback() {
        let switch = chromium_switch("https://dns.quad9.net/dns-query{?dns}");
        assert_eq!(
            switch,
            "--enable-features=DnsOverHttps:Fallback/true/Templates/https%3A%2F%2Fdns.quad9.net%2Fdns-query%7B%3Fdns%7D"
        );
        assert!(!switch.contains(' '));
    }

    #[test]
    fn the_macos_profile_names_the_resolver_and_is_stable() {
        let a = mobileconfig("https://dns.quad9.net/dns-query", "BlackHole – DNS (Quad9)");
        assert!(a.contains("<string>https://dns.quad9.net/dns-query</string>"));
        assert!(a.contains("com.apple.dnsSettings.managed"));
        assert!(a.contains("<string>app.blackhole.dns</string>"));
        assert_eq!(a, mobileconfig("https://dns.quad9.net/dns-query", "BlackHole – DNS (Quad9)"));
        assert_ne!(a, mobileconfig("https://dns.mullvad.net/dns-query", "x"));
        // Markup in a custom address or name cannot break the file.
        assert!(mobileconfig("https://a.example/?a=1&b=<x>", "<b>").contains("a=1&amp;b=&lt;x&gt;"));
        let uuid = uuid_of("seed");
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.matches('-').count(), 4);
    }
}

//! Email aliases: one address per site, made by a relay service the user already has. BlackHole
//! relays nothing itself; these are the rules (which services, which servers, what is kept).

use url::Url;
use zeroize::Zeroize;

/// A service aliases can be made with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    SimpleLogin,
    Addy,
    FirefoxRelay,
}

impl Provider {
    pub const ALL: [Self; 3] = [Self::SimpleLogin, Self::Addy, Self::FirefoxRelay];

    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::SimpleLogin => "simplelogin",
            Self::Addy => "addy",
            Self::FirefoxRelay => "relay",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The product's own name (never translated).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::SimpleLogin => "SimpleLogin",
            Self::Addy => "Addy.io",
            Self::FirefoxRelay => "Firefox Relay",
        }
    }

    #[must_use]
    pub fn default_server(self) -> &'static str {
        match self {
            Self::SimpleLogin => "https://app.simplelogin.io",
            Self::Addy => "https://app.addy.io",
            Self::FirefoxRelay => "https://relay.firefox.com",
        }
    }

    /// The page of the service's own site where the user finds their API key.
    #[must_use]
    pub fn key_page(self) -> &'static str {
        match self {
            Self::SimpleLogin => "https://app.simplelogin.io/dashboard/api_key",
            Self::Addy => "https://app.addy.io/settings/api",
            Self::FirefoxRelay => "https://relay.firefox.com/accounts/settings/",
        }
    }

    /// The service can run on the user's own server.
    #[must_use]
    pub fn self_hosted(self) -> bool {
        self != Self::FirefoxRelay
    }

    /// The server the key is sent to: the service's own when `typed` is empty, else the origin
    /// typed (https, or http on this machine only: the key never travels in clear).
    #[must_use]
    pub fn server(self, typed: &str) -> Option<String> {
        let typed = typed.trim();
        if typed.is_empty() || !self.self_hosted() {
            return Some(self.default_server().to_owned());
        }
        let with_scheme = if typed.contains("://") {
            typed.to_owned()
        } else {
            format!("https://{typed}")
        };
        let url = Url::parse(&with_scheme).ok()?;
        if !url.username().is_empty() || url.password().is_some() {
            return None;
        }
        crate::passwords::origin_of(&url)
    }
}

/// The account aliases are made with. The key is wiped when dropped and never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    pub provider: Provider,
    pub server: String,
    pub key: String,
}

impl Drop for Account {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("provider", &self.provider)
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

/// One alias made from BlackHole, with the site it was made for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias {
    /// BlackHole's own id.
    pub id: String,
    pub provider: Provider,
    pub server: String,
    /// The service's id, needed to switch it off or delete it.
    pub remote: String,
    pub email: String,
    /// `scheme://host[:port]` of the site (see `passwords::origin_of`).
    pub origin: String,
    pub enabled: bool,
    /// Unix milliseconds.
    pub created: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Aliases {
    pub account: Option<Account>,
    pub list: Vec<Alias>,
}

impl Aliases {
    /// The aliases still receiving mail made for exactly `origin`, the latest first.
    #[must_use]
    pub fn for_origin(&self, origin: &str) -> Vec<&Alias> {
        let mut out: Vec<&Alias> = self
            .list
            .iter()
            .filter(|a| a.enabled && a.origin == origin)
            .collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.created));
        out
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Alias> {
        self.list.iter().find(|a| a.id == id)
    }

    /// `username` is one of the aliases kept here (the mark in the password list).
    #[must_use]
    pub fn is_alias(&self, username: &str) -> bool {
        !username.is_empty() && self.list.iter().any(|a| a.email.eq_ignore_ascii_case(username))
    }

    /// The connected account can act on `alias` (same service, same server).
    #[must_use]
    pub fn reaches(&self, alias: &Alias) -> bool {
        self.account
            .as_ref()
            .is_some_and(|a| a.provider == alias.provider && a.server == alias.server)
    }

    /// Keeps `alias`; an alias already known (same service id) is replaced.
    pub fn add(&mut self, alias: Alias) {
        self.list.retain(|a| {
            a.provider != alias.provider || a.server != alias.server || a.remote != alias.remote
        });
        self.list.push(alias);
    }

    pub fn set_enabled(&mut self, id: &str, on: bool) {
        if let Some(a) = self.list.iter_mut().find(|a| a.id == id) {
            a.enabled = on;
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<Alias> {
        let at = self.list.iter().position(|a| a.id == id)?;
        Some(self.list.remove(at))
    }

    /// In the order the Settings list them: by site, then the latest first.
    #[must_use]
    pub fn sorted(&self) -> Vec<&Alias> {
        let host = |x: &Alias| crate::passwords::display_host(&x.origin);
        let mut out: Vec<&Alias> = self.list.iter().collect();
        out.sort_by(|a, b| host(a).cmp(&host(b)).then(b.created.cmp(&a.created)));
        out
    }
}

/// An address a service answered with: kept only if it looks like one (it is written in pages).
#[must_use]
pub fn valid_email(s: &str) -> bool {
    let Some((local, host)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && host.contains('.')
        && !host.contains('@')
        && s.len() <= 254
        && s.chars()
            .all(|c| c.is_ascii_graphic() && !"<>\"'`\\,;()".contains(c))
}

/// A service id put in an address path: letters, digits and dashes only.
#[must_use]
pub fn valid_remote(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(id: &str, origin: &str, created: i64) -> Alias {
        Alias {
            id: id.into(),
            provider: Provider::SimpleLogin,
            server: "https://app.simplelogin.io".into(),
            remote: format!("r{id}"),
            email: format!("{id}@alias.test"),
            origin: origin.into(),
            enabled: true,
            created,
        }
    }

    #[test]
    fn the_key_only_goes_to_https_or_this_machine() {
        let p = Provider::SimpleLogin;
        assert_eq!(p.server("").as_deref(), Some("https://app.simplelogin.io"));
        assert_eq!(p.server("mail.home.example/x").as_deref(), Some("https://mail.home.example"));
        assert_eq!(p.server("http://127.0.0.1:99").as_deref(), Some("http://127.0.0.1:99"));
        assert_eq!(p.server("http://mail.home.example"), None);
        assert_eq!(p.server("https://u:p@mail.home.example"), None);
        assert_eq!(p.server("javascript://x"), None);
        assert_eq!(
            Provider::FirefoxRelay.server("https://else.example").as_deref(),
            Some("https://relay.firefox.com"),
            "Relay is never self-hosted"
        );
    }

    #[test]
    fn provider_ids_read_back() {
        for p in Provider::ALL {
            assert_eq!(Provider::from_id(p.id()), Some(p));
        }
        assert_eq!(Provider::from_id("other"), None);
    }

    #[test]
    fn an_alias_stays_on_its_site_until_switched_off() {
        let mut a = Aliases::default();
        a.add(alias("a", "https://shop.test", 1));
        a.add(alias("b", "https://shop.test", 2));
        a.add(alias("c", "https://other.test", 3));
        let ids = |a: &Aliases| -> Vec<String> {
            a.for_origin("https://shop.test").iter().map(|x| x.id.clone()).collect()
        };
        assert_eq!(ids(&a), ["b", "a"]);
        a.set_enabled("b", false);
        assert_eq!(ids(&a), ["a"]);
        assert!(a.is_alias("B@alias.test") && !a.is_alias("") && !a.is_alias("me@mail.test"));
        assert!(a.remove("a").is_some() && a.remove("a").is_none());
    }

    #[test]
    fn only_the_account_that_made_it_acts_on_an_alias() {
        let mut a = Aliases::default();
        let x = alias("a", "https://shop.test", 1);
        assert!(!a.reaches(&x));
        let mut account = Account {
            provider: x.provider,
            server: x.server.clone(),
            key: "k-secret".into(),
        };
        assert!(!format!("{account:?}").contains("k-secret"));
        a.account = Some(account.clone());
        assert!(a.reaches(&x));
        account.server = "https://mine.test".into();
        a.account = Some(account);
        assert!(!a.reaches(&x));
    }

    #[test]
    fn answers_of_a_service_are_checked() {
        assert!(valid_email("a.b-1@alias.example"));
        for bad in ["", "a", "a@b", "@a.b", "a@b@c.d", "a b@c.d", "a\"@c.d", "<a>@c.d"] {
            assert!(!valid_email(bad), "{bad}");
        }
        assert!(valid_remote("12") && valid_remote("50c9-ab"));
        assert!(!valid_remote("") && !valid_remote("../x") && !valid_remote("1?a"));
    }
}

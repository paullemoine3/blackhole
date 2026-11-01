use url::Url;

/// Privacy-respecting search engines only (no account, no profiling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchEngine {
    #[default]
    DuckDuckGo,
    Brave,
    Startpage,
    Qwant,
    Mojeek,
}

impl SearchEngine {
    pub const ALL: [SearchEngine; 5] = [
        Self::DuckDuckGo,
        Self::Brave,
        Self::Startpage,
        Self::Qwant,
        Self::Mojeek,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "duckduckgo",
            Self::Brave => "brave",
            Self::Startpage => "startpage",
            Self::Qwant => "qwant",
            Self::Mojeek => "mojeek",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Brave => "Brave Search",
            Self::Startpage => "Startpage",
            Self::Qwant => "Qwant",
            Self::Mojeek => "Mojeek",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.id() == id)
    }

    fn template(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "https://duckduckgo.com/",
            Self::Brave => "https://search.brave.com/search",
            Self::Startpage => "https://www.startpage.com/do/search",
            Self::Qwant => "https://www.qwant.com/",
            Self::Mojeek => "https://www.mojeek.com/search",
        }
    }

    pub fn search_url(self, query: &str) -> Url {
        let mut url = Url::parse(self.template()).expect("static url");
        let key = if self == Self::Startpage {
            "query"
        } else {
            "q"
        };
        url.query_pairs_mut().append_pair(key, query);
        url
    }
}

/// Page served by an installed browser extension (chrome-extension://<id>/...).
pub fn is_extension_page(url: &Url) -> bool {
    matches!(
        url.scheme(),
        "chrome-extension" | "safari-web-extension" | "webkit-extension"
    ) && url.host_str().is_some()
}

/// Extension page meant to be shown in the toolbar popup (e.g. Bitwarden's unlock popout),
/// as opposed to a full page such as options or onboarding.
pub fn is_extension_popout(url: &Url) -> bool {
    is_extension_page(url)
        && (url.path().contains("/popup/")
            || url
                .query_pairs()
                .any(|(k, v)| k == "singleActionPopout" || (k == "uilocation" && v == "popout")))
}

/// Turns what the user typed in the command bar into a navigable URL.
/// Bare hosts become `https://`; anything else is a search query.
pub fn resolve_input(input: &str, engine: SearchEngine) -> Url {
    let input = input.trim();
    if let Ok(url) = Url::parse(input) {
        if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
            return url;
        }
        if is_extension_page(&url) {
            return url;
        }
    }
    let looks_like_host = !input.contains(char::is_whitespace)
        && (input.contains('.') || input.starts_with("localhost"))
        && !input.starts_with('.')
        && !input.ends_with('.');
    if looks_like_host {
        if let Ok(url) = Url::parse(&format!("https://{input}")) {
            return url;
        }
    }
    engine.search_url(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_gets_https() {
        assert_eq!(
            resolve_input("rust-lang.org", Default::default()).as_str(),
            "https://rust-lang.org/"
        );
    }

    #[test]
    fn text_becomes_search() {
        let url = resolve_input("rust tabs & more", Default::default());
        assert_eq!(url.host_str(), Some("duckduckgo.com"));
    }

    #[test]
    fn engine_ids_roundtrip_and_default_is_duckduckgo() {
        assert_eq!(SearchEngine::default(), SearchEngine::DuckDuckGo);
        for e in SearchEngine::ALL {
            assert_eq!(SearchEngine::from_id(e.id()), Some(e));
        }
        assert_eq!(SearchEngine::from_id("nope"), None);
    }

    #[test]
    fn extension_pages_open_as_is() {
        let raw = "chrome-extension://abcdefgh/popup/index.html?uilocation=popout";
        assert_eq!(resolve_input(raw, Default::default()).as_str(), raw);
    }

    #[test]
    fn only_popup_pages_are_popouts() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(is_extension_popout(&u("chrome-extension://abc/popup/index.html?uilocation=popout")));
        assert!(!is_extension_popout(&u("chrome-extension://abc/options.html")));
        assert!(!is_extension_popout(&u("https://example.com/popup/")));
    }

    #[test]
    fn javascript_scheme_is_never_navigated() {
        let url = resolve_input("javascript:alert(1)", Default::default());
        assert_eq!(url.scheme(), "https");
    }
}

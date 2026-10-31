//! Extensions: the content-script subset of the WebExtension format (CSS and JS injected into
//! pages whose URL matches a Chrome-style match pattern). Background pages and the `chrome.*`
//! APIs are out of scope: the native engines behind the shell expose no extension host.

use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentScript {
    /// Chrome match patterns, e.g. `*://*.example.com/*` or `<all_urls>`.
    pub matches: Vec<String>,
    pub css: Vec<String>,
    pub js: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    /// Folder name, stable across restarts.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub scripts: Vec<ContentScript>,
}

/// CSS and JS an extension wants to run in a page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Injection {
    pub css: String,
    pub js: String,
}

impl Injection {
    pub fn is_empty(&self) -> bool {
        self.css.is_empty() && self.js.is_empty()
    }
}

impl Extension {
    /// Everything this extension injects into `url`, in manifest order.
    pub fn injection_for(&self, url: &Url) -> Injection {
        let mut out = Injection::default();
        for script in self
            .scripts
            .iter()
            .filter(|s| s.matches.iter().any(|p| matches_pattern(p, url)))
        {
            for css in &script.css {
                out.css.push_str(css);
                out.css.push('\n');
            }
            for js in &script.js {
                // Each file runs in its own scope so extensions cannot clash with each other.
                out.js.push_str("(function(){try{\n");
                out.js.push_str(js);
                out.js
                    .push_str("\n}catch(e){console.warn('[extension]',e)}})();\n");
            }
        }
        out
    }
}

/// Chrome match pattern: `<scheme>://<host>/<path>` with `*` wildcards; `<all_urls>` matches any
/// http(s) page. Only http and https pages are ever eligible.
pub fn matches_pattern(pattern: &str, url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    if pattern == "<all_urls>" {
        return true;
    }
    let Some((scheme, rest)) = pattern.split_once("://") else {
        return false;
    };
    let (host, path) = rest.split_once('/').map_or((rest, ""), |(h, p)| (h, p));
    let scheme_ok = scheme == "*" || scheme == url.scheme();
    let url_host = url.host_str().unwrap_or_default();
    let host_ok = if host == "*" {
        true
    } else if let Some(base) = host.strip_prefix("*.") {
        url_host == base || url_host.ends_with(&format!(".{base}"))
    } else {
        url_host == host
    };
    let mut target = url.path().to_owned();
    if let Some(q) = url.query() {
        target.push('?');
        target.push_str(q);
    }
    scheme_ok && host_ok && glob(&format!("/{path}"), &target)
}

fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || text.len() < first.len() + last.len() || !text.ends_with(last) {
        return false;
    }
    let mut at = first.len();
    let end = text.len() - last.len();
    for mid in &parts[1..parts.len() - 1] {
        match text[at..end].find(mid) {
            Some(i) => at += i + mid.len(),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn match_patterns_follow_chrome_rules() {
        assert!(matches_pattern("<all_urls>", &u("https://a.org/x")));
        assert!(!matches_pattern("<all_urls>", &u("file:///etc/hosts")));
        assert!(matches_pattern(
            "*://*.example.com/*",
            &u("https://www.example.com/a?b=1")
        ));
        assert!(matches_pattern(
            "*://*.example.com/*",
            &u("http://example.com/")
        ));
        assert!(!matches_pattern(
            "*://*.example.com/*",
            &u("https://badexample.com/")
        ));
        assert!(matches_pattern(
            "https://example.com/blog/*",
            &u("https://example.com/blog/post")
        ));
        assert!(!matches_pattern(
            "https://example.com/blog/*",
            &u("https://example.com/shop")
        ));
        assert!(!matches_pattern(
            "https://example.com/*",
            &u("http://example.com/")
        ));
        assert!(matches_pattern(
            "*://example.com/a*c",
            &u("https://example.com/abbc")
        ));
    }

    #[test]
    fn injection_collects_only_matching_scripts_in_isolated_scopes() {
        let ext = Extension {
            id: "dark".into(),
            name: "Dark".into(),
            version: "1".into(),
            description: String::new(),
            scripts: vec![
                ContentScript {
                    matches: vec!["*://*.a.org/*".into()],
                    css: vec!["body{x:1}".into()],
                    js: vec!["var a=1".into()],
                },
                ContentScript {
                    matches: vec!["*://b.org/*".into()],
                    css: vec!["body{x:2}".into()],
                    js: vec![],
                },
            ],
        };
        let hit = ext.injection_for(&u("https://www.a.org/"));
        assert!(hit.css.contains("x:1") && !hit.css.contains("x:2"));
        assert!(hit.js.contains("var a=1") && hit.js.contains("try{"));
        assert!(ext.injection_for(&u("https://c.org/")).is_empty());
    }
}

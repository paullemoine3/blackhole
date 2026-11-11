//! Clean links: tracking parameters are dropped from an address and the redirect pages of
//! search engines and social networks (which only note the click) are skipped. Pure rules, used
//! at navigation time and when a link is copied.

use url::Url;

/// Parameters that only identify the click or the campaign, on any site.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "gclsrc", "dclid", "msclkid", "yclid", "twclid", "ttclid", "li_fat_id",
    "mc_eid", "mc_cid", "igshid", "igsh", "_hsenc", "_hsmi", "__hssc", "__hstc", "__hsfp",
    "hsctatracking", "ref_src", "ref_url", "wbraid", "gbraid", "_ga", "_gl", "mkt_tok",
    "oly_enc_id", "oly_anon_id", "vero_id", "vero_conv", "s_cid", "srsltid",
];

/// Prefixes of campaign parameters (Google Analytics, Matomo/Piwik).
const TRACKING_PREFIXES: &[&str] = &["utm_", "pk_", "mtm_"];

/// Parameters that track only on some sites (elsewhere the same name may mean something):
/// `(host or parent domain, names)`.
const SITE_PARAMS: &[(&str, &[&str])] = &[
    ("youtube.com", &["si", "pp", "feature"]),
    ("youtu.be", &["si", "feature"]),
    ("spotify.com", &["si"]),
    ("twitter.com", &["s", "t"]),
    ("x.com", &["s", "t"]),
];

/// Redirect pages that carry the real address in a parameter: `(host, path prefix, parameter)`.
/// Google's `/url` page is matched apart (`is_google`: one host per country).
const REDIRECTORS: &[(&str, &str, &str)] = &[
    ("l.facebook.com", "/l.php", "u"),
    ("lm.facebook.com", "/l.php", "u"),
    ("l.messenger.com", "/l.php", "u"),
    ("l.instagram.com", "/", "u"),
    ("out.reddit.com", "/", "url"),
    ("www.youtube.com", "/redirect", "q"),
    ("steamcommunity.com", "/linkfilter", "u"),
    ("away.vk.com", "/away.php", "to"),
    ("slack-redir.net", "/link", "url"),
    ("duckduckgo.com", "/l/", "uddg"),
    ("t.umblr.com", "/redirect", "z"),
    ("www.linkedin.com", "/redir/redirect", "url"),
];

/// How many redirect pages are skipped in a row (one wraps another at times).
const MAX_HOPS: usize = 3;

fn under(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
}

fn is_tracking_param(host: &str, key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    TRACKING_PREFIXES.iter().any(|p| key.starts_with(p))
        || TRACKING_PARAMS.contains(&key.as_str())
        || SITE_PARAMS
            .iter()
            .any(|(site, names)| under(host, site) && names.contains(&key.as_str()))
}

/// The address without its tracking parameters; `None` when it has none.
pub fn strip_tracking(url: &Url) -> Option<Url> {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if !url.query_pairs().any(|(k, _)| is_tracking_param(&host, &k)) {
        return None;
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !is_tracking_param(&host, k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut out = url.clone();
    if kept.is_empty() {
        out.set_query(None);
    } else {
        out.query_pairs_mut().clear().extend_pairs(kept);
    }
    Some(out)
}

/// Google's own hosts: `google.com`, `www.google.fr`, `www.google.co.uk`, `www.google.com.br`.
fn is_google(host: &str) -> bool {
    let Some(tld) = host.strip_prefix("www.").unwrap_or(host).strip_prefix("google.") else {
        return false;
    };
    let alpha = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase());
    match tld.split_once('.') {
        None => alpha(tld),
        Some((second, country)) => matches!(second, "co" | "com") && country.len() == 2 && alpha(country),
    }
}

/// The address a known redirect page leads to; `None` for any other page, or when the target
/// is not a web address.
pub fn unwrap_redirect(url: &Url) -> Option<Url> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let path = url.path();
    let names: &[&str] = if is_google(&host) && path == "/url" {
        &["q", "url"]
    } else {
        let (_, _, name) = REDIRECTORS
            .iter()
            .find(|(h, prefix, _)| host == *h && path.starts_with(prefix))?;
        std::slice::from_ref(name)
    };
    let raw = url
        .query_pairs()
        .find(|(k, v)| names.contains(&k.as_ref()) && !v.is_empty())
        .map(|(_, v)| v.into_owned())?;
    let target = Url::parse(&raw).ok()?;
    (matches!(target.scheme(), "http" | "https") && target.host_str().is_some()).then_some(target)
}

/// The clean form of a web address: redirect pages skipped, tracking parameters dropped.
/// `None` when the address is already clean (or is not http(s)).
pub fn clean(url: &Url) -> Option<Url> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let mut out = url.clone();
    let mut changed = false;
    for _ in 0..MAX_HOPS {
        match unwrap_redirect(&out) {
            Some(target) => {
                out = target;
                changed = true;
            }
            None => break,
        }
    }
    if let Some(stripped) = strip_tracking(&out) {
        out = stripped;
        changed = true;
    }
    changed.then_some(out)
}

/// `raw` cleaned when it is a web address, unchanged otherwise (what "Copy link" copies).
pub fn cleaned(raw: &str) -> String {
    Url::parse(raw)
        .ok()
        .and_then(|u| clean(&u))
        .map_or_else(|| raw.to_owned(), String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(raw: &str) -> Option<String> {
        clean(&Url::parse(raw).unwrap()).map(String::from)
    }

    #[test]
    fn tracking_parameters_go_and_the_others_stay_in_order() {
        assert_eq!(
            c("https://shop.test/p?id=3&utm_source=mail&fbclid=abc&color=red").as_deref(),
            Some("https://shop.test/p?id=3&color=red")
        );
        assert_eq!(c("https://a.test/?UTM_Medium=x&gclid=1").as_deref(), Some("https://a.test/"));
        assert_eq!(c("https://a.test/?mtm_campaign=x&pk_kwd=y#top").as_deref(), Some("https://a.test/#top"));
        assert_eq!(c("https://a.test/p?id=3"), None);
        assert_eq!(c("https://a.test/"), None);
    }

    #[test]
    fn site_parameters_only_go_on_their_site() {
        assert_eq!(
            c("https://www.youtube.com/watch?v=abc&si=xyz&t=42").as_deref(),
            Some("https://www.youtube.com/watch?v=abc&t=42")
        );
        assert_eq!(c("https://youtu.be/abc?si=xyz").as_deref(), Some("https://youtu.be/abc"));
        assert_eq!(c("https://x.com/a/status/1?s=20&t=k").as_deref(), Some("https://x.com/a/status/1"));
        // Elsewhere `s`, `t` and `si` are ordinary parameters (a search, a time).
        assert_eq!(c("https://blog.test/?s=rust&t=3&si=1"), None);
        assert_eq!(c("https://notyoutube.com/?si=1"), None);
    }

    #[test]
    fn redirect_pages_are_skipped_and_their_target_is_cleaned() {
        assert_eq!(
            c("https://www.google.com/url?sa=t&url=https%3A%2F%2Fa.test%2Fdoc%3Futm_source%3Dg&usg=x").as_deref(),
            Some("https://a.test/doc")
        );
        assert_eq!(
            c("https://www.google.fr/url?q=https://a.test/x").as_deref(),
            Some("https://a.test/x")
        );
        assert_eq!(
            c("https://l.facebook.com/l.php?u=https%3A%2F%2Fa.test%2F%3Ffbclid%3D1&h=AT0").as_deref(),
            Some("https://a.test/")
        );
        assert_eq!(
            c("https://duckduckgo.com/l/?uddg=https%3A%2F%2Fa.test%2F&rut=abc").as_deref(),
            Some("https://a.test/")
        );
        assert_eq!(
            c("https://www.youtube.com/redirect?event=x&q=https%3A%2F%2Fa.test%2F").as_deref(),
            Some("https://a.test/")
        );
    }

    #[test]
    fn a_redirect_to_anything_but_a_web_address_is_left_alone() {
        for raw in [
            "https://www.google.com/url?q=javascript:alert(1)",
            "https://www.google.com/url?q=file:///etc/passwd",
            "https://www.google.com/url?q=",
            "https://www.google.com/url?q=not a url",
            "https://l.facebook.com/l.php?u=data:text/html,x",
            // A site that merely looks like one, or another page of the same site.
            "https://www.google.com.evil.test/url?q=https://a.test/",
            "https://www.google.com/search?q=https://a.test/",
            "https://evil.test/l.php?u=https://a.test/",
        ] {
            let url = Url::parse(raw).unwrap();
            assert_eq!(unwrap_redirect(&url), None, "{raw}");
        }
        assert_eq!(c("ftp://a.test/?utm_source=x"), None);
    }

    #[test]
    fn nested_redirects_stop_after_a_few_hops() {
        let inner = "https://a.test/";
        let mut raw = inner.to_owned();
        for _ in 0..MAX_HOPS + 2 {
            raw = Url::parse_with_params("https://www.google.com/url", [("q", raw.as_str())])
                .unwrap()
                .into();
        }
        let out = c(&raw).unwrap();
        assert!(out.starts_with("https://www.google.com/url"), "{out}");
        assert_eq!(cleaned("https://a.test/?utm_source=x"), "https://a.test/");
        assert_eq!(cleaned("not a url"), "not a url");
    }
}

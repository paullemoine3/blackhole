//! Interface languages: string catalogs (`i18n/<code>.toml`, embedded), the choice of language
//! (setting, then system, then English) and text with parameters and simple plurals.
//!
//! A catalog is a flat subset of TOML: `[section]` headers and `key = "text"` lines (escapes
//! `\"`, `\\`, `\n`, `\t`, `\uXXXX`), so `key` under `[toast]` is `toast.key`. A plural is a pair
//! of keys `name.one` / `name.other`, chosen by the language's rule (`Catalog::plural`).
//! Parameters are written `{name}`; every language must use the same ones for a key (tested).

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Setting value that follows the system's language.
pub const SYSTEM: &str = "system";
/// The fallback language, for a missing key and an unsupported system language.
pub const DEFAULT: &str = "en";

/// Supported languages: code and own name (a language is always listed in its own words).
pub const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("fr", "Français")];

const SOURCES: [(&str, &str); 2] = [
    ("en", include_str!("../i18n/en.toml")),
    ("fr", include_str!("../i18n/fr.toml")),
];

/// One language's strings. Lookups fall back to English, then to the key itself.
#[derive(Debug)]
pub struct Catalog {
    lang: &'static str,
    entries: BTreeMap<String, String>,
}

/// Whether `code` is a supported language or the system choice: anything else is not stored.
pub fn is_choice(code: &str) -> bool {
    code == SYSTEM || LANGUAGES.iter().any(|(c, _)| *c == code)
}

/// The language to show: the user's choice when it is a supported language, else the first
/// supported one among the system's preferred locales (`fr-FR`, `fr_CA.UTF-8`, `en`…), else
/// English.
pub fn resolve<S: AsRef<str>>(choice: &str, system: &[S]) -> &'static str {
    if let Some((code, _)) = LANGUAGES.iter().find(|(c, _)| *c == choice) {
        return code;
    }
    system
        .iter()
        .filter_map(|locale| {
            let primary = locale
                .as_ref()
                .split(['-', '_', '.', '@'])
                .next()?
                .trim()
                .to_ascii_lowercase();
            LANGUAGES.iter().find(|(c, _)| *c == primary).map(|(c, _)| *c)
        })
        .next()
        .unwrap_or(DEFAULT)
}

/// The catalog of a supported language (English for any other code), parsed once.
pub fn catalog(lang: &str) -> &'static Catalog {
    static CATALOGS: OnceLock<Vec<Catalog>> = OnceLock::new();
    let all = CATALOGS.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(lang, src)| Catalog {
                lang,
                // The embedded catalogs are checked by the tests; a broken line only loses keys.
                entries: parse(src).unwrap_or_default(),
            })
            .collect()
    });
    all.iter()
        .find(|c| c.lang == lang)
        .unwrap_or_else(|| &all[0])
}

impl Catalog {
    pub fn lang(&self) -> &'static str {
        self.lang
    }

    /// The text of `key`, in English when this language lacks it, or the key itself when no
    /// catalog has it (visible, never empty).
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.find(key).unwrap_or(key)
    }

    fn find(&self, key: &str) -> Option<&str> {
        self.entries
            .get(key)
            .or_else(|| {
                (self.lang != DEFAULT)
                    .then(|| catalog(DEFAULT).entries.get(key))
                    .flatten()
            })
            .map(String::as_str)
    }

    /// `key` with its `{name}` parameters replaced; unknown parameters stay as written.
    pub fn format(&self, key: &str, params: &[(&str, &str)]) -> String {
        fill(self.get(key), params)
    }

    /// The plural form of `key` for `n` (`key.one` or `key.other`), with `{n}` and `params`.
    pub fn plural(&self, key: &str, n: u64, params: &[(&str, &str)]) -> String {
        let form = format!("{key}.{}", plural_form(self.lang, n));
        let text = self
            .find(&form)
            .or_else(|| self.find(&format!("{key}.other")))
            .unwrap_or(key);
        let count = n.to_string();
        let mut all = vec![("n", count.as_str())];
        all.extend_from_slice(params);
        fill(text, &all)
    }

    /// Every key with its text, English filling the keys this language lacks (sent to pages).
    pub fn merged(&self) -> BTreeMap<&str, &str> {
        let mut all: BTreeMap<&str, &str> = catalog(DEFAULT)
            .entries
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        all.extend(self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        all
    }

    /// Keys starting with `prefix` and their text (merged), e.g. the error page's `error.`.
    pub fn with_prefix(&self, prefix: &str) -> BTreeMap<&str, &str> {
        let mut all = self.merged();
        all.retain(|k, _| k.starts_with(prefix));
        all
    }
}

/// Prefix of a message made by `message`.
const MESSAGE: &str = "i18n:";
const SEP: char = '\u{1f}';

/// A text for the user made by a layer that has no language (infrastructure, domain): the key
/// and its parameters, kept in a plain `String` (errors, statuses) and translated by the shell
/// with `Catalog::render`, so it follows a later change of language too.
pub fn message(key: &str, params: &[(&str, &str)]) -> String {
    let mut out = format!("{MESSAGE}{key}");
    for (name, value) in params {
        out.push(SEP);
        out.push_str(name);
        out.push('=');
        out.extend(value.chars().filter(|c| *c != SEP));
    }
    out
}

impl Catalog {
    /// A `message` in this language; any other text (a system error) is returned as it is.
    pub fn render(&self, text: &str) -> String {
        let Some(encoded) = text.strip_prefix(MESSAGE) else {
            return text.to_owned();
        };
        let mut parts = encoded.split(SEP);
        let key = parts.next().unwrap_or_default();
        // A parameter may itself be a message without parameters ("the other device").
        let values: Vec<(&str, String)> = parts
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k, self.render(v)))
            .collect();
        let params: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.format(key, &params)
    }
}

/// `one` or `other`: English uses the singular for 1 only, French for 0 and 1.
pub fn plural_form(lang: &str, n: u64) -> &'static str {
    let one = match lang {
        "fr" => n < 2,
        _ => n == 1,
    };
    if one {
        "one"
    } else {
        "other"
    }
}

fn fill(text: &str, params: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if is_name(&after[..close]) => {
                let name = &after[..close];
                match params.iter().find(|(k, _)| *k == name) {
                    Some((_, value)) => out.push_str(value),
                    None => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The `{name}` parameters used by a text, sorted and without repeats.
pub fn params_of(text: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if is_name(&after[..close]) => {
                names.push(&after[..close]);
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// Reads a catalog. Errors name the line (1-based) and what is wrong with it.
pub fn parse(src: &str) -> Result<BTreeMap<String, String>, String> {
    let mut entries = BTreeMap::new();
    let mut section = String::new();
    for (i, raw) in src.lines().enumerate() {
        let line = raw.trim();
        let at = |what: &str| format!("line {}: {what}", i + 1);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[') {
            let name = name.strip_suffix(']').ok_or_else(|| at("unclosed section"))?;
            if !is_key(name) {
                return Err(at("bad section name"));
            }
            section = name.to_owned();
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| at("expected key = \"text\""))?;
        let key = key.trim();
        if !is_key(key) {
            return Err(at("bad key"));
        }
        let value = unquote(value.trim()).ok_or_else(|| at("bad quoted text"))?;
        let full = if section.is_empty() {
            key.to_owned()
        } else {
            format!("{section}.{key}")
        };
        if entries.insert(full, value).is_some() {
            return Err(at("duplicate key"));
        }
    }
    Ok(entries)
}

fn is_key(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

fn unquote(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return None,
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok().filter(|_| hex.len() == 4)?;
                    out.push(char::from_u32(code)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalogs_parse() {
        for (lang, src) in SOURCES {
            let entries = parse(src).unwrap_or_else(|e| panic!("{lang}.toml: {e}"));
            assert!(entries.len() > 100, "{lang}.toml looks empty");
        }
    }

    #[test]
    fn catalogs_have_the_same_keys_and_parameters() {
        let en = parse(SOURCES[0].1).unwrap();
        for (lang, src) in &SOURCES[1..] {
            let other = parse(src).unwrap();
            let missing: Vec<_> = en.keys().filter(|k| !other.contains_key(*k)).collect();
            let extra: Vec<_> = other.keys().filter(|k| !en.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{lang}.toml lacks {missing:?}");
            assert!(extra.is_empty(), "{lang}.toml has unknown keys {extra:?}");
            for (key, text) in &en {
                assert_eq!(
                    params_of(text),
                    params_of(&other[key]),
                    "{lang}.toml: parameters of {key}"
                );
            }
        }
    }

    #[test]
    fn plurals_come_in_pairs() {
        for (lang, src) in SOURCES {
            let entries = parse(src).unwrap();
            for key in entries.keys() {
                if let Some(base) = key.strip_suffix(".one") {
                    assert!(
                        entries.contains_key(&format!("{base}.other")),
                        "{lang}: {key} without .other"
                    );
                }
            }
        }
    }

    #[test]
    fn no_text_is_empty_or_untrimmed() {
        for (lang, src) in SOURCES {
            for (key, text) in parse(src).unwrap() {
                assert!(!text.trim().is_empty(), "{lang}: {key} is empty");
                assert_eq!(text.trim(), text, "{lang}: {key} has outer spaces");
            }
        }
    }

    #[test]
    fn resolves_choice_then_system_then_english() {
        assert_eq!(resolve("fr", &["en-US"]), "fr");
        assert_eq!(resolve("en", &["fr-FR"]), "en");
        assert_eq!(resolve(SYSTEM, &["fr-FR"]), "fr");
        assert_eq!(resolve(SYSTEM, &["fr_CA.UTF-8"]), "fr");
        assert_eq!(resolve(SYSTEM, &["de-DE", "FR"]), "fr");
        assert_eq!(resolve(SYSTEM, &["de-DE", "ja"]), "en");
        assert_eq!(resolve::<&str>(SYSTEM, &[]), "en");
        assert_eq!(resolve("xx", &["fr"]), "fr");
        assert!(is_choice("system") && is_choice("fr") && is_choice("en"));
        assert!(!is_choice("de") && !is_choice(""));
    }

    #[test]
    fn formats_parameters_and_plurals() {
        assert_eq!(fill("Open {host}", &[("host", "a.b")]), "Open a.b");
        assert_eq!(fill("{a}{b} {c}", &[("a", "1"), ("b", "2")]), "12 {c}");
        assert_eq!(fill("{ not a param } {", &[]), "{ not a param } {");
        assert_eq!(plural_form("en", 0), "other");
        assert_eq!(plural_form("en", 1), "one");
        assert_eq!(plural_form("fr", 0), "one");
        assert_eq!(plural_form("fr", 1), "one");
        assert_eq!(plural_form("fr", 2), "other");
        assert_eq!(params_of("{n} tabs in {space}, {n}"), vec!["n", "space"]);
    }

    #[test]
    fn parses_sections_escapes_and_rejects_mistakes() {
        let src = "# c\nroot = \"a\"\n[toast]\nhi = \"say \\\"hi\\\"\\n\\u00e9\"\n";
        let e = parse(src).unwrap();
        assert_eq!(e["root"], "a");
        assert_eq!(e["toast.hi"], "say \"hi\"\né");
        assert!(parse("a = b").is_err());
        assert!(parse("a = \"x\"\na = \"y\"").is_err());
        assert!(parse("[x\n").is_err());
        assert!(parse("bad key = \"x\"").is_err());
        assert!(parse("a = \"x\"y\"").is_err());
    }

    #[test]
    fn messages_from_other_layers_are_rendered_later() {
        let m = message("toast.zoom", &[("n", "120=a\u{1f}b")]);
        assert!(m.starts_with("i18n:toast.zoom"));
        assert_eq!(
            catalog("en").render(&m),
            catalog("en").format("toast.zoom", &[("n", "120=ab")])
        );
        assert_eq!(catalog("fr").render("disk full"), "disk full");
        let nested = message("msg.device_silent", &[("name", &message("msg.other_device", &[]))]);
        assert_eq!(
            catalog("en").render(&nested),
            catalog("en").format("msg.device_silent", &[("name", "The other device")])
        );
    }

    #[test]
    fn lookups_fall_back_to_english_then_the_key() {
        let fr = catalog("fr");
        assert_eq!(fr.lang(), "fr");
        assert_eq!(catalog("de").lang(), "en");
        assert_eq!(fr.get("no.such.key"), "no.such.key");
        let one = |c: &Catalog, n: &str| c.format("toast.tabs_closed.one", &[("n", n)]);
        let other = |c: &Catalog, n: &str| c.format("toast.tabs_closed.other", &[("n", n)]);
        assert_eq!(fr.plural("toast.tabs_closed", 0, &[]), one(fr, "0"));
        assert_eq!(fr.plural("toast.tabs_closed", 2, &[]), other(fr, "2"));
        let en = catalog("en");
        assert_eq!(en.plural("toast.tabs_closed", 0, &[]), other(en, "0"));
        assert_eq!(en.plural("toast.tabs_closed", 1, &[]), one(en, "1"));
        assert!(fr.with_prefix("error.").keys().all(|k| k.starts_with("error.")));
        assert_eq!(fr.merged().len(), catalog("en").merged().len());
    }
}
